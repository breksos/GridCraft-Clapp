//! The agent's surface: words in, lines out. `help` is the manual and there is no other one.
//!
//! The groups are the grant grain (`Bash(<cli> <group>:*)`), drawn on what each can reach:
//! `doc`, `window` and `command` only look; `run` changes the open document and nothing outside
//! it; `file` is the only door to the disk; `ui` drives the window as a person would.
//!
//! Exit: 0 done, 1 refused (nothing changed, or the window says what did), 2 usage.

use crate::{craft, out, policy, CLI};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The declared groups, in the order the help gives them. `clatch.json` must say the same.
pub const GROUPS: &[&str] = &["doc", "window", "command", "run", "file", "ui"];

fn help() -> String {
    format!(
        "{cli} - work in {name} with the person, in {name}'s own window.\n\
\n\
Every change shows in the window as it happens and lands on the same undo history as the\n\
person's. Nothing is remembered between runs: every command names what it acts on.\n\
\n\
Look (changes nothing)\n\
  {cli} doc view [--json]                 the open {doc}: everything the app reports about it\n\
  {cli} doc render --out <png> [--page <n>] [--scale <s>]\n\
                                          a picture of the {doc}, written to <png>\n\
  {cli} window view [--json]              the window: open {doc}s, tool, dialogs\n\
  {cli} window shot --out <png>           a picture of the whole window\n\
  {cli} command list [<words>]            every command the app has, or those matching; [off] is not\n\
                                          available now, [not run] is one `run` will not send\n\
  {cli} command view <id> [--json]        one command: what it takes, and whether `run` sends it\n\
\n\
Change the open {doc}\n\
  {cli} run <id> [<params-json>] [--json] run one command, for example\n\
                                          {cli} run history.undo\n\
\n\
Files (the only group that reads or writes your disk)\n\
  {cli} file new [<params-json>]\n\
  {cli} file open <path>\n\
  {cli} file save [<path>]                to its own file; a path saves it there from now on\n\
  {cli} file export <path> [<params-json>] a copy in the format the extension names\n\
  {cli} file close                        refused while it has unsaved changes\n\
{insert}\
\n\
Drive the window, as a person would\n\
  {cli} ui click <x> <y> [--right | --middle] [--double]\n\
  {cli} ui key <key> [--cmd] [--shift] [--alt] [--ctrl]\n\
  {cli} ui type <text>\n\
  {cli} ui call <method> [<params-json>]   any of: {ui}\n\
\n\
{notes}\n\
\n\
A path is read from where you run the command. Nothing here closes or replaces a {doc}\n\
with unsaved changes: that stays the person's decision.\n\
Exit: 0 done, 1 refused, 2 the command line is wrong.",
        cli = CLI,
        name = craft::NAME,
        doc = craft::DOCUMENT,
        ui = craft::UI_METHODS.join(" "),
        notes = craft::HELP_NOTES,
        insert = if craft::OFFERS_INSERT {
            format!("  {CLI} file insert <path> [<params-json>] put a picture or media file into the {}\n", craft::DOCUMENT)
        } else {
            String::new()
        },
    )
}

enum Fail {
    Refused(String),
    Usage(String),
}

type Res<T> = Result<T, Fail>;

fn usage<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail::Usage(msg.into()))
}

pub async fn run(args: Vec<String>) {
    let code = match dispatch(args).await {
        Ok(()) => 0,
        Err(Fail::Refused(msg)) => {
            eprintln!("{}", prefixed(&msg));
            1
        }
        Err(Fail::Usage(msg)) => {
            eprintln!("{}\nSee `{CLI} help`.", prefixed(&msg));
            2
        }
    };
    std::process::exit(code)
}

fn prefixed(msg: &str) -> String {
    if msg.starts_with(&format!("{CLI}:")) {
        msg.to_string()
    } else {
        format!("{CLI}: {msg}")
    }
}

/// The command line: words, `--flag value` pairs and bare `--switch`es.
struct Args {
    words: Vec<String>,
    flags: HashMap<String, String>,
}

const SWITCHES: &[&str] = &["json", "right", "middle", "double", "cmd", "shift", "alt", "ctrl", "help"];

