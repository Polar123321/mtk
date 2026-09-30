//! The compression pipeline. Every stage is optional and the whole thing is
//! guarded by "never return something bigger than the normalized input".

pub mod budget;
pub mod grep;
pub mod json;
pub mod normalize;
pub mod roblox;
pub mod text;

use crate::config::Config;
use crate::tokens::estimate;

pub struct Ctx<'a> {
    pub cfg: &'a Config,
}

pub struct Outcome {
    pub text: String,
    /// Which compressors did the work, e.g. "roblox-tree" or "log+budget".
    pub kind: String,
}

pub fn compress_text(input: &str, ctx: &Ctx) -> Outcome {
    let normalized = normalize::run(input);
    let mut kinds: Vec<&'static str> = vec![];
    let mut current = normalized.clone();

    if ctx.cfg.json {
        if let Some(sp) = json::split(&current) {
            let (rendered, kind) = match roblox::render(&sp.value, ctx.cfg) {
                Some(r) => (r.text, r.kind),
                None => (json::render(&sp.value, ctx.cfg), "json"),
            };
            let joined = sp.join(&rendered);
            if estimate(&joined) < estimate(&current) {
                current = joined;
                kinds.push(kind);
            }
        }
    }

    if kinds.is_empty() {
        let capped = normalize::cap_lines(&current);
        let (collapsed, changed) = text::collapse(&capped);
        if changed && estimate(&collapsed) < estimate(&capped) {
            current = collapsed;
            kinds.push("log");
        } else if capped != current {
            current = capped;
            kinds.push("long-lines");
        }
    }

    let (budgeted, cut) = budget::apply(&current, ctx.cfg.max_tokens);
    if cut {
        current = budgeted;
        kinds.push("budget");
    }

    if estimate(&current) >= estimate(&normalized) {
        current = normalized.clone();
        kinds.clear();
    }
    if kinds.is_empty() && current != input {
        kinds.push("normalize");
    }
    Outcome { text: current, kind: kinds.join("+") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(cfg: &Config) -> Ctx<'_> {
        Ctx { cfg }
    }

    #[test]
    fn never_returns_bigger_than_input() {
        let cfg = Config::default();
        for input in ["hello world", "{\"a\":1}", "a\nb\nc\n", "x"] {
            let out = compress_text(input, &ctx(&cfg));
            assert!(estimate(&out.text) <= estimate(input), "grew: {input:?} -> {:?}", out.text);
        }
    }

    #[test]
    fn errors_survive_the_budget() {
        let mut cfg = Config::default();
        cfg.max_tokens = 200;
        let mut s = String::new();
        for i in 0..3000 {
            s.push_str(&format!("line number {i} doing ordinary work with some words\n"));
            if i == 1500 {
                s.push_str("ERROR: database connection refused at 10.0.0.5\n");
            }
        }
        let out = compress_text(&s, &ctx(&cfg));
        assert!(out.text.contains("database connection refused"));
        assert!(estimate(&out.text) < 600);
    }
}
