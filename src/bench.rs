//! `mtk bench <files|dirs>`: run the pipeline on saved outputs and print a
//! savings table.

use crate::compress::{self, Ctx};
use crate::config::Config;
use crate::tokens::estimate;
use std::path::{Path, PathBuf};

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            collect(&e, out);
        }
    } else if p.is_file() {
        out.push(p.to_path_buf());
    }
}

pub fn run(paths: &[String], cfg: &Config) -> String {
    let mut files = vec![];
    for p in paths {
        collect(Path::new(p), &mut files);
    }
    let mut out = format!("{:<44}{:>8}{:>8}{:>8}  {}\n", "fixture", "before", "after", "saved", "compressor");
    out.push_str(&"─".repeat(90));
    out.push('\n');
    let (mut ti, mut to) = (0usize, 0usize);
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let ctx = Ctx { cfg };
        let before = estimate(&text);
        let res = compress::compress_text(&text, &ctx);
        let after = estimate(&res.text).min(before);
        ti += before;
        to += after;
        let pct = if before == 0 { 0.0 } else { (before - after) as f64 * 100.0 / before as f64 };
        let name: String = stem.chars().take(42).collect();
        out.push_str(&format!("{:<44}{:>8}{:>8}{:>7.1}%  {}\n", name, before, after, pct, res.kind));
    }
    out.push_str(&"─".repeat(90));
    out.push('\n');
    let pct = if ti == 0 { 0.0 } else { (ti - to) as f64 * 100.0 / ti as f64 };
    out.push_str(&format!("{:<44}{:>8}{:>8}{:>7.1}%\n", "TOTAL", ti, to, pct));
    out
}
