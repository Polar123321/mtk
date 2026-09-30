//! Hard token budget. Keeps the head and the tail (where commands print their
//! summary) and rescues error/warning lines from the middle, so the one line
//! that matters is never lost to truncation.

use crate::tokens::estimate;

fn is_signal(line: &str) -> bool {
    re!(r"(?i)\b(error|err!|fail(ed|ure|ing)?|panic(ked)?|exception|fatal|traceback|assert(ion)?|denied|cannot|unable|warning|warn)\b|✗|✘|^E\s")
        .is_match(line)
}

pub fn apply(s: &str, max_tokens: usize) -> (String, bool) {
    if estimate(s) <= max_tokens {
        return (s.to_string(), false);
    }
    let lines: Vec<&str> = s.lines().collect();
    let cost: Vec<usize> = lines.iter().map(|l| estimate(l) + 1).collect();
    let head_budget = max_tokens * 40 / 100;
    let tail_budget = max_tokens * 45 / 100;
    let rescue_budget = max_tokens * 15 / 100;

    let mut head_end = 0;
    let mut spent = 0;
    while head_end < lines.len() && spent + cost[head_end] <= head_budget {
        spent += cost[head_end];
        head_end += 1;
    }
    let mut tail_start = lines.len();
    spent = 0;
    while tail_start > head_end && spent + cost[tail_start - 1] <= tail_budget {
        spent += cost[tail_start - 1];
        tail_start -= 1;
    }
    if tail_start <= head_end {
        return (s.to_string(), false);
    }

    let mut rescued: Vec<String> = vec![];
    let mut rescued_cost = 0;
    for (i, l) in lines.iter().enumerate().take(tail_start).skip(head_end) {
        if is_signal(l) {
            let short: String = l.chars().take(300).collect();
            let c = estimate(&short) + 3;
            if rescued_cost + c > rescue_budget {
                break;
            }
            rescued_cost += c;
            rescued.push(format!("⋮ L{}: {}", i + 1, short));
        }
    }

    let omitted_lines = tail_start - head_end;
    let omitted_tokens: usize = cost[head_end..tail_start].iter().sum();
    let mut out: Vec<String> = lines[..head_end].iter().map(|l| l.to_string()).collect();
    let kept = if rescued.is_empty() {
        String::new()
    } else {
        format!(", {} error/warning lines kept below", rescued.len())
    };
    out.push(format!("… [{omitted_lines} lines / ~{omitted_tokens} tokens omitted{kept}] …"));
    out.extend(rescued);
    out.extend(lines[tail_start..].iter().map(|l| l.to_string()));
    (out.join("\n"), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_input_untouched() {
        let (s, cut) = apply("a\nb", 100);
        assert_eq!(s, "a\nb");
        assert!(!cut);
    }

    #[test]
    fn keeps_head_tail_and_errors() {
        let mut v: Vec<String> = (0..2000).map(|i| format!("row {i} some ordinary payload text")).collect();
        v[1000] = "thread main panicked at src/lib.rs:5".into();
        let (s, cut) = apply(&v.join("\n"), 300);
        assert!(cut);
        assert!(s.contains("row 0 "));
        assert!(s.contains("row 1999 "));
        assert!(s.contains("panicked at src/lib.rs:5"));
        assert!(s.contains("omitted"));
    }
}
