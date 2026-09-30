use crate::config::mtk_home;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn record(tool: &str, kinds: &[&str], in_tok: usize, out_tok: usize) {
    if cfg!(test) {
        return; // unit tests must never write into the user's real stats
    }
    let dir = mtk_home();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = json!({"t": ts, "tool": tool, "kind": kinds.join("+"), "in": in_tok, "out": out_tok});
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("stats.jsonl")) {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Default)]
struct Agg {
    n: usize,
    i: usize,
    o: usize,
}

fn bar(pct: f64) -> String {
    let filled = ((pct / 100.0) * 24.0).round() as usize;
    format!("{}{}", "█".repeat(filled.min(24)), "░".repeat(24 - filled.min(24)))
}

pub fn gain() -> String {
    let text = std::fs::read_to_string(mtk_home().join("stats.jsonl")).unwrap_or_default();
    let mut total = Agg::default();
    let mut by_tool: BTreeMap<String, Agg> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, Agg> = BTreeMap::new();
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let i = v["in"].as_u64().unwrap_or(0) as usize;
        let o = v["out"].as_u64().unwrap_or(0) as usize;
        let tool = v["tool"].as_str().unwrap_or("?").to_string();
        let kind = v["kind"].as_str().unwrap_or("?").to_string();
        for a in [&mut total, by_tool.entry(tool).or_default(), by_kind.entry(kind).or_default()] {
            a.n += 1;
            a.i += i;
            a.o += o;
        }
    }
    if total.n == 0 {
        return "MTK: nothing recorded yet. Run `mtk install`, use your agent, then come back.\n".into();
    }
    let pct = |a: &Agg| if a.i == 0 { 0.0 } else { (a.i - a.o.min(a.i)) as f64 * 100.0 / a.i as f64 };
    let mut out = String::new();
    out.push_str("MTK token savings (estimated tokens)\n");
    out.push_str(&"═".repeat(60));
    out.push('\n');
    out.push_str(&format!("Outputs compressed: {}\n", total.n));
    out.push_str(&format!("Tokens in:          {}\n", total.i));
    out.push_str(&format!("Tokens out:         {}\n", total.o));
    out.push_str(&format!("Tokens saved:       {} ({:.1}%)\n", total.i.saturating_sub(total.o), pct(&total)));
    out.push_str(&format!("Efficiency:         {} {:.1}%\n\n", bar(pct(&total)), pct(&total)));
    for (title, map) in [("By tool", &by_tool), ("By compressor", &by_kind)] {
        out.push_str(&format!("{title}\n{}\n", "─".repeat(60)));
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by_key(|(_, a)| std::cmp::Reverse(a.i.saturating_sub(a.o)));
        for (name, a) in rows.iter().take(12) {
            let short: String = name.chars().take(34).collect();
            out.push_str(&format!(
                "{:<36}{:>5}x {:>8} saved {:>6.1}%\n",
                short,
                a.n,
                a.i.saturating_sub(a.o),
                pct(a)
            ));
        }
        out.push('\n');
    }
    out
}
