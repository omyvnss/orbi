//! Optional "bring your own key" model that rewrites the rule-based line
//! (see `explain.rs`) into more natural English.
//!
//! * Every provider speaks the OpenAI chat format:
//!   `POST {base}/chat/completions` → `choices[0].message.content`.
//! * Keys live in the macOS Keychain (service `dev.orbi.app`, account
//!   `byok-<provider>`), written through the Security framework directly — the
//!   key never appears in a process argv, a file, or a log.
//! * `rewrite` is blocking with a hard 4 s budget. Any failure (network,
//!   timeout, bad output) is an `Err`, and the caller keeps the rules line.
//! * Keys are only ever sent over https, or plain http to loopback.
//! * Nothing here logs keys, prompts, or model output.

use serde_json::{json, Value};
use std::time::Duration;

/// Keychain service name (same as the bundle id).
const KEYCHAIN_SERVICE: &str = "dev.orbi.app";
/// Hard wall-clock budget for one rewrite (connect + request + response).
const TIMEOUT: Duration = Duration::from_secs(4);
/// Max characters of `detail` sent to the model.
const DETAIL_MAX: usize = 1500;
/// Max characters accepted back from the model.
const OUTPUT_MAX: usize = 120;
/// Max bytes of a response body we are willing to read.
const RESPONSE_MAX: usize = 256 * 1024;
const MAX_TOKENS: u32 = 60;
const TEMPERATURE: f64 = 0.2;

pub const DEFAULT_OLLAMA_BASE: &str = "http://127.0.0.1:11434/v1";
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
pub const OPENAI_BASE: &str = "https://api.openai.com/v1";

const SYSTEM_PROMPT: &str = "You rewrite an AI coding agent's permission request as ONE short plain-English line for a busy developer.\n\
Rules:\n\
- Output exactly one line, at most 90 characters. No preamble, no quotes, no explanation.\n\
- Start with a verb phrase such as \"wants to ...\". Do not include the agent's name.\n\
- If there is a real risk (deleting files, force-push, sudo, piping a download into a shell, touching secrets or files outside the project, network access), name it briefly.\n\
- Only use facts present in the request. Never invent files, commands, hosts, or effects.\n\
- Never say or imply that the action is safe, harmless, or fine to approve.\n\
- No markdown except `backticks` around commands, paths, and hosts.";

/// Which model to use. The key is passed separately to `rewrite`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    /// "ollama" | "openrouter" | "openai" | "custom"
    pub provider: String,
    /// OpenAI-compatible base URL (ending in `/v1` or similar). Used for
    /// "ollama" (empty → default) and "custom" (required). Ignored for
    /// "openrouter" and "openai", which always use their official endpoints.
    pub base_url: String,
    pub model: String,
}

// ---------------------------------------------------------------------------
// Keychain
// ---------------------------------------------------------------------------

fn keychain_account(provider: &str) -> Result<String, String> {
    let p = provider.trim().to_ascii_lowercase();
    if p.is_empty()
        || p.len() > 32
        || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("invalid provider name".into());
    }
    Ok(format!("byok-{p}"))
}

/// The stored API key for `provider`, if any.
pub fn key_get(provider: &str) -> Option<String> {
    let account = keychain_account(provider).ok()?;
    keychain::get(&account)
}

/// Store (`Some`) or remove (`None` / empty) the API key for `provider`.
pub fn key_set(provider: &str, key: Option<&str>) -> Result<(), String> {
    let account = keychain_account(provider)?;
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            if k.len() > 4096 || k.chars().any(char::is_control) {
                return Err("API key looks malformed".into());
            }
            keychain::set(&account, k)
        }
        None => keychain::delete(&account),
    }
}

