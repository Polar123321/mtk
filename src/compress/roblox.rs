//! Roblox Studio MCP specialists. These tools return JSON that is extremely
//! redundant for an LLM:
//!  - `search_game_tree`: a flat array where `name` and `parentName` are both
//!    derivable from `fullPath` -> rebuilt as an indented tree.
//!  - `inspect_instance`: dozens of properties sitting at their engine default
//!    -> only non-default properties are printed, with a count of what was hidden.

use crate::compress::json::tidy_string;
use crate::config::Config;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub struct Rendered {
    pub text: String,
    pub kind: &'static str,
}

pub fn render(v: &Value, cfg: &Config) -> Option<Rendered> {
    match v {
        Value::Array(a) if is_tree(a) => Some(Rendered { text: tree(a), kind: "roblox-tree" }),
        Value::Object(m) if is_instance(m) => Some(Rendered { text: inspect(m, cfg), kind: "roblox-inspect" }),
        Value::Object(m) => {
            let matches = m.get("matches")?.as_array()?;
            if matches.is_empty() || !matches.iter().all(|x| x.as_object().map(is_instance).unwrap_or(false)) {
                return None;
            }
            let mut parts = vec![format!("{} matches", matches.len())];
            for x in matches {
                parts.push(inspect(x.as_object().unwrap(), cfg));
            }
            Some(Rendered { text: parts.join("\n\n"), kind: "roblox-inspect" })
        }
        _ => None,
    }
}

fn is_tree(a: &[Value]) -> bool {
    !a.is_empty()
        && a.iter().all(|x| {
            x.as_object()
                .map(|o| o.get("fullPath").map(|p| p.is_string()).unwrap_or(false) && o.get("className").map(|c| c.is_string()).unwrap_or(false))
                .unwrap_or(false)
        })
}

fn is_instance(m: &Map<String, Value>) -> bool {
    m.get("properties").map(|p| p.is_object()).unwrap_or(false)
        && m.get("className").map(|c| c.is_string()).unwrap_or(false)
        && m.get("path").map(|c| c.is_string()).unwrap_or(false)
}

// ---- game tree -------------------------------------------------------------

struct Node {
    full: String,
    label: String,
    class: String,
    note: String,
    children: Vec<usize>,
}

fn parent_of<'a>(full: &'a str, name: &str) -> Option<&'a str> {
    if !name.is_empty() && full.ends_with(name) {
        let cut = full.len() - name.len();
        if cut >= 1 && full.as_bytes()[cut - 1] == b'.' {
            return Some(&full[..cut - 1]);
        }
    }
    full.rsplit_once('.').map(|(p, _)| p)
}

fn tree(a: &[Value]) -> String {
    let mut nodes: Vec<Node> = vec![];
    let mut parents: Vec<Option<String>> = vec![];
    let mut by_full: HashMap<String, usize> = HashMap::new();
    for x in a {
        let o = x.as_object().unwrap();
        let full = o["fullPath"].as_str().unwrap().to_string();
        let name = o.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let mut note = String::new();
        if let Some(s) = o.get("childSummary").and_then(|s| s.as_str()) {
            note = format!(" ({s})");
        } else if let Some(n) = o.get("unexploredChildCount") {
            note = format!(" (+{n} unexplored)");
        }
        for (k, v) in o {
            if !matches!(k.as_str(), "fullPath" | "name" | "className" | "parentName" | "childSummary" | "unexploredChildCount") {
                note.push_str(&format!(" {{{k}:{}}}", crate::compress::json::inline(v)));
            }
        }
        parents.push(parent_of(&full, &name).map(|s| s.to_string()));
        by_full.insert(full.clone(), nodes.len());
        nodes.push(Node {
            label: if name.is_empty() { full.rsplit('.').next().unwrap_or(&full).to_string() } else { name },
            full: full.clone(),
            class: o["className"].as_str().unwrap().to_string(),
            note,
            children: vec![],
        });
    }
    let mut roots: Vec<usize> = vec![];
    for i in 0..nodes.len() {
        match parents[i].as_ref().and_then(|p| by_full.get(p).copied()) {
            Some(pi) if pi != i => nodes[pi].children.push(i),
            _ => roots.push(i),
        }
    }

    let mut out = String::from("# name:Class, indent = child, [Class] a,b,c = leaf siblings\n");
    for r in roots {
        // A root whose parent was not returned keeps its full path for context.
        let has_parent = nodes[r].full.contains('.') && nodes[r].full != nodes[r].label;
        let label = if has_parent { nodes[r].full.clone() } else { nodes[r].label.clone() };
        out.push_str(&format!("{label}:{}{}\n", nodes[r].class, nodes[r].note));
        write_children(&nodes, r, 1, &mut out);
    }
    out.trim_end().to_string()
}

fn is_leaf(n: &Node) -> bool {
    n.children.is_empty() && n.note.is_empty()
}

