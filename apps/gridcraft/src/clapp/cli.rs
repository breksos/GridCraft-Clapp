//! The agent's CLI. `gridcraft help` is its only manual: Clatch ships no other document, so a
//! verb missing from it does not exist as far as an agent is concerned. Every verb is declared
//! in clatch.json's `connector.commands` and is its own grant (`Bash(gridcraft <verb>:*)`).
//!
//! Words in, lines out: data on stdout, complaints on stderr. Exit 0 done, 1 refused, 2 usage.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value as Json, json};

use super::CLI;

/// The declared verbs, in the order the help gives them. clatch.json must say the same.
pub const VERBS: &[&str] = &["state", "read", "commands", "shot", "set", "write", "run", "new", "open", "save", "export", "insert", "focus", "close"];

const HELP: &str = r#"gridcraft - work on the open workbook with the person, in GridCraft's own window.

Everything you change shows in the window as it happens and lands on the same undo history
as the person's edits. You do not poll: Clatch tells you what the person does. `changed`
(context) lists the commands they ran once they pause; `selected` (buffered) rides their
next message with what they have selected, so "this cell" or "this block" is that selection.

Look (changes nothing)
  gridcraft state                       the open workbooks, the sheet in front, the selection
  gridcraft read [<range>] [--sheet <name>] [--formulas] [--csv | --json]
                                        a range's values (default: the used range)
  gridcraft commands [<words>]          GridCraft's commands and what each takes;
                                        [not run] marks one `run` will not send
  gridcraft shot --out <png> [--sheet <n>]
                                        a picture of the window, showing sheet n (from 1)

Change the open workbook
  gridcraft set <cell> <input> [--sheet <name>]
                                        a value or =formula, as typing it would enter it
  gridcraft write <range> --csv <file|-> | --json '<rows>'
                                        a block of values; text starting with = is a formula
  gridcraft run <command> ['<params-json>']
                                        any of GridCraft's commands, for example
                                        gridcraft run home.bold '{"range": "A1:C1"}'

Files (the only verbs that touch the disk)
  gridcraft new [--sample budget|sales|grades]
  gridcraft open <path>                 .xlsx .xlsm .xls .xlsb .ods .csv .tsv .json
  gridcraft save [<path>]               to its own file; a .xlsx path becomes its file
  gridcraft export <path>               a copy as .pdf, .csv, .tsv or .html
  gridcraft insert <picture> [--at <cell>]
                                        a PNG or JPEG onto the sheet in front

The window
  gridcraft focus                       bring the window to the front
  gridcraft close                       quit GridCraft; refused while anything is unsaved

Nothing here closes or overwrites unsaved work: that stays the person's decision.
Several workbooks can be open; every verb acts on the one in front, and
`gridcraft run window.activate '{"index": <n>}'` switches (from 0). Ranges are A1
references: B2, A1:D10, Sheet2!A1:C3. A path is read from where you run the command.
These verbs reach the app over a local socket, which a sandbox can block: if one fails
that way, run it again with escalated permissions.
Exit: 0 done, 1 refused, 2 the command line is wrong."#;

enum Fail {
    Refused(String),
    Usage(String),
}

type Res<T> = Result<T, Fail>;

fn usage<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail::Usage(msg.into()))
}

pub async fn run(args: Vec<String>) -> ! {
    let code = match dispatch(args).await {
        Ok(()) => 0,
        Err(Fail::Refused(msg)) => {
            eprintln!("{CLI}: {msg}");
            1
        }
        Err(Fail::Usage(msg)) => {
            eprintln!("{CLI}: {msg}\nSee `{CLI} help`.");
            2
        }
    };
    std::process::exit(code)
}

/// The CLI role's `main`: a fresh runtime, one request, then exit.
pub fn main(args: Vec<String>) -> ! {
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt.block_on(run(args)),
        Err(e) => {
            eprintln!("{CLI}: cannot start a runtime: {e}");
            std::process::exit(1)
        }
    }
}

/// The command line: words, `--name value` pairs and the bare switches a verb takes.
struct Args {
    words: Vec<String>,
    flags: HashMap<String, String>,
}