#[cfg(target_os = "macos")]
mod keychain {
    use super::KEYCHAIN_SERVICE;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    pub fn get(account: &str) -> Option<String> {
        let bytes = get_generic_password(KEYCHAIN_SERVICE, account).ok()?;
        let s = String::from_utf8(bytes).ok()?;
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_string())
    }

    pub fn set(account: &str, key: &str) -> Result<(), String> {
        // Creates the item, or updates it in place if it already exists.
        set_generic_password(KEYCHAIN_SERVICE, account, key.as_bytes())
            .map_err(|e| format!("Keychain write failed (OSStatus {})", e.code()))
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match delete_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(format!("Keychain delete failed (OSStatus {})", e.code())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod keychain {
    pub fn get(_account: &str) -> Option<String> {
        None
    }
    pub fn set(_account: &str, _key: &str) -> Result<(), String> {
        Err("key storage is only supported on macOS".into())
    }
    pub fn delete(_account: &str) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Rewrite
// ---------------------------------------------------------------------------

/// Rewrites the rule-based line more naturally. Blocking, hard 4 s timeout.
/// Must never be the reason a request waits: callers show the rules line first.
pub fn rewrite(
    cfg: &ModelConfig,
    key: Option<&str>,
    agent_label: &str,
    tool: &str,
    rules_line: &str,
    detail: &str,
) -> Result<String, String> {
    let base = resolve_base(cfg)?;
    check_base_url(&base)?;
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    if key_required(&cfg.provider) && key.is_none() {
        return Err(format!("no API key set for {}", cfg.provider.trim()));
    }
    if cfg.model.trim().is_empty() {
        return Err("no model set".into());
    }

    let body = build_body(cfg, agent_label, tool, rules_line, detail);
    let url = format!("{base}/chat/completions");

    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT) // whole request: connect, send, headers, body
        .timeout_connect(TIMEOUT)
        .redirects(0) // never forward the Authorization header anywhere else
        .user_agent(concat!("Orbi/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut req = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json");
    if let Some(k) = key {
        req = req.set("Authorization", &format!("Bearer {k}"));
    }
    if provider(cfg) == "openrouter" {
        // App attribution on OpenRouter; add `HTTP-Referer` once there is a
        // public site URL.
        req = req.set("X-Title", "Orbi");
    }

    let payload = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let (status, text) = match req.send_string(&payload) {
        Ok(resp) => (resp.status(), read_limited(resp)?),
        Err(ureq::Error::Status(code, resp)) => (code, read_limited(resp).unwrap_or_default()),
        Err(ureq::Error::Transport(t)) => return Err(transport_error(&t)),
    };
    if !(200..300).contains(&status) {
        return Err(format!("model request failed: HTTP {status}{}", api_error_suffix(&text)));
    }
    let content = parse_response(&text)?;
    clean_output(&content, agent_label)
}

fn read_limited(resp: ureq::Response) -> Result<String, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(RESPONSE_MAX as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                "model timed out".to_string()
            } else {
                "model response could not be read".to_string()
            }
        })?;
    if buf.len() > RESPONSE_MAX {
        return Err("model response too large".into());
    }
    String::from_utf8(buf).map_err(|_| "model response is not UTF-8".into())
}

fn transport_error(t: &ureq::Transport) -> String {
    use ureq::ErrorKind::*;
    match t.kind() {
        ConnectionFailed => "could not connect to the model server".into(),
        Dns => "could not resolve the model server".into(),
        Io => "model timed out or the connection dropped".into(),
        InvalidUrl | UnknownScheme => "invalid model URL".into(),
        _ => "model request failed".into(),
    }
}

