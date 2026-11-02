//! One background check at startup (and on Help ▸ Check for Updates…) and the modal prompt that
//! offers the new release: Update and Restart (direct self-update), Download or Later. The flow
//! itself is `craft_update::updater::Updater`.

use craft_update::CheckResultResponse;
use craft_update::updater::{Action, Status, Updater};

use crate::SheetApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The check at start was asked for; `tick` starts it with the context.
    startup: bool,
    /// Help ▸ Check for Updates… was chosen; the next `tick` starts it.
    manual: bool,
    /// The result dialog of a requested check is showing.
    result_open: bool,
    /// clapp: a launcher installs this copy's updates (see [`SheetApp::updates_come_from`]).
    host: Option<&'static str>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), startup: false, manual: false, result_open: false, host: None }
    }
}

impl SheetApp {
    /// clapp: `host` (a launcher such as Clatch) installs this copy and its updates. Updating in
    /// place would rewrite the launcher's copy behind its back, so GridCraft never checks for,
    /// offers or installs an update itself; Help ▸ Check for Updates… says where updates come from.
    pub fn updates_come_from(&mut self, host: &'static str) {
        self.updates_rt.host = Some(host);
        self.updates_rt.startup = false;
        self.updates_rt.manual = false;
    }
}

/// Ask for a check; the next frame's `tick` starts it. A manual one reports every outcome.
pub(crate) fn start(app: &mut SheetApp, manual: bool) {
    if let Some(host) = app.updates_rt.host {
        if manual {
            app.message = Some(("GridCraft".into(), format!("GridCraft's updates come from {host}: update it there.")));
        }
        return;
    }
    if manual {
        app.updates_rt.manual = true;
    } else {
        app.updates_rt.startup = true;
    }
}

pub(crate) fn tick(app: &mut SheetApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if rt.host.is_some() {
        return;
    }
    if std::mem::take(&mut rt.manual) {
        rt.updater.check_now(ctx);
        rt.result_open = true;
    } else if rt.startup {
        rt.updater.check_at_start(ctx);
    }
    rt.updater.tick(ctx);
    // A requested check that found a release leaves the status idle and the prompt opens.
    if matches!(rt.updater.status(), Status::Idle) {
        rt.result_open = false;
    }
}

pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    if app.updates_rt.host.is_some() {
        return;
    }
    // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => {
            if let Some(open) = &app.services.open_url {
                open(&url);
            }
        }
        // The guarded close path asks about unsaved work; the restart helper relaunches once the
        // process has exited.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Action::None => {}
    }
    if app.updates_rt.result_open {
        let rt = &mut app.updates_rt;
        match craft_update::CheckResultDialog::new(&crate::UPDATE_APP, VERSION).show(ctx, rt.updater.status()) {
            CheckResultResponse::Close => {
                rt.updater.dismiss_status();
                rt.result_open = false;
            }
            CheckResultResponse::CheckAgain => rt.updater.check_now(ctx),
            CheckResultResponse::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::Session;

    use crate::{Services, SheetApp};

    /// clapp: a launcher-installed copy never starts a check, and Check for Updates says where
    /// updates come from.
    #[test]
    fn a_host_installed_copy_never_checks_for_updates() {
        let mut app = SheetApp::new(Session::new(), Services::default());
        app.start_update_check();
        app.updates_come_from("Clatch");
        assert!(!app.updates_rt.startup, "no startup check");
        super::start(&mut app, true);
        assert!(!app.updates_rt.manual && !app.updates_rt.result_open, "no check, no dialog");
        assert!(app.message.as_ref().is_some_and(|(_, m)| m.contains("come from Clatch")), "{:?}", app.message);
    }
}
