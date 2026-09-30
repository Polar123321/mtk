//! Generic JSON compaction. Prunes null/empty values, rounds float noise, hoists
//! constant columns and renders arrays of similar objects as compact tables
//! (a TOON-style notation: `key[N]{col,col}:` followed by one row per line).

use crate::config::Config;
use serde_json::{Map, Number, Value};

pub struct Split {
    pub prefix: String,
    pub value: Value,
    pub suffix: String,
}

impl Split {
    pub fn join(&self, rendered: &str) -> String {
        let mut s = String::new();
        if !self.prefix.trim().is_empty() {
            s.push_str(self.prefix.trim_end());
            s.push('\n');
        }
        s.push_str(rendered);
        if !self.suffix.trim().is_empty() {
            s.push('\n');
            s.push_str(self.suffix.trim());
        }
        s
    }
}

/// Finds a JSON document in the text, allowing a short prose prefix/suffix
/// (Roblox prepends "Note: Output limited to..." to its JSON).
pub fn split(text: &str) -> Option<Split> {
    let t = text.trim();
    if t.len() < 2 {
        return None;
    }
    let start = if t.starts_with('{') || t.starts_with('[') {
        0
    } else {
        let mut found = None;
        let mut offset = 0;
        for line in t.split_inclusive('\n') {
            if offset > 0 && (line.starts_with('{') || line.starts_with('[')) {
                found = Some(offset);
                break;
            }
            offset += line.len();
        }
        found?
    };
    let candidate = &t[start..];
    let mut de = serde_json::Deserializer::from_str(candidate).into_iter::<Value>();
    let value = de.next()?.ok()?;
    if !(value.is_object() || value.is_array()) {
        return None;
    }
    let consumed = de.byte_offset();
    Some(Split {
        prefix: t[..start].to_string(),
        value,
        suffix: candidate[consumed..].to_string(),
    })
}

pub fn render(v: &Value, cfg: &Config) -> String {
    let v = prune(v.clone(), cfg);
    let mut out = String::new();
    match &v {
        Value::Object(m) => write_object(m, 0, &mut out),
        Value::Array(a) => write_field("", a, 0, &mut out),
        other => out.push_str(&inline(other)),
    }
    out.trim_end().to_string()
}

// ---- pruning ---------------------------------------------------------------

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

pub fn prune(v: Value, cfg: &Config) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                let p = prune(val, cfg);
                if !is_empty_value(&p) {
                    out.insert(k, p);
                }
            }
            if cfg.drop_noise_keys {
                drop_noise_keys(&mut out);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|x| prune(x, cfg)).collect()),
        Value::String(s) => Value::String(tidy_string(&s, cfg)),
        Value::Number(n) => Value::Number(round_number(n, cfg)),
        other => other,
    }
}

/// REST-style objects carry a canonical link (`html_url`) plus a pile of
/// auxiliary API links (`followers_url`, `timeline_url`...) and opaque ids.
/// The agent never follows those, so they are dropped when a canonical link exists.
fn drop_noise_keys(m: &mut Map<String, Value>) {
    m.retain(|k, _| k != "node_id" && k != "gravatar_id");
    if !(m.contains_key("html_url") || m.contains_key("url")) {
        return;
    }
    let has_html = m.contains_key("html_url");
    m.retain(|k, v| {
        let is_link = v.as_str().map(|s| s.starts_with("http")).unwrap_or(false);
        let aux = k.ends_with("_url") && k != "html_url" && is_link;
        let api_dup = k == "url" && has_html && is_link;
        !(aux || api_dup)
    });
}

/// Nested objects made only of scalars become dotted columns (`user.login`),
/// which lets constant columns be hoisted out of tables.
fn flatten(v: &Value) -> Value {
    let Value::Object(m) = v else { return v.clone() };
    let mut out = Map::new();
    for (k, x) in m {
        match x {
            Value::Object(inner) if !inner.is_empty() && inner.values().all(|y| !y.is_object() && !y.is_array()) => {
                for (ik, iv) in inner {
                    out.insert(format!("{k}.{ik}"), iv.clone());
                }
            }
            _ => {
                out.insert(k.clone(), x.clone());
            }
        }
    }
    Value::Object(out)
}