/// " — <provider error message>" (short, single line) if the body has one.
fn api_error_suffix(body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.pointer("/error/message")
            .or_else(|| v.get("error"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    match msg {
        Some(m) if !m.trim().is_empty() => {
            let one: String = m.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(" — {}", truncate_chars(&one, 160))
        }
        _ => String::new(),
    }
}

fn provider(cfg: &ModelConfig) -> String {
    cfg.provider.trim().to_ascii_lowercase()
}

fn key_required(p: &str) -> bool {
    matches!(p.trim().to_ascii_lowercase().as_str(), "openrouter" | "openai")
}

/// Default OpenAI-compatible base URL for a provider ("" for custom/unknown).
pub fn default_base_url(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "ollama" => DEFAULT_OLLAMA_BASE,
        "openrouter" => OPENROUTER_BASE,
        "openai" => OPENAI_BASE,
        _ => "",
    }
}

/// The base URL to call, without a trailing slash.
pub fn resolve_base(cfg: &ModelConfig) -> Result<String, String> {
    let base = match provider(cfg).as_str() {
        "openrouter" => OPENROUTER_BASE.to_string(),
        "openai" => OPENAI_BASE.to_string(),
        "ollama" => {
            let b = cfg.base_url.trim();
            if b.is_empty() { DEFAULT_OLLAMA_BASE.to_string() } else { b.to_string() }
        }
        "custom" => {
            let b = cfg.base_url.trim();
            if b.is_empty() {
                return Err("custom provider needs a base URL".into());
            }
            b.to_string()
        }
        other => return Err(format!("unknown provider '{other}'")),
    };
    Ok(base.trim_end_matches('/').to_string())
}

/// https anywhere; plain http only to loopback. No userinfo, no whitespace.
pub fn check_base_url(url: &str) -> Result<(), String> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("model URL contains whitespace".into());
    }
    let lower = url.to_ascii_lowercase();
    let (https, rest) = if let Some(r) = lower.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = lower.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("model URL must start with https:// (or http:// for localhost)".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("model URL must not contain credentials".into());
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        match v6.split_once(']') {
            Some((h, _)) => h,
            None => return Err("invalid model URL".into()),
        }
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        return Err("model URL has no host".into());
    }
    if https {
        return Ok(());
    }
    let host = host.trim_end_matches('.');
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        Ok(())
    } else {
        Err("Use https:// — keys must not travel in plain text.".into())
    }
}

/// The OpenAI-format request body.
pub fn build_body(
    cfg: &ModelConfig,
    agent_label: &str,
    tool: &str,
    rules_line: &str,
    detail: &str,
) -> Value {
    let detail = truncate_chars(detail.trim(), DETAIL_MAX);
    let user = format!(
        "Agent: {agent}\nTool: {tool}\nCurrent line: {line}\nDetails:\n{detail}\n\nRewrite the current line.",
        agent = one_line(agent_label, 60),
        tool = one_line(tool, 60),
        line = one_line(rules_line, 300),
        detail = if detail.is_empty() { "(none)".to_string() } else { detail },
    );
    let mut body = json!({
        "model": cfg.model.trim(),
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user },
        ],
        "max_tokens": MAX_TOKENS,
        "temperature": TEMPERATURE,
        "stream": false,
    });
    // Thinking models would burn the whole token budget on reasoning.
    // Only send these where the server is known to accept them
    // (api.openai.com rejects unknown parameters).
    match provider(cfg).as_str() {
        "ollama" => body["reasoning_effort"] = json!("none"),
        "openrouter" => body["reasoning"] = json!({ "effort": "none", "exclude": true }),
        _ => {}
    }
    body
}

/// `choices[0].message.content` from an OpenAI-format response.
pub fn parse_response(text: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "model returned invalid JSON".to_string())?;
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return Err(format!("model returned an error{}", api_error_suffix(text)));
    }
    let content = v
        .pointer("/choices/0/message/content")
        .ok_or_else(|| "model response has no content".to_string())?;
    match content {
        Value::String(s) => Ok(s.clone()),
        // Some servers return content parts: [{"type":"text","text":"..."}]
        Value::Array(parts) => {
            let s: String = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect();
            Ok(s)
        }
        _ => Err("model response has no content".into()),
    }
}

