//! Answers as lines. The windows answer in JSON whose shape grows every day upstream, so this
//! prints any answer the same plain way rather than describing each one: `key: value`, nested
//! keys indented, a list item under `-`. `--json` gives the window's own object.
//!
//! Data on stdout, complaints on stderr, so a pipe carries data only.

use serde_json::Value;

pub fn json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
}

/// The answer as lines; `null` (a command that returns nothing) prints `done`.
pub fn print(v: &Value) {
    if v.is_null() {
        println!("done");
        return;
    }
    for line in lines(v) {
        println!("{line}");
    }
}

pub fn lines(v: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    walk(v, 0, &mut lines);
    lines
}

fn walk(v: &Value, indent: usize, out: &mut Vec<String>) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                match inline(v) {
                    Some(text) => out.push(format!("{pad}{k}: {text}")),
                    None => {
                        out.push(format!("{pad}{k}:"));
                        walk(v, indent + 2, out);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                match inline(item) {
                    Some(text) => out.push(format!("{pad}- {text}")),
                    None => {
                        out.push(format!("{pad}-"));
                        walk(item, indent + 2, out);
                    }
                }
            }
        }
        other => out.push(format!("{pad}{}", scalar(other))),
    }
}

/// A value that fits on its key's line: a scalar, an empty container, or a short list of scalars.
fn inline(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) if m.is_empty() => Some("{}".into()),
        Value::Array(a) if a.is_empty() => Some("[]".into()),
        Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => {
            Some(format!("[{}]", a.iter().map(scalar).collect::<Vec<_>>().join(", ")))
        }
        Value::Object(_) | Value::Array(_) => None,
        other => Some(scalar(other)),
    }
}

/// A string as itself, unless it holds a line break or starts or ends with space: then quoted,
/// so one value stays one line.
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) if s.contains(['\n', '\r', '\t']) || s.trim() != s || s.is_empty() => {
            serde_json::to_string(s).unwrap_or_default()
        }
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::lines;
    use serde_json::json;

    #[test]
    fn nested_keys_indent_and_lists_of_scalars_stay_on_one_line() {
        let v = json!({"name": "Untitled", "size": {"width": 64, "height": 64}, "tags": ["a", "b"], "path": null});
        assert_eq!(lines(&v), ["name: Untitled", "size:", "  width: 64", "  height: 64", "tags: [a, b]", "path: -"]);
    }

    #[test]
    fn a_list_of_objects_is_dashed() {
        let v = json!({"layers": [{"id": 1, "name": "Background"}, {"id": 5, "name": "Layer 1"}]});
        assert_eq!(lines(&v), ["layers:", "  -", "    id: 1", "    name: Background", "  -", "    id: 5", "    name: Layer 1"]);
    }

    /// One value, one line: a paragraph with a break in it cannot pass for two fields.
    #[test]
    fn a_line_break_inside_a_value_is_quoted() {
        assert_eq!(lines(&json!({"text": "one\ntwo"})), [r#"text: "one\ntwo""#]);
        assert_eq!(lines(&json!({"text": "Şu iş"})), ["text: Şu iş"]);
    }
}
