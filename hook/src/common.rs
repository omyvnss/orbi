//! Agent-independent pieces: Orbi request bodies, decision parsing, short
//! status lines, and normalising each agent's tool names to the Claude Code
//! vocabulary Orbi's explainer understands (Bash, Edit, Write, WebFetch, …).
//! No I/O here, so it is all unit-testable.

use serde_json::{json, Map, Value};

pub const REASON_ALLOW: &str = "Approved in Orbi";
pub const REASON_DENY: &str = "Denied in Orbi";

/// Largest request body we send; Orbi rejects bodies over 256 KB with 413.
pub const MAX_BODY: usize = 240 * 1024;
const MAX_STRING: usize = 32 * 1024;

pub fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// First non-empty string among `keys`.
pub fn first<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().map(|k| s(v, k)).find(|x| !x.is_empty()).unwrap_or("")
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Decision {
    Allow,
    Deny,
    /// Fall back to the agent's own prompt: timeout, paused, Orbi not
    /// running, unverified server, anything unexpected.
    Ask,
}

impl Decision {
    pub fn word(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

/// Interpret a *verified* `/ask` response. Only a 200 whose JSON says exactly
/// "allow" or "deny" counts; everything else is Ask.
pub fn parse_decision(status: u16, body: &[u8]) -> Decision {
    if status != 200 {
        return Decision::Ask;
    }
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return Decision::Ask };
    match v.get("decision").and_then(Value::as_str) {
        Some("allow") => Decision::Allow,
        Some("deny") => Decision::Deny,
        _ => Decision::Ask,
    }
}

/// Body for `POST /ask`.
pub fn ask_body(agent: &str, tool: &str, input: Value, cwd: &str, session: &str, app_bundle: Option<&str>) -> Value {
    let mut body = Map::new();
    body.insert("agent".into(), json!(agent));
    body.insert("tool".into(), json!(tool));
    body.insert("input".into(), if input.is_null() { json!({}) } else { input });
    body.insert("cwd".into(), json!(cwd));
    if !session.is_empty() {
        body.insert("session".into(), json!(session));
    }
    if let Some(b) = app_bundle.filter(|b| !b.is_empty()) {
        body.insert("app_bundle".into(), json!(b));
    }
    Value::Object(body)
}

/// Body for `POST /event`.
pub fn event_body(agent: &str, kind: &str, summary: &str, detail: Option<&str>, session: &str, app_bundle: Option<&str>) -> Value {
    let mut b = Map::new();
    b.insert("agent".into(), json!(agent));
    b.insert("kind".into(), json!(kind));
    b.insert("summary".into(), json!(summary));
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        b.insert("detail".into(), json!(d));
    }
    if !session.is_empty() {
        b.insert("session".into(), json!(session));
    }
    if let Some(a) = app_bundle.filter(|a| !a.is_empty()) {
        b.insert("app_bundle".into(), json!(a));
    }
    Value::Object(b)
}

/// Serialize a request body, shrinking huge string fields (e.g. a Write's
/// `content`) so it stays under Orbi's 256 KB limit instead of being dropped.
pub fn encode_body(mut body: Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    if bytes.len() <= MAX_BODY {
        return bytes;
    }
    shrink_strings(&mut body);
    serde_json::to_vec(&body).unwrap_or_default()
}

fn shrink_strings(v: &mut Value) {
    match v {
        Value::String(s) if s.len() > MAX_STRING => {
            let mut cut = MAX_STRING;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            s.truncate(cut);
            s.push_str("\n\u{2026}[truncated by orbi-hook]");
        }
        Value::Array(a) => a.iter_mut().for_each(shrink_strings),
        Value::Object(o) => o.values_mut().for_each(shrink_strings),
        _ => {}
    }
}

pub fn short(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ").replace('`', "'");
    if flat.chars().count() <= max {
        flat
    } else {
        flat.chars().take(max - 1).collect::<String>().trim_end().to_string() + "\u{2026}"
    }
}

