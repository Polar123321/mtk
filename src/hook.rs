//! PostToolUse hook. Reads the payload Claude Code sends on stdin and, when it
//! can shrink the tool's output, answers with `updatedToolOutput` in the exact
//! same structure as the original `tool_response`. Anything else -> no output
//! (Claude Code then keeps the original), so every failure mode is fail-open.

use crate::compress::{self, grep, Ctx};
use crate::config::Config;
use crate::{store, tokens::estimate};
use serde_json::{json, Map, Value};

pub struct Processed {
    pub output: Value,
    pub tool: String,
    pub in_tok: usize,
    pub out_tok: usize,
    pub kinds: Vec<String>,
}

#[derive(Default)]
struct Acc {
    in_tok: usize,
    out_tok: usize,
    kinds: Vec<String>,
}

/// Response keys whose string values are identifiers / binary, never prose.
const SKIP_KEYS: &[&str] = &["filePath", "file_path", "path", "url", "data", "image", "base64", "filenames", "mimeType"];
/// Built-ins that must reach the model byte-for-byte (edits depend on them).
const NEVER: &[&str] = &["Read", "Edit", "Write", "NotebookEdit", "MultiEdit", "Glob", "TodoWrite"];

pub fn process(input: &str, cfg: &Config) -> Option<Processed> {
    if !cfg.enabled {
        return None;
    }
    let payload: Value = serde_json::from_str(input).ok()?;
    if payload.get("hook_event_name").and_then(|v| v.as_str()) != Some("PostToolUse") {
        return None;
    }
    let tool = payload.get("tool_name")?.as_str()?.to_string();
    let resp = payload.get("tool_response")?;
    let command = payload.pointer("/tool_input/command").and_then(|c| c.as_str());
    if let Some(c) = command {
        if cfg.command_is_file_read(c) {
            return None;
        }
    }
    let t = transform(&tool, resp, cfg)?;
    Some(Processed {
        output: json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": t.value}}),
        tool,
        in_tok: t.in_tok,
        out_tok: t.out_tok,
        kinds: t.kinds,
    })
}

pub struct Transformed {
    pub value: Value,
    pub in_tok: usize,
    pub out_tok: usize,
    pub kinds: Vec<String>,
}

/// Agent-independent core: compress one tool result. Used by the Claude Code
/// hook and by the MCP proxy. `None` means "leave the original alone".
pub fn transform(tool: &str, resp: &Value, cfg: &Config) -> Option<Transformed> {
    if !cfg.enabled || NEVER.contains(&tool) || cfg.tool_excluded(tool) || cfg.tool_is_file_read(tool) {
        return None;
    }
    let ctx = Ctx { cfg };
    let mut acc = Acc::default();
    let value = match resp {
        Value::Object(o) if o.contains_key("stdout") => shell(o, &ctx, &mut acc),
        Value::Object(o) if tool == "Grep" => grep_tool(o, &ctx, &mut acc),
        other => walk(other, &ctx, &mut acc),
    };
    if acc.kinds.is_empty() || acc.out_tok >= acc.in_tok {
        return None;
    }
    Some(Transformed { value, in_tok: acc.in_tok, out_tok: acc.out_tok, kinds: acc.kinds })
}

/// Compress one block of text, add the recall footer, and account for it.
fn shrink(text: &str, ctx: &Ctx, acc: &mut Acc) -> Option<String> {
    let cfg = ctx.cfg;
    let t_in = estimate(text);
    if t_in < cfg.min_tokens {
        return None;
    }
    let out = compress::compress_text(text, ctx);
    finish(text, out.text, &out.kind, t_in, ctx, acc)
}

fn finish(original: &str, mut text: String, kind: &str, t_in: usize, ctx: &Ctx, acc: &mut Acc) -> Option<String> {
    let cfg = ctx.cfg;
    let saved = t_in.saturating_sub(estimate(&text));
    if saved == 0 || (saved as f64 * 100.0 / t_in as f64) < cfg.min_gain_pct {
        return None;
    }
    if cfg.footer && cfg.store && saved >= cfg.footer_min_saved {
        if let Some(id) = store::save(original) {
            let pct = saved * 100 / t_in;
            text.push_str(&format!("\n[mtk -{pct}% · full: mtk recall {id}]"));
        }
    }
    let t_out = estimate(&text);
    if t_out >= t_in {
        return None;
    }
    acc.in_tok += t_in;
    acc.out_tok += t_out;
    if !acc.kinds.iter().any(|k| k == kind) {
        acc.kinds.push(kind.to_string());
    }
    Some(text)
}

/// Bash / PowerShell: `{stdout, stderr, interrupted, isImage}`.
fn shell(o: &Map<String, Value>, ctx: &Ctx, acc: &mut Acc) -> Value {
    let mut new = o.clone();
    if o.get("isImage").and_then(|v| v.as_bool()) == Some(true) {
        return Value::Object(new);
    }
    for key in ["stdout", "stderr"] {
        if let Some(Value::String(s)) = o.get(key) {
            if let Some(n) = shrink(s, ctx, acc) {
                new.insert(key.to_string(), Value::String(n));
            }
        }
    }
    Value::Object(new)
}

