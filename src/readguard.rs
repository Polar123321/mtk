//! PreToolUse guard for `Read`. A whole-file read of a big source file is the
//! single most wasteful thing an agent does: thousands of lines in, a handful
//! used. The first time it happens per file per session we deny with an outline
//! (symbols + line spans) so the agent re-reads just the range it needs. A
//! repeated identical read goes through, so legitimate full reads still work.

use crate::config::{mtk_home, Config};
use crate::{outline, tokens::estimate};
use serde_json::{json, Value};
use std::io::Write;

fn seen_file(home: &std::path::Path, session: &str) -> std::path::PathBuf {
    let safe: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    home.join("seen").join(if safe.is_empty() { "nosession".into() } else { safe })
}

fn already_seen(home: &std::path::Path, session: &str, path: &str) -> bool {
    std::fs::read_to_string(seen_file(home, session)).map(|t| t.lines().any(|l| l == path)).unwrap_or(false)
}

fn mark_seen(home: &std::path::Path, session: &str, path: &str) {
    let f = seen_file(home, session);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
        let _ = writeln!(h, "{path}");
    }
}

/// `Some(json)` = deny with this reason; `None` = let the read through.
pub fn handle(input: &str, cfg: &Config) -> Option<String> {
    handle_in(input, cfg, &mtk_home())
}

fn handle_in(input: &str, cfg: &Config, home: &std::path::Path) -> Option<String> {
    if !cfg.enabled || !cfg.read_guard {
        return None;
    }
    let v: Value = serde_json::from_str(input).ok()?;
    if v.get("hook_event_name")?.as_str()? != "PreToolUse" || v.get("tool_name")?.as_str()? != "Read" {
        return None;
    }
    let ti = v.get("tool_input")?;
    if ti.get("offset").is_some() || ti.get("limit").is_some() || ti.get("pages").is_some() {
        return None; // already a targeted read
    }
    let path = ti.get("file_path")?.as_str()?;
    let text = std::fs::read_to_string(path).ok()?;
    let lines = text.lines().count();
    if lines < cfg.read_guard_min_lines {
        return None;
    }
    let syms = outline::extract(path, &text);
    if syms.len() < 3 {
        return None;
    }
    let session = v.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
    if already_seen(home, session, path) {
        return None;
    }
    mark_seen(home, session, path);

    let reason = format!(
        "mtk: {path} has {lines} lines (~{} tokens). Outline (start-end kind name):\n{}\n\n\
         Read only what you need: Read with offset=<start> and limit=<end-start+1>. \
         If you really need the whole file, repeat the same Read and it will go through.",
        estimate(&text),
        outline::render(&syms, 120)
    );
    Some(
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }})
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big_rust(n: usize) -> String {
        (0..n).map(|i| format!("fn f{i}() {{\n    let x = {i};\n}}\n")).collect()
    }

    fn payload(path: &str, extra: Value, session: &str) -> String {
        let mut ti = json!({"file_path": path});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            ti[k] = v;
        }
        json!({"hook_event_name":"PreToolUse","tool_name":"Read","session_id":session,"tool_input":ti}).to_string()
    }

    fn home_of(f: &std::path::Path) -> std::path::PathBuf {
        f.parent().unwrap().join("home")
    }

    fn setup(name: &str, lines: usize) -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!("mtk-rg-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("big.rs");
        std::fs::write(&f, big_rust(lines)).unwrap();
        (f, Config::default())
    }

    #[test]
    fn denies_first_full_read_then_allows_the_repeat() {
        let (f, cfg) = setup("first", 400);
        let p = payload(f.to_str().unwrap(), json!({}), "sess-a");
        let out = handle_in(&p, &cfg, &home_of(&f)).expect("first read denied");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], json!("deny"));
        let reason = v["hookSpecificOutput"]["permissionDecisionReason"].as_str().unwrap();
        assert!(reason.contains("1-3 fn f0") && reason.contains("offset"), "{reason}");
        assert!(handle_in(&p, &cfg, &home_of(&f)).is_none(), "repeat must be allowed");
        // a different session is nudged again
        assert!(handle_in(&payload(f.to_str().unwrap(), json!({}), "sess-b"), &cfg, &home_of(&f)).is_some());
    }

    #[test]
    fn targeted_small_and_disabled_reads_pass() {
        let (f, mut cfg) = setup("pass", 400);
        let path = f.to_str().unwrap();
        assert!(handle_in(&payload(path, json!({"offset": 10, "limit": 20}), "s1"), &cfg, &home_of(&f)).is_none());
        let (small, cfg2) = setup("small", 50);
        assert!(handle_in(&payload(small.to_str().unwrap(), json!({}), "s2"), &cfg2, &home_of(&small)).is_none());
        let (f3, _) = setup("dis", 400);
        cfg.read_guard = false;
        assert!(handle_in(&payload(f3.to_str().unwrap(), json!({}), "s3"), &cfg, &home_of(&f3)).is_none());
        assert!(handle_in("garbage", &cfg, &home_of(&f)).is_none());
    }
}
