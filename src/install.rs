//! `mtk install` / `mtk uninstall`: merge our PostToolUse hook into the Claude
//! Code settings.json without touching anything else in it.

use serde_json::{json, Map, Value};
use std::path::PathBuf;

const MATCHER: &str = "Bash|PowerShell|Grep|WebFetch|mcp__.*";
const MARKER: &str = "mtk hook";

pub fn settings_path(project: bool) -> PathBuf {
    if project {
        return PathBuf::from(".claude").join("settings.json");
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude").join("settings.json")
}

fn on_path() -> bool {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ""] } else { &[""] };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| exts.iter().any(|e| d.join(format!("mtk{e}")).is_file())))
        .unwrap_or(false)
}

pub fn hook_command() -> String {
    if on_path() {
        return MARKER.to_string();
    }
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| "mtk".into());
    format!("\"{exe}\" hook")
}

fn is_ours(entry: &Value) -> bool {
    entry["hooks"]
        .as_array()
        .map(|hs| hs.iter().any(|h| h["command"].as_str().map(|c| c.contains(MARKER)).unwrap_or(false)))
        .unwrap_or(false)
}

fn load(path: &PathBuf) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => Ok(json!({})),
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{} is not valid JSON: {e}", path.display())),
        Err(_) => Ok(json!({})),
    }
}

fn save(path: &PathBuf, v: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let bak = path.with_extension("json.mtk.bak");
        std::fs::copy(path, &bak).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap() + "\n").map_err(|e| e.to_string())
}

/// (event, matcher): the post hook compresses output, the pre hook guards big `Read`s.
const HOOKS: &[(&str, &str)] = &[("PostToolUse", MATCHER), ("PreToolUse", "Read")];

pub fn install(project: bool, dry_run: bool) -> Result<String, String> {
    let path = settings_path(project);
    let mut root = load(&path)?;
    let obj = root.as_object_mut().ok_or("settings.json root is not an object")?;
    let hooks = obj.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let cmd = hook_command();
    for (event, matcher) in HOOKS {
        let arr = hooks
            .as_object_mut()
            .ok_or("`hooks` is not an object")?
            .entry(event.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{event}` is not an array"))?;
        arr.retain(|e| !is_ours(e));
        arr.push(json!({"matcher": matcher, "hooks": [{"type": "command", "command": cmd, "timeout": 10}]}));
    }
    if dry_run {
        return Ok(format!("Would write to {}:
{}", path.display(), serde_json::to_string_pretty(&root).unwrap()));
    }
    save(&path, &root)?;
    Ok(format!(
        "Installed MTK hooks in {}
  PostToolUse matcher: {MATCHER}
  PreToolUse  matcher: Read (big-file guard)
  command: {cmd}",
        path.display()
    ))
}

pub fn uninstall(project: bool) -> Result<String, String> {
    let path = settings_path(project);
    let mut root = load(&path)?;
    let mut removed = 0;
    for (event, _) in HOOKS {
        let ptr = format!("/hooks/{event}");
        let mut now_empty = false;
        if let Some(arr) = root.pointer_mut(&ptr).and_then(|v| v.as_array_mut()) {
            let before = arr.len();
            arr.retain(|e| !is_ours(e));
            removed += before - arr.len();
            now_empty = arr.is_empty();
        }
        if now_empty {
            root["hooks"].as_object_mut().unwrap().remove(*event);
        }
    }
    if removed == 0 {
        return Ok(format!("No MTK hook found in {}", path.display()));
    }
    save(&path, &root)?;
    Ok(format!("Removed {removed} MTK hook entr{} from {}", if removed == 1 { "y" } else { "ies" }, path.display()))
}

pub fn is_installed(project: bool) -> bool {
    load(&settings_path(project))
        .ok()
        .map(|r| {
            HOOKS.iter().all(|(event, _)| {
                r.pointer(&format!("/hooks/{event}")).and_then(|v| v.as_array()).map(|a| a.iter().any(is_ours)).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}