/// Validate and tidy the model's output. `Err` → caller keeps the rules line.
pub fn clean_output(raw: &str, agent_label: &str) -> Result<String, String> {
    let mut s = strip_think(raw).trim().to_string();

    // Tolerate a leading "Line:" style label.
    for label in ["line:", "rewritten:", "rewrite:", "output:", "answer:"] {
        if s.len() >= label.len() && s[..label.len()].eq_ignore_ascii_case(label) {
            s = s[label.len()..].trim_start().to_string();
        }
    }
    s = strip_quotes(&s);

    if s.is_empty() {
        return Err("model returned nothing".into());
    }
    if s.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        return Err("model returned more than one line".into());
    }
    let mut s = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();

    // The face prefixes the agent name itself.
    let label = agent_label.trim();
    if !label.is_empty()
        && s.len() > label.len()
        && s.is_char_boundary(label.len())
        && s[..label.len()].eq_ignore_ascii_case(label)
        && s[label.len()..].starts_with(' ')
    {
        s = s[label.len()..].trim_start().to_string();
    }
    for prefix in ["The agent ", "the agent ", "Agent ", "It "] {
        if let Some(r) = s.strip_prefix(prefix) {
            s = r.to_string();
        }
    }
    s = strip_quotes(&s);
    let s = s.trim_end_matches(['.', ' ']).trim().to_string();

    if s.is_empty() {
        return Err("model returned nothing".into());
    }
    if s.chars().count() > OUTPUT_MAX {
        return Err("model line too long".into());
    }
    if s.chars().any(|c| c.is_control()) {
        return Err("model line has control characters".into());
    }
    if s.contains("**")
        || s.contains("](")
        || s.contains("```")
        || s.starts_with('#')
        || s.starts_with("- ")
        || s.starts_with("* ")
        || s.matches('`').count() % 2 != 0
    {
        return Err("model line has markdown".into());
    }
    let first = s.chars().next().unwrap_or(' ');
    if !first.is_alphabetic() {
        return Err("model line does not start with a verb".into());
    }
    let lower = s.to_lowercase();
    const REASSURING: [&str; 10] = [
        "is safe", "it's safe", "its safe", "safe to", "perfectly safe", "harmless",
        "no risk", "risk-free", "nothing to worry", "fine to approve",
    ];
    if REASSURING.iter().any(|p| lower.contains(p)) {
        return Err("model line claims the action is safe".into());
    }
    // "Wants to …" → "wants to …" (the face renders "<Agent> wants to …").
    let mut out = String::with_capacity(s.len());
    let mut cs = s.chars();
    if let Some(f) = cs.next() {
        let rest = cs.as_str();
        let second_upper = rest.chars().next().is_some_and(char::is_uppercase);
        if f.is_uppercase() && !second_upper {
            out.extend(f.to_lowercase());
        } else {
            out.push(f);
        }
        out.push_str(rest);
    }
    Ok(out)
}

