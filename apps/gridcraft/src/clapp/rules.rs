//! What an agent may run through `gridcraft run`. Held in the window (the door checks it), so a
//! program talking to the socket directly is held to it too.
//!
//! Measured against the running app: closing the window throws away unsaved changes without
//! asking; `file.close {force}` does the same to one workbook; a save with no path on a workbook
//! that has no file opens the Save As dialog and answers as if it had saved; only .xlsx (and
//! .json) become the workbook's file, while .csv, .tsv, .html and .pdf are copies. Files are
//! therefore the file verbs' alone, and they refuse the cases above.
//!
//! The namespace list is an allow-list: a namespace upstream adds tomorrow stays out until
//! someone has read what is in it.

use serde_json::Value as Json;

/// Never run for an agent, by any verb. `.` ends a prefix.
pub const NEVER: &[(&str, &str)] = &[
    (
        "file.",
        "files are `gridcraft open`, `save`, `export` and `new`, which never throw away unsaved work \
         and never leave a Save As dialog waiting",
    ),
    ("app.", "the app's own dialogs, preferences and language are the person's"),
    ("automate.", "a saved script runs commands past these rules, files among them"),
    ("ui.", "the window's own controls are the person's"),
];

/// The namespaces `run` reaches: commands that change the open workbook and nothing else.
pub const NAMESPACES: &[&str] = &[
    "arrange",
    "cell",
    "chart",
    "data",
    "document",
    "draw",
    "edit",
    "formulas",
    "history",
    "home",
    "insert",
    "object",
    "pageLayout",
    "pivot",
    "range",
    "review",
    "selection",
    "shape",
    "sheet",
    "slicer",
    "table",
    "view",
    "window",
];

/// Inside those namespaces, what `run` still refuses.
pub const REFUSED: &[(&str, &str)] = &[
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

/// Parameters that name a file on disk. The same commands take the file's contents instead.
pub const DISK_PARAMS: &[(&str, &str)] =
    &[("insert.picture", "path"), ("data.fromTextCsv", "path"), ("data.getData", "path"), ("pageLayout.background", "path")];

const DISK_HINT: &str = "A picture comes in with `gridcraft insert <path>`; insert.picture also takes its bytes as \
                         `base64`, and data.fromTextCsv and data.getData take the file's contents as `text`.";

/// `run`'s rule: the command changes the open workbook and nothing outside it.
pub fn run(id: &str, params: &Json) -> Result<(), String> {
    if let Some(why) = listed(NEVER, id) {
        return Err(format!("`{id}` is not run here: {why}"));
    }
    let namespace = id.split('.').next().unwrap_or("");
    if !NAMESPACES.contains(&namespace) {
        return Err(format!("`{id}` is outside what `run` reaches: it changes the open workbook only"));
    }
    if let Some(why) = listed(REFUSED, id) {
        return Err(format!("`{id}` is not run here: {why}"));
    }
    for (command, param) in DISK_PARAMS {
        if *command == id && params.get(*param).is_some() {
            return Err(format!("`{param}` names a file on disk, which `run` never reaches. {DISK_HINT}"));
        }
    }
    Ok(())
}

/// Whether `run` sends `id` at all, parameters aside: for `commands`.
pub fn runnable(id: &str) -> bool {
    run(id, &Json::Null).is_ok()
}

/// The extensions that become the workbook's own file when saved.
pub fn becomes_the_file(path: &str) -> bool {
    matches!(extension(path).as_str(), "xlsx" | "xlsm" | "json")
}

pub fn extension(path: &str) -> String {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn listed(table: &'static [(&'static str, &'static str)], id: &str) -> Option<&'static str> {
    table.iter().find(|(entry, _)| if entry.ends_with('.') { id.starts_with(entry) } else { id == *entry }).map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_run_reaches() {
        assert!(run("cell.set", &json!({"cell": "B2", "input": "=SUM(A1:A9)"})).is_ok());
        assert!(run("home.bold", &json!({"range": "A1:C1"})).is_ok());
        assert!(run("view.zoom", &json!({"percent": 150})).is_ok(), "zoom is saved with the sheet");
        assert!(run("file.close", &json!({"force": true})).is_err(), "force throws away unsaved work");
        assert!(run("app.language.set", &json!({"language": "ja"})).is_err());
        assert!(run("automate.runScript", &json!({"name": "x"})).is_err());
        assert!(run("ui.printSubmit", &json!({})).is_err());
        assert!(run("view.theme", &json!({"mode": "dark"})).is_err());
        assert!(run("brandNew.thing", &json!({})).is_err(), "an unread namespace stays out");
        assert!(run("insert.picture", &json!({"path": "/x/logo.png"})).is_err());
        assert!(run("insert.picture", &json!({"base64": "AAAA"})).is_ok(), "bytes are not a path");
    }

    #[test]
    fn only_full_fidelity_formats_become_the_file() {
        assert!(becomes_the_file("/x/Book.XLSX"));
        assert!(becomes_the_file("b.json"));
        assert!(!becomes_the_file("b.csv"));
        assert!(!becomes_the_file("b.pdf"));
    }
}
