//! `mtk mcp-proxy [--name N] -- <command> [args...]`
//!
//! A transparent stdio proxy for any MCP server. It launches the real server,
//! forwards every byte the client sends, and rewrites only the *results* of
//! `tools/call` on the way back (newline-delimited JSON-RPC). This is how MTK
//! reaches agents whose hooks cannot rewrite tool output (Codex, Cursor...).
//!
//! Fail-open by construction: a line that is not a `tools/call` result, or that
//! cannot be compressed, is forwarded byte-for-byte.

use crate::config::Config;
use crate::{hook, stats};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

type Pending = Arc<Mutex<HashMap<String, String>>>;

pub fn run(args: &[String], cfg: &Config) -> i32 {
    let Some(sep) = args.iter().position(|a| a == "--") else {
        eprintln!("usage: mtk mcp-proxy [--name NAME] -- <command> [args...]");
        return 2;
    };
    let server = args[..sep]
        .iter()
        .position(|a| a == "--name")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "mcp".into());
    let cmd = &args[sep + 1..];
    let Some(program) = cmd.first() else {
        eprintln!("mtk mcp-proxy: missing server command after --");
        return 2;
    };

    let mut child = match Command::new(program)
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mtk mcp-proxy: cannot start `{program}`: {e}");
            return 1;
        }
    };
    let mut child_in = child.stdin.take().unwrap();
    let child_out = child.stdout.take().unwrap();
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

    // client -> server: verbatim, only peeking at tools/call to learn tool names.
    let pending_w = Arc::clone(&pending);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(std::io::stdin().lock());
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            note_request(&buf, &pending_w);
            if child_in.write_all(&buf).is_err() || child_in.flush().is_err() {
                break;
            }
        }
        // Client closed stdin: dropping child_in tells the server to finish.
    });

    // server -> client: compress tools/call results.
    let mut reader = BufReader::new(child_out);
    let mut out = std::io::stdout().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let forwarded = rewrite(&buf, &pending, &server, cfg).unwrap_or_else(|| buf.clone());
        if out.write_all(&forwarded).is_err() || out.flush().is_err() {
            break;
        }
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(0)
}

fn note_request(line: &[u8], pending: &Pending) {
    let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
    if v.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return;
    }
    if let (Some(id), Some(name)) = (v.get("id"), v.pointer("/params/name").and_then(|n| n.as_str())) {
        pending.lock().unwrap().insert(id.to_string(), name.to_string());
    }
}

/// Some(new line) only when a tools/call result was actually shrunk.
fn rewrite(line: &[u8], pending: &Pending, server: &str, cfg: &Config) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(line).ok()?;
    let id = v.get("id")?.to_string();
    let tool = pending.lock().unwrap().remove(&id)?;
    let content = v.pointer("/result/content")?;
    if v.pointer("/result/isError").and_then(|e| e.as_bool()) == Some(true) {
        return None; // never touch error results
    }
    let full_name = format!("mcp__{server}__{tool}");
    let t = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook::transform(&full_name, content, cfg))).ok()??;
    *v.pointer_mut("/result/content")? = t.value;
    let kinds: Vec<&str> = t.kinds.iter().map(|s| s.as_str()).collect();
    stats::record(&full_name, &kinds, t.in_tok, t.out_tok);
    let mut bytes = serde_json::to_vec(&v).ok()?;
    bytes.push(b'\n');
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.store = false;
        c
    }

    fn pending_with(id: u64, tool: &str) -> Pending {
        let p: Pending = Arc::new(Mutex::new(HashMap::new()));
        p.lock().unwrap().insert(id.to_string(), tool.to_string());
        p
    }

    fn line(v: Value) -> Vec<u8> {
        let mut b = serde_json::to_vec(&v).unwrap();
        b.push(b'\n');
        b
    }

    #[test]
    fn compresses_tool_call_results_and_keeps_envelope() {
        let text: String = (0..300).map(|i| format!("   Compiling crate{i} v0.{i}.0\n")).collect();
        let msg = line(json!({"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":text}],"isError":false}}));
        let out = rewrite(&msg, &pending_with(7, "run_build"), "srv", &cfg()).expect("rewritten");
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["id"], json!(7));
        assert_eq!(v["jsonrpc"], json!("2.0"));
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().len() < 300);
        assert!(out.ends_with(b"\n"));
    }

    #[test]
    fn passthrough_cases_return_none() {
        let big: String = (0..300).map(|i| format!("   Compiling crate{i} v0.{i}.0\n")).collect();
        let ok = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":big}]}});
        // unknown id (not a tools/call we saw), notification, error result, file-read tool
        assert!(rewrite(&line(ok.clone()), &pending_with(2, "x"), "s", &cfg()).is_none());
        assert!(rewrite(&line(json!({"jsonrpc":"2.0","method":"notifications/progress"})), &pending_with(1, "x"), "s", &cfg()).is_none());
        let mut err = ok.clone();
        err["result"]["isError"] = json!(true);
        assert!(rewrite(&line(err), &pending_with(1, "x"), "s", &cfg()).is_none());
        assert!(rewrite(&line(ok), &pending_with(1, "script_read"), "s", &cfg()).is_none());
        assert!(rewrite(b"not json\n", &pending_with(1, "x"), "s", &cfg()).is_none());
    }

    #[test]
    fn learns_tool_names_from_requests() {
        let p: Pending = Arc::new(Mutex::new(HashMap::new()));
        note_request(&line(json!({"jsonrpc":"2.0","id":"abc","method":"tools/call","params":{"name":"inspect_instance"}})), &p);
        note_request(&line(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})), &p);
        assert_eq!(p.lock().unwrap().get("\"abc\"").map(|s| s.as_str()), Some("inspect_instance"));
        assert_eq!(p.lock().unwrap().len(), 1);
    }
}