/// Drop `<think>…</think>` blocks (and an unterminated trailing one).
fn strip_think(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        match rest.find("<think>") {
            Some(i) => {
                out.push_str(&rest[..i]);
                match rest[i..].find("</think>") {
                    Some(j) => rest = &rest[i + j + "</think>".len()..],
                    None => break,
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn strip_quotes(s: &str) -> String {
    let mut t = s.trim();
    for (open, close) in [('"', '"'), ('\'', '\''), ('\u{201C}', '\u{201D}'), ('\u{2018}', '\u{2019}')] {
        if t.chars().count() >= 2 && t.starts_with(open) && t.ends_with(close) {
            let inner = &t[open.len_utf8()..t.len() - close.len_utf8()];
            if !inner.contains(open) && !inner.contains(close) {
                t = inner.trim();
            }
        }
    }
    t.to_string()
}

fn one_line(s: &str, max: usize) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&joined, max)
}

/// First `max` chars of `s` (char-boundary safe), with "…" if cut.
fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(p: &str, base: &str, model: &str) -> ModelConfig {
        ModelConfig { provider: p.into(), base_url: base.into(), model: model.into() }
    }

    #[test]
    fn url_safety() {
        for ok in [
            "https://openrouter.ai/api/v1",
            "https://example.com:8443/v1",
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "HTTP://LOCALHOST/v1",
            "http://[::1]:11434/v1",
        ] {
            assert!(check_base_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://example.com/v1",
            "http://192.168.1.5:11434/v1",
            "http://localhost.evil.com/v1",
            "http://127.0.0.1.nip.io/v1",
            "http://localhost@evil.com/v1",
            "https://user:pw@example.com/v1",
            "ftp://example.com",
            "example.com/v1",
            "https:///v1",
            "http://[::2]/v1",
            "https://exa mple.com",
            "",
        ] {
            assert!(check_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn default_bases() {
        assert_eq!(default_base_url("ollama"), "http://127.0.0.1:11434/v1");
        assert_eq!(default_base_url("OpenRouter"), "https://openrouter.ai/api/v1");
        assert_eq!(default_base_url("openai"), "https://api.openai.com/v1");
        assert_eq!(default_base_url("custom"), "");
        assert_eq!(default_base_url("x"), "");
        assert_eq!(
            check_base_url("http://example.com").unwrap_err(),
            "Use https:// — keys must not travel in plain text."
        );
    }

    #[test]
    fn base_resolution() {
        assert_eq!(resolve_base(&cfg("ollama", "", "m")).unwrap(), DEFAULT_OLLAMA_BASE);
        assert_eq!(resolve_base(&cfg("ollama", "http://localhost:9/v1/", "m")).unwrap(), "http://localhost:9/v1");
        // Official providers ignore a stale / hostile base_url.
        assert_eq!(resolve_base(&cfg("openai", "http://evil.com", "m")).unwrap(), OPENAI_BASE);
        assert_eq!(resolve_base(&cfg("OpenRouter", "", "m")).unwrap(), OPENROUTER_BASE);
        assert!(resolve_base(&cfg("custom", " ", "m")).is_err());
        assert_eq!(resolve_base(&cfg("custom", "https://x.dev/v1", "m")).unwrap(), "https://x.dev/v1");
        assert!(resolve_base(&cfg("nope", "", "m")).is_err());
    }

    #[test]
    fn body_shape() {
        let long = "x".repeat(5000);
        let b = build_body(&cfg("openai", "", "gpt-4o-mini"), "Claude Code", "Bash", "wants to run `npm test` in orbi/", &long);
        assert_eq!(b["model"], "gpt-4o-mini");
        assert_eq!(b["max_tokens"], MAX_TOKENS);
        assert_eq!(b["temperature"], TEMPERATURE);
        assert_eq!(b["stream"], false);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["role"], "user");
        assert!(b.get("reasoning_effort").is_none() && b.get("reasoning").is_none());
        let user = b["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("Tool: Bash"));
        assert!(user.contains("wants to run `npm test` in orbi/"));
        let xs = user.chars().filter(|&c| c == 'x').count();
        assert_eq!(xs, DETAIL_MAX);

        let o = build_body(&cfg("ollama", "", "qwen3:8b"), "Codex", "Edit", "wants to edit `a.rs`", "");
        assert_eq!(o["reasoning_effort"], "none");
        assert!(o["messages"][1]["content"].as_str().unwrap().contains("(none)"));
        let r = build_body(&cfg("openrouter", "", "m"), "A", "T", "l", "d");
        assert_eq!(r["reasoning"]["effort"], "none");
    }

    #[test]
    fn detail_truncation_is_char_safe() {
        let s = "é".repeat(2000);
        let t = truncate_chars(&s, DETAIL_MAX);
        assert_eq!(t.chars().count(), DETAIL_MAX + 1); // + "…"
    }

    #[test]
    fn response_parsing() {
        let ok = r#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"wants to run tests"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_response(ok).unwrap(), "wants to run tests");
        let parts = r#"{"choices":[{"message":{"content":[{"type":"text","text":"wants to "},{"type":"text","text":"go"}]}}]}"#;
        assert_eq!(parse_response(parts).unwrap(), "wants to go");
        let err = r#"{"error":{"message":"No auth credentials found","code":401}}"#;
        let e = parse_response(err).unwrap_err();
        assert!(e.contains("No auth credentials found"), "{e}");
        assert!(parse_response(r#"{"choices":[]}"#).is_err());
        assert!(parse_response(r#"{"choices":[{"message":{"content":null}}]}"#).is_err());
        assert!(parse_response("<html>").is_err());
    }

    #[test]
    fn output_validation_accepts_and_tidies() {
        let c = |s: &str| clean_output(s, "Claude Code");
        assert_eq!(c("wants to run `npm test` in orbi/").unwrap(), "wants to run `npm test` in orbi/");
        assert_eq!(c("\"Wants to delete `dist/` — removes build output.\"\n").unwrap(), "wants to delete `dist/` — removes build output");
        assert_eq!(c("Claude Code wants to push to `main`").unwrap(), "wants to push to `main`");
        assert_eq!(c("<think>\nhmm\n</think>\n\nwants to open `example.com`").unwrap(), "wants to open `example.com`");
        assert_eq!(c("Line: wants to edit `.env` (secrets)").unwrap(), "wants to edit `.env` (secrets)");
        assert_eq!(c("\u{201C}wants to fetch a URL\u{201D}").unwrap(), "wants to fetch a URL");
        // Acronym at the start keeps its case.
        assert_eq!(c("SSH into `prod` as root").unwrap(), "SSH into `prod` as root");
    }

    #[test]
    fn output_validation_rejects() {
        let c = |s: &str| clean_output(s, "Claude Code");
        assert!(c("").is_err());
        assert!(c("   \n ").is_err());
        assert!(c("<think>still thinking").is_err());
        assert!(c("wants to run tests\nThis is fine.").is_err());
        assert!(c(&format!("wants to {}", "a".repeat(130))).is_err());
        assert!(c("**wants** to run tests").is_err());
        assert!(c("- wants to run tests").is_err());
        assert!(c("wants to run `npm test").is_err());
        assert!(c("wants to run tests, which is safe").is_err());
        assert!(c("wants to list files (harmless)").is_err());
        assert!(c("`rm -rf dist`").is_err());
    }

    #[test]
    fn keychain_account_names() {
        assert_eq!(keychain_account("OpenRouter").unwrap(), "byok-openrouter");
        assert!(keychain_account("").is_err());
        assert!(keychain_account("a b").is_err());
        assert!(keychain_account("../x").is_err());
    }

    #[test]
    fn missing_key_and_insecure_url_fail_fast() {
        let t = std::time::Instant::now();
        assert!(rewrite(&cfg("openai", "", "gpt-4o-mini"), None, "A", "Bash", "l", "d").is_err());
        assert!(rewrite(&cfg("custom", "http://example.com/v1", "m"), Some("k"), "A", "Bash", "l", "d").is_err());
        assert!(t.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn unreachable_server_errs_within_budget() {
        // Port 9 (discard) on loopback is normally closed → fast refusal.
        let t = std::time::Instant::now();
        let r = rewrite(&cfg("custom", "http://127.0.0.1:9/v1", "m"), None, "A", "Bash", "l", "d");
        assert!(r.is_err());
        assert!(t.elapsed() < TIMEOUT + Duration::from_secs(1));
    }

    /// Live round-trip against a local Ollama, if one is running.
    /// Model: $ORBI_TEST_OLLAMA_MODEL or the first installed model.
    #[test]
    fn live_ollama_if_running() {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(500)).build();
        let tags = match agent.get("http://127.0.0.1:11434/api/tags").call() {
            Ok(r) => r.into_string().unwrap_or_default(),
            Err(_) => {
                eprintln!("ollama not running — skipping live test");
                return;
            }
        };
        let model = std::env::var("ORBI_TEST_OLLAMA_MODEL").ok().or_else(|| {
            serde_json::from_str::<Value>(&tags)
                .ok()?
                .pointer("/models/0/name")?
                .as_str()
                .map(str::to_string)
        });
        let Some(model) = model else {
            eprintln!("ollama has no models — skipping live test");
            return;
        };
        let t = std::time::Instant::now();
        let r = rewrite(
            &cfg("ollama", "", &model),
            None,
            "Claude Code",
            "Bash",
            "wants to run `rm -rf dist && npm run build` in orbi/ ⚠ deletes files",
            "rm -rf dist && npm run build\ncwd: /work/orbi",
        );
        let ms = t.elapsed().as_millis();
        assert!(t.elapsed() < TIMEOUT + Duration::from_millis(500), "took {ms} ms");
        // A cold model load can exceed 4 s; that must be an Err, never a hang.
        match r {
            Ok(line) => {
                eprintln!("live ({model}, {ms} ms): {line}");
                assert!(!line.contains('\n') && line.chars().count() <= OUTPUT_MAX);
            }
            Err(e) => eprintln!("live ({model}, {ms} ms) returned Err: {e}"),
        }
    }
}