fn write_children(nodes: &[Node], parent: usize, depth: usize, out: &mut String) {
    let kids = &nodes[parent].children;
    let pad = " ".repeat(depth);
    let mut i = 0;
    while i < kids.len() {
        let k = &nodes[kids[i]];
        if is_leaf(k) {
            let mut j = i + 1;
            while j < kids.len() && is_leaf(&nodes[kids[j]]) && nodes[kids[j]].class == k.class {
                j += 1;
            }
            if j - i >= 3 {
                let names: Vec<&str> = kids[i..j].iter().map(|&c| nodes[c].label.as_str()).collect();
                out.push_str(&format!("{pad}[{}] {}\n", k.class, names.join(", ")));
                i = j;
                continue;
            }
        }
        out.push_str(&format!("{pad}{}:{}{}\n", k.label, k.class, k.note));
        write_children(nodes, kids[i], depth + 1, out);
        i += 1;
    }
}

// ---- inspect_instance ------------------------------------------------------

const DEFAULTS: &[(&str, &str)] = &[
    ("Archivable", "true"),
    ("Sandboxed", "false"),
    ("RootPriority", "0"),
    ("Transparency", "0"),
    ("Reflectance", "0"),
    ("Massless", "false"),
    ("CanTouch", "true"),
    ("CanQuery", "true"),
    ("CanCollide", "true"),
    ("CastShadow", "true"),
    ("EnableFluidForces", "true"),
    ("AudioCanCollide", "true"),
    ("CollisionGroup", "Default"),
    ("MaterialVariant", ""),
    ("Capabilities", ""),
    ("TopSurface", "Enum.SurfaceType.Smooth"),
    ("BottomSurface", "Enum.SurfaceType.Smooth"),
    ("LeftSurface", "Enum.SurfaceType.Smooth"),
    ("RightSurface", "Enum.SurfaceType.Smooth"),
    ("FrontSurface", "Enum.SurfaceType.Smooth"),
    ("BackSurface", "Enum.SurfaceType.Smooth"),
    ("Rotation", "0, 0, 0"),
    ("AssemblyLinearVelocity", "0, 0, 0"),
    ("AssemblyAngularVelocity", "0, 0, 0"),
    ("PivotOffset.Position", "0, 0, 0"),
    ("PivotOffset.Rotation", "1, 0, 0, 0, 1, 0, 0, 0, 1"),
    ("CFrame.Rotation", "1, 0, 0, 0, 1, 0, 0, 0, 1"),
];

fn prop_string(v: &Value, cfg: &Config) -> String {
    match v {
        Value::String(s) => tidy_string(s, cfg),
        other => other.to_string(),
    }
}

