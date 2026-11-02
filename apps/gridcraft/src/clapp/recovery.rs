//! Unsaved work survives the window ending. Under Clatch the window ends whenever Clatch stops
//! it or its pipe drops (clappkit/docs/protocol.md § 9), and GridCraft quits without asking, so
//! unsaved changes were simply gone. Now, while a workbook has unsaved changes, a copy of it is
//! kept in the app's data directory (`<data>/recovery/`), and the next launch opens each copy
//! again: still unsaved, titled "(recovered)", with no file, so saving it asks where.
//!
//! - The copy is written shortly after the person (or the agent) stops changing it: 1 s of quiet,
//!   or every 5 s during a long burst. Nothing waits for a shutdown hook, which Clatch does not
//!   leave time for: the copy on disk is never more than a few seconds behind.
//! - Writing happens on one worker thread, in order, from a cheap `Arc` clone of the workbook,
//!   so the frame never waits for an .xlsx to be encoded, and a removal can never be overtaken
//!   by an older write.
//! - A copy is removed once its workbook is saved, back to its saved state, or closed in the app.
//!   Quitting the window leaves them: that is exactly the case they exist for.
//! - A copy is written to `<name>.tmp` and renamed, so a copy cut off mid-write is never read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use gridcraft_engine::{DocState, UiRequest, io};
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

use super::CLI;

/// Quiet after a change before the copy is written.
const QUIET: Duration = Duration::from_secs(1);
/// The longest a copy lags behind during a long burst of changes.
const AT_MOST: Duration = Duration::from_secs(5);
/// Copies to open at launch, at most: a folder full of them is not a reason to open hundreds.
const MAX_RESTORED: usize = 20;

enum Job {
    Write { name: String, wb: Arc<gridcraft_engine::model::Workbook>, ext: &'static str, meta: Json },
    Remove { name: String },
}

/// What is known about one open workbook's copy.
struct Kept {
    /// The revision on disk, if any.
    written: Option<u64>,
    /// The revision last seen, and when it last changed.
    seen: u64,
    last_change: Instant,
    /// Since when the copy has been behind: the workbook became unsaved, or the last write.
    behind: Option<Instant>,
}

pub struct Recovery {
    dir: PathBuf,
    jobs: Option<Sender<Job>>,
    /// This run's prefix, so its copies never collide with an earlier run's.
    run: String,
    kept: HashMap<u64, Kept>,
    restored: bool,
}

impl Recovery {
    pub fn new() -> Recovery {
        let dir = clappkit::paths::data_subdir(CLI, "recovery");
        let (tx, rx) = channel::<Job>();
        let worker_dir = dir.clone();
        let spawned = std::thread::Builder::new().name("recovery".into()).spawn(move || {
            for job in rx {
                match job {
                    Job::Write { name, wb, ext, meta } => write_copy(&worker_dir, &name, &wb, ext, &meta),
                    Job::Remove { name } => remove_copy(&worker_dir, &name),
                }
            }
        });
        let jobs = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                log::error!("recovery: cannot start its writer, unsaved work is not kept: {e}");
                None
            }
        };
        let run = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
        );
        Recovery { dir, jobs, run, kept: HashMap::new(), restored: false }
    }

    /// Once per frame, after everything that can change a workbook has run.
    pub fn tick(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        if !self.restored {
            self.restored = true;
            restore(&self.dir, app);
            ctx.request_repaint();
        }
        let Some(jobs) = &self.jobs else { return };
        let now = Instant::now();
        let mut open = Vec::new();
        for d in app.session.documents() {
            open.push(d.uid);
            let name = format!("{}-{}", self.run, d.uid);
            let kept = self.kept.entry(d.uid).or_insert(Kept { written: None, seen: d.revision, last_change: now, behind: None });
            if !d.is_dirty() {
                if kept.written.take().is_some() {
                    let _ = jobs.send(Job::Remove { name });
                }
                kept.behind = None;
                kept.seen = d.revision;
                continue;
            }
            if kept.seen != d.revision {
                kept.seen = d.revision;
                kept.last_change = now;
            }
            if kept.written == Some(d.revision) {
                kept.behind = None;
                continue;
            }
            let behind = *kept.behind.get_or_insert(now);
            let quiet = now.saturating_duration_since(kept.last_change);
            if quiet >= QUIET || now.saturating_duration_since(behind) >= AT_MOST {
                let _ = jobs.send(Job::Write { name, wb: d.wb.clone(), ext: extension(d), meta: meta(d) });
                kept.written = Some(d.revision);
                kept.behind = None;
            } else {
                ctx.request_repaint_after(QUIET.saturating_sub(quiet));
            }
        }
        // Closed in the app (its own Save Changes question answered "Don't Save", or saved and
        // closed): the copy goes with it.
        self.kept.retain(|uid, kept| {
            let still = open.contains(uid);
            if !still && kept.written.is_some() {
                let _ = jobs.send(Job::Remove { name: format!("{}-{}", self.run, uid) });
            }
            still
        });
    }
}

/// A macro-enabled workbook keeps its macros in its copy.
fn extension(d: &DocState) -> &'static str {
    let macros = d.path.as_deref().is_some_and(|p| super::rules::extension(p) == "xlsm");
    if macros { "xlsm" } else { "xlsx" }
}

fn meta(d: &DocState) -> Json {
    json!({"title": d.display_title(), "path": d.path})
}