fn round_number(n: Number, cfg: &Config) -> Number {
    if !n.is_f64() {
        return n;
    }
    let s = n.to_string();
    let decimals = s.find('.').map(|d| s.len() - d - 1).unwrap_or(0);
    if s.contains('e') || s.contains('E') || decimals < 9 {
        return n;
    }
    let f = n.as_f64().unwrap_or(0.0);
    let p = 10f64.powi(cfg.float_decimals as i32);
    Number::from_f64((f * p).round() / p).unwrap_or(n)
}

/// Rounds float noise inside strings, drops base64 blobs, caps very long text.
pub fn tidy_string(s: &str, cfg: &Config) -> String {
    let mut out: String = s.to_string();
    if out.len() > 400 && re!(r"^(data:[\w/+.\-]+;base64,)?[A-Za-z0-9+/=\s]{400,}$").is_match(&out) {
        return format!("<base64 {} chars>", out.len());
    }
    if out.contains('.') {
        let decimals = cfg.float_decimals;
        out = re!(r"-?\d+\.\d{9,}")
            .replace_all(&out, |c: &regex::Captures| {
                let f: f64 = c[0].parse().unwrap_or(0.0);
                let r = format!("{:.*}", decimals, f);
                let r = r.trim_end_matches('0').trim_end_matches('.').to_string();
                if r.is_empty() || r == "-" {
                    "0".to_string()
                } else {
                    r
                }
            })
            .into_owned();
    }
    let n = out.chars().count();
    if n > 1500 {
        let head: String = out.chars().take(1000).collect();
        out = format!("{head}…[+{} chars]", n - 1000);
    }
    out
}

// ---- writing ---------------------------------------------------------------

fn is_bare_key(k: &str) -> bool {
    re!(r"^[A-Za-z_][A-Za-z0-9_.\-]*$").is_match(k)
}

fn key(k: &str) -> String {
    if is_bare_key(k) {
        k.to_string()
    } else {
        serde_json::to_string(k).unwrap()
    }
}

fn looks_numeric_or_keyword(s: &str) -> bool {
    matches!(s, "true" | "false" | "null") || s.parse::<f64>().is_ok()
}

/// A string as a bare token when unambiguous, JSON-quoted otherwise.
fn string_token(s: &str, in_row: bool) -> String {
    let needs_quote = s.is_empty()
        || s.starts_with(char::is_whitespace)
        || s.ends_with(char::is_whitespace)
        || s.contains(['\n', '\r', '\t'])
        || s.starts_with(['{', '[', '"'])
        || looks_numeric_or_keyword(s)
        || (in_row && s.contains(','));
    if needs_quote {
        serde_json::to_string(s).unwrap()
    } else {
        s.to_string()
    }
}

fn scalar(v: &Value, in_row: bool) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(string_token(s, in_row)),
        _ => None,
    }
}

pub fn inline(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let parts: Vec<String> = m.iter().map(|(k, x)| format!("{}:{}", key(k), inline(x))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(inline).collect();
            format!("[{}]", parts.join(","))
        }
        other => scalar(other, true).unwrap(),
    }
}

fn pad(depth: usize) -> String {
    " ".repeat(depth)
}

fn write_object(m: &Map<String, Value>, depth: usize, out: &mut String) {
    for (k, v) in m {
        match v {
            Value::Object(inner) => {
                if inner.is_empty() {
                    continue;
                }
                out.push_str(&format!("{}{}:\n", pad(depth), key(k)));
                write_object(inner, depth + 1, out);
            }
            Value::Array(a) => write_field(&key(k), a, depth, out),
            other => out.push_str(&format!("{}{}: {}\n", pad(depth), key(k), scalar(other, false).unwrap())),
        }
    }
}

/// Columns of a table: union of keys in order of first appearance.
fn table_columns(a: &[Value]) -> Option<Vec<String>> {
    if a.is_empty() || !a.iter().all(|x| x.is_object()) {
        return None;
    }
    let mut cols: Vec<String> = vec![];
    let mut cells = 0usize;
    for x in a {
        let m = x.as_object().unwrap();
        cells += m.len();
        for k in m.keys() {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
        }
    }
    if cols.is_empty() || cols.len() > 40 {
        return None;
    }
    let all_scalar = a
        .iter()
        .all(|x| x.as_object().unwrap().values().all(|v| !v.is_object() && !v.is_array()));
    if a.len() < 2 && !all_scalar {
        return None;
    }
    if (cells as f64) / ((a.len() * cols.len()) as f64) < 0.5 {
        return None;
    }
    Some(cols)
}

