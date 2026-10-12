//! The window's side of the agent's socket: one verb per request (`{"verb": …}`), carried out
//! with GridCraft's own control methods through the [`Bridge`]. The rules live here rather than
//! in the CLI, so a program that talks to the socket directly is held to them as well.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value as Json, json};

use super::CLI;
use super::rules;
use super::wiring::Bridge;

/// An answer bigger than this travels as a file: clappkit's socket takes 1 MiB at most.
const SPILL: usize = 512 << 10;

#[derive(Clone)]
pub struct Door {
    bridge: Bridge,
    spill_dir: Arc<PathBuf>,
    next: Arc<AtomicU64>,
}

impl Door {
    pub fn new(bridge: Bridge) -> Door {
        // Answers too big for the socket wait here for one request; an earlier run's are swept.
        let spill_dir = clappkit::paths::data_subdir(CLI, "exchange");
        if let Ok(entries) = std::fs::read_dir(&spill_dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        Door { bridge, spill_dir: Arc::new(spill_dir), next: Arc::new(AtomicU64::new(1)) }
    }

    pub async fn answer(&self, req: Json) -> Json {
        let verb = req.get("verb").and_then(Json::as_str).unwrap_or("");
        let s = |k: &str| req.get(k).and_then(Json::as_str).map(str::to_string);
        let result = match verb {
            "state" => self.state().await,
            "read" => self.bridge.call("sheet.read", pick(&req, &["range", "sheet", "formulas", "formatted"])).await,
            "set" => self.bridge.exec("cell.set", pick(&req, &["cell", "input", "sheet"])).await,
            "write" => self.bridge.exec("range.setValues", pick(&req, &["range", "values"])).await,
            "run" => self.run(&req).await,
            "commands" => self.commands(s("search")).await,
            "shot" => self.shot(s("path"), req.get("sheet").and_then(Json::as_u64)).await,
            "new" => self.bridge.exec("file.new", pick(&req, &["sample"])).await,
            "open" => self.bridge.call("app.open", pick(&req, &["path"])).await,
            "save" => self.save(s("path")).await,
            "export" => self.export(s("path")).await,
            "insert" => self.insert(&req).await,
            "focus" => self.bridge.call("ui.focus", json!({})).await.map(|_| json!({"focused": true})),
            "close" => self.close().await,
            "" => Err("a request names its `verb`".into()),
            other => Err(format!("unknown verb `{other}`")),
        };
        match result {
            Ok(v) => self.deliver(v),
            Err(e) => json!({"ok": false, "error": e}),
        }
    }

    /// The open workbooks, the one in front, its sheets and the selection: what an agent needs
    /// to know before it reads anything.
    async fn state(&self) -> Result<Json, String> {
        let ui = self.bridge.call("ui.inspect", json!({})).await?;
        let mut out = json!({
            "workbooks": ui["documents"],
            "mode": ui["mode"],
            "editing": ui["editor"],
            "dialog": ui["dialog"]["name"],
        });
        if ui["documents"].as_array().is_some_and(|d| !d.is_empty()) {
            let doc = self.bridge.call("document.inspect", json!({})).await?;
            let sheets = doc["sheets"].as_array().cloned().unwrap_or_default();
            let front = doc["activeSheet"].as_u64().unwrap_or(0) as usize;
            out["workbook"] = json!({
                "title": doc["title"],
                "path": doc["path"],
                "dirty": doc["dirty"],
                "sheets": sheets.iter().map(|s| s["name"].clone()).collect::<Vec<_>>(),
                "sheet": sheets.get(front).map(|s| s["name"].clone()),
                "usedRange": sheets.get(front).map(|s| s["usedRange"].clone()),
                "selection": doc["selection"],
                "activeCell": doc["activeCell"],
                "tables": sheets.get(front).map(|s| s["tables"].clone()),
                "charts": sheets.get(front).map(|s| s["charts"].clone()),
            });
        }
        Ok(out)
    }

    async fn run(&self, req: &Json) -> Result<Json, String> {
        let id = req.get("command").and_then(Json::as_str).ok_or("`run` names a command")?;
        let params = match req.get("params") {
            None | Some(Json::Null) => json!({}),
            Some(p) if p.is_object() => p.clone(),
            Some(_) => return Err("a command's parameters are a JSON object".into()),
        };
        rules::run(id, &params)?;
        self.bridge.exec(id, params).await
    }

    async fn commands(&self, search: Option<String>) -> Result<Json, String> {
        let all = self.bridge.call("engine.commands", json!({"search": search})).await?;
        let rows = all.as_array().cloned().unwrap_or_default();
        Ok(Json::Array(
            rows.into_iter()
                .map(|r| {
                    let id = r["id"].as_str().unwrap_or("").to_string();
                    json!({"id": id, "label": r["label"], "params": r["params"], "enabled": r["enabled"], "run": rules::runnable(&id)})
                })
                .collect(),
        ))
    }

    /// A picture of the window. GridCraft has no off-screen picture of a sheet, so `sheet`
    /// (from 1) is shown in the window for the picture and the sheet in front put back after.
    /// Switching sheets touches neither the unsaved state nor the undo history (measured).
    async fn shot(&self, path: Option<String>, sheet: Option<u64>) -> Result<Json, String> {
        let path = path.ok_or("`shot` writes to a path")?;
        let mut back = None;
        if let Some(n) = sheet {
            let doc = self.bridge.call("document.inspect", json!({})).await?;
            let count = doc["sheets"].as_array().map_or(0, Vec::len) as u64;
            if n == 0 || n > count {
                return Err(format!("the workbook has {count} sheets; --sheet counts them from 1"));
            }
            let front = doc["activeSheet"].as_u64().unwrap_or(0);
            if front != n - 1 {
                self.bridge.exec("sheet.activate", json!({"sheet": n - 1})).await?;
                back = Some(front);
            }
        }
        let shot = self.bridge.call("ui.screenshot", json!({"path": path})).await;
        if let Some(front) = back {
            let _ = self.bridge.exec("sheet.activate", json!({"sheet": front})).await;
        }
        shot
    }

    async fn save(&self, path: Option<String>) -> Result<Json, String> {
        if let Some(p) = path {
            if !rules::becomes_the_file(&p) {
                return Err(format!(
                    "`save` takes .xlsx (or .json), which becomes the workbook's file; a .{} copy is `{CLI} export <path>`",
                    rules::extension(&p)
                ));
            }
            return self.bridge.call("app.save", json!({"path": p})).await;
        }
        let doc = self.bridge.call("document.inspect", json!({})).await?;
        if doc["path"].as_str().is_none_or(str::is_empty) {
            // Without a file the app opens its Save As dialog and answers as if it had saved.
            let title = doc["title"].as_str().unwrap_or("the workbook");
            return Err(format!("`{title}` has no file yet: `{CLI} save <path.xlsx>`"));
        }
        self.bridge.call("app.save", json!({})).await
    }

    async fn export(&self, path: Option<String>) -> Result<Json, String> {
        let p = path.ok_or("`export` writes to a path")?;
        match rules::extension(&p).as_str() {
            "pdf" => self.bridge.exec("file.exportPdf", json!({"path": p})).await,
            "csv" => self.bridge.exec("file.exportCsv", json!({"path": p})).await,
            "html" | "htm" => self.bridge.exec("file.exportHtml", json!({"path": p})).await,
            // A .tsv save is a copy: the workbook keeps its own file and unsaved state (measured).
            "tsv" => self.bridge.exec("file.save", json!({"path": p})).await,
            other => Err(format!("`export` writes .pdf, .csv, .tsv or .html, not .{other}; .xlsx is `{CLI} save <path>`")),
        }
    }

    async fn insert(&self, req: &Json) -> Result<Json, String> {
        let path = req.get("path").and_then(Json::as_str).ok_or("`insert` names a picture")?;
        if !matches!(rules::extension(path).as_str(), "png" | "jpg" | "jpeg") {
            return Err(format!("`insert` takes a PNG or JPEG picture, not .{}", rules::extension(path)));
        }
        self.bridge.exec("insert.picture", pick(req, &["path", "at"])).await
    }

    /// Quit GridCraft, which closes without asking: so only while nothing is unsaved.
    async fn close(&self) -> Result<Json, String> {
        let ui = self.bridge.call("ui.inspect", json!({})).await?;
        if !ui["editor"].is_null() {
            return Err("the person is editing a cell: leave closing to them".into());
        }
        let unsaved: Vec<String> = ui["documents"]
            .as_array()
            .map(|d| d.iter().filter(|w| w["dirty"].as_bool() == Some(true)).filter_map(|w| w["title"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if !unsaved.is_empty() {
            return Err(format!(
                "unsaved changes in {}: GridCraft closes without asking, so save first (`{CLI} save`) or leave closing to the person",
                unsaved.join(", ")
            ));
        }
        self.bridge.call("app.quit", json!({})).await.map(|_| json!({"closed": true}))
    }

    /// The answer, or a file holding it when it is too big for the socket.
    fn deliver(&self, result: Json) -> Json {
        let body = json!({"ok": true, "result": result});
        let text = body.to_string();
        if text.len() <= SPILL {
            return body;
        }
        let file = self.spill_dir.join(format!("{}.json", self.next.fetch_add(1, Ordering::Relaxed)));
        match std::fs::write(&file, text) {
            Ok(()) => json!({"ok": true, "spill": file}),
            Err(e) => json!({"ok": false, "error": format!("an answer too big to send, and {} cannot be written: {e}", file.display())}),
        }
    }
}

/// The named fields of a request that are present, as a command's parameters.
fn pick(req: &Json, keys: &[&str]) -> Json {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = req.get(*k).filter(|v| !v.is_null()) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    Json::Object(out)
}

#[cfg(test)]
mod tests {
    use super::pick;
    use serde_json::json;

    #[test]
    fn only_present_fields_become_parameters() {
        let req = json!({"verb": "read", "range": "A1:B2", "sheet": null, "formulas": true});
        assert_eq!(pick(&req, &["range", "sheet", "formulas", "formatted"]), json!({"range": "A1:B2", "formulas": true}));
    }
}