fn write_copy(dir: &Path, name: &str, wb: &gridcraft_engine::model::Workbook, ext: &str, meta: &Json) {
    let bytes = match io::save_bytes(wb, &format!("{name}.{ext}")) {
        Ok(b) => b,
        Err(e) => {
            log::error!("recovery: cannot encode a copy of {}: {e}", meta["title"]);
            return;
        }
    };
    // The workbook first, then what it is: a description never points at a missing copy.
    let book = dir.join(format!("{name}.{ext}"));
    let about = dir.join(format!("{name}.json"));
    if let Err(e) = replace(&book, &bytes).and_then(|()| replace(&about, meta.to_string().as_bytes())) {
        log::error!("recovery: cannot keep a copy of {} in {}: {e}", meta["title"], dir.display());
    }
}

fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn remove_copy(dir: &Path, name: &str) {
    for ext in ["json", "xlsx", "xlsm", "tmp"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}.{ext}")));
    }
}

/// Open every copy an earlier run left, then remove it: from here on it is an open, unsaved
/// workbook, which this run keeps a copy of in turn.
fn restore(dir: &Path, app: &mut SheetApp) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    if found.is_empty() {
        sweep(dir);
        return;
    }
    found.sort();
    let mut titles = Vec::new();
    let mut failed = Vec::new();
    for (_, about) in found.iter().rev().take(MAX_RESTORED).rev() {
        let meta: Json = std::fs::read_to_string(about).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Json::Null);
        let title = meta["title"].as_str().unwrap_or("Workbook").to_string();
        let book = ["xlsx", "xlsm"].iter().map(|e| about.with_extension(e)).find(|p| p.is_file());
        let opened = book.as_ref().ok_or_else(|| "its copy is missing".to_string()).and_then(|p| {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            io::open_bytes(&p.to_string_lossy(), &bytes).map_err(|e| e.to_string())
        });
        match opened {
            Ok((wb, _)) => {
                add_unsaved(app, wb, recovered_title(&title));
                titles.push(match meta["path"].as_str() {
                    Some(p) => format!("{title} (from {p})"),
                    None => title,
                });
                remove_files(about);
            }
            Err(e) => {
                // Left where it is, for the person to find: never deleted unread.
                log::error!("recovery: cannot open the copy of {title} at {}: {e}", about.display());
                failed.push(title);
            }
        }
    }
    let mut message = String::new();
    if !titles.is_empty() {
        message = format!("GridCraft closed with unsaved changes. Recovered, still unsaved:\n\n{}\n\nSave each one to keep it.", titles.join("\n"));
    }
    if !failed.is_empty() {
        message.push_str(&format!("\n\nThese could not be opened and were left in {}:\n{}", dir.display(), failed.join("\n")));
    }
    if !message.is_empty() {
        log::info!("recovery: {message}");
        app.session.ui_requests.push(UiRequest::Message(message.trim().to_string()));
        // Shown by the window's own message box, which picks requests up here.
        app.after_engine();
    }
}

fn recovered_title(title: &str) -> String {
    let mut stem = Path::new(title).file_stem().and_then(|s| s.to_str()).unwrap_or(title).trim_end();
    // A recovered workbook that was never saved comes back again with the title it already had:
    // it says "(recovered)" once, however many times it ends up here (measured: four).
    while let Some(rest) = stem.strip_suffix("(recovered)") {
        stem = rest.trim_end();
    }
    format!("{stem} (recovered)")
}

/// The copy as an open workbook: no file (saving asks where, and never overwrites the original
/// unasked), and unsaved, so it is kept again and quitting can't lose it twice.
fn add_unsaved(app: &mut SheetApp, wb: gridcraft_engine::model::Workbook, title: String) {
    let s = &mut app.session;
    // Replace an untouched blank Book1, as opening a file does.
    if s.documents().len() == 1
        && s.active().is_some_and(|d| d.path.is_none() && !d.is_dirty() && d.undo.is_empty() && d.wb.sheets.iter().all(|sh| sh.cells.is_empty()))
    {
        s.close_document(0);
    }
    // Read back from a file on disk: macros in it ask before they run, as a file opened does.
    let mut d = DocState::new(wb, None, title, gridcraft_engine::Trust::FromFile);
    d.saved = Arc::new((*d.wb).clone());
    s.add_document(d);
}

fn remove_files(about: &Path) {
    for ext in ["xlsx", "xlsm", "tmp", "json"] {
        let _ = std::fs::remove_file(about.with_extension(ext));
    }
}

/// Leftovers with no description (a write cut off between its two files): nothing can say what
/// they were, and the description is written last, so a workbook without one was never complete.
fn sweep(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for p in entries.flatten().map(|e| e.path()) {
            if p.extension().and_then(|e| e.to_str()) == Some("tmp") {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recovered_workbook_says_so() {
        assert_eq!(recovered_title("Budget.xlsx"), "Budget (recovered)");
        assert_eq!(recovered_title("Book1"), "Book1 (recovered)");
        assert_eq!(recovered_title("Gradebook (recovered)"), "Gradebook (recovered)", "once, not again");
        assert_eq!(recovered_title("Gradebook (recovered) (recovered) (recovered)"), "Gradebook (recovered)");
    }

    #[test]
    fn a_copy_round_trips_and_opens_unsaved() {
        let dir = std::env::temp_dir().join(format!("gc-recovery-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wb = gridcraft_engine::model::Workbook::new();
        write_copy(&dir, "run-1", &wb, "xlsx", &json!({"title": "Sales.xlsx", "path": "/x/Sales.xlsx"}));
        assert!(dir.join("run-1.xlsx").is_file() && dir.join("run-1.json").is_file());
        assert!(!dir.join("run-1.tmp").exists());
        remove_copy(&dir, "run-1");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "removing leaves nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
