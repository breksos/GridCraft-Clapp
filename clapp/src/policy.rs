//! What the agent may reach. The tables are the app's own (`craft.rs`); the rules that read
//! them are the same in every Craft clapp.
//!
//! Two rules, two places. [`forward`] is the launcher's: it holds for anything that reaches the
//! window, so a program talking to the socket directly is held to it too. [`run`] is the `run`
//! group's, and it is a grant boundary: a command that leaves the open document — a file, a
//! preference, the undo history the person shares — is not `run`'s to send, whatever an agent
//! granted `run` types. Its namespace list is an allow-list on purpose: a namespace upstream
//! adds tomorrow stays out until someone has read what is in it.

use crate::craft;
use serde_json::Value;

/// The launcher's rule for a forwarded call.
pub fn forward(method: &str, params: &Value) -> Result<(), String> {
    if !craft::FORWARD_METHODS.contains(&method) {
        return Err(format!("`{method}` is not something this clapp sends to the window"));
    }
    if let Some(id) = command_of(method, params) {
        if let Some(why) = listed(craft::NEVER, id) {
            return Err(format!("`{id}` is never sent through this clapp: {why}"));
        }
    }
    Ok(())
}

/// `run`'s rule: the command changes the open document and nothing outside it.
pub fn run(id: &str, params: &Value) -> Result<(), String> {
    if let Some(why) = listed(craft::NEVER, id) {
        return Err(format!("`{id}` is not run here: {why}"));
    }
    let namespace = id.split('.').next().unwrap_or("");
    if !craft::RUN_NAMESPACES.contains(&namespace) {
        return Err(format!(
            "`{id}` is outside what `run` reaches: it changes the open {doc} only. Files are `{cli} file`; \
             the window itself is `{cli} ui`",
            doc = craft::DOCUMENT,
            cli = crate::CLI
        ));
    }
    if let Some(why) = listed(craft::RUN_REFUSED, id) {
        return Err(format!("`{id}` is not run here: {why}"));
    }
    for (command, param, path_like_only) in craft::DISK_PARAMS {
        if *command != id {
            continue;
        }
        if let Some(value) = params.get(*param) {
            if !path_like_only || looks_like_path(value) {
                return Err(format!("`{param}` names a file on disk, which `run` never reaches. {}", craft::DISK_HINT));
            }
        }
    }
    Ok(())
}

/// Whether `run` may send `id` at all (no parameters looked at): for `command list`.
pub fn runnable(id: &str) -> Result<(), String> {
    run(id, &Value::Null)
}

/// The parameters of `id` that `run` refuses because they name a file on disk.
pub fn disk_params(id: &str) -> Vec<&'static str> {
    craft::DISK_PARAMS.iter().filter(|(command, _, _)| *command == id).map(|(_, param, _)| *param).collect()
}

/// The command id a method carries, for the methods that run one.
fn command_of<'a>(method: &str, params: &'a Value) -> Option<&'a str> {
    match method {
        "engine.execute" | "command" | "ui.menu.invoke" => {
            params.get("command").or_else(|| params.get("id")).and_then(Value::as_str)
        }
        _ => None,
    }
}

/// The reason `id` is in `table`. An entry ending in `.` stands for every id it begins.
fn listed(table: &'static [(&'static str, &'static str)], id: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(entry, _)| if entry.ends_with('.') { id.starts_with(entry) } else { id == *entry })
        .map(|(_, why)| *why)
}

/// A value that names a file rather than one of the app's built-in choices.
fn looks_like_path(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.contains('/') || s.contains('\\') || s.starts_with('~'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn everything_never_sent_is_also_never_run() {
        for (entry, _) in craft::NEVER {
            let id = if entry.ends_with('.') { format!("{entry}anything") } else { entry.to_string() };
            assert!(run(&id, &json!({})).is_err(), "{id}");
            assert!(forward("engine.execute", &json!({"command": id})).is_err(), "{id}");
        }
    }

    #[test]
    fn a_namespace_nobody_has_read_stays_out() {
        assert!(run("brandNewFeature.doSomething", &json!({})).is_err());
    }

    #[test]
    fn a_disk_parameter_is_refused_and_the_same_command_without_it_is_not() {
        for (command, param, path_like_only) in craft::DISK_PARAMS {
            let namespace = command.split('.').next().unwrap();
            if !craft::RUN_NAMESPACES.contains(&namespace) || listed(craft::RUN_REFUSED, command).is_some() {
                continue;
            }
            let disk = if *path_like_only { "/Users/x/profile.icc" } else { "anything.bin" };
            assert!(run(command, &json!({ *param: disk })).is_err(), "{command} {param}");
            assert!(run(command, &json!({})).is_ok(), "{command} with no {param}");
        }
    }

    #[test]
    fn only_the_methods_the_cli_uses_are_forwarded() {
        assert!(forward("app.quit", &json!({})).is_err(), "quitting throws away unsaved work");
        assert!(forward("engine.commands", &json!({})).is_ok());
    }
}
