//! `path:line:text` grep output, regrouped per file with the common directory
//! prefix removed. Lossless.

use std::collections::BTreeMap;

pub fn compress(content: &str) -> Option<String> {
    let re = re!(r"^(.+?):(\d+):(.*)$");
    let mut order: Vec<String> = vec![];
    let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut total = 0usize;
    for line in content.lines() {
        let caps = re.captures(line)?;
        let file = caps[1].to_string();
        if !groups.contains_key(&file) {
            order.push(file.clone());
        }
        groups.entry(file).or_default().push((caps[2].to_string(), caps[3].to_string()));
        total += 1;
    }
    if total < 6 {
        return None;
    }
    let prefix = common_dir_prefix(&order);
    let mut out = String::new();
    if !prefix.is_empty() {
        out.push_str(&format!("root: {prefix}\n"));
    }
    for f in &order {
        let shown = f.strip_prefix(&prefix).unwrap_or(f);
        out.push_str(shown);
        out.push('\n');
        for (n, t) in &groups[f] {
            out.push_str(&format!(" {n}: {t}\n"));
        }
    }
    Some(out.trim_end().to_string())
}

fn common_dir_prefix(files: &[String]) -> String {
    let first = match files.first() {
        Some(f) => f,
        None => return String::new(),
    };
    let sep_positions: Vec<usize> = first.match_indices(['/', '\\']).map(|(i, _)| i + 1).collect();
    let mut best = String::new();
    for p in sep_positions {
        let cand = &first[..p];
        if files.iter().all(|f| f.starts_with(cand)) {
            best = cand.to_string();
        } else {
            break;
        }
    }
    if best.len() >= 12 {
        best
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_file_and_strips_root() {
        let mut s = String::new();
        for i in 1..=4 {
            s.push_str(&format!("C:\\Users\\me\\proj\\src\\a.rs:{i}:let x = {i};\n"));
        }
        for i in 1..=3 {
            s.push_str(&format!("C:\\Users\\me\\proj\\src\\b.rs:{i}:fn f{i}() {{}}\n"));
        }
        let out = compress(&s).unwrap();
        assert!(out.starts_with("root: C:\\Users\\me\\proj\\src\\"));
        assert_eq!(out.matches("a.rs").count(), 1);
        assert!(out.contains(" 3: fn f3() {}"));
    }

    #[test]
    fn non_grep_text_returns_none() {
        assert!(compress("just\nsome\ntext\nlines\nhere\nplease\nok").is_none());
    }
}
