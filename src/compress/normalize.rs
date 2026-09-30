//! Lossless-in-spirit cleanup: ANSI escapes, carriage-return progress bars,
//! trailing whitespace, blank-line runs and absurdly long lines.

const LINE_CAP: usize = 1200;

pub fn run(input: &str) -> String {
    let no_ansi = re!(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").replace_all(input, "");
    let unified = no_ansi.replace("\r\n", "\n");
    let mut out: Vec<String> = Vec::new();
    let mut blank_run = 0;
    for raw in unified.split('\n') {
        // A progress bar redraws with '\r'; only the last non-empty frame matters.
        let line = if raw.contains('\r') {
            raw.split('\r').rev().find(|s| !s.trim().is_empty()).unwrap_or("")
        } else {
            raw
        };
        let line = line.trim_end();
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push(line.to_string());
    }
    while out.first().map(|l| l.is_empty()).unwrap_or(false) {
        out.remove(0);
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

/// Trims absurdly long lines (minified blobs). Only ever applied to plain text,
/// never before a structured parse, or it would cut JSON in half.
pub fn cap_lines(s: &str) -> String {
    s.lines().map(cap_line).collect::<Vec<_>>().join("\n")
}

fn cap_line(line: &str) -> String {
    let n = line.chars().count();
    if n <= LINE_CAP {
        return line.to_string();
    }
    let head: String = line.chars().take(800).collect();
    let tail: String = line.chars().skip(n - 200).collect();
    format!("{head}…[+{} chars]…{tail}", n - 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_progress() {
        let s = "\x1b[32mok\x1b[0m\r10%\r50%\r100%\ndone   \n\n\n\nend";
        assert_eq!(run(s), "100%\ndone\n\nend");
    }

    #[test]
    fn caps_giant_lines() {
        let s = "x".repeat(5000);
        assert_eq!(run(&s).len(), 5000, "run() must never cut content");
        let out = cap_lines(&s);
        assert!(out.chars().count() < 1100);
        assert!(out.contains("+4000 chars"));
    }
}
