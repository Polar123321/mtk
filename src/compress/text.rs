//! Log / shell-output compression: collapse noise runs (`Compiling x`, `PASS y`,
//! progress), consecutive near-identical lines, and far-apart exact repeats.

use std::collections::HashMap;

fn noise_class(l: &str) -> Option<&'static str> {
    if re!(r"^\s*(Compiling|Downloaded|Downloading|Fresh|Checking|Updating|Locking|Adding|Blocking|Packaging|Verifying) \S").is_match(l) {
        Some("build-progress")
    } else if re!(r"^\s*npm (http|notice|warn (deprecated|old|EBADENGINE))").is_match(l) {
        Some("npm")
    } else if re!(r"(?i)^\s*(remote: (counting|compressing|enumerating|total|resolving)|receiving objects|resolving deltas|unpacking objects|writing objects|counting objects|compressing objects)").is_match(l) {
        Some("git-progress")
    } else if re!(r"^\s*(✓|✔|PASS\b|\.+\s*(\[\s*\d+%\])?\s*$|test .+ \.\.\. ok$)").is_match(l) {
        Some("pass")
    } else if re!(r"^\s*(\d{1,3}%\||[#=>\-\.]{10,}\s*\d*%?\s*$|(Fetching|Extracting|Pulling|Waiting|Retrieving) )").is_match(l) {
        Some("progress")
    } else {
        None
    }
}

/// Lines that only differ by numbers / hashes / ids share a signature.
fn signature(l: &str) -> String {
    let s = re!(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").replace_all(l, "U");
    let s = re!(r"\b[0-9a-f]{8,}\b").replace_all(&s, "H");
    re!(r"\d+").replace_all(&s, "#").into_owned()
}

fn is_stacky(l: &str) -> bool {
    re!(r#"^\s*(at |File "|Script '|Stack |#\d+ |from |in |-->|\|)"#).is_match(l)
}

pub fn collapse(s: &str) -> (String, bool) {
    let lines: Vec<&str> = s.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = false;

    let mut i = 0;
    while i < lines.len() {
        let class = noise_class(lines[i]);
        let sig = signature(lines[i]);
        let runnable = class.is_some() || lines[i].trim().chars().count() >= 8;
        let mut j = i + 1;
        if runnable {
            while j < lines.len() {
                let same = match class {
                    Some(c) => noise_class(lines[j]) == Some(c),
                    None => noise_class(lines[j]).is_none() && signature(lines[j]) == sig,
                };
                if !same {
                    break;
                }
                j += 1;
            }
        }
        let n = j - i;
        if n >= 4 {
            let tag = class.map(|c| format!(" {c}")).unwrap_or_default();
            out.push(lines[i].to_string());
            out.push(format!("… [{} more similar{tag} lines]", n - 2));
            out.push(lines[j - 1].to_string());
            changed = true;
        } else {
            out.extend(lines[i..j].iter().map(|l| l.to_string()));
        }
        i = j;
    }

    // Exact repeats that are not adjacent (e.g. the same warning every frame).
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for l in &out {
        if l.chars().count() >= 30 && !is_stacky(l) && !l.starts_with('…') {
            *counts.entry(l.as_str()).or_insert(0) += 1;
        }
    }
    let repeated: HashMap<String, usize> =
        counts.into_iter().filter(|(_, n)| *n >= 3).map(|(l, n)| (l.to_string(), n)).collect();
    if !repeated.is_empty() {
        let mut seen: HashMap<String, bool> = HashMap::new();
        let mut deduped = Vec::with_capacity(out.len());
        for l in out {
            match repeated.get(&l) {
                Some(n) => {
                    if seen.insert(l.clone(), true).is_none() {
                        deduped.push(format!("{l}  [×{n}]"));
                    }
                }
                None => deduped.push(l),
            }
        }
        out = deduped;
        changed = true;
    }
    (out.join("\n"), changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_cargo_compile_run() {
        let mut s = String::new();
        for i in 0..50 {
            s.push_str(&format!("   Compiling crate{i} v1.{i}.0\n"));
        }
        s.push_str("error[E0308]: mismatched types\n");
        let (out, changed) = collapse(&s);
        assert!(changed);
        assert!(out.contains("Compiling crate0"));
        assert!(out.contains("Compiling crate49"));
        assert!(out.contains("more similar build-progress"));
        assert!(out.contains("mismatched types"));
        assert!(out.lines().count() < 8);
    }

    #[test]
    fn dedupes_far_apart_repeats_with_count() {
        let rep = "DataStore request was added to queue. If request queue fills, drops.";
        let s = format!("{rep}\nother thing happened here today\n{rep}\nanother distinct event line\n{rep}\n{rep}");
        let (out, _) = collapse(&s);
        assert_eq!(out.matches("DataStore request").count(), 1);
        assert!(out.contains("[×4]"));
    }

    #[test]
    fn stack_lines_are_not_globally_deduped() {
        let st = "Script 'Players.x.ParkourController', Line 1238 - function tryLedgeHop";
        let s = format!("err one occurred\n{st}\nfoo bar baz\n{st}\nsomething else entirely\n{st}");
        let (out, _) = collapse(&s);
        assert_eq!(out.matches("tryLedgeHop").count(), 3);
    }
}
