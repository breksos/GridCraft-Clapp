//! Everything in this clapp that is GridCraft's own: its names, how its window is started and
//! signed in to, which of its commands the agent reaches, and how its files are handled. The
//! DeckCraft, PhotoCraft and WordCraft clapps hold this same file with their app's answers; the
//! rest of `clapp/src` is the same in all of them.
//!
//! Each file rule below was measured against the running app, not read off its docs: closing the
//! window (its close button, Quit, or `app.quit`) throws away unsaved changes without asking;
//! the engine's `file.close` asks with its own Save Changes dialog unless `force` is set, which
//! throws them away; a save with no path on a workbook that has no file yet opens GridCraft's Save
//! As dialog and answers as if it had saved; and only .xlsx (and .json) become the workbook's file,
//! while a save to .csv, .tsv, .html or .pdf writes a copy.

use crate::app::{Link, Secret};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "com.arfium.gridcraft";
pub const CLI: &str = "gridcraft";
pub const NAME: &str = "GridCraft";
/// What the app holds open, in the words answers use.
pub const DOCUMENT: &str = "workbook";
/// A development build of the window, used instead of the depot's.
pub const GUI_ENV: &str = "GRIDCRAFT_CLAPP_GUI";
/// The window inside a depot, from its root.
pub const BUNDLE_EXE: &str = "GridCraft.app/Contents/MacOS/GridCraft";

/// GridCraft's settings folder, where the window writes its control key (`crates/control-key`).
fn settings_dir() -> Option<PathBuf> {
    let home = clappkit::paths::home();
    if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".config"))).map(|c| c.join("gridcraft"))
    }
}

fn key_file(port: u16) -> anyhow::Result<PathBuf> {
    let dir = settings_dir().ok_or_else(|| anyhow::anyhow!("no settings folder for GridCraft's control key"))?;
    Ok(dir.join(format!("control-key.local-{port}")))
}

/// The window's command line and the secret its control port will want: GridCraft makes its own
/// key once its port is bound (this fork's port of DeckCraft's) and writes it owner-only. A key
/// left by a window that did not exit cleanly is removed first, so the one read is this window's.
pub fn launch(port: u16, _data: &Path) -> anyhow::Result<(Vec<OsString>, Secret)> {
    let key = key_file(port)?;
    let _ = std::fs::remove_file(&key);
    Ok((vec!["--control".into(), port.to_string().into()], Secret::File(key)))
}

pub fn auth_frame(secret: &str) -> Value {
    json!({"method": "auth", "key": secret})
}

/// After the window has gone: it removes its key itself, unless it was killed.
pub fn cleanup(port: u16, _data: &Path) {
    if let Ok(key) = key_file(port) {
        let _ = std::fs::remove_file(key);
    }
}

/// The methods `call` forwards: the reads the groups make and the window's own input.
/// `ui.menu.invoke` and `command` are not among them: in GridCraft both are `engine.execute`
/// under another name. `ui.set` is not either: it changes the person's preferences.
pub const FORWARD_METHODS: &[&str] = &[
    "engine.execute",
    "engine.commands",
    "document.inspect",
    "ui.inspect",
    "ui.key",
    "ui.text",
    "ui.move",
    "ui.click",
    "ui.drag",
    "ui.cell.click",
    "ui.cell.drag",
    "ui.ribbon",
    "ui.dialog",
    "ui.dialog.cancel",
    "ui.message.dismiss",
    "ui.edit.begin",
    "ui.edit.text",
    "ui.edit.commit",
    "ui.edit.cancel",
    "ui.resize",
    "ui.focus",
];

/// The window's input methods `ui call` may send. A subset of [`FORWARD_METHODS`].
/// `ui.dialog.confirm` is not one: it runs the dialog's command past `run`'s rules.
pub const UI_METHODS: &[&str] = &[
    "ui.move",
    "ui.drag",
    "ui.cell.click",
    "ui.cell.drag",
    "ui.ribbon",
    "ui.dialog",
    "ui.dialog.cancel",
    "ui.message.dismiss",
    "ui.edit.begin",
    "ui.edit.text",
    "ui.edit.commit",
    "ui.edit.cancel",
    "ui.resize",
];

/// Commands never sent through this clapp, by `run` or by anyone else. `.` ends a prefix.
pub const NEVER: &[(&str, &str)] = &[
    (
        "file.",
        "workbooks are opened, saved and closed with `gridcraft file`, which never closes one with unsaved \
         changes (the engine's own `file.close` does with `force`) and never leaves a Save As dialog waiting",
    ),
    ("app.", "the app's own dialogs, preferences and language are the person's"),
    ("automate.", "a script runs saved commands past these rules, files among them"),
    ("ui.printSubmit", "printing puts paper through the person's printer; `gridcraft file export <path.pdf>` writes the same pages"),
];

/// The namespaces `run` reaches: commands that change the open workbook and nothing else.
pub const RUN_NAMESPACES: &[&str] = &[
    "arrange", "cell", "chart", "data", "document", "draw", "edit", "formulas", "history", "home", "insert", "object",
    "pageLayout", "pivot", "range", "review", "selection", "shape", "sheet", "slicer", "table", "view", "window",
];

