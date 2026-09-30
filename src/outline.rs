//! Symbol outline of a source file (line-based heuristics, no parser): enough to
//! let an agent jump straight to `offset`/`limit` instead of reading everything.

#[derive(Debug, Clone, PartialEq)]
pub struct Sym {
    pub line: usize,
    pub end: usize,
    pub kind: &'static str,
    pub name: String,
    pub depth: usize,
}

struct Raw {
    line: usize,
    indent: usize,
    kind: &'static str,
    name: String,
}

fn indent_of(l: &str) -> usize {
    l.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn classify(ext: &str, l: &str) -> Option<(&'static str, String)> {
    match ext {
        "rs" => {
            let c = re!(r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|const\s+|extern\s+"[^"]*"\s+)*(fn|struct|enum|trait|impl|mod|macro_rules!)\s*([A-Za-z_][\w:<>, ']*)?"#).captures(l)?;
            let kind = match &c[1] {
                "fn" => "fn",
                "struct" => "struct",
                "enum" => "enum",
                "trait" => "trait",
                "impl" => "impl",
                "mod" => "mod",
                _ => "macro",
            };
            Some((kind, c.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default()))
        }
        "py" => {
            let c = re!(r"^\s*(async\s+def|def|class)\s+(\w+)").captures(l)?;
            Some((if c[1].ends_with("def") { "def" } else { "class" }, c[2].to_string()))
        }
        "lua" | "luau" => {
            if let Some(c) = re!(r"^\s*(?:local\s+)?function\s+([\w.:]+)").captures(l) {
                return Some(("function", c[1].to_string()));
            }
            if let Some(c) = re!(r"^\s*(?:local\s+)?([\w.:]+)\s*=\s*function\b").captures(l) {
                return Some(("function", c[1].to_string()));
            }
            let c = re!(r"^local\s+(\w+)\s*=\s*\{\s*$").captures(l)?;
            Some(("table", c[1].to_string()))
        }
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            if let Some(c) = re!(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(function\*?|class|interface|type|enum)\s+(\w+)").captures(l) {
                let k = match &c[1] {
                    "class" => "class",
                    "interface" => "interface",
                    "type" => "type",
                    "enum" => "enum",
                    _ => "function",
                };
                return Some((k, c[2].to_string()));
            }
            let c = re!(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*(?::[^=]+)?=>").captures(l)?;
            Some(("function", c[1].to_string()))
        }
        "go" => {
            if let Some(c) = re!(r"^func\s+(?:\(([^)]*)\)\s*)?(\w+)").captures(l) {
                let recv = c.get(1).map(|r| r.as_str().split_whitespace().last().unwrap_or("").trim_start_matches('*').to_string());
                let name = match recv {
                    Some(r) if !r.is_empty() => format!("{r}.{}", &c[2]),
                    _ => c[2].to_string(),
                };
                return Some(("func", name));
            }
            let c = re!(r"^type\s+(\w+)\s+(struct|interface)").captures(l)?;
            Some((if &c[2] == "struct" { "struct" } else { "interface" }, c[1].to_string()))
        }
        "md" | "markdown" => {
            let c = re!(r"^(#{1,6})\s+(.+?)\s*#*$").captures(l)?;
            Some(("h", format!("{} {}", &c[1], &c[2])))
        }
        "java" | "kt" | "cs" | "c" | "h" | "cpp" | "hpp" | "cc" | "swift" => {
            if let Some(c) = re!(r"^\s*(?:(?:public|private|protected|internal|static|final|abstract|sealed|partial|open|data)\s+)*(class|interface|struct|enum|object)\s+(\w+)").captures(l) {
                return Some((if &c[1] == "class" { "class" } else { "type" }, c[2].to_string()));
            }
            let c = re!(r"^\s*(?:[\w<>\[\],:\*&]+\s+)+(\w+)\s*\([^;]*\)\s*(?:const\s*)?(?:\{|$)").captures(l)?;
            let name = c[1].to_string();
            if matches!(name.as_str(), "if" | "for" | "while" | "switch" | "return" | "else" | "catch" | "sizeof" | "using") {
                return None;
            }
            Some(("method", name))
        }
        _ => None,
    }
}

pub fn extract(path: &str, text: &str) -> Vec<Sym> {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let all: Vec<&str> = text.lines().collect();
    let total = all.len();
    let mut raws: Vec<Raw> = vec![];
    for (i, l) in text.lines().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        if let Some((kind, name)) = classify(&ext, l) {
            // markdown depth comes from the heading level, not indentation
            let indent = if kind == "h" { name.chars().take_while(|c| *c == '#').count() - 1 } else { indent_of(l) };
            raws.push(Raw { line: i + 1, indent, kind, name });
        }
    }
    let unit = if ext == "md" || ext == "markdown" {
        1
    } else {
        raws.iter().map(|r| r.indent).filter(|i| *i > 0).min().unwrap_or(4)
    };
    let depths: Vec<usize> = raws.iter().map(|r| r.indent / unit.max(1)).collect();
    let mut out = vec![];
    for (i, r) in raws.iter().enumerate() {
        let mut end = raws[i + 1..]
            .iter()
            .zip(&depths[i + 1..])
            .find(|(_, d)| **d <= depths[i])
            .map(|(n, _)| n.line - 1)
            .unwrap_or(total);
        while end > r.line && all[end - 1].trim().is_empty() {
            end -= 1;
        }
        out.push(Sym { line: r.line, end: end.max(r.line), kind: r.kind, name: r.name.clone(), depth: depths[i] });
    }
    out
}