impl Args {
    fn parse(raw: Vec<String>) -> Res<Args> {
        let (mut words, mut flags) = (Vec::new(), HashMap::new());
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            if a == "-h" {
                flags.insert("help".into(), String::new());
            } else if a == "--" {
                words.extend(it.by_ref());
            } else if let Some(name) = a.strip_prefix("--") {
                if SWITCHES.contains(&name) {
                    flags.insert(name.to_string(), String::new());
                } else {
                    let Some(value) = it.next() else { return usage(format!("--{name} needs a value")) };
                    flags.insert(name.to_string(), value);
                }
            } else {
                words.push(a);
            }
        }
        Ok(Args { words, flags })
    }
    fn word(&self, i: usize) -> Option<&str> {
        self.words.get(i).map(String::as_str)
    }
    fn on(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
    fn number(&self, name: &str) -> Res<Option<f64>> {
        match self.flag(name) {
            None => Ok(None),
            Some(v) => v.parse().map(Some).or_else(|_| usage(format!("--{name} takes a number, not `{v}`"))),
        }
    }
    /// The words from `from` on must be at most `max` long.
    fn at_most(&self, max: usize) -> Res<()> {
        match self.words.get(max) {
            Some(extra) => usage(format!("unexpected `{extra}`")),
            None => Ok(()),
        }
    }
}

async fn dispatch(raw: Vec<String>) -> Res<()> {
    let a = Args::parse(raw)?;
    let group = a.word(0).unwrap_or("help");
    if group == "help" || group == "--help" || (a.on("help") && a.words.len() <= 1 && !GROUPS.contains(&group)) {
        println!("{}", help());
        return Ok(());
    }
    if a.on("help") {
        println!("{}", section(group));
        return Ok(());
    }
    match group {
        "doc" => doc(&a).await,
        "window" => window(&a).await,
        "command" => command(&a).await,
        "run" => run_one(&a).await,
        "file" => file(&a).await,
        "ui" => ui(&a).await,
        other => usage(format!("no group `{other}`; the groups are {}", GROUPS.join(", "))),
    }
}

/// The help's lines for one group.
fn section(group: &str) -> String {
    let needle = format!("{CLI} {group}");
    let text = help();
    let lines: Vec<&str> = text.lines().filter(|l| l.trim_start().starts_with(&needle)).collect();
    if lines.is_empty() {
        help()
    } else {
        lines.join("\n")
    }
}

// --- talking to the launcher ---------------------------------------------------------------

async fn ask(request: Value) -> Res<Value> {
    let reply = clappkit::ipc::request(CLI, &request).await.map_err(|e| Fail::Refused(format!("{e:#}")))?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(Fail::Refused(reply.get("error").and_then(Value::as_str).unwrap_or("refused").to_string()));
    }
    // An answer too big for the socket comes as a file, read once and removed.
    if let Some(spill) = reply.get("spill").and_then(Value::as_str) {
        let text = std::fs::read_to_string(spill).map_err(|e| Fail::Refused(format!("cannot read the answer at {spill}: {e}")))?;
        let _ = std::fs::remove_file(spill);
        let body: Value = serde_json::from_str(&text).map_err(|e| Fail::Refused(format!("an unreadable answer: {e}")))?;
        return Ok(body.get("result").cloned().unwrap_or(Value::Null));
    }
    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
}

async fn call(method: &str, params: Value) -> Res<Value> {
    ask(json!({"op": "call", "method": method, "params": params})).await
}

fn show(a: &Args, v: &Value) {
    if a.on("json") {
        out::json(v)
    } else {
        out::print(v)
    }
}

/// A JSON object from the command line; none is `{}`.
fn params(arg: Option<&str>) -> Res<Value> {
    match arg {
        None => Ok(json!({})),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(v) if v.is_object() => Ok(v),
            Ok(_) => usage("parameters are a JSON object, such as '{\"radius\": 2}'"),
            Err(e) => usage(format!("parameters are not JSON ({e}): {text}")),
        },
    }
}

/// A path as the person running the command means it: from their folder, `~` their home.
fn full(path: &str) -> Res<PathBuf> {
    let p = match path.strip_prefix("~/") {
        Some(rest) => clappkit::paths::home().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    };
    std::path::absolute(&p).map_err(|e| Fail::Usage(format!("{path}: {e}")))
}

/// Move a file the window wrote into the exchange folder to where the agent asked for it.
fn deliver(from: &str, to: &Path) -> Res<()> {
    let moved = std::fs::copy(from, to);
    let _ = std::fs::remove_file(from);
    moved.map(|_| ()).map_err(|e| Fail::Refused(format!("cannot write {}: {e}", to.display())))
}

// --- the groups ----------------------------------------------------------------------------

