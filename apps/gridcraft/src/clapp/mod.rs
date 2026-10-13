//! clapp: GridCraft as a Clatch app (clappkit/docs/architecture.md). One binary, two roles over
//! one state: `gridcraft app` is this window, which Clatch launches; `gridcraft <verb>` is the
//! agent's CLI, which reaches the running window over clappkit's private socket and drives the
//! same `Session` the ribbon does. Everything Clatch-specific lives in this module; the rest of
//! the app is upstream's.
//!
//! - [`cli`]: the verbs and the help, which is the agent's only manual.
//! - [`door`]: the window's side of the socket: each verb, as GridCraft's own control methods.
//! - [`rules`]: what an agent may never do here (lose unsaved work, change preferences, ...).
//! - [`recovery`]: unsaved work kept on disk, and opened again after the window ends.
//! - [`signals`]: what the person does, told to the agent (`changed`, `selected`).
//! - [`wiring`]: the control pipe and the socket, on a background runtime beside eframe.

// The desktop app never runs on wasm, so this module may read the clock (clippy.toml bans
// `Instant::now` only because it traps there).
#![allow(clippy::disallowed_methods)]

pub mod cli;
pub mod door;
pub mod recovery;
pub mod rules;
pub mod signals;
pub mod wiring;

/// The app's id: `clatch.json`'s `id`.
pub const APP_ID: &str = "com.arfium.gridcraft";
/// The agent's shorthand: `clatch.json`'s `connector.cli`, and the socket's name.
pub const CLI: &str = "gridcraft";

/// What this invocation is.
#[derive(Debug, PartialEq, Eq)]
pub enum Role {
    /// The window, with these of upstream's own arguments (files, `--sample`, `--control`).
    Gui(Vec<String>),
    /// The agent's CLI: a verb and its arguments.
    Cli(Vec<String>),
    /// `--version`: printed, no window, no launcher.
    Version,
}

/// `app` or nothing is the window Clatch launches; a declared verb (or `help`) is the agent's
/// CLI. Anything else is upstream's own command line (a file, `--sample`), so a workbook opened
/// from the desktop still opens a window.
pub fn role(args: &[String]) -> Role {
    match args.first().map(String::as_str) {
        None => Role::Gui(Vec::new()),
        Some("app") => Role::Gui(args.iter().skip(1).cloned().collect()),
        Some("--version" | "-V") => Role::Version,
        Some(verb) if cli::VERBS.contains(&verb) || matches!(verb, "help" | "-h" | "--help") => Role::Cli(args.to_vec()),
        Some(_) => Role::Gui(args.to_vec()),
    }
}

/// Run only under Clatch (clappkit/docs/protocol.md § 1): `true` when Clatch was asked to
/// relaunch the installed copy and this process must end now. `CLATCH_STANDALONE=1` runs the
/// window without a launcher, for development.
pub fn must_relaunch() -> bool {
    match clappkit::bootstrap(APP_ID) {
        Ok(relaunch) => relaunch,
        Err(e) => {
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{CLI}: {e}\n"));
            std::process::exit(1);
        }
    }
}

/// Where the window keeps its settings and logs under Clatch: the app's data directory, which
/// uninstalling can erase whole. `None` outside Clatch, where upstream's own folder stays.
pub fn settings_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("CLATCH_DATA_DIR").filter(|d| !d.is_empty()).map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_launcher_and_the_desktop_open_the_window() {
        assert_eq!(role(&[]), Role::Gui(vec![]));
        assert_eq!(role(&args(&["app"])), Role::Gui(vec![]));
        assert_eq!(role(&args(&["app", "--sample", "sales"])), Role::Gui(args(&["--sample", "sales"])));
        assert_eq!(role(&args(&["book.xlsx"])), Role::Gui(args(&["book.xlsx"])));
        assert_eq!(role(&args(&["--sample", "budget"])), Role::Gui(args(&["--sample", "budget"])));
    }

    #[test]
    fn every_verb_and_help_is_the_cli() {
        for v in cli::VERBS {
            assert_eq!(role(&args(&[v, "x"])), Role::Cli(args(&[v, "x"])), "{v}");
        }
        assert_eq!(role(&args(&["--help"])), Role::Cli(args(&["--help"])));
        assert_eq!(role(&args(&["-V"])), Role::Version);
    }
}
