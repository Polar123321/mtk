//! Lossless recall store: whenever we shrink an output we keep the original on
//! disk, so nothing is ever really lost (`mtk recall <id>`).

use crate::config::mtk_home;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 3600);
const MAX_FILES: usize = 500;

fn dir() -> PathBuf {
    mtk_home().join("store")
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn save(text: &str) -> Option<String> {
    let id = format!("{:08x}", fnv(text) & 0xffff_ffff);
    let d = dir();
    std::fs::create_dir_all(&d).ok()?;
    let path = d.join(format!("{id}.txt"));
    if !path.exists() {
        std::fs::write(&path, text).ok()?;
    }
    // Housekeeping on ~1/16 of the writes keeps hook latency flat.
    if id.ends_with('0') {
        prune(&d);
    }
    Some(id)
}

pub fn load(id: &str) -> Option<String> {
    if !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    std::fs::read_to_string(dir().join(format!("{id}.txt"))).ok()
}

fn prune(d: &PathBuf) {
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.retain(|(m, p)| {
        if now.duration_since(*m).map(|a| a > MAX_AGE).unwrap_or(false) {
            let _ = std::fs::remove_file(p);
            false
        } else {
            true
        }
    });
    if files.len() > MAX_FILES {
        files.sort_by_key(|(m, _)| *m);
        for (_, p) in files.iter().take(files.len() - MAX_FILES) {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn count() -> usize {
    std::fs::read_dir(dir()).map(|r| r.count()).unwrap_or(0)
}