/// One line per symbol, top-level plus one nesting level, capped.
pub fn render(syms: &[Sym], cap: usize) -> String {
    let shown: Vec<&Sym> = syms.iter().filter(|s| s.depth <= 1).collect();
    let mut out = String::new();
    for s in shown.iter().take(cap) {
        let pad = if s.depth == 0 { "" } else { "  " };
        out.push_str(&format!("{pad}{}-{} {} {}\n", s.line, s.end, s.kind, s.name));
    }
    if shown.len() > cap {
        out.push_str(&format!("… {} more symbols\n", shown.len() - cap));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_symbols_with_spans_and_nesting() {
        let src = "use std::io;\n\npub struct A {\n    x: u32,\n}\n\nimpl A {\n    pub fn new() -> Self {\n        A { x: 1 }\n    }\n\n    fn helper(&self) {}\n}\n\npub async fn main() {}\n";
        let s = extract("a.rs", src);
        let names: Vec<(&str, &str, usize)> = s.iter().map(|x| (x.kind, x.name.as_str(), x.depth)).collect();
        assert_eq!(names, [("struct", "A", 0), ("impl", "A", 0), ("fn", "new", 1), ("fn", "helper", 1), ("fn", "main", 0)]);
        assert_eq!((s[1].line, s[1].end), (7, 13)); // impl spans its methods
        assert_eq!((s[4].line, s[4].end), (15, 15));
    }

    #[test]
    fn lua_functions_and_module_tables() {
        let src = "local M = {\n}\n\nlocal function tryLedgeHop(a)\n  if a then\n    local function inner() end\n  end\nend\n\nfunction M.update(dt)\nend\n\nM.jump = function()\nend\nreturn M\n";
        let s = extract("c.luau", src);
        let n: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
        assert!(n.contains(&"M") && n.contains(&"tryLedgeHop") && n.contains(&"M.update") && n.contains(&"M.jump"), "{n:?}");
    }

    #[test]
    fn python_and_markdown_and_unknown() {
        let py = extract("x.py", "class A:\n    def f(self):\n        pass\n\ndef g():\n    pass\n");
        assert_eq!(py.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["A", "f", "g"]);
        let md = extract("r.md", "# Title\ntext\n## Sub\nmore\n");
        assert_eq!((md[0].depth, md[1].depth), (0, 1));
        assert!(extract("data.json", "{\"a\":1}").is_empty());
    }

    #[test]
    fn render_caps_and_skips_deep_symbols() {
        let mut src = String::new();
        for i in 0..30 {
            src.push_str(&format!("fn f{i}() {{\n    fn deep{i}() {{}}\n}}\n"));
        }
        let syms = extract("a.rs", &src);
        let r = render(&syms, 10);
        assert!(r.contains("1-3 fn f0"), "{r}");
        assert!(r.contains("more symbols"));
        assert_eq!(r.lines().count(), 11);
    }
}
