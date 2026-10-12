//! The app as a clapp: one binary, two roles. `<cli> app` is what Clatch launches (it starts
//! the app's window and relays for it); `<cli> <group> …` is the agent's CLI.
//!
//! Deliberately not `windows_subsystem = "windows"`: this image is two roles, and a
//! GUI-subsystem process has no console for the CLI to print to.

mod app;
mod cli;
mod craft;
mod out;
mod policy;

pub use craft::{APP_ID, CLI};

fn main() {
    clappkit::role::main_dispatch(APP_ID, CLI, cli::run, app::run)
}