/// Longest common prefix of the strings, cut right after its last separator so
/// values stay readable (`.../issues/` rather than `.../issues/1`).
fn common_prefix(vals: &[&str]) -> String {
    let first = vals[0];
    let mut len = first.len();
    for v in &vals[1..] {
        len = len.min(first.bytes().zip(v.bytes()).take_while(|(x, y)| x == y).count());
    }
    let head = &first.as_bytes()[..len];
    match head.iter().rposition(|b| matches!(b, b'/' | b'.' | b'_' | b'-' | b':' | b' ' | b'\\')) {
        Some(i) => first[..i + 1].to_string(),
        None => String::new(),
    }
}

fn write_field(name: &str, a: &[Value], depth: usize, out: &mut String) {
    let p = pad(depth);
    if a.is_empty() {
        return;
    }
    let flat: Vec<Value>;
    let a = if a.iter().all(|x| x.is_object()) {
        flat = a.iter().map(flatten).collect();
        &flat[..]
    } else {
        a
    };
    if let Some(cols) = table_columns(a) {
        // Hoist columns that hold the same value in every row.
        let mut hoisted: Vec<(String, String)> = vec![];
        let mut kept: Vec<String> = vec![];
        for c in &cols {
            let vals: Vec<Option<&Value>> = a.iter().map(|x| x.get(c)).collect();
            let same = a.len() >= 3
                && vals.iter().all(|v| v.is_some())
                && vals.iter().all(|v| v.unwrap() == vals[0].unwrap());
            if same {
                hoisted.push((c.clone(), inline(vals[0].unwrap())));
            } else {
                kept.push(c.clone());
            }
        }
        if kept.is_empty() {
            kept = cols.clone();
            hoisted.clear();
        }
        // Columns of strings sharing a long prefix (URLs, paths): strip and state once.
        let mut prefixes: Vec<(String, String)> = vec![];
        if a.len() >= 3 {
            for c in &kept {
                let vals: Vec<&str> = a.iter().filter_map(|x| x.get(c).and_then(|v| v.as_str())).collect();
                if vals.len() == a.len() {
                    let pre = common_prefix(&vals);
                    if pre.len() >= 10 && vals.iter().all(|v| v.len() > pre.len()) {
                        prefixes.push((c.clone(), pre));
                    }
                }
            }
        }
        out.push_str(&format!("{p}{name}[{}]{{{}}}:\n", a.len(), kept.join(",")));
        if !hoisted.is_empty() {
            let h: Vec<String> = hoisted.iter().map(|(c, v)| format!("{c}={v}")).collect();
            out.push_str(&format!("{p} (every row: {})\n", h.join(", ")));
        }
        if !prefixes.is_empty() {
            let h: Vec<String> = prefixes.iter().map(|(c, v)| format!("{c}={v}")).collect();
            out.push_str(&format!("{p} (values start with: {})\n", h.join(", ")));
        }
        for x in a {
            let m = x.as_object().unwrap();
            let cells: Vec<String> = kept
                .iter()
                .map(|c| match m.get(c) {
                    None => String::new(),
                    Some(v) => match (prefixes.iter().find(|(pc, _)| pc == c), v.as_str()) {
                        (Some((_, pre)), Some(s)) => {
                            // The header already says these are strings, so numeric-looking suffixes need no quotes.
                            let rest = &s[pre.len()..];
                            if rest.contains([',', '\n', '"']) || rest.starts_with(char::is_whitespace) || rest.ends_with(char::is_whitespace) {
                                serde_json::to_string(rest).unwrap()
                            } else {
                                rest.to_string()
                            }
                        }
                        _ => scalar(v, true).unwrap_or_else(|| inline(v)),
                    },
                })
                .collect();
            out.push_str(&format!("{p} {}\n", cells.join(",")));
        }
        return;
    }
    if a.iter().all(|x| !x.is_object() && !x.is_array()) {
        let items: Vec<String> = a.iter().map(|x| scalar(x, true).unwrap()).collect();
        out.push_str(&format!("{p}{name}[{}]: {}\n", a.len(), items.join(",")));
        return;
    }
    out.push_str(&format!("{p}{name}[{}]:\n", a.len()));
    for x in a {
        out.push_str(&format!("{p} {}\n", inline(x)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::estimate;
    use serde_json::json;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn splits_prose_prefix() {
        let s = "Note: limited to 2 nodes\n\n[{\"a\":1},{\"a\":2}]";
        let sp = split(s).unwrap();
        assert!(sp.prefix.contains("limited"));
        assert!(sp.value.is_array());
    }

    #[test]
    fn ignores_log_lines_that_start_with_bracket() {
        assert!(split("[INFO] started\nsomething else happened here").is_none());
    }

    #[test]
    fn table_and_hoist() {
        let v = json!([
            {"id": 1, "state": "open", "user": "ana"},
            {"id": 2, "state": "open", "user": "bob"},
            {"id": 3, "state": "open", "user": "cy"}
        ]);
        let out = render(&v, &cfg());
        assert!(out.contains("[3]{id,user}:"), "{out}");
        assert!(out.contains("every row: state=open"), "{out}");
        assert!(out.contains(" 2,bob"), "{out}");
    }

    #[test]
    fn prunes_nulls_and_rounds_floats() {
        let v = json!({"a": null, "b": "", "c": [], "x": 0.35686275362968445, "keep": 0, "flag": false});
        let out = render(&v, &cfg());
        assert!(!out.contains("a:"));
        assert!(out.contains("x: 0.35686"), "{out}");
        assert!(out.contains("keep: 0"));
        assert!(out.contains("flag: false"));
    }

    #[test]
    fn quotes_ambiguous_strings() {
        let v = json!({"n": "123", "t": "true", "s": "a,b"});
        let out = render(&v, &cfg());
        assert!(out.contains("n: \"123\""));
        assert!(out.contains("t: \"true\""));
    }

    #[test]
    fn rest_noise_links_dropped_and_nested_objects_flattened() {
        let mk = |i: u32, login: &str| {
            json!({
                "id": i, "node_id": "MDQ6", "html_url": format!("https://github.com/x/{i}"),
                "url": format!("https://api.github.com/x/{i}"), "timeline_url": "https://api.github.com/t",
                "user": {"login": login, "type": "User", "site_admin": false}
            })
        };
        let v = json!([mk(1, "ana"), mk(2, "bob"), mk(3, "cy")]);
        let out = render(&v, &cfg());
        assert!(!out.contains("node_id") && !out.contains("timeline_url") && !out.contains("api.github.com"), "{out}");
        assert!(out.contains("html_url"), "{out}");
        assert!(out.contains("user.login"), "{out}");
        assert!(out.contains("user.type=User"), "{out}");
    }

    #[test]
    fn shared_url_prefix_is_hoisted_and_values_stay_recoverable() {
        let rows: Vec<Value> = (1..=4).map(|i| json!({"n": i, "link": format!("https://example.com/items/{i}")})).collect();
        let out = render(&Value::Array(rows), &cfg());
        assert!(out.contains("values start with: link=https://example.com/items/"), "{out}");
        assert!(out.contains("\n 3,3\n"), "{out}");
    }

    #[test]
    fn common_prefix_needs_a_separator_boundary() {
        assert_eq!(common_prefix(&["abcdef", "abcxyz"]), "");
        assert_eq!(common_prefix(&["a/b/c1", "a/b/c2"]), "a/b/");
    }

    #[test]
    fn big_uniform_array_saves_over_half() {
        let rows: Vec<Value> = (0..40)
            .map(|i| json!({"id": i, "name": format!("item{i}"), "url": format!("https://api.example.com/items/{i}"), "meta": null, "kind": "widget"}))
            .collect();
        let v = Value::Array(rows);
        let before = serde_json::to_string_pretty(&v).unwrap();
        let after = render(&v, &cfg());
        assert!(estimate(&after) * 2 < estimate(&before), "{} vs {}", estimate(&after), estimate(&before));
    }
}