fn inspect(m: &Map<String, Value>, cfg: &Config) -> String {
    let s = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let mut out = format!("{} [{}]", s("path"), s("className"));
    if !s("uniqueId").is_empty() {
        out.push_str(&format!(" id={}", s("uniqueId")));
    }
    out.push('\n');

    if let Some(attrs) = m.get("attributes").and_then(|a| a.as_object()) {
        if !attrs.is_empty() {
            let parts: Vec<String> = attrs.iter().map(|(k, v)| format!("{k}={}", prop_string(v, cfg))).collect();
            out.push_str(&format!("attrs: {}\n", parts.join("; ")));
        }
    }

    let mut hidden = 0usize;
    if let Some(props) = m.get("properties").and_then(|p| p.as_object()) {
        let unique = s("uniqueId");
        for (k, v) in props {
            let val = prop_string(v, cfg);
            let raw = match v {
                Value::String(x) => x.clone(),
                other => other.to_string(),
            };
            let is_default = DEFAULTS.iter().any(|(n, d)| n == k && *d == raw) || (k == "UniqueId" && raw == unique);
            if is_default {
                hidden += 1;
                continue;
            }
            out.push_str(&format!("{k}={}\n", val.replace("Enum.", "")));
        }
    }
    if hidden > 0 {
        out.push_str(&format!("({hidden} default-valued props omitted)\n"));
    }

    if let Some(ch) = m.get("children").and_then(|c| c.as_object()) {
        let count = ch.get("childrenCount").map(|c| c.to_string()).unwrap_or_default();
        let total = ch.get("totalDescendants").map(|c| c.to_string()).unwrap_or_default();
        let prefix = format!("{}.", s("path"));
        let items: Vec<String> = ch
            .get("immediateChildren")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        let p = c.get("path")?.as_str()?;
                        let cls = c.get("className").and_then(|x| x.as_str()).unwrap_or("?");
                        Some(format!("{}:{}", p.strip_prefix(&prefix).unwrap_or(p), cls))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push_str(&format!("children({count}, {total} total): {}\n", items.join(", ")));
    }

    // Anything the tool adds that we do not know about is kept verbatim.
    let known = ["path", "className", "uniqueId", "name", "attributes", "properties", "children"];
    for (k, v) in m {
        if !known.contains(&k.as_str()) {
            out.push_str(&format!("{k}: {}\n", crate::compress::json::inline(v)));
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tree_rebuilds_hierarchy_and_groups_leaves() {
        let v = json!([
            {"parentName":"Game","fullPath":"Workspace","name":"Workspace","className":"Workspace"},
            {"parentName":"Workspace","fullPath":"Workspace.Map","name":"Map","className":"Folder"},
            {"parentName":"Workspace","fullPath":"Workspace.Combat Animations","name":"Combat Animations","className":"Model"},
            {"parentName":"Map","fullPath":"Workspace.Map.A","name":"A","className":"Part"},
            {"parentName":"Map","fullPath":"Workspace.Map.B","name":"B","className":"Part"},
            {"parentName":"Map","fullPath":"Workspace.Map.C","name":"C","className":"Part"},
            {"parentName":"Combat Animations","fullPath":"Workspace.Combat Animations.Torso","name":"Torso","className":"Part"}
        ]);
        let out = render(&v, &Config::default()).unwrap().text;
        assert!(out.contains("Workspace:Workspace\n Map:Folder\n  [Part] A, B, C"), "{out}");
        assert!(out.contains(" Combat Animations:Model\n  Torso:Part"), "{out}");
    }

    #[test]
    fn orphan_roots_keep_full_path() {
        let v = json!([
            {"parentName":"Map","fullPath":"Workspace.Map.A","name":"A","className":"Part"}
        ]);
        let out = render(&v, &Config::default()).unwrap().text;
        assert!(out.contains("Workspace.Map.A:Part"), "{out}");
    }

    #[test]
    fn inspect_hides_defaults_only() {
        let v = json!({
            "path":"Workspace.Baseplate","name":"Baseplate","className":"Part","uniqueId":"abc",
            "attributes":{"tag":"x"},
            "children":{"immediateChildren":[{"path":"Workspace.Baseplate.Texture","className":"Texture"}],"totalDescendants":1,"childrenCount":1},
            "properties":{"Anchored":true,"Archivable":true,"Transparency":0,"Size":"2048, 16, 2048","Material":"Enum.Material.Plastic","UniqueId":"abc","Transparency2":0.5,"Rotation":"0, 0, 0"}
        });
        let out = render(&v, &Config::default()).unwrap().text;
        assert!(out.contains("Anchored=true"));
        assert!(out.contains("Size=2048, 16, 2048"));
        assert!(out.contains("Material=Material.Plastic"));
        assert!(out.contains("Transparency2=0.5"));
        assert!(!out.contains("Archivable"));
        assert!(!out.contains("UniqueId="));
        assert!(out.contains("(4 default-valued props omitted)"), "{out}");
        assert!(out.contains("children(1, 1 total): Texture:Texture"), "{out}");
    }

    #[test]
    fn real_tree_fixture_is_lossless_for_names_and_classes() {
        let raw = include_str!("../../tests/fixtures/mcp__Roblox_Studio__search_game_tree.txt");
        let sp = crate::compress::json::split(raw).expect("fixture parses");
        let out = render(&sp.value, &Config::default()).unwrap().text;
        for node in sp.value.as_array().unwrap() {
            let name = node["name"].as_str().unwrap();
            let class = node["className"].as_str().unwrap();
            assert!(out.contains(name), "lost name {name}");
            assert!(out.contains(class), "lost class {class}");
        }
        assert!(out.contains(" Combat Animations:Model\n  Humanoid:Humanoid"), "{out}");
        assert!(out.contains("[Folder] Clearing, Chase1"), "{out}");
    }

    #[test]
    fn real_inspect_fixture_keeps_every_non_default_property() {
        let raw = include_str!("../../tests/fixtures/mcp__Roblox_Studio__inspect_instance.txt");
        let sp = crate::compress::json::split(raw).expect("fixture parses");
        let out = render(&sp.value, &Config::default()).unwrap().text;
        for needle in ["Anchored=true", "Size=2048, 16, 2048", "CFrame.Position=0, -8, 0", "Locked=true", "BrickColor=Dark grey metallic", "Material=Material.Plastic", "Shape=PartType.Block", "Color=0.35686, 0.35686, 0.35686", "Texture:Texture", "_lemonadeUniqueId=default_baseplate", "id=30ab3de8-f763-dde0-0ac1-a80c000003ed"] {
            assert!(out.contains(needle), "missing {needle}\n{out}");
        }
        assert!(!out.contains("Archivable"));
    }

    #[test]
    fn non_roblox_json_is_ignored() {
        assert!(render(&json!({"a": 1}), &Config::default()).is_none());
        assert!(render(&json!([{"x": 1}]), &Config::default()).is_none());
    }
}