/// Inside those namespaces, what `run` still refuses.
pub const RUN_REFUSED: &[(&str, &str)] = &[
    ("edit.clearClipboard", "the clipboard is the person's"),
    ("review.addToDictionary", "the spelling dictionary is the person's"),
    ("review.protectSheet", "locking the sheet's editing is the person's call"),
    ("review.protectWorkbook", "locking the workbook's structure is the person's call"),
    ("review.allowEditRanges", "who may edit which range is the person's call"),
    ("view.formulaBar", "showing the formula bar is the person's preference"),
    ("view.ruler", "the ruler is an application option, the person's"),
    ("view.theme", "the display theme is the person's preference"),
    ("view.darkMode", "the display theme is the person's preference"),
    ("view.animations", "animations are the person's preference"),
    ("view.interfaceScale", "the interface size is the person's preference"),
    ("view.collapseRibbon", "the ribbon is the person's to fold"),
];

/// Parameters that name a file on disk: (command, parameter, only when the value looks like a
/// path). With the file's contents instead (`base64`, `text`) the same commands work.
pub const DISK_PARAMS: &[(&str, &str, bool)] = &[
    ("insert.picture", "path", false),
    ("data.fromTextCsv", "path", false),
    ("data.getData", "path", false),
    ("pageLayout.background", "path", false),
];

pub const DISK_HINT: &str = "A picture comes into the workbook with `gridcraft file insert <path>`; insert.picture also \
                             takes its bytes as `base64`, and data.fromTextCsv and data.getData take the file's \
                             contents as `text`.";

/// What `doc view` asks for.
pub fn doc_view() -> (&'static str, Value) {
    ("document.inspect", json!({}))
}

/// The parts of `ui.inspect` `window view` prints; `--json` has the rest.
pub const WINDOW_KEYS: &[&str] = &["documents", "activeDocument", "mode", "editor", "dialog", "backstage", "message", "window"];

pub fn click(x: f64, y: f64, button: &str, count: u64) -> (&'static str, Value) {
    ("ui.click", json!({"x": x, "y": y, "button": button, "count": count}))
}

/// GridCraft takes Command and Control apart; off the Mac, `ctrl` is the shortcut key too.
pub fn key(key: &str, cmd: bool, shift: bool, alt: bool, ctrl: bool) -> (&'static str, Value) {
    ("ui.key", json!({"key": key, "cmd": cmd, "shift": shift, "alt": alt, "ctrl": ctrl}))
}

pub fn typing(text: &str) -> (&'static str, Value) {
    ("ui.text", json!({"text": text}))
}

/// `file insert` is offered: PNG and JPEG pictures.
pub const OFFERS_INSERT: bool = true;