fn rel(path: &str, cwd: &str) -> String {
    let c = cwd.trim_end_matches('/');
    if !c.is_empty() {
        if let Some(r) = path.strip_prefix(&format!("{}/", c)) {
            return r.to_string();
        }
    }
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn host(url: &str) -> String {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    let a = rest.split(['/', '?', '#']).next().unwrap_or("");
    let a = a.rsplit('@').next().unwrap_or(a);
    a.split(':').next().unwrap_or("").trim_start_matches("www.").to_string()
}

/// A short present-tense status for a (normalised) tool call: "running `npm test`".
pub fn tool_summary(tool: &str, input: &Value, cwd: &str) -> String {
    match tool {
        "Bash" | "PowerShell" => format!("running `{}`", short(s(input, "command"), 50)),
        "Edit" | "MultiEdit" | "NotebookEdit" => {
            let p = first(input, &["file_path", "notebook_path"]);
            format!("editing `{}`", short(&rel(p, cwd), 50))
        }
        "Write" => format!("writing `{}`", short(&rel(s(input, "file_path"), cwd), 50)),
        "Read" => format!("reading `{}`", short(&rel(s(input, "file_path"), cwd), 50)),
        "Glob" | "Grep" | "LS" => "searching files".into(),
        "WebFetch" => format!("reading {}", host(s(input, "url"))),
        "WebSearch" => format!("searching the web for \u{201c}{}\u{201d}", short(s(input, "query"), 40)),
        "Task" | "Agent" => {
            let d = s(input, "description");
            if d.is_empty() {
                "running a sub-agent".into()
            } else {
                format!("running a sub-agent: {}", short(d, 40))
            }
        }
        "TodoWrite" | "TaskCreate" | "TaskUpdate" => "updating its plan".into(),
        t if t.starts_with("mcp__") => {
            let rest = &t[5..];
            match rest.split_once("__") {
                Some((server, name)) => format!("using `{}` from {}", name, server),
                None => format!("using {}", rest),
            }
        }
        "" => "working".into(),
        t => format!("using {}", t),
    }
}

/// Parse an `apply_patch` envelope (Codex): the files it touches and the
/// removed / added lines, so Orbi can show "edit `x` (+N −M)".
pub fn parse_patch(patch: &str) -> (Vec<String>, String, String) {
    let mut files = Vec::new();
    let (mut old, mut new) = (String::new(), String::new());
    for line in patch.lines() {
        let file = ["*** Update File: ", "*** Add File: ", "*** Delete File: ", "*** Move to: "]
            .iter()
            .find_map(|p| line.strip_prefix(p));
        if let Some(f) = file {
            let f = f.trim().to_string();
            if !f.is_empty() && !files.contains(&f) {
                files.push(f);
            }
        } else if line.starts_with("***") || line.starts_with("@@") {
            continue;
        } else if let Some(l) = line.strip_prefix('-') {
            old.push_str(l);
            old.push('\n');
        } else if let Some(l) = line.strip_prefix('+') {
            new.push_str(l);
            new.push('\n');
        }
    }
    (files, old, new)
}

fn join_path(cwd: &str, p: &str) -> String {
    if p.starts_with('/') || cwd.is_empty() {
        p.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), p)
    }
}

