//! orbi-hook — connects AI coding agents to Orbi.
//!
//!   orbi-hook ask   --agent <id>                    (stdin: that agent's hook JSON)
//!   orbi-hook ask   --agent X --tool T --input '<json>' [--cwd D]
//!                                                   prints allow|deny|ask, exits 0|1|2
//!   orbi-hook event --agent <id>                    (stdin: that agent's hook JSON)
//!   orbi-hook event --agent X --kind working|done|error --summary S [--detail D]
//!   orbi-hook codex-notify <json>                   (Codex CLI `notify`)
//!
//! Safety contract: `ask` prints an approval only when a server that proved it
//! knows Orbi's token (docs/APP.md handshake) answered "allow". If Orbi isn't
//! running, isn't configured, can't be verified, or anything goes wrong, the
//! agent gets its "fall back to your own prompt" answer — for most agents that
//! is no output and exit 0, so they behave exactly as without Orbi.

mod agents;
mod auth;
mod common;
mod http;

use common::Decision;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_PORT: u16 = 47821;
const DEFAULT_TIMEOUT_SECS: u64 = 45;

/// `$ORBI_DATA_DIR` if set, else `~/Library/Application Support/Orbi`.
fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ORBI_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join("Library/Application Support/Orbi"))
}

struct Setup {
    conn: auth::Conn,
    /// Orbi's ask timeout (config.json), used to size our read timeout.
    ask_timeout_secs: u64,
}