impl Args {
    fn parse(raw: Vec<String>, switches: &[&str]) -> Res<Args> {
        let (mut words, mut flags) = (Vec::new(), HashMap::new());
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            if a == "--" {
                words.extend(it.by_ref());
            } else if let Some(name) = a.strip_prefix("--").filter(|n| !n.is_empty()) {
                if switches.contains(&name) {
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
    fn at_most(&self, n: usize) -> Res<()> {
        match self.words.get(n) {
            Some(extra) => usage(format!("unexpected `{extra}`")),
            None => Ok(()),
        }
    }
}

async fn dispatch(raw: Vec<String>) -> Res<()> {
    let verb = raw.first().cloned().unwrap_or_else(|| "help".into());
    // `write` reads its own `--csv <file>`, which takes a value where `read`'s bare `--csv` does not.
    let a = Args::parse(raw.clone(), &["formulas", "csv", "json", "help"])?;
    if a.on("help") && verb != "write" {
        println!("{HELP}");
        return Ok(());
    }
    let req = match verb.as_str() {
        "help" | "-h" | "--help" => {
            println!("{HELP}");
            return Ok(());
        }
        "state" => {
            a.at_most(1)?;
            json!({"verb": "state"})
        }
        "read" => read_request(&a)?,
        "commands" => json!({"verb": "commands", "search": (a.words.len() > 1).then(|| a.words[1..].join(" "))}),
        "shot" => shot_request(&a)?,
        "set" => set_request(&a)?,
        "write" => write_request(&raw)?,
        "run" => run_request(&a)?,
        "new" => {
            a.at_most(1)?;
            json!({"verb": "new", "sample": a.flag("sample")})
        }
        "open" | "export" | "insert" => path_request(&verb, &a)?,
        "save" => {
            a.at_most(2)?;
            json!({"verb": "save", "path": a.word(1).map(full).transpose()?})
        }
        "focus" | "close" => {
            a.at_most(1)?;
            json!({"verb": verb})
        }
        other => return usage(format!("no verb `{other}`; the verbs are {}", VERBS.join(", "))),
    };
    let result = ask(req).await?;
    show(&verb, &a, &result);
    Ok(())
}

fn read_request(a: &Args) -> Res<Json> {
    a.at_most(2)?;
    Ok(json!({
        "verb": "read",
        "range": a.word(1),
        "sheet": a.flag("sheet"),
        "formulas": a.on("formulas"),
    }))
}

fn shot_request(a: &Args) -> Res<Json> {
    a.at_most(1)?;
    let Some(out) = a.flag("out") else { return usage("`shot` writes to --out <png>") };
    let sheet = match a.flag("sheet") {
        None => None,
        Some(n) => Some(n.parse::<u64>().map_err(|_| Fail::Usage(format!("--sheet takes a number from 1, not `{n}`")))?),
    };
    Ok(json!({"verb": "shot", "path": full(out)?, "sheet": sheet}))
}

fn set_request(a: &Args) -> Res<Json> {
    let (Some(cell), Some(_)) = (a.word(1), a.word(2)) else { return usage("set <cell> <input>") };
    Ok(json!({"verb": "set", "cell": cell, "input": a.words[2..].join(" "), "sheet": a.flag("sheet")}))
}

fn write_request(raw: &[String]) -> Res<Json> {
    let a = Args::parse(raw.to_vec(), &[])?;
    a.at_most(2)?;
    let Some(range) = a.word(1) else { return usage("write <range> --csv <file|-> | --json '<rows>'") };
    let values = match (a.flag("csv"), a.flag("json")) {
        (Some(src), None) => {
            let text = if src == "-" {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| Fail::Refused(format!("stdin: {e}")))?;
                s
            } else {
                std::fs::read_to_string(src).map_err(|e| Fail::Refused(format!("{src}: {e}")))?
            };
            Json::Array(parse_csv(&text).into_iter().map(|row| Json::Array(row.into_iter().map(typed).collect())).collect())
        }
        (None, Some(text)) => match serde_json::from_str::<Json>(text) {
            Ok(v) if v.as_array().is_some_and(|rows| rows.iter().all(Json::is_array)) => v,
            _ => return usage("--json takes rows: '[[1, 2], [\"a\", \"=A1*2\"]]'"),
        },
        _ => return usage("write takes its values from one of --csv <file|-> or --json '<rows>'"),
    };
    Ok(json!({"verb": "write", "range": range, "values": values}))
}

fn run_request(a: &Args) -> Res<Json> {
    a.at_most(3)?;
    let Some(command) = a.word(1) else { return usage("run <command> ['<params-json>']") };
    let params = match a.word(2) {
        None => json!({}),
        Some(text) => match serde_json::from_str::<Json>(text) {
            Ok(v) if v.is_object() => v,
            _ => return usage(format!("parameters are a JSON object, such as '{{\"range\": \"A1:C1\"}}', not {text}")),
        },
    };
    Ok(json!({"verb": "run", "command": command, "params": params}))
}

fn path_request(verb: &str, a: &Args) -> Res<Json> {
    a.at_most(2)?;
    let Some(path) = a.word(1) else { return usage(format!("{verb} <path>")) };
    let path = full(path)?;
    if verb == "insert" && !std::path::Path::new(&path).is_file() {
        return Err(Fail::Refused(format!("no file at {path}")));
    }
    Ok(json!({"verb": verb, "path": path, "at": a.flag("at")}))
}

/// A path as the person running the command means it: from their folder, `~` their home.
fn full(path: &str) -> Res<String> {
    let p = match path.strip_prefix("~/") {
        Some(rest) => clappkit::paths::home().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    };
    std::path::absolute(&p).map(|p| p.to_string_lossy().into_owned()).map_err(|e| Fail::Usage(format!("{path}: {e}")))
}

// --- talking to the window --------------------------------------------------------------------

async fn ask(mut req: Json) -> Res<Json> {
    // Who is calling: Clatch puts the agent's id in its shell.
    if let Some(agent) = std::env::var("CLATCH_AGENT_ID").ok().filter(|s| !s.is_empty()) {
        req["agent"] = json!(agent);
    }
    let reply = clappkit::ipc::request(CLI, &req).await.map_err(|e| Fail::Refused(strip(&format!("{e:#}"))))?;
    if reply.get("ok").and_then(Json::as_bool) != Some(true) {
        return Err(Fail::Refused(reply.get("error").and_then(Json::as_str).unwrap_or("refused").to_string()));
    }
    if let Some(spill) = reply.get("spill").and_then(Json::as_str) {
        let text = std::fs::read_to_string(spill).map_err(|e| Fail::Refused(format!("cannot read the answer at {spill}: {e}")))?;
        let _ = std::fs::remove_file(spill);
        let body: Json = serde_json::from_str(&text).map_err(|e| Fail::Refused(format!("an unreadable answer: {e}")))?;
        return Ok(body.get("result").cloned().unwrap_or(Json::Null));
    }
    Ok(reply.get("result").cloned().unwrap_or(Json::Null))
}

/// clappkit's messages already start with `<cli>: `; `run` adds it once.
fn strip(msg: &str) -> String {
    msg.strip_prefix(&format!("{CLI}: ")).unwrap_or(msg).to_string()
}

// --- answers as lines -------------------------------------------------------------------------

fn show(verb: &str, a: &Args, v: &Json) {
    if a.on("json") && verb != "write" {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
        return;
    }
    match verb {
        "read" => print_rows(v, a.on("csv")),
        "commands" => {
            let rows = v.as_array().cloned().unwrap_or_default();
            for r in &rows {
                let id = r["id"].as_str().unwrap_or("");
                let off = if r["enabled"].as_bool() == Some(false) { "  [off]" } else { "" };
                let not_run = if r["run"].as_bool() == Some(false) { "  [not run]" } else { "" };
                println!("{id:<36} {}{off}{not_run}", r["label"].as_str().unwrap_or(""));
                if let Some(p) = r["params"].as_str().filter(|p| !p.is_empty() && rows.len() <= 10) {
                    println!("{:<36}   {p}", "");
                }
            }
            eprintln!("{} commands", rows.len());
        }
        "shot" => println!("{}", v["path"].as_str().unwrap_or("")),
        _ => {
            for line in lines(v, 0) {
                println!("{line}");
            }
        }
    }
}

fn print_rows(v: &Json, csv: bool) {
    let rows = v["values"].as_array().cloned().unwrap_or_default();
    if !csv {
        let at = match (v["sheet"].as_str(), v["range"].as_str()) {
            (Some(s), Some(r)) => format!("{s}!{r}"),
            (_, Some(r)) => r.to_string(),
            _ => String::new(),
        };
        if !at.is_empty() {
            println!("{at}");
        }
    }
    for row in rows {
        let cells: Vec<String> = row.as_array().map(|c| c.iter().map(|x| cell_text(x, csv)).collect()).unwrap_or_default();
        println!("{}", cells.join(if csv { "," } else { "\t" }));
    }
}

fn cell_text(v: &Json, csv: bool) -> String {
    let s = match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        Json::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    };
    if csv && s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

/// Any answer as `key: value` lines, nested keys indented, list items under `-`.
fn lines(v: &Json, indent: usize) -> Vec<String> {
    let pad = " ".repeat(indent);
    let mut out = Vec::new();
    match v {
        Json::Null => out.push(format!("{pad}done")),
        Json::Object(map) => {
            for (k, val) in map {
                match inline(val) {
                    Some(t) => out.push(format!("{pad}{k}: {t}")),
                    None => {
                        out.push(format!("{pad}{k}:"));
                        out.extend(lines(val, indent + 2));
                    }
                }
            }
        }
        Json::Array(items) => {
            for item in items {
                match inline(item) {
                    Some(t) => out.push(format!("{pad}- {t}")),
                    None => {
                        out.push(format!("{pad}-"));
                        out.extend(lines(item, indent + 2));
                    }
                }
            }
        }
        other => out.push(format!("{pad}{}", scalar(other))),
    }
    out
}

fn inline(v: &Json) -> Option<String> {
    match v {
        Json::Object(m) if m.is_empty() => Some("{}".into()),
        Json::Array(a) if a.is_empty() => Some("[]".into()),
        Json::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => {
            Some(format!("[{}]", a.iter().map(scalar).collect::<Vec<_>>().join(", ")))
        }
        Json::Object(_) | Json::Array(_) => None,
        other => Some(scalar(other)),
    }
}

fn scalar(v: &Json) -> String {
    match v {
        Json::String(s) if s.contains(['\n', '\r', '\t']) || s.trim() != s || s.is_empty() => serde_json::to_string(s).unwrap_or_default(),
        Json::String(s) => s.clone(),
        Json::Null => "-".into(),
        other => other.to_string(),
    }
}

// --- CSV in -----------------------------------------------------------------------------------

/// RFC 4180 rows: quoted fields may hold commas, quotes (doubled) and line breaks.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut field) = (Vec::new(), Vec::new(), String::new());
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A CSV field as the value typing it would enter: a number, TRUE/FALSE, empty, or text.
fn typed(s: String) -> Json {
    let t = s.trim();
    if t.is_empty() {
        return Json::Null;
    }
    if let Ok(n) = t.parse::<f64>()
        && n.is_finite()
    {
        return json!(n);
    }
    match t.to_ascii_uppercase().as_str() {
        "TRUE" => json!(true),
        "FALSE" => json!(false),
        _ => Json::String(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest declares exactly the verbs the help documents, in the same order.
    #[test]
    fn the_manifest_declares_exactly_the_verbs_the_help_gives() {
        let manifest: Json = serde_json::from_str(include_str!("../../../../clatch.json")).unwrap();
        let declared: Vec<&str> = manifest["connector"]["commands"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(declared, VERBS);
        for v in VERBS {
            assert!(HELP.lines().any(|l| l.starts_with(&format!("  {CLI} {v}"))), "the help never shows `{v}`");
        }
    }

    /// What the binary prints of its own is ASCII: a legacy Windows console turns anything else
    /// into mojibake, and this output is an agent's input.
    #[test]
    fn the_help_is_ascii() {
        assert!(HELP.is_ascii());
    }

    #[test]
    fn csv_fields_keep_commas_quotes_and_breaks() {
        let rows = parse_csv("a,\"b, c\",\"say \"\"hi\"\"\"\n1,\"two\nlines\",\n");
        assert_eq!(rows, vec![vec!["a", "b, c", "say \"hi\""], vec!["1", "two\nlines", ""]]);
    }

    #[test]
    fn csv_values_are_typed_as_typing_would() {
        assert_eq!(typed("12.5".into()), json!(12.5));
        assert_eq!(typed("true".into()), json!(true));
        assert_eq!(typed("".into()), Json::Null);
        assert_eq!(typed("=A1*2".into()), json!("=A1*2"));
        assert_eq!(typed("north".into()), json!("north"));
    }

    #[test]
    fn write_takes_csv_with_a_value_and_read_takes_it_bare() {
        let w = write_request(&["write".into(), "A1".into(), "--json".into(), "[[1,2]]".into()]).ok().unwrap();
        assert_eq!(w["values"], json!([[1, 2]]));
        let r = Args::parse(vec!["read".into(), "A1:B2".into(), "--csv".into()], &["formulas", "csv", "json", "help"]).ok().unwrap();
        assert!(r.on("csv"));
    }
}
