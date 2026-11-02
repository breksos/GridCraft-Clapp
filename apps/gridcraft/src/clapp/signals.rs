//! What the person does, told to the agent (clappkit/docs/protocol.md § 7), so it keeps up
//! without polling. Only the person's actions: the agent knows its own, and hearing them back
//! would wake it in a loop.
//!
//! - `changed` (context): the commands the person ran, in order, sent once they pause, so a
//!   burst of typing becomes one notice instead of a queue full of them.
//! - `selected` (buffered): what the person has selected, the latest only, riding their next
//!   message. "Fix this block" means that selection.
//!
//! Telling the two apart needs no hook in the UI. Within a frame the agent's requests run in
//! `SheetApp::logic` (its control poll) and the person's clicks and keys in `ui`, so the app
//! calls [`Observer::observe`] before `logic` and [`Observer::settle`] after it: whatever changed
//! between a settle and the next observe was the person's. GridCraft journals every command,
//! whoever ran it (`Session::journal`); the selection is compared directly.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

/// How long the person pauses before their changes are told: one notice per burst.
const QUIET: Duration = Duration::from_millis(1200);
/// How long a selection rests before it is told: a drag reports where it ends, not each cell.
const SETTLE: Duration = Duration::from_millis(300);
/// The most commands one `changed` lists; the rest are counted.
const MAX_LISTED: usize = 50;
/// A command's parameters longer than this are cut: the agent reads real values with `read`.
const MAX_PARAMS: usize = 300;

/// Where the person is: the workbook and sheet in front, and the selection.
#[derive(Clone, PartialEq, Eq, Debug)]
struct View {
    workbook: String,
    sheet: String,
    selection: String,
    active: String,
}

pub struct Observer {
    control: Arc<OnceLock<clappkit::Control>>,
    /// Journal entries already accounted for.
    seen: usize,
    /// Where things stood at the last settle.
    view: Option<View>,
    /// The person's commands not yet told.
    pending: Vec<Json>,
    uncounted: usize,
    last_change: Option<Instant>,
    /// When the person last moved the selection, until it is told.
    moved: Option<Instant>,
    told: Option<View>,
    /// The first frame is where things start, not something the person did.
    started: bool,
}

impl Observer {
    pub fn new(control: Arc<OnceLock<clappkit::Control>>) -> Observer {
        Observer { control, seen: 0, view: None, pending: Vec::new(), uncounted: 0, last_change: None, moved: None, told: None, started: false }
    }

    /// Before the agent's requests run: whatever changed since [`Self::settle`] was the person's.
    pub fn observe(&mut self, app: &SheetApp) {
        if !self.started {
            self.started = true;
            self.settle(app);
            self.told = self.view.clone();
            return;
        }
        let journal = &app.session.journal;
        if journal.len() < self.seen {
            // The journal keeps its last 5,000 once it passes 10,000.
            self.seen = self.seen.saturating_sub(5_000).min(journal.len());
        }
        for (id, params) in journal.iter().skip(self.seen) {
            if moves_only(id) {
                continue;
            }
            if self.pending.len() < MAX_LISTED {
                self.pending.push(json!({"command": id, "params": brief(params)}));
            } else {
                self.uncounted += 1;
            }
            self.last_change = Some(Instant::now());
        }
        self.seen = journal.len();
        let now = view(app);
        if now != self.view {
            self.view = now;
            self.moved = Some(Instant::now());
        }
    }

    /// After the agent's requests ran: what they changed is the agent's own, never told.
    pub fn settle(&mut self, app: &SheetApp) {
        self.seen = app.session.journal.len();
        self.view = view(app);
    }

    /// Tell what is due, and wake the frame loop when the rest will be.
    pub fn tick(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        let Some(control) = self.control.get() else { return };
        if let Some(at) = self.last_change {
            let quiet = at.elapsed();
            if quiet >= QUIET {
                // The selection too: a command like `home.bold` acts on it and names no range.
                let (workbook, sheet, selection) =
                    self.view.as_ref().map(|v| (v.workbook.clone(), v.sheet.clone(), v.selection.clone())).unwrap_or_default();
                let mut payload = json!({"workbook": workbook, "sheet": sheet, "selection": selection, "changes": std::mem::take(&mut self.pending)});
                if self.uncounted > 0 {
                    payload["more"] = json!(self.uncounted);
                }
                control.emit("changed", Vec::new(), payload);
                self.uncounted = 0;
                self.last_change = None;
            } else {
                ctx.request_repaint_after(QUIET - quiet);
            }
        }
        if let Some(at) = self.moved {
            let rested = at.elapsed();
            if rested < SETTLE {
                ctx.request_repaint_after(SETTLE - rested);
            } else {
                self.moved = None;
                if self.view != self.told
                    && let Some(v) = self.view.clone()
                {
                    let cell = app.session.run("cell.get", json!({"cell": v.active})).unwrap_or(Json::Null);
                    control.emit(
                        "selected",
                        Vec::new(),
                        json!({
                            "workbook": v.workbook,
                            "sheet": v.sheet,
                            "selection": v.selection,
                            "activeCell": v.active,
                            "value": cell.get("text").cloned().unwrap_or(Json::Null),
                            "formula": cell.get("formula").cloned().unwrap_or(Json::Null),
                        }),
                    );
                    self.told = Some(v);
                }
            }
        }
    }
}

fn view(app: &SheetApp) -> Option<View> {
    let d = app.session.active()?;
    Some(View {
        workbook: d.display_title(),
        sheet: d.wb.active().map(|s| s.name.clone()).unwrap_or_default(),
        selection: d.selection.a1(),
        active: d.selection.active.a1(),
    })
}

/// Commands that only move where the person is; `selected` tells those.
fn moves_only(id: &str) -> bool {
    id.starts_with("selection.")
        || matches!(
            id,
            "sheet.activate"
                | "sheet.next"
                | "sheet.previous"
                | "window.activate"
                | "edit.goTo"
                | "edit.goToSpecial"
                | "edit.selectAll"
                | "edit.copy"
                | "edit.find"
                | "view.zoom"
                | "view.zoom100"
                | "view.zoomToSelection"
        )
}

/// A command's parameters, cut to a readable length (on a character boundary).
fn brief(params: &Json) -> Json {
    let text = params.to_string();
    if text.len() <= MAX_PARAMS {
        return params.clone();
    }
    let cut: String = text.chars().take(MAX_PARAMS).collect();
    json!(format!("{cut}..."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_is_not_changing() {
        assert!(moves_only("selection.set"));
        assert!(moves_only("sheet.activate"));
        assert!(!moves_only("cell.set"));
        assert!(!moves_only("home.bold"));
    }

    #[test]
    fn long_parameters_are_cut() {
        let long = json!({"values": vec!["x".repeat(50); 20]});
        let b = brief(&long);
        assert!(b.as_str().is_some_and(|s| s.ends_with("...") && s.chars().count() == MAX_PARAMS + 3));
        assert_eq!(brief(&json!({"cell": "B2"})), json!({"cell": "B2"}));
    }
}