/// Grep: `{mode, numFiles, filenames, content, numLines, totalLines}`.
fn grep_tool(o: &Map<String, Value>, ctx: &Ctx, acc: &mut Acc) -> Value {
    let mut new = o.clone();
    if let Some(Value::String(content)) = o.get("content") {
        let t_in = estimate(content);
        if t_in >= ctx.cfg.min_tokens {
            let text = grep::compress(content).unwrap_or_else(|| content.clone());
            let out = compress::compress_text(&text, ctx);
            let kind = if text != *content { "grep" } else { out.kind.as_str() };
            let kind = if kind.is_empty() { "grep" } else { kind };
            if let Some(n) = finish(content, out.text, kind, t_in, ctx, acc) {
                new.insert("content".into(), Value::String(n));
            }
        }
    }
    Value::Object(new)
}

/// Generic structural walk: MCP content blocks and any unknown tool shape.
/// Only long string leaves are rewritten; keys, arrays and objects stay put.
fn walk(v: &Value, ctx: &Ctx, acc: &mut Acc) -> Value {
    match v {
        Value::String(s) => shrink(s, ctx, acc).map(Value::String).unwrap_or_else(|| v.clone()),
        Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, ctx, acc)).collect()),
        Value::Object(o) => {
            // Images and other binary blocks pass through untouched.
            if matches!(o.get("type").and_then(|t| t.as_str()), Some("image") | Some("audio") | Some("resource")) {
                return v.clone();
            }
            let mut out = Map::new();
            for (k, x) in o {
                let nv = if SKIP_KEYS.contains(&k.as_str()) { x.clone() } else { walk(x, ctx, acc) };
                out.insert(k.clone(), nv);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(tool: &str, input: Value, resp: Value) -> String {
        json!({"hook_event_name":"PostToolUse","tool_name":tool,"tool_input":input,"tool_response":resp}).to_string()
    }

    fn cfg() -> Config {
        let mut c = Config::default();
        c.store = false;
        c
    }

    fn big_log() -> String {
        (0..300).map(|i| format!("   Compiling crate{i} v0.{i}.0\n")).collect()
    }

    #[test]
    fn shell_output_shrinks_and_keeps_structure() {
        let p = payload(
            "Bash",
            json!({"command": "cargo build"}),
            json!({"stdout": big_log(), "stderr": "", "interrupted": false, "isImage": false}),
        );
        let r = process(&p, &cfg()).expect("should compress");
        let upd = &r.output["hookSpecificOutput"]["updatedToolOutput"];
        assert_eq!(upd["interrupted"], json!(false));
        assert_eq!(upd["stderr"], json!(""));
        assert!(upd["stdout"].as_str().unwrap().len() < 400);
        assert!(r.out_tok < r.in_tok / 5);
    }

    #[test]
    fn mcp_text_blocks_shrink_images_untouched() {
        let rows: Vec<Value> = (0..60).map(|i| json!({"id": i, "state": "open", "title": format!("Issue number {i}"), "body": null})).collect();
        let text = serde_json::to_string_pretty(&rows).unwrap();
        let resp = json!([{"type":"text","text":text},{"type":"image","data":"AAAA","mimeType":"image/png"}]);
        let p = payload("mcp__github__list_issues", json!({}), resp);
        let r = process(&p, &cfg()).expect("should compress");
        let upd = r.output["hookSpecificOutput"]["updatedToolOutput"].as_array().unwrap().clone();
        assert_eq!(upd.len(), 2);
        assert_eq!(upd[1]["data"], json!("AAAA"));
        assert!(upd[0]["text"].as_str().unwrap().contains("[60]{id,title}"));
    }

    #[test]
    fn file_reads_are_never_touched() {
        let big = "x\n".repeat(5000);
        for (tool, cmd) in [("Read", ""), ("mcp__fs__read_text_file", ""), ("Bash", "cat foo.json"), ("Bash", "rtk cat foo.json")] {
            let resp = if tool == "Bash" { json!({"stdout": big, "stderr": ""}) } else { json!(big) };
            let p = payload(tool, json!({"command": cmd}), resp);
            assert!(process(&p, &cfg()).is_none(), "{tool} {cmd}");
        }
    }

    #[test]
    fn piped_cat_is_not_a_plain_file_read() {
        let p = payload("Bash", json!({"command": "cat log.txt | sort"}), json!({"stdout": big_log(), "stderr": ""}));
        assert!(process(&p, &cfg()).is_some());
    }

    #[test]
    fn tiny_output_and_garbage_are_passthrough() {
        let p = payload("Bash", json!({"command": "echo hi"}), json!({"stdout": "hi", "stderr": ""}));
        assert!(process(&p, &cfg()).is_none());
        assert!(process("not json", &cfg()).is_none());
        assert!(process("{}", &cfg()).is_none());
    }

    #[test]
    fn disabled_config_is_passthrough() {
        let mut c = cfg();
        c.enabled = false;
        let p = payload("Bash", json!({"command": "x"}), json!({"stdout": big_log(), "stderr": ""}));
        assert!(process(&p, &c).is_none());
    }

    #[test]
    fn grep_content_is_grouped() {
        let content: String = (1..=30).map(|i| format!("C:\\proj\\src\\module\\a.rs:{i}:    let value_{i} = compute({i});\n")).collect();
        let resp = json!({"mode":"content","numFiles":1,"filenames":[],"content":content,"numLines":30,"totalLines":30});
        let r = process(&payload("Grep", json!({}), resp), &cfg()).expect("compress");
        let upd = &r.output["hookSpecificOutput"]["updatedToolOutput"];
        assert!(upd["content"].as_str().unwrap().starts_with("root: C:\\proj\\src\\module\\"));
        assert_eq!(upd["numLines"], json!(30));
    }
}