/// Map an agent's own tool name + input onto the Claude Code vocabulary that
/// Orbi's explainer and status lines understand. Unknown tools pass through.
pub fn normalize_tool(agent: &str, tool: &str, input: &Value, cwd: &str) -> (String, Value) {
    let t = tool.to_string();
    let same = || (t.clone(), input.clone());
    match (agent, tool) {
        // Codex: Bash is already Bash; file edits arrive as an apply_patch envelope.
        (_, "apply_patch") => {
            let patch = first(input, &["command", "input", "patch"]);
            let (files, old, new) = parse_patch(patch);
            let path = files.first().map(|f| join_path(cwd, f)).unwrap_or_default();
            (
                "Edit".into(),
                json!({ "file_path": path, "old_string": old, "new_string": new, "files": files, "patch": patch }),
            )
        }
        // Gemini CLI built-ins.
        ("gemini", "run_shell_command") => ("Bash".into(), json!({ "command": s(input, "command"), "directory": s(input, "dir_path") })),
        ("gemini", "replace") => ("Edit".into(), input.clone()),
        ("gemini", "write_file") => ("Write".into(), input.clone()),
        ("gemini", "read_file") | ("gemini", "read_many_files") => ("Read".into(), json!({ "file_path": first(input, &["file_path", "absolute_path"]) })),
        ("gemini", "glob") | ("gemini", "search_file_content") | ("gemini", "grep_search") | ("gemini", "list_directory") => ("Grep".into(), input.clone()),
        ("gemini", "web_fetch") => {
            let prompt = s(input, "prompt");
            let url = prompt.split_whitespace().find(|w| w.starts_with("http://") || w.starts_with("https://")).unwrap_or("");
            ("WebFetch".into(), json!({ "url": url, "prompt": prompt }))
        }
        ("gemini", "google_web_search") => ("WebSearch".into(), input.clone()),
        ("gemini", t) if t.starts_with("mcp_") && !t.starts_with("mcp__") => same(),
        // Hermes.
        ("hermes", "terminal") => ("Bash".into(), json!({ "command": s(input, "command") })),
        ("hermes", "write_file") => ("Write".into(), json!({ "file_path": first(input, &["path", "file_path"]), "content": s(input, "content") })),
        ("hermes", "patch") => (
            "Edit".into(),
            json!({ "file_path": first(input, &["path", "file_path"]), "old_string": s(input, "old_string"), "new_string": s(input, "new_string") }),
        ),
        ("hermes", "read_file") => ("Read".into(), json!({ "file_path": first(input, &["path", "file_path"]) })),
        ("hermes", "web_search") => ("WebSearch".into(), json!({ "query": s(input, "query") })),
        ("hermes", "web_extract") | ("hermes", "web_fetch") => {
            let url = input.get("urls").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or_else(|| s(input, "url"));
            ("WebFetch".into(), json!({ "url": url }))
        }
        ("hermes", "search_files") => ("Grep".into(), input.clone()),
        // OpenCode (camelCase inputs).
        ("opencode", "bash") => ("Bash".into(), json!({ "command": s(input, "command"), "description": s(input, "description") })),
        ("opencode", "edit") => (
            "Edit".into(),
            json!({ "file_path": s(input, "filePath"), "old_string": s(input, "oldString"), "new_string": s(input, "newString") }),
        ),
        ("opencode", "write") => ("Write".into(), json!({ "file_path": s(input, "filePath"), "content": s(input, "content") })),
        ("opencode", "read") => ("Read".into(), json!({ "file_path": s(input, "filePath") })),
        ("opencode", "webfetch") => ("WebFetch".into(), json!({ "url": s(input, "url") })),
        ("opencode", "websearch") => ("WebSearch".into(), json!({ "query": s(input, "query") })),
        ("opencode", "glob") | ("opencode", "grep") | ("opencode", "list") => ("Grep".into(), input.clone()),
        ("opencode", "task") => ("Task".into(), input.clone()),
        ("opencode", "todowrite") => ("TodoWrite".into(), input.clone()),
        // Cursor.
        ("cursor", "Shell") => ("Bash".into(), json!({ "command": s(input, "command") })),
        _ => same(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_only_from_explicit_200() {
        assert_eq!(parse_decision(200, br#"{"decision":"allow","reason":"x"}"#), Decision::Allow);
        assert_eq!(parse_decision(200, br#"{"decision":"deny"}"#), Decision::Deny);
        for (st, b) in [
            (200, &br#"{"decision":"ask"}"#[..]),
            (200, br#"{"decision":"ALLOW"}"#),
            (200, br#"{"decision":true}"#),
            (200, b"allow"),
            (200, b""),
            (401, br#"{"decision":"allow"}"#),
            (500, br#"{"decision":"allow"}"#),
        ] {
            assert_eq!(parse_decision(st, b), Decision::Ask, "{st} {:?}", String::from_utf8_lossy(b));
        }
    }

    #[test]
    fn ask_and_event_bodies() {
        assert_eq!(
            ask_body("codex", "Bash", json!({"command": "ls"}), "/w", "s1", Some("com.apple.Terminal")),
            json!({"agent": "codex", "tool": "Bash", "input": {"command": "ls"}, "cwd": "/w", "session": "s1", "app_bundle": "com.apple.Terminal"})
        );
        assert_eq!(ask_body("x", "", Value::Null, "", "", Some("")), json!({"agent": "x", "tool": "", "input": {}, "cwd": ""}));
        assert_eq!(
            event_body("my-script", "done", "deployed", None, "", None),
            json!({"agent": "my-script", "kind": "done", "summary": "deployed"})
        );
        assert_eq!(
            event_body("x", "error", "boom", Some("trace"), "s1", Some("b")),
            json!({"agent": "x", "kind": "error", "summary": "boom", "detail": "trace", "session": "s1", "app_bundle": "b"})
        );
    }

    #[test]
    fn huge_bodies_are_shrunk() {
        let big = "x\n".repeat(400 * 1024);
        let body = ask_body("claude-code", "Write", json!({"file_path": "/w/a", "content": big}), "/w", "", None);
        let bytes = encode_body(body);
        assert!(bytes.len() <= MAX_BODY);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["input"]["file_path"], "/w/a");
        assert!(v["input"]["content"].as_str().unwrap().ends_with("[truncated by orbi-hook]"));
        assert_eq!(encode_body(json!({"a": "b"})), br#"{"a":"b"}"#);
        let emoji = "\u{1F600}".repeat(100_000);
        let _ = encode_body(json!({ "input": { "content": emoji } }));
    }

    #[test]
    fn summaries() {
        let c = "/work/orbi";
        assert_eq!(tool_summary("Bash", &json!({"command": "npm test"}), c), "running `npm test`");
        assert_eq!(tool_summary("Read", &json!({"file_path": "/work/orbi/a/b.rs"}), c), "reading `a/b.rs`");
        assert_eq!(tool_summary("Read", &json!({"file_path": "/etc/hosts"}), c), "reading `hosts`");
        assert_eq!(tool_summary("Edit", &json!({"file_path": "/work/orbi/src/app.ts"}), c), "editing `src/app.ts`");
        assert_eq!(tool_summary("Write", &json!({"file_path": "/work/orbi/x"}), c), "writing `x`");
        assert_eq!(tool_summary("Grep", &json!({"pattern": "x"}), c), "searching files");
        assert_eq!(tool_summary("WebFetch", &json!({"url": "https://www.example.com/a"}), c), "reading example.com");
        assert_eq!(tool_summary("Task", &json!({"description": "Explore repo"}), c), "running a sub-agent: Explore repo");
        assert_eq!(tool_summary("mcp__github__list_prs", &json!({}), c), "using `list_prs` from github");
        assert_eq!(tool_summary("Weird", &json!({}), c), "using Weird");
        let long = tool_summary("Bash", &json!({"command": "a".repeat(200)}), c);
        assert!(long.chars().count() < 70 && long.contains('\u{2026}'));
    }

    #[test]
    fn patches() {
        let p = "*** Begin Patch\n*** Update File: src/a.rs\n@@ fn x\n-old1\n-old2\n+new1\n context\n*** Add File: b.txt\n+hi\n*** End Patch\n";
        let (files, old, new) = parse_patch(p);
        assert_eq!(files, vec!["src/a.rs", "b.txt"]);
        assert_eq!(old, "old1\nold2\n");
        assert_eq!(new, "new1\nhi\n");
        let (t, i) = normalize_tool("codex", "apply_patch", &json!({"command": p}), "/w");
        assert_eq!(t, "Edit");
        assert_eq!(i["file_path"], "/w/src/a.rs");
        assert_eq!(i["new_string"], "new1\nhi\n");
    }

    #[test]
    fn normalizes_each_agent() {
        let n = |a: &str, t: &str, i: Value| normalize_tool(a, t, &i, "/w");
        assert_eq!(n("gemini", "run_shell_command", json!({"command": "ls"})).0, "Bash");
        assert_eq!(n("gemini", "run_shell_command", json!({"command": "ls"})).1["command"], "ls");
        assert_eq!(n("gemini", "replace", json!({"file_path": "/w/a"})).0, "Edit");
        assert_eq!(n("gemini", "write_file", json!({"file_path": "/w/a"})).0, "Write");
        assert_eq!(n("gemini", "web_fetch", json!({"prompt": "summarize https://x.dev/a please"})).1["url"], "https://x.dev/a");
        assert_eq!(n("hermes", "terminal", json!({"command": "rm -rf /"})), ("Bash".into(), json!({"command": "rm -rf /"})));
        assert_eq!(n("hermes", "patch", json!({"path": "/w/a", "old_string": "x", "new_string": "y"})).1["file_path"], "/w/a");
        assert_eq!(n("hermes", "web_extract", json!({"urls": ["https://a.b/c"]})).1["url"], "https://a.b/c");
        assert_eq!(n("opencode", "edit", json!({"filePath": "/w/a", "oldString": "x", "newString": "y"})).1["old_string"], "x");
        assert_eq!(n("opencode", "bash", json!({"command": "ls"})).0, "Bash");
        assert_eq!(n("cursor", "Shell", json!({"command": "ls", "working_directory": "/w"})).1, json!({"command": "ls"}));
        assert_eq!(n("claude-code", "Bash", json!({"command": "ls"})), ("Bash".into(), json!({"command": "ls"})));
        assert_eq!(n("claude-code", "Mystery", json!({"a": 1})), ("Mystery".into(), json!({"a": 1})));
    }
}
