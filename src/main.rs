/// Lazily compiled, cached regex. Must be defined before the modules that use it.
macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        R.get_or_init(|| regex::Regex::new($p).unwrap())
    }};
}

mod bench;
mod codex;
mod compress;
mod config;
mod hook;
mod install;
mod mcp_proxy;
mod outline;
mod readguard;
mod stats;
mod store;
mod tokens;

use std::io::{Read, Write};

const HELP: &str = "\
MTK - universal token saver for AI coding agents

USAGE
  mtk install [--project] [--dry-run]   add the PostToolUse hook to Claude Code
  mtk uninstall [--project]             remove it
  mtk hook                              (called by Claude Code) reads payload on stdin
  mtk gain                              token savings so far
  mtk mcp-proxy [--name N] -- CMD ARGS  transparent MCP proxy that compresses tool results
  mtk outline FILE                      symbols with line spans (what the Read guard shows)
  mtk mcp wrap|unwrap|status SERVER     route a Codex config.toml MCP server through the proxy
  mtk recall <id> [--grep RE] [-C N]    show the original, uncompressed output
  mtk filter [FILE]                     compress a file/stdin and print it (debug)
  mtk bench <files|dirs>                savings table over saved outputs
  mtk doctor                            check the installation

CONFIG  ~/.mtk/config.toml   (MTK_HOME overrides the folder, MTK_DISABLE=1 turns MTK off)
";

fn read_stdin() -> String {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn cmd_hook() -> i32 {
    std::panic::set_hook(Box::new(|_| {}));
    let input = read_stdin();
    let cfg = config::Config::load();
    if input.contains("\"PreToolUse\"") {
        if let Ok(Some(out)) = std::panic::catch_unwind(|| readguard::handle(&input, &cfg)) {
            let mut o = std::io::stdout().lock();
            let _ = o.write_all(out.as_bytes());
            let _ = o.flush();
        }
        return 0;
    }
    let result = std::panic::catch_unwind(|| hook::process(&input, &cfg));
    if let Ok(Some(p)) = result {
        stats::record(&p.tool, &p.kinds.iter().map(|s| s.as_str()).collect::<Vec<_>>(), p.in_tok, p.out_tok);
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(p.output.to_string().as_bytes());
        let _ = out.flush();
    }
    0
}

fn cmd_recall(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("usage: mtk recall <id> [--grep RE] [-C N]");
        return 2;
    };
    let Some(text) = store::load(id.trim_start_matches("mtk:")) else {
        eprintln!("mtk: no stored output for id {id} (expired or wrong id)");
        return 1;
    };
    match opt(args, "--grep") {
        None => println!("{text}"),
        Some(pat) => {
            let ctx: usize = opt(args, "-C").and_then(|n| n.parse().ok()).unwrap_or(0);
            let Ok(re) = regex::Regex::new(&pat) else {
                eprintln!("mtk: invalid regex");
                return 2;
            };
            let lines: Vec<&str> = text.lines().collect();
            let mut show = vec![false; lines.len()];
            for (i, l) in lines.iter().enumerate() {
                if re.is_match(l) {
                    for s in show.iter_mut().take((i + ctx + 1).min(lines.len())).skip(i.saturating_sub(ctx)) {
                        *s = true;
                    }
                }
            }
            for (i, l) in lines.iter().enumerate() {
                if show[i] {
                    println!("{}: {}", i + 1, l);
                }
            }
        }
    }
    0
}

fn cmd_filter(args: &[String]) -> i32 {
    let cfg = config::Config::load();
    let file = args.first().filter(|a| !a.starts_with("--"));
    let text = match file {
        Some(f) => std::fs::read_to_string(f).unwrap_or_default(),
        None => read_stdin(),
    };
    let ctx = compress::Ctx { cfg: &cfg };
    let out = compress::compress_text(&text, &ctx);
    println!("{}", out.text);
    eprintln!("[mtk] {} -> {} tokens ({})", tokens::estimate(&text), tokens::estimate(&out.text), out.kind);
    0
}

fn cmd_doctor() -> i32 {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    println!("mtk {}", env!("CARGO_PKG_VERSION"));
    println!("binary:        {exe}");
    println!("home:          {}", config::mtk_home().display());
    println!("user hook:     {}", if install::is_installed(false) { "installed" } else { "NOT installed (run `mtk install`)" });
    println!("hook command:  {}", install::hook_command());
    println!("stored recalls: {}", store::count());
    let cfg = config::Config::load();
    let sample = "   Compiling a v1\n".repeat(200);
    let ctx = compress::Ctx { cfg: &cfg };
    let out = compress::compress_text(&sample, &ctx);
    println!("self-test:     {} -> {} tokens ({})", tokens::estimate(&sample), tokens::estimate(&out.text), out.kind);
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    let code = match args.first().map(|s| s.as_str()) {
        Some("hook") => cmd_hook(),
        Some("gain") => {
            print!("{}", stats::gain());
            0
        }
        Some("recall") => cmd_recall(rest),
        Some("filter") => cmd_filter(rest),
        Some("bench") => {
            print!("{}", bench::run(rest, &config::Config::load()));
            0
        }
        Some("doctor") => cmd_doctor(),
        Some("mcp-proxy") => mcp_proxy::run(rest, &config::Config::load()),
        Some("mcp") => cmd_mcp(rest),
        Some("outline") => cmd_outline(rest),
        Some("install") => report(install::install(flag(rest, "--project"), flag(rest, "--dry-run"))),
        Some("uninstall") => report(install::uninstall(flag(rest, "--project"))),
        Some("--version") | Some("-V") => {
            println!("mtk {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            print!("{HELP}");
            0
        }
    };
    std::process::exit(code);
}

fn cmd_outline(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("usage: mtk outline FILE");
        return 2;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("mtk: cannot read {path} as text");
        return 1;
    };
    let syms = outline::extract(path, &text);
    println!("{} lines, ~{} tokens, {} symbols", text.lines().count(), tokens::estimate(&text), syms.len());
    println!("{}", outline::render(&syms, 400));
    0
}

fn cmd_mcp(args: &[String]) -> i32 {
    let (Some(action), Some(server)) = (args.first(), args.get(1)) else {
        eprintln!("usage: mtk mcp wrap|unwrap|status SERVER [--config PATH]");
        return 2;
    };
    let path = opt(args, "--config").map(std::path::PathBuf::from).unwrap_or_else(codex::config_path);
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "mtk".into());
    report(match action.as_str() {
        "wrap" => codex::wrap(server, &path, &exe),
        "unwrap" => codex::unwrap(server, &path),
        "status" => codex::status(server, &path),
        _ => Err("action must be wrap, unwrap or status".into()),
    })
}

fn report(r: Result<String, String>) -> i32 {
    match r {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("mtk: {e}");
            1
        }
    }
}