/// The help's notes that are GridCraft's alone.
pub const HELP_NOTES: &str = "\
Several workbooks can be open: `file new` and `file open` add one, `run window.activate
'{\"index\": <n>}'` moves between them (from 0), and everything else acts on the one in front.
Cells are A1 references: `run cell.set '{\"cell\": \"B2\", \"input\": \"=SUM(A1:A9)\"}'` enters one,
`run range.setValues` a block, `run sheet.read '{\"range\": \"A1:D10\"}'` reads values back.
`doc render --page <n>` is a picture of the window showing the n-th sheet, from 1 (the sheet in
front by default, and put back afterwards); `--scale` is not used.
`ui click` takes window points; `ui call ui.cell.click '{\"cell\": \"B2\"}'` clicks a cell.
`file save <path>` takes .xlsx (or .json) and makes that the workbook's file; `file export`
writes a copy as .pdf, .csv, .tsv or .html. `file insert` takes a PNG or JPEG picture.";

pub struct Open {
    pub name: String,
    pub dirty: bool,
    pub has_file: bool,
}

/// The workbook in front, if any.
async fn active(link: &Link) -> Result<Option<Open>, String> {
    let ui = link.call("ui.inspect", json!({})).await?;
    if ui["documents"].as_array().is_none_or(|d| d.is_empty()) {
        return Ok(None);
    }
    let doc = link.call("document.inspect", json!({})).await?;
    Ok(Some(Open {
        name: doc["title"].as_str().unwrap_or("the workbook").to_string(),
        dirty: doc["dirty"].as_bool().unwrap_or(true),
        has_file: doc["path"].as_str().is_some_and(|p| !p.is_empty()),
    }))
}

/// `doc render`: GridCraft has no off-screen picture of a sheet, so this is the window showing
/// it. `--page` picks the sheet (from 1, the app from 0); the sheet in front is put back after.
/// Switching sheets changes neither the workbook's unsaved state nor its undo history (measured).
pub async fn render(link: &Link, file: &str, page: Option<u64>, _scale: Option<f64>) -> Result<Value, String> {
    let mut back = None;
    if let Some(n) = page {
        let doc = link.call("document.inspect", json!({})).await?;
        let count = doc["sheets"].as_array().map_or(0, Vec::len) as u64;
        if n == 0 || n > count {
            return Err(format!("the workbook has {count} sheets; --page counts them from 1"));
        }
        let front = doc["activeSheet"].as_u64().unwrap_or(0);
        if front != n - 1 {
            link.exec("sheet.activate", json!({"sheet": n - 1})).await?;
            back = Some(front);
        }
    }
    let shot = link.call("ui.screenshot", json!({"path": file})).await;
    if let Some(front) = back {
        let _ = link.exec("sheet.activate", json!({"sheet": front})).await;
    }
    shot
}

/// `file …`, as the launcher carries it out.
pub async fn file(link: &Link, req: &Value) -> Result<Value, String> {
    let verb = req["verb"].as_str().unwrap_or("");
    let params = if req["params"].is_object() { req["params"].clone() } else { json!({}) };
    let path = || req["path"].as_str().filter(|p| !p.is_empty()).ok_or_else(|| format!("`file {verb}` needs a path"));
    match verb {
        "new" => link.exec("file.new", params).await,
        "open" => link.call("app.open", json!({"path": path()?})).await,
        "save" => match req["path"].as_str() {
            Some(p) => {
                // Only these become the workbook's file; anything else would be a copy that leaves
                // it unsaved while answering as if it were saved.
                if !matches!(extension(p).as_str(), "xlsx" | "xlsm" | "json") {
                    return Err(format!("`file save` takes .xlsx (or .json); a .{} copy is `gridcraft file export <path>`", extension(p)));
                }
                link.call("app.save", json!({"path": p})).await
            }
            None => {
                let Some(open) = active(link).await? else { return Err("no workbook is open".into()) };
                if !open.has_file {
                    // Without a file the app opens its Save As dialog and answers as if it saved.
                    return Err(format!("`{}` has no file yet: `gridcraft file save <path.xlsx>`", open.name));
                }
                link.call("app.save", json!({})).await
            }
        },
        "export" => {
            let p = path()?;
            let mut params = params;
            params["path"] = json!(p);
            match extension(p).as_str() {
                "pdf" => link.exec("file.exportPdf", params).await,
                "csv" => link.exec("file.exportCsv", params).await,
                "html" | "htm" => link.exec("file.exportHtml", params).await,
                // A .tsv save is a copy: the workbook keeps its own file and unsaved state (measured).
                "tsv" => link.exec("file.save", json!({"path": p})).await,
                other => Err(format!("`file export` writes .pdf, .csv, .tsv or .html, not .{other}; .xlsx is `file save`")),
            }
        }
        "close" => {
            let Some(open) = active(link).await? else { return Err("no workbook is open".into()) };
            if open.dirty {
                return Err(format!(
                    "`{}` has unsaved changes: save it with `gridcraft file save [<path>]` first, or leave closing it to the person",
                    open.name
                ));
            }
            link.exec("file.close", json!({})).await.map(|_| json!({"closed": open.name}))
        }
        "insert" => {
            let p = path()?;
            let name = req["name"].as_str().unwrap_or(p);
            if !matches!(extension(name).as_str(), "png" | "jpg" | "jpeg") {
                return Err(format!("`file insert` takes a PNG or JPEG picture, not .{}", extension(name)));
            }
            let mut params = params;
            params["path"] = json!(p);
            if params.get("alt").is_none() {
                params["alt"] = json!(name);
            }
            link.exec("insert.picture", params).await
        }
        other => Err(format!("unknown file verb `{other}`")),
    }
}

fn extension(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use crate::policy;
    use serde_json::json;

    /// The rules measured against the running app, kept.
    #[test]
    fn what_run_reaches() {
        assert!(policy::run("cell.set", &json!({"cell": "B2", "input": "=SUM(A1:A9)"})).is_ok());
        assert!(policy::run("home.bold", &json!({"range": "A1:C1"})).is_ok());
        assert!(policy::run("insert.chart", &json!({"range": "A1:B5"})).is_ok());
        assert!(policy::run("file.close", &json!({"force": true})).is_err(), "force throws away unsaved work");
        assert!(policy::run("file.saveAs", &json!({"path": "/x/b.xlsx"})).is_err());
        assert!(policy::run("app.language.set", &json!({"language": "ja"})).is_err());
        assert!(policy::run("automate.runScript", &json!({"name": "x"})).is_err(), "a script runs file commands too");
        assert!(policy::run("view.theme", &json!({"mode": "dark"})).is_err(), "the person's preference");
        assert!(policy::run("view.zoom", &json!({"percent": 150})).is_ok(), "zoom is saved with the sheet");
        assert!(policy::run("insert.picture", &json!({"path": "/x/logo.png"})).is_err());
        assert!(policy::run("insert.picture", &json!({"base64": "AAAA"})).is_ok(), "bytes are not a path");
        assert!(policy::run("data.getData", &json!({"text": "a,b\n1,2"})).is_ok());
        assert!(policy::run("ui.printSubmit", &json!({})).is_err());
        assert!(policy::forward("ui.menu.invoke", &json!({"command": "file.close"})).is_err(), "the engine's path here");
        assert!(policy::forward("ui.set", &json!({"dark": true})).is_err(), "the person's preferences");
        assert!(policy::forward("engine.execute", &json!({"command": "ui.printSubmit"})).is_err());
    }
}