async fn doc(a: &Args) -> Res<()> {
    match a.word(1) {
        Some("view") => {
            a.at_most(2)?;
            let (method, p) = craft::doc_view();
            show(a, &call(method, p).await?);
            Ok(())
        }
        Some("render") => {
            a.at_most(2)?;
            let Some(to) = a.flag("out") else { return usage("`doc render` writes to --out <png>") };
            let to = full(to)?;
            let page = a.number("page")?.map(|n| n.max(1.0) as u64);
            let reply = ask(json!({"op": "render", "page": page, "scale": a.number("scale")?})).await?;
            let file = reply["file"].as_str().ok_or_else(|| Fail::Refused("the window wrote no picture".into()))?;
            deliver(file, &to)?;
            println!("{}", to.display());
            Ok(())
        }
        _ => usage("doc view | doc render --out <png>"),
    }
}

async fn window(a: &Args) -> Res<()> {
    match a.word(1) {
        Some("view") => {
            a.at_most(2)?;
            let ui = call("ui.inspect", json!({})).await?;
            if a.on("json") {
                out::json(&ui);
            } else {
                let picked: serde_json::Map<String, Value> =
                    craft::WINDOW_KEYS.iter().filter_map(|k| ui.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
                out::print(&Value::Object(picked));
            }
            Ok(())
        }
        Some("shot") => {
            a.at_most(2)?;
            let Some(to) = a.flag("out") else { return usage("`window shot` writes to --out <png>") };
            let to = full(to)?;
            let reply = ask(json!({"op": "shot"})).await?;
            let file = reply["file"].as_str().ok_or_else(|| Fail::Refused("the window wrote no picture".into()))?;
            deliver(file, &to)?;
            println!("{}", to.display());
            Ok(())
        }
        _ => usage("window view | window shot --out <png>"),
    }
}

/// The app's command table, whatever shape it comes in.
async fn commands() -> Res<Vec<Value>> {
    let all = call("engine.commands", json!({})).await?;
    let rows = all.get("commands").cloned().unwrap_or(all);
    Ok(rows.as_array().cloned().unwrap_or_default())
}

async fn command(a: &Args) -> Res<()> {
    match a.word(1) {
        Some("list") => {
            // `--search blur gaussian` and `list blur gaussian` both search: words are words.
            let mut words: Vec<&str> = a.words.iter().skip(2).map(String::as_str).collect();
            words.extend(a.flag("search"));
            let search = (!words.is_empty()).then(|| words.join(" ").to_lowercase());
            let mut shown = 0;
            for row in commands().await? {
                let id = row["id"].as_str().unwrap_or("");
                let label = row["label"].as_str().unwrap_or("");
                if let Some(words) = &search {
                    let hay = format!("{id} {label} {}", row["params"].as_str().unwrap_or("")).to_lowercase();
                    if !words.split_whitespace().all(|w| hay.contains(w)) {
                        continue;
                    }
                }
                let off = if row["enabled"].as_bool() == Some(false) { "  [off]" } else { "" };
                let not_run = if policy::runnable(id).is_err() { "  [not run]" } else { "" };
                println!("{id:<40} {label}{off}{not_run}");
                shown += 1;
            }
            eprintln!("{shown} commands");
            Ok(())
        }
        Some("view") => {
            a.at_most(3)?;
            let Some(id) = a.word(2) else { return usage("command view <id>") };
            let rows = commands().await?;
            let Some(row) = rows.iter().find(|r| r["id"].as_str() == Some(id)) else {
                return Err(Fail::Refused(format!("no command `{id}`; `{CLI} command list <words>` finds one")));
            };
            let mut row = row.clone();
            row["run"] = match (policy::runnable(id), policy::disk_params(id).as_slice()) {
                (Err(why), _) => json!(format!("no - {why}")),
                (Ok(()), []) => json!("yes"),
                (Ok(()), disk) => json!(format!("yes, but not with {}: a file on disk is `{CLI} file`'s", disk.join(" or "))),
            };
            show(a, &row);
            Ok(())
        }
        _ => usage("command list [<words>] | command view <id>"),
    }
}

async fn run_one(a: &Args) -> Res<()> {
    a.at_most(3)?;
    let Some(id) = a.word(1) else { return usage("run <id> [<params-json>]") };
    let p = params(a.word(2))?;
    policy::run(id, &p).map_err(Fail::Refused)?;
    show(a, &call("engine.execute", json!({"command": id, "params": p})).await?);
    Ok(())
}

async fn file(a: &Args) -> Res<()> {
    let verb = a.word(1).unwrap_or("");
    let mut req = json!({"op": "file", "verb": verb});
    match verb {
        "new" => {
            a.at_most(3)?;
            req["params"] = params(a.word(2))?;
        }
        "open" | "export" | "insert" => {
            a.at_most(if verb == "open" { 3 } else { 4 })?;
            let Some(path) = a.word(2) else { return usage(format!("file {verb} <path>")) };
            let path = full(path)?;
            req["params"] = params(a.word(3))?;
            if verb == "insert" {
                // Read here, with the agent's own permissions, and hand the window a copy: the
                // window never reads a path an agent names.
                if !path.is_file() {
                    return Err(Fail::Refused(format!("no file at {}", path.display())));
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_string();
                let staged = ask(json!({"op": "stage", "ext": ext})).await?;
                let staged = staged["file"].as_str().unwrap_or_default().to_string();
                std::fs::copy(&path, &staged).map_err(|e| Fail::Refused(format!("cannot hand {} to the window: {e}", path.display())))?;
                req["path"] = json!(staged);
                req["name"] = json!(path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
                let answer = ask(req).await;
                let _ = std::fs::remove_file(&staged);
                show(a, &answer?);
                return Ok(());
            }
            req["path"] = json!(path.to_string_lossy());
        }
        "save" => {
            a.at_most(3)?;
            if let Some(path) = a.word(2) {
                req["path"] = json!(full(path)?.to_string_lossy());
            }
        }
        "close" => a.at_most(2)?,
        _ => return usage("file new | open <path> | save [<path>] | export <path> | close | insert <path>"),
    }
    show(a, &ask(req).await?);
    Ok(())
}

async fn ui(a: &Args) -> Res<()> {
    let (method, p) = match a.word(1) {
        Some("click") => {
            a.at_most(4)?;
            let coord = |i: usize| -> Res<f64> {
                let v = a.word(i).unwrap_or("");
                v.parse().or_else(|_| usage(format!("ui click <x> <y>: `{v}` is not a number")))
            };
            let button = if a.on("right") { "right" } else if a.on("middle") { "middle" } else { "left" };
            craft::click(coord(2)?, coord(3)?, button, if a.on("double") { 2 } else { 1 })
        }
        Some("key") => {
            a.at_most(3)?;
            let Some(k) = a.word(2) else { return usage("ui key <key>, such as Enter, Escape, ArrowLeft or B") };
            craft::key(k, a.on("cmd"), a.on("shift"), a.on("alt"), a.on("ctrl"))
        }
        Some("type") => {
            a.at_most(3)?;
            let Some(text) = a.word(2) else { return usage("ui type <text>") };
            craft::typing(text)
        }
        Some("call") => {
            a.at_most(4)?;
            let Some(m) = a.word(2) else { return usage(format!("ui call <method>; one of {}", craft::UI_METHODS.join(" "))) };
            let Some(method) = craft::UI_METHODS.iter().find(|x| **x == m) else {
                return Err(Fail::Refused(format!("`{m}` is not one `ui call` sends; it sends {}", craft::UI_METHODS.join(" "))));
            };
            (*method, params(a.word(3))?)
        }
        _ => return usage("ui click <x> <y> | ui key <key> | ui type <text> | ui call <method>"),
    };
    show(a, &call(method, p).await?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule 8: the top level lists exactly the declared groups, and nothing a group is not.
    #[test]
    fn the_manifest_declares_exactly_the_groups_the_help_gives() {
        let manifest: Value = serde_json::from_str(include_str!("../../clatch.json")).unwrap();
        let declared: Vec<&str> = manifest["connector"]["commands"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(declared, GROUPS);
        let text = help();
        for g in GROUPS {
            assert!(text.contains(&format!("{CLI} {g} ")), "the help never shows `{g}`");
        }
    }

    /// What the binary prints of its own is ASCII: a legacy Windows console turns anything else
    /// into mojibake, and this output is an agent's input.
    #[test]
    fn the_help_is_ascii() {
        assert!(help().is_ascii(), "{}", help().chars().filter(|c| !c.is_ascii()).collect::<String>());
    }

    #[test]
    fn every_ui_method_is_one_the_launcher_forwards() {
        for m in craft::UI_METHODS {
            assert!(craft::FORWARD_METHODS.contains(m), "{m}");
        }
    }

    #[test]
    fn flags_and_words_are_told_apart() {
        let a = Args::parse(["ui", "key", "B", "--cmd", "--out", "x.png"].map(String::from).to_vec()).ok().unwrap();
        assert_eq!(a.words, ["ui", "key", "B"]);
        assert!(a.on("cmd"));
        assert_eq!(a.flag("out"), Some("x.png"));
    }
}
