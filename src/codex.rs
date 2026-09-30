//! `mtk mcp wrap|unwrap|status <server>`: route a stdio MCP server declared in
//! Codex's `config.toml` through `mtk mcp-proxy`. Edited as text so the user's
//! comments and formatting survive; a `.mtk.bak` copy is written first.

use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    if let Ok(h) = std::env::var("CODEX_HOME") {
        return PathBuf::from(h).join("config.toml");
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".codex").join("config.toml")
}

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

struct Section {
    /// Line range of the `[mcp_servers.NAME]` body (header excluded).
    start: usize,
    end: usize,
    command: String,
    args: Vec<String>,
    command_line: usize,
    args_line: Option<usize>,
}

fn find_section(lines: &[String], server: &str) -> Result<Section, String> {
    let header = format!("[mcp_servers.{server}]");
    let quoted = format!("[mcp_servers.\"{server}\"]");
    let h = lines
        .iter()
        .position(|l| l.trim() == header || l.trim() == quoted)
        .ok_or_else(|| format!("no [mcp_servers.{server}] section in the config"))?;
    let end = lines[h + 1..].iter().position(|l| l.trim_start().starts_with('[')).map(|i| h + 1 + i).unwrap_or(lines.len());
    let body = lines[h + 1..end].join("\n");
    let parsed: toml::Table = toml::from_str(&body).map_err(|e| format!("cannot parse section: {e}"))?;
    let command = parsed.get("command").and_then(|c| c.as_str()).ok_or("section has no `command` (only stdio servers can be wrapped)")?.to_string();
    let args: Vec<String> = parsed
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let key_line = |key: &str| (h + 1..end).find(|&i| lines[i].trim_start().starts_with(&format!("{key} =")) || lines[i].trim_start().starts_with(&format!("{key}=")));
    let command_line = key_line("command").ok_or("`command` must be on a single line")?;
    let args_line = key_line("args");
    if let Some(a) = args_line {
        if !lines[a].contains(']') {
            return Err("multi-line `args` arrays are not supported; put args on one line".into());
        }
    }
    Ok(Section { start: h + 1, end, command, args, command_line, args_line })
}

fn is_wrapped(s: &Section) -> bool {
    s.args.first().map(|a| a == "mcp-proxy").unwrap_or(false)
}

fn load(path: &PathBuf) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(text.lines().map(|l| l.to_string()).collect())
}

fn save(path: &PathBuf, lines: &[String]) -> Result<(), String> {
    let bak = path.with_extension("toml.mtk.bak");
    std::fs::copy(path, &bak).map_err(|e| e.to_string())?;
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn apply(lines: &mut Vec<String>, s: &Section, command: &str, args: &[String]) {
    let args_str = format!("args = [{}]", args.iter().map(|a| toml_str(a)).collect::<Vec<_>>().join(", "));
    lines[s.command_line] = format!("command = {}", toml_str(command));
    match s.args_line {
        Some(i) => lines[i] = args_str,
        None => lines.insert(s.command_line + 1, args_str),
    }
}

pub fn wrap(server: &str, path: &PathBuf, mtk_exe: &str) -> Result<String, String> {
    let mut lines = load(path)?;
    let s = find_section(&lines, server)?;
    if is_wrapped(&s) {
        return Ok(format!("{server} is already wrapped by MTK"));
    }
    let mut new_args: Vec<String> = vec!["mcp-proxy".into(), "--name".into(), server.into(), "--".into(), s.command.clone()];
    new_args.extend(s.args.iter().cloned());
    apply(&mut lines, &s, mtk_exe, &new_args);
    save(path, &lines)?;
    Ok(format!("Wrapped [mcp_servers.{server}] in {}\n  was: {} {}\n  now: {mtk_exe} {}", path.display(), s.command, s.args.join(" "), new_args.join(" ")))
}

pub fn unwrap(server: &str, path: &PathBuf) -> Result<String, String> {
    let mut lines = load(path)?;
    let s = find_section(&lines, server)?;
    if !is_wrapped(&s) {
        return Ok(format!("{server} is not wrapped by MTK"));
    }
    let sep = s.args.iter().position(|a| a == "--").ok_or("wrapped entry has no `--` separator")?;
    let original = &s.args[sep + 1..];
    let (cmd, rest) = original.split_first().ok_or("wrapped entry has no original command")?;
    apply(&mut lines, &s, cmd, rest);
    save(path, &lines)?;
    Ok(format!("Restored [mcp_servers.{server}]: {cmd} {}", rest.join(" ")))
}

pub fn status(server: &str, path: &PathBuf) -> Result<String, String> {
    let lines = load(path)?;
    let s = find_section(&lines, server)?;
    let _ = (s.start, s.end);
    Ok(format!("{server}: {}", if is_wrapped(&s) { "wrapped by MTK" } else { "not wrapped" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "model = \"x\"\n\n[mcp_servers.Other]\ncommand = \"other.exe\"\n\n[mcp_servers.RobloxStudio]\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"C:\\\\Users\\\\me\\\\mcp.bat\"]\nstartup_timeout_sec = 30\n\n[features]\nmemories = true\n";

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("mtk-test-{}-{name}.toml", std::process::id()));
        std::fs::write(&p, CFG).unwrap();
        p
    }

    #[test]
    fn wrap_then_unwrap_restores_exactly() {
        let p = tmp("roundtrip");
        wrap("RobloxStudio", &p, "C:\\bin\\mtk.exe").unwrap();
        let wrapped = std::fs::read_to_string(&p).unwrap();
        let t: toml::Table = toml::from_str(&wrapped).unwrap();
        let rs = &t["mcp_servers"]["RobloxStudio"];
        assert_eq!(rs["command"].as_str(), Some("C:\\bin\\mtk.exe"));
        let args: Vec<&str> = rs["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(args, ["mcp-proxy", "--name", "RobloxStudio", "--", "cmd.exe", "/c", "C:\\Users\\me\\mcp.bat"]);
        assert!(wrapped.contains("startup_timeout_sec = 30") && wrapped.contains("command = \"other.exe\""));
        assert!(status("RobloxStudio", &p).unwrap().contains("wrapped by MTK"));
        assert!(wrap("RobloxStudio", &p, "x").unwrap().contains("already wrapped"));
        unwrap("RobloxStudio", &p).unwrap();
        let back = std::fs::read_to_string(&p).unwrap();
        let (a, b): (toml::Table, toml::Table) = (toml::from_str(&back).unwrap(), toml::from_str(CFG).unwrap());
        assert_eq!(a, b, "unwrap must restore the same configuration");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("toml.mtk.bak"));
    }

    #[test]
    fn unknown_or_non_stdio_server_is_an_error() {
        let p = tmp("errors");
        assert!(wrap("Nope", &p, "x").is_err());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("toml.mtk.bak"));
    }
}