fn load() -> Option<Setup> {
    let dir = data_dir()?;
    let token = std::fs::read_to_string(dir.join("token")).ok()?.trim().to_string();
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let port = std::fs::read_to_string(dir.join("port"))
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .unwrap_or(DEFAULT_PORT);
    let ask_timeout_secs = std::fs::read_to_string(dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("timeout_secs").and_then(Value::as_u64))
        .filter(|t| *t > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Some(Setup { conn: auth::Conn { port, token }, ask_timeout_secs })
}

fn app_bundle() -> Option<String> {
    std::env::var("__CFBundleIdentifier").ok().filter(|s| !s.is_empty())
}

fn read_stdin() -> Option<Value> {
    let mut buf = Vec::new();
    std::io::stdin().take(16 << 20).read_to_end(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// Parse `--flag value` / `--flag=value`.
fn flag(args: &[String], name: &str) -> Option<String> {
    let long = format!("--{}", name);
    let eq = format!("--{}=", name);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == long {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v.to_string());
        }
    }
    None
}

/// Send an `/ask` body and wait for the user. Anything but a verified
/// explicit answer is `Ask`.
fn ask_orbi(body: Value) -> Decision {
    let Some(setup) = load() else { return Decision::Ask };
    let body = common::encode_body(body);
    // Wait a bit longer than Orbi's own timeout so Orbi's "ask" wins the race;
    // capped below the agents' hook timeouts (120 s).
    let total = Duration::from_secs((setup.ask_timeout_secs + 10).clamp(55, 110));
    match auth::call(&setup.conn, "POST", "/ask", Some(&body), Duration::from_millis(300), total) {
        Some((status, resp)) => common::parse_decision(status, &resp),
        None => Decision::Ask,
    }
}

fn send_event(body: Value) {
    let Some(setup) = load() else { return };
    let body = common::encode_body(body);
    let _ = auth::call(&setup.conn, "POST", "/event", Some(&body), Duration::from_millis(200), Duration::from_millis(800));
}

fn emit(out: &agents::Out) -> i32 {
    if let Some(s) = &out.stdout {
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{}", s);
        let _ = so.flush();
    }
    out.code
}

fn cmd_ask(args: &[String]) -> i32 {
    let agent = flag(args, "agent").unwrap_or_else(|| "claude-code".into());
    if let Some(tool) = flag(args, "tool") {
        // Flag form, for scripts and harnesses: word + exit code.
        let d = std::panic::catch_unwind(|| {
            let raw = flag(args, "input").unwrap_or_default();
            let input = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
            let cwd = flag(args, "cwd")
                .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let session = flag(args, "session").unwrap_or_default();
            ask_orbi(common::ask_body(&agent, &tool, input, &cwd, &session, app_bundle().as_deref()))
        })
        .unwrap_or(Decision::Ask);
        return emit(&agents::word_output(d));
    }
    let res = std::panic::catch_unwind(|| {
        let Some(hook) = read_stdin() else { return (String::new(), Decision::Ask) };
        let (body, event) = agents::ask_request(&agent, &hook, app_bundle().as_deref());
        (event, ask_orbi(body))
    });
    let (event, d) = res.unwrap_or((String::new(), Decision::Ask));
    emit(&agents::ask_output(&agent, &event, d))
}

fn cmd_event(args: &[String]) {
    let kind = flag(args, "kind");
    let summary = flag(args, "summary");
    let agent = flag(args, "agent");
    let body = if kind.is_some() || summary.is_some() {
        // Generic wrapper mode: flags only, stdin ignored.
        let kind = kind.unwrap_or_else(|| "working".into());
        if !matches!(kind.as_str(), "working" | "done" | "error") {
            return;
        }
        let agent = agent.unwrap_or_else(|| "agent".into());
        common::event_body(
            &agent,
            &kind,
            &summary.unwrap_or_default(),
            flag(args, "detail").as_deref(),
            &flag(args, "session").unwrap_or_default(),
            app_bundle().as_deref(),
        )
    } else {
        let agent = agent.unwrap_or_else(|| "claude-code".into());
        let Some(hook) = read_stdin() else { return };
        match agents::event_from(&agent, &hook, app_bundle().as_deref()) {
            Some(b) => b,
            None => return,
        }
    };
    send_event(body);
}

/// `notify = ["/abs/path/orbi-hook", "codex-notify"]` in ~/.codex/config.toml;
/// Codex appends one JSON argument.
fn cmd_codex_notify(args: &[String]) {
    let Some(raw) = args.last() else { return };
    let Ok(v) = serde_json::from_str::<Value>(raw) else { return };
    if let Some(body) = agents::event_from_codex_notify(&v, app_bundle().as_deref()) {
        send_event(body);
    }
}

fn usage() {
    eprintln!(
        "orbi-hook {}\n\n\
         usage:\n  \
         orbi-hook ask   --agent <id>                       read the agent's hook JSON on stdin, ask Orbi\n  \
         orbi-hook ask   --agent X --tool T --input JSON [--cwd D]   prints allow|deny|ask, exits 0|1|2\n  \
         orbi-hook event --agent <id>                       read the agent's hook JSON on stdin\n  \
         orbi-hook event --agent X --kind working|done|error --summary S [--detail D]\n  \
         orbi-hook codex-notify <json>                      Codex CLI `notify` program\n\n\
         agents: claude-code codex gemini cursor hermes opencode (others: Claude Code hook JSON)",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() {
    // A panic must never leak noise into the agent.
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => ("", &[][..]),
    };
    let code = match cmd {
        "ask" => cmd_ask(rest),
        "event" => {
            let _ = std::panic::catch_unwind(|| cmd_event(rest));
            0
        }
        "codex-notify" => {
            let _ = std::panic::catch_unwind(|| cmd_codex_notify(rest));
            0
        }
        "--version" | "-V" | "version" => {
            println!("orbi-hook {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "--help" | "-h" | "help" => {
            usage();
            0
        }
        _ => {
            usage();
            2
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags() {
        let a = v(&["--agent", "codex", "--kind=done", "--summary", "all good"]);
        assert_eq!(flag(&a, "agent").as_deref(), Some("codex"));
        assert_eq!(flag(&a, "kind").as_deref(), Some("done"));
        assert_eq!(flag(&a, "summary").as_deref(), Some("all good"));
        assert_eq!(flag(&a, "detail"), None);
        assert_eq!(flag(&v(&["--agent"]), "agent"), None);
    }
}
