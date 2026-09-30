use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Master switch. `MTK_DISABLE=1` also turns it off.
    pub enabled: bool,
    /// Outputs smaller than this many (estimated) tokens are left alone.
    pub min_tokens: usize,
    /// Hard budget: anything still bigger gets head + tail + rescued error lines.
    pub max_tokens: usize,
    /// Skip the rewrite unless it saves at least this percentage.
    pub min_gain_pct: f64,
    /// Append the `[mtk -X% · full: mtk recall <id>]` footer.
    pub footer: bool,
    /// Footer is only worth its tokens when at least this many were saved.
    pub footer_min_saved: usize,
    /// Keep the untouched original on disk for `mtk recall`.
    pub store: bool,
    /// Structured (JSON / Roblox) rewriting.
    pub json: bool,
    /// Drop `node_id`, `gravatar_id` and auxiliary `*_url` links when an object
    /// already has a canonical `html_url` / `url` (GitHub-style REST payloads).
    pub drop_noise_keys: bool,
    /// PreToolUse guard: first whole-file `Read` of a big source file gets an outline instead.
    pub read_guard: bool,
    pub read_guard_min_lines: usize,
    /// Floats with more than 8 decimals are rounded to this many.
    pub float_decimals: usize,
    /// Tools (exact name or regex-free substring) that are never touched.
    pub exclude_tools: Vec<String>,
    /// Tool-name substrings that mean "returns file contents verbatim".
    pub passthrough_tool_substrings: Vec<String>,
    /// Shell commands (first word) that print file contents verbatim.
    pub passthrough_commands: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            min_tokens: 60,
            max_tokens: 4000,
            min_gain_pct: 8.0,
            footer: true,
            footer_min_saved: 150,
            store: true,
            json: true,
            drop_noise_keys: true,
            read_guard: true,
            read_guard_min_lines: 500,
            float_decimals: 5,
            exclude_tools: vec![],
            passthrough_tool_substrings: [
                "read_file",
                "read_text",
                "read_multiple",
                "get_file",
                "file_content",
                "script_read",
                "read_script",
                "get_script",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            passthrough_commands: ["cat", "type", "head", "tail", "less", "more", "bat", "sed", "Get-Content", "gc"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

pub fn mtk_home() -> PathBuf {
    if let Ok(p) = std::env::var("MTK_HOME") {
        return PathBuf::from(p);
    }
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(".mtk")
}

impl Config {
    pub fn load() -> Config {
        let mut cfg = std::fs::read_to_string(mtk_home().join("config.toml"))
            .ok()
            .and_then(|t| toml::from_str::<Config>(&t).ok())
            .unwrap_or_default();
        if std::env::var("MTK_DISABLE").map(|v| v == "1").unwrap_or(false) {
            cfg.enabled = false;
        }
        cfg
    }

    pub fn tool_excluded(&self, tool: &str) -> bool {
        self.exclude_tools.iter().any(|t| tool == t || tool.contains(t.as_str()))
    }

    pub fn tool_is_file_read(&self, tool: &str) -> bool {
        let lower = tool.to_lowercase();
        self.passthrough_tool_substrings.iter().any(|s| lower.contains(&s.to_lowercase()))
    }

    /// True when the shell command just dumps a file (so exact bytes matter).
    pub fn command_is_file_read(&self, command: &str) -> bool {
        let cmd = command.trim();
        let cmd = cmd.strip_prefix("rtk ").unwrap_or(cmd);
        if cmd.contains('|') || cmd.contains("&&") || cmd.contains(';') {
            return false;
        }
        let first = cmd.split_whitespace().next().unwrap_or("");
        self.passthrough_commands.iter().any(|c| c.eq_ignore_ascii_case(first))
    }
}
