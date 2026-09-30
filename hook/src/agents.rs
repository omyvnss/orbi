//! Per-agent translation: each agent's hook stdin → Orbi request, and Orbi's
//! decision → exactly what that agent expects back on stdout.
//! Sources for every shape are in adapters/README.md. No I/O here.

use crate::common::{
    ask_body, event_body, first, normalize_tool, s, short, tool_summary, Decision, REASON_ALLOW, REASON_DENY,
};
use serde_json::{json, Value};

/// The hook-JSON family an agent id belongs to.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Family {
    /// Claude Code, and any unknown agent that speaks its hook JSON.
    Claude,
    /// Codex CLI hooks — same event names and output shapes as Claude Code.
    Codex,
    Gemini,
    Cursor,
    Hermes,
    /// The generated OpenCode plugin (prints a decision word).
    OpenCode,
}

pub fn family(agent: &str) -> Family {
    match agent {
        "codex" => Family::Codex,
        "gemini" => Family::Gemini,
        "cursor" => Family::Cursor,
        "hermes" => Family::Hermes,
        "opencode" => Family::OpenCode,
        _ => Family::Claude,
    }
}

/// What `orbi-hook` prints and how it exits.
#[derive(Debug, PartialEq, Eq)]
pub struct Out {
    pub stdout: Option<String>,
    pub code: i32,
}

impl Out {
    fn silent() -> Out {
        Out { stdout: None, code: 0 }
    }
    fn json(v: Value) -> Out {
        Out { stdout: Some(v.to_string()), code: 0 }
    }
}

/// The decision word + exit code used by the flag form and the OpenCode
/// plugin: allow → 0, deny → 1, ask / unavailable → 2.
pub fn word_output(d: Decision) -> Out {
    let code = match d {
        Decision::Allow => 0,
        Decision::Deny => 1,
        Decision::Ask => 2,
    };
    Out { stdout: Some(d.word().into()), code }
}

/// `(POST /ask body, hook event name)` from an agent's permission-hook stdin.
pub fn ask_request(agent: &str, hook: &Value, app_bundle: Option<&str>) -> (Value, String) {
    let event = s(hook, "hook_event_name").to_string();
    let empty = json!({});
    match family(agent) {
        Family::Cursor => {
            let cwd = cursor_cwd(hook);
            let session = s(hook, "conversation_id");
            let (tool, input) = match event.as_str() {
                "beforeShellExecution" => ("Bash".to_string(), json!({ "command": s(hook, "command") })),
                "beforeMCPExecution" => {
                    let name = format!("mcp__{}__{}", s(hook, "mcp_server_name"), s(hook, "tool_name"));
                    let raw = hook.get("tool_input").cloned().unwrap_or(Value::Null);
                    let input = match &raw {
                        Value::String(t) => serde_json::from_str(t).unwrap_or(raw.clone()),
                        _ => raw,
                    };
                    (name, input)
                }
                _ => normalize_tool("cursor", s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), &cwd),
            };
            (ask_body(agent, &tool, input, &cwd, session, app_bundle), event)
        }
        Family::OpenCode => {
            let req = hook.get("request").unwrap_or(&empty);
            let cwd = s(hook, "cwd");
            let (tool, input) = opencode_permission(req);
            (ask_body(agent, &tool, input, cwd, s(req, "sessionID"), app_bundle), event)
        }
        _ => {
            let cwd = s(hook, "cwd");
            let (tool, input) = normalize_tool(agent, s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), cwd);
            (ask_body(agent, &tool, input, cwd, s(hook, "session_id"), app_bundle), event)
        }
    }
}

fn cursor_cwd(hook: &Value) -> String {
    let c = s(hook, "cwd");
    if !c.is_empty() {
        return c.to_string();
    }
    hook.get("workspace_roots")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// OpenCode `permission.asked` request → Claude-style tool + input.
fn opencode_permission(req: &Value) -> (String, Value) {
    let meta = req.get("metadata").cloned().unwrap_or_else(|| json!({}));
    let patterns: Vec<&str> = req
        .get("patterns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    match s(req, "permission") {
        "bash" | "shell" => {
            let cmd = s(&meta, "command");
            let cmd = if cmd.is_empty() { patterns.join(" ") } else { cmd.to_string() };
            ("Bash".into(), json!({ "command": cmd }))
        }
        "edit" => {
            let diff = s(&meta, "diff");
            let (mut old, mut new) = (String::new(), String::new());
            for l in diff.lines() {
                if l.starts_with("---") || l.starts_with("+++") {
                    continue;
                }
                if let Some(x) = l.strip_prefix('-') {
                    old.push_str(x);
                    old.push('\n');
                } else if let Some(x) = l.strip_prefix('+') {
                    new.push_str(x);
                    new.push('\n');
                }
            }
            let path = first(&meta, &["filepath", "filePath"]);
            let path = if path.is_empty() { patterns.first().copied().unwrap_or("") } else { path };
            ("Edit".into(), json!({ "file_path": path, "old_string": old, "new_string": new, "diff": diff }))
        }
        "webfetch" => {
            let url = s(&meta, "url");
            ("WebFetch".into(), json!({ "url": if url.is_empty() { patterns.first().copied().unwrap_or("") } else { url } }))
        }
        "websearch" => ("WebSearch".into(), json!({ "query": first(&meta, &["query"]) })),
        "" => ("".into(), meta),
        other => {
            let mut m = meta;
            if let Some(o) = m.as_object_mut() {
                o.insert("patterns".into(), json!(patterns));
            }
            (other.to_string(), m)
        }
    }
}

/// Claude Code / Codex hook output for a decision (None for Ask).
pub fn claude_output(event: &str, d: Decision) -> Option<String> {
    let v = match (event, d) {
        (_, Decision::Ask) => return None,
        ("PreToolUse", _) => {
            let (pd, reason) = if d == Decision::Allow { ("allow", REASON_ALLOW) } else { ("deny", REASON_DENY) };
            json!({ "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": pd,
                "permissionDecisionReason": reason,
            }})
        }
        _ => {
            let decision = if d == Decision::Allow {
                json!({ "behavior": "allow" })
            } else {
                json!({ "behavior": "deny", "message": REASON_DENY })
            };
            json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
        }
    };
    Some(v.to_string())
}

/// What to print for an agent's permission hook. Never an approval unless
/// `d` is Allow, which only comes from a verified Orbi server.
pub fn ask_output(agent: &str, event: &str, d: Decision) -> Out {
    match family(agent) {
        Family::Claude | Family::Codex => Out { stdout: claude_output(event, d), code: 0 },
        // Gemini: a hook can block, never pre-approve (its policy engine still
        // runs), so only a deny is worth printing.
        Family::Gemini => match d {
            Decision::Deny => Out::json(json!({ "decision": "deny", "reason": REASON_DENY })),
            _ => Out::silent(),
        },
        // Hermes pre_tool_call: block, or nothing (its own approval gate runs).
        Family::Hermes => match d {
            Decision::Deny => Out::json(json!({ "action": "block", "message": REASON_DENY })),
            _ => Out::silent(),
        },
        // Cursor treats empty/invalid output as a block and crashes as fail-open,
        // so always print a valid answer. preToolUse can't "ask", so its
        // fallback is deny — never allow.
        Family::Cursor => {
            if event == "preToolUse" {
                match d {
                    Decision::Allow => Out::json(json!({ "permission": "allow" })),
                    Decision::Deny => Out::json(json!({ "permission": "deny", "user_message": REASON_DENY, "agent_message": REASON_DENY })),
                    Decision::Ask => Out::json(json!({
                        "permission": "deny",
                        "user_message": "Orbi got no answer, so this was not run",
                        "agent_message": "The user did not approve this in time. Ask them before retrying."
                    })),
                }
            } else {
                match d {
                    Decision::Allow => Out::json(json!({ "permission": "allow" })),
                    Decision::Deny => Out::json(json!({ "permission": "deny", "user_message": REASON_DENY, "agent_message": REASON_DENY })),
                    Decision::Ask => Out::json(json!({ "permission": "ask" })),
                }
            }
        }
        Family::OpenCode => word_output(d),
    }
}

/// Body for `POST /event` from an agent's status hook, or None when the event
/// isn't one Orbi shows.
pub fn event_from(agent: &str, hook: &Value, app_bundle: Option<&str>) -> Option<Value> {
    let ev = s(hook, "hook_event_name");
    let empty = json!({});
    let mk = |kind: &str, summary: String, detail: Option<String>, session: &str| {
        Some(event_body(agent, kind, &summary, detail.as_deref(), session, app_bundle))
    };
    let thinking = || "is thinking\u{2026}".to_string();
    // Marks an event that proves any permission prompt still pending for this
    // session was already answered somewhere else (the terminal): the tool
    // ran or failed, the turn ended, or the user typed a new prompt.
    let settles = |v: Option<Value>| {
        v.map(|mut b| {
            b["settles"] = json!(true);
            b
        })
    };
    match family(agent) {
        Family::Claude | Family::Codex => {
            let cwd = s(hook, "cwd");
            let session = s(hook, "session_id");
            // Lifecycle only: tracked as a session, never shown as activity.
            let phase = |p: &str, summary: String| {
                let mut b = event_body(agent, "session", &summary, None, session, app_bundle);
                b["phase"] = json!(p);
                Some(b)
            };
            let out = match ev {
                "SessionStart" => phase("start", String::new()),
                "SessionEnd" => phase("end", String::new()),
                "SubagentStart" => phase("subagent_start", short(s(hook, "agent_type"), 40)),
                "SubagentStop" => phase("subagent_stop", short(s(hook, "agent_type"), 40)),
                "PreToolUse" if s(hook, "tool_name") == "AskUserQuestion" => {
                    // Shown in Orbi; answered in the terminal (⌃⌥J jumps there).
                    let q = question_of(hook.get("tool_input").unwrap_or(&empty));
                    mk("working", "has a question for you".into(), None, session).map(|mut b| {
                        if let Some(q) = q {
                            b["question"] = q;
                        }
                        b
                    })
                }
                "UserPromptSubmit" => settles(mk("working", thinking(), None, session)),
                "PostToolUse" | "PostToolUseFailure" => {
                    let (tool, input) = normalize_tool(agent, s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), cwd);
                    let mut summary = tool_summary(&tool, &input, cwd);
                    if ev == "PostToolUseFailure" {
                        summary = format!("{summary} — failed");
                    }
                    settles(mk("working", summary, None, session))
                }
                "PreToolUse" => {
                    let (tool, input) = normalize_tool(agent, s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), cwd);
                    let detail = if tool == "Bash" { Some(s(&input, "command").to_string()) } else { None };
                    mk("working", tool_summary(&tool, &input, cwd), detail.filter(|d| !d.is_empty()), session)
                }
                "Notification" => {
                    let msg = first(hook, &["message", "title"]);
                    if msg.is_empty() {
                        return None;
                    }
                    mk("working", short(msg, 120), None, session)
                }
                "Stop" => settles(mk("done", "finished".into(), Some(s(hook, "last_assistant_message").to_string()), session)),
                "StopFailure" => settles(mk("error", "stopped with an error".into(), None, session)),
                _ => None,
            };
            // The project folder names the session on the face.
            out.map(|mut b| {
                if !cwd.is_empty() {
                    b["cwd"] = json!(cwd);
                }
                b
            })
        }
        Family::Gemini => {
            let cwd = s(hook, "cwd");
            let session = s(hook, "session_id");
            match ev {
                "BeforeAgent" => mk("working", thinking(), None, session),
                "BeforeTool" => {
                    let (tool, input) = normalize_tool(agent, s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), cwd);
                    let detail = if tool == "Bash" { Some(s(&input, "command").to_string()) } else { None };
                    mk("working", tool_summary(&tool, &input, cwd), detail.filter(|d| !d.is_empty()), session)
                }
                "AfterAgent" => mk("done", "finished".into(), Some(s(hook, "prompt_response").to_string()), session),
                "Notification" => {
                    let msg = s(hook, "message");
                    let summary = if msg.is_empty() { "needs your permission".to_string() } else { short(msg, 120) };
                    mk("working", summary, None, session)
                }
                _ => None,
            }
        }
        Family::Cursor => {
            let cwd = cursor_cwd(hook);
            let session = s(hook, "conversation_id");
            match ev {
                "beforeSubmitPrompt" => mk("working", thinking(), None, session),
                "afterShellExecution" => {
                    let cmd = s(hook, "command");
                    mk("working", format!("ran `{}`", short(cmd, 50)), Some(cmd.to_string()), session)
                }
                "afterFileEdit" => {
                    let summary = tool_summary("Edit", &json!({ "file_path": s(hook, "file_path") }), &cwd).replacen("editing", "edited", 1);
                    mk("working", summary, None, session)
                }
                "afterMCPExecution" => {
                    let t = format!("mcp__{}__{}", s(hook, "mcp_server_name"), s(hook, "tool_name"));
                    mk("working", tool_summary(&t, &empty, &cwd), None, session)
                }
                "stop" => match s(hook, "status") {
                    "error" => mk("error", "stopped with an error".into(), None, session),
                    "aborted" => mk("done", "stopped".into(), None, session),
                    _ => mk("done", "finished".into(), None, session),
                },
                _ => None,
            }
        }
        Family::Hermes => {
            let cwd = s(hook, "cwd");
            let session = s(hook, "session_id");
            let extra = hook.get("extra").unwrap_or(&empty);
            match ev {
                "pre_llm_call" => mk("working", thinking(), None, session),
                "pre_tool_call" => {
                    let (tool, input) = normalize_tool(agent, s(hook, "tool_name"), hook.get("tool_input").unwrap_or(&empty), cwd);
                    let detail = if tool == "Bash" { Some(s(&input, "command").to_string()) } else { None };
                    mk("working", tool_summary(&tool, &input, cwd), detail.filter(|d| !d.is_empty()), session)
                }
                "pre_approval_request" => {
                    let cmd = s(extra, "command");
                    let why = s(extra, "description");
                    let summary = if cmd.is_empty() {
                        "needs your approval".to_string()
                    } else {
                        format!("needs your approval to run `{}`", short(cmd, 40))
                    };
                    let detail = [why, cmd].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join("\n");
                    let session = if session.is_empty() { s(extra, "session_key") } else { session };
                    mk("working", summary, Some(detail), session)
                }
                "post_llm_call" => mk("done", "finished".into(), Some(s(extra, "assistant_response").to_string()), session),
                _ => None,
            }
        }
        Family::OpenCode => {
            let cwd = s(hook, "cwd");
            let props = hook.get("properties").unwrap_or(&empty);
            let session = first(hook, &["sessionID"]);
            let session = if session.is_empty() { s(props, "sessionID") } else { session };
            match ev {
                "tool.execute.before" => {
                    let (tool, input) = normalize_tool(agent, s(hook, "tool"), hook.get("args").unwrap_or(&empty), cwd);
                    let detail = if tool == "Bash" { Some(s(&input, "command").to_string()) } else { None };
                    mk("working", tool_summary(&tool, &input, cwd), detail.filter(|d| !d.is_empty()), session)
                }
                "session.status" => match props.get("status").map(|st| s(st, "type")) {
                    Some("busy") => mk("working", thinking(), None, session),
                    Some("retry") => mk("working", "retrying\u{2026}".into(), None, session),
                    _ => None,
                },
                "session.idle" => mk("done", "finished".into(), None, session),
                "session.error" => {
                    let err = props.get("error").unwrap_or(&empty);
                    let msg = err.get("data").map(|d| s(d, "message")).filter(|m| !m.is_empty()).unwrap_or_else(|| s(err, "name"));
                    mk("error", "stopped with an error".into(), Some(msg.to_string()), session)
                }
                _ => None,
            }
        }
    }
}

/// The first question of an `AskUserQuestion` call: its text and up to four
/// option labels. Everything is length-capped; Orbi only displays it.
fn question_of(input: &Value) -> Option<Value> {
    let q = input.get("questions").and_then(Value::as_array)?.first()?;
    let text = short(s(q, "question"), 200);
    if text.is_empty() {
        return None;
    }
    let options: Vec<String> = q
        .get("options")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(4).map(|o| short(s(o, "label"), 40)).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let more = input.get("questions").and_then(Value::as_array).map(|a| a.len().saturating_sub(1)).unwrap_or(0);
    Some(json!({ "text": text, "options": options, "more": more }))
}

/// Codex CLI `notify` program: Codex appends one JSON argument.
pub fn event_from_codex_notify(arg: &Value, app_bundle: Option<&str>) -> Option<Value> {
    match s(arg, "type") {
        "agent-turn-complete" => {
            let last = s(arg, "last-assistant-message");
            Some(event_body("codex", "done", "finished", Some(last), s(arg, "thread-id"), app_bundle))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(o: &Out) -> Value {
        serde_json::from_str(o.stdout.as_deref().unwrap()).unwrap()
    }

    // ---------------------------------------------------------------- Claude Code

    #[test]
    fn claude_settling_events() {
        let post = json!({"hook_event_name": "PostToolUse", "session_id": "s1", "cwd": "/w",
                          "tool_name": "Bash", "tool_input": {"command": "npm test"}});
        let b = event_from("claude-code", &post, None).unwrap();
        assert_eq!(b["settles"], true);
        assert_eq!(b["session"], "s1");
        let fail = json!({"hook_event_name": "PostToolUseFailure", "session_id": "s1", "cwd": "/w",
                          "tool_name": "Bash", "tool_input": {"command": "npm test"}});
        let b = event_from("claude-code", &fail, None).unwrap();
        assert!(b["summary"].as_str().unwrap().ends_with("failed"));
        for ev in ["Stop", "UserPromptSubmit"] {
            let b = event_from("claude-code", &json!({"hook_event_name": ev, "session_id": "s1"}), None).unwrap();
            assert_eq!(b["settles"], true, "{ev}");
        }
        // PreToolUse fires *before* the permission prompt, so it must not settle.
        let pre = json!({"hook_event_name": "PreToolUse", "session_id": "s1", "cwd": "/w",
                         "tool_name": "Bash", "tool_input": {"command": "npm test"}});
        assert!(event_from("claude-code", &pre, None).unwrap().get("settles").is_none());
    }

    #[test]
    fn claude_ask_request() {
        let hook = json!({
            "session_id": "abc123", "transcript_path": "/x/t.jsonl", "cwd": "/work/orbi",
            "permission_mode": "default", "hook_event_name": "PermissionRequest",
            "tool_name": "Bash", "tool_input": { "command": "npm test", "description": "Run tests" },
            "permission_suggestions": []
        });
        let (body, ev) = ask_request("claude-code", &hook, Some("com.apple.Terminal"));
        assert_eq!(ev, "PermissionRequest");
        assert_eq!(
            body,
            json!({"agent": "claude-code", "tool": "Bash", "input": {"command": "npm test", "description": "Run tests"},
                   "cwd": "/work/orbi", "session": "abc123", "app_bundle": "com.apple.Terminal"})
        );
        let (body, _) = ask_request("claude-code", &json!({}), None);
        assert_eq!(body, json!({"agent": "claude-code", "tool": "", "input": {}, "cwd": ""}));
    }

    #[test]
    fn claude_outputs() {
        let o = ask_output("claude-code", "PermissionRequest", Decision::Allow);
        assert_eq!(parse(&o), json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}));
        assert!(!o.stdout.as_ref().unwrap().contains('\n'));
        let o = ask_output("claude-code", "PermissionRequest", Decision::Deny);
        assert_eq!(
            parse(&o),
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "deny", "message": "Denied in Orbi"}}})
        );
        assert_eq!(ask_output("claude-code", "PermissionRequest", Decision::Ask), Out { stdout: None, code: 0 });
        let o = ask_output("claude-code", "PreToolUse", Decision::Allow);
        assert_eq!(
            parse(&o),
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "Approved in Orbi"}})
        );
        assert_eq!(parse(&ask_output("claude-code", "PreToolUse", Decision::Deny))["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(ask_output("claude-code", "PreToolUse", Decision::Ask).stdout, None);
        // unknown event → PermissionRequest shape; unknown agent → Claude family
        assert_eq!(parse(&ask_output("my-agent", "", Decision::Deny))["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
    }

    #[test]
    fn claude_events() {
        let e = event_from("claude-code", &json!({"hook_event_name": "UserPromptSubmit", "session_id": "s", "prompt": "hi"}), Some("com.googlecode.iterm2")).unwrap();
        assert_eq!(e, json!({"agent": "claude-code", "kind": "working", "summary": "is thinking\u{2026}", "session": "s", "app_bundle": "com.googlecode.iterm2", "settles": true}));
        let e = event_from("claude-code", &json!({"hook_event_name": "PreToolUse", "cwd": "/w", "tool_name": "Bash", "tool_input": {"command": "npm test"}}), None).unwrap();
        assert_eq!((e["summary"].as_str(), e["detail"].as_str()), (Some("running `npm test`"), Some("npm test")));
        let e = event_from("claude-code", &json!({"hook_event_name": "PreToolUse", "cwd": "/w", "tool_name": "Edit", "tool_input": {"file_path": "/w/src/app.ts"}}), None).unwrap();
        assert_eq!(e["summary"], "editing `src/app.ts`");
        assert!(e.get("detail").is_none());
        let e = event_from("claude-code", &json!({"hook_event_name": "Notification", "message": "Claude needs your permission to use Bash"}), None).unwrap();
        assert_eq!(e["summary"], "Claude needs your permission to use Bash");
        let e = event_from("claude-code", &json!({"hook_event_name": "Stop", "last_assistant_message": "Done."}), None).unwrap();
        assert_eq!((e["kind"].as_str(), e["detail"].as_str()), (Some("done"), Some("Done.")));
        assert_eq!(event_from("claude-code", &json!({"hook_event_name": "StopFailure"}), None).unwrap()["kind"], "error");
        assert_eq!(event_from("claude-code", &json!({"hook_event_name": "SessionStart"}), None).unwrap()["phase"], "start");
        assert!(event_from("claude-code", &json!({"hook_event_name": "Notification"}), None).is_none());
        assert!(event_from("claude-code", &json!(null), None).is_none());
    }

    // ---------------------------------------------------------------- Codex

    #[test]
    fn codex() {
        let hook = json!({"session_id": "t1", "turn_id": "u1", "cwd": "/w", "hook_event_name": "PermissionRequest", "model": "gpt",
                          "permission_mode": "default", "tool_name": "Bash", "tool_input": {"command": "cargo publish", "description": "publish"}});
        let (body, ev) = ask_request("codex", &hook, None);
        assert_eq!(body, json!({"agent": "codex", "tool": "Bash", "input": {"command": "cargo publish", "description": "publish"}, "cwd": "/w", "session": "t1"}));
        assert_eq!(parse(&ask_output("codex", &ev, Decision::Allow)), json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}));
        assert_eq!(parse(&ask_output("codex", &ev, Decision::Deny))["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(ask_output("codex", &ev, Decision::Ask), Out { stdout: None, code: 0 });

        let patch = "*** Begin Patch\n*** Update File: src/main.rs\n-a\n+b\n+c\n*** End Patch";
        let (body, _) = ask_request("codex", &json!({"hook_event_name": "PermissionRequest", "cwd": "/w", "tool_name": "apply_patch", "tool_input": {"command": patch}}), None);
        assert_eq!(body["tool"], "Edit");
        assert_eq!(body["input"]["file_path"], "/w/src/main.rs");

        let e = event_from("codex", &json!({"hook_event_name": "PreToolUse", "cwd": "/w", "tool_name": "apply_patch", "tool_input": {"command": patch}}), None).unwrap();
        assert_eq!((e["agent"].as_str(), e["summary"].as_str()), (Some("codex"), Some("editing `src/main.rs`")));
        let e = event_from("codex", &json!({"hook_event_name": "Stop", "session_id": "t1", "last_assistant_message": "All set."}), None).unwrap();
        assert_eq!(e, json!({"agent": "codex", "kind": "done", "summary": "finished", "detail": "All set.", "session": "t1", "settles": true}));
        assert_eq!(event_from("codex", &json!({"hook_event_name": "UserPromptSubmit"}), None).unwrap()["summary"], "is thinking\u{2026}");
    }

    #[test]
    fn codex_notify() {
        let e = event_from_codex_notify(
            &json!({"type": "agent-turn-complete", "thread-id": "t1", "turn-id": "u1", "cwd": "/w", "input-messages": ["fix it"], "last-assistant-message": "Fixed."}),
            None,
        )
        .unwrap();
        assert_eq!(e, json!({"agent": "codex", "kind": "done", "summary": "finished", "detail": "Fixed.", "session": "t1"}));
        assert!(event_from_codex_notify(&json!({"type": "something-else"}), None).is_none());
    }

    // ---------------------------------------------------------------- Gemini

    #[test]
    fn gemini() {
        let hook = json!({"session_id": "g1", "cwd": "/w", "hook_event_name": "BeforeTool", "timestamp": "t",
                          "tool_name": "run_shell_command", "tool_input": {"command": "rm -rf build"}});
        let (body, ev) = ask_request("gemini", &hook, None);
        assert_eq!((body["tool"].as_str(), body["input"]["command"].as_str()), (Some("Bash"), Some("rm -rf build")));
        assert_eq!(parse(&ask_output("gemini", &ev, Decision::Deny)), json!({"decision": "deny", "reason": "Denied in Orbi"}));
        assert_eq!(ask_output("gemini", &ev, Decision::Allow).stdout, None);
        assert_eq!(ask_output("gemini", &ev, Decision::Ask).stdout, None);

        let e = event_from("gemini", &hook, None).unwrap();
        assert_eq!((e["kind"].as_str(), e["summary"].as_str()), (Some("working"), Some("running `rm -rf build`")));
        assert_eq!(event_from("gemini", &json!({"hook_event_name": "BeforeAgent", "prompt": "hi"}), None).unwrap()["summary"], "is thinking\u{2026}");
        let e = event_from("gemini", &json!({"hook_event_name": "AfterAgent", "prompt_response": "ok"}), None).unwrap();
        assert_eq!((e["kind"].as_str(), e["detail"].as_str()), (Some("done"), Some("ok")));
        let e = event_from("gemini", &json!({"hook_event_name": "Notification", "notification_type": "ToolPermission", "message": "Allow shell?"}), None).unwrap();
        assert_eq!(e["summary"], "Allow shell?");
        assert!(event_from("gemini", &json!({"hook_event_name": "SessionStart"}), None).is_none());
    }

    // ---------------------------------------------------------------- Cursor

    #[test]
    fn cursor() {
        let hook = json!({"conversation_id": "c1", "hook_event_name": "beforeShellExecution", "command": "git push --force",
                          "cwd": "/w", "sandbox": false, "workspace_roots": ["/w"]});
        let (body, ev) = ask_request("cursor", &hook, None);
        assert_eq!(body, json!({"agent": "cursor", "tool": "Bash", "input": {"command": "git push --force"}, "cwd": "/w", "session": "c1"}));
        assert_eq!(parse(&ask_output("cursor", &ev, Decision::Allow)), json!({"permission": "allow"}));
        assert_eq!(parse(&ask_output("cursor", &ev, Decision::Deny))["permission"], "deny");
        assert_eq!(parse(&ask_output("cursor", &ev, Decision::Ask)), json!({"permission": "ask"}));
        // preToolUse can't ask: its fallback is deny, never allow
        assert_eq!(parse(&ask_output("cursor", "preToolUse", Decision::Ask))["permission"], "deny");
        assert_eq!(parse(&ask_output("cursor", "preToolUse", Decision::Allow))["permission"], "allow");

        let mcp = json!({"hook_event_name": "beforeMCPExecution", "tool_name": "create_issue", "tool_input": "{\"title\":\"x\"}",
                         "mcp_server_name": "linear", "workspace_roots": ["/w"]});
        let (body, _) = ask_request("cursor", &mcp, None);
        assert_eq!((body["tool"].as_str(), body["input"]["title"].as_str(), body["cwd"].as_str()), (Some("mcp__linear__create_issue"), Some("x"), Some("/w")));

        let e = event_from("cursor", &json!({"hook_event_name": "afterShellExecution", "command": "npm test", "output": "ok", "workspace_roots": ["/w"]}), None).unwrap();
        assert_eq!(e["summary"], "ran `npm test`");
        let e = event_from("cursor", &json!({"hook_event_name": "afterFileEdit", "file_path": "/w/src/a.ts", "workspace_roots": ["/w"]}), None).unwrap();
        assert_eq!(e["summary"], "edited `src/a.ts`");
        assert_eq!(event_from("cursor", &json!({"hook_event_name": "stop", "status": "completed"}), None).unwrap()["kind"], "done");
        assert_eq!(event_from("cursor", &json!({"hook_event_name": "stop", "status": "error"}), None).unwrap()["kind"], "error");
        assert_eq!(event_from("cursor", &json!({"hook_event_name": "stop", "status": "aborted"}), None).unwrap()["summary"], "stopped");
    }

    // ---------------------------------------------------------------- Hermes

    #[test]
    fn hermes() {
        let hook = json!({"hook_event_name": "pre_tool_call", "tool_name": "terminal", "tool_input": {"command": "rm -rf /"},
                          "session_id": "sess_abc123", "cwd": "/home/user/project", "profile": "default", "extra": {"task_id": "t"}});
        let (body, ev) = ask_request("hermes", &hook, None);
        assert_eq!((body["tool"].as_str(), body["session"].as_str()), (Some("Bash"), Some("sess_abc123")));
        assert_eq!(parse(&ask_output("hermes", &ev, Decision::Deny)), json!({"action": "block", "message": "Denied in Orbi"}));
        assert_eq!(ask_output("hermes", &ev, Decision::Allow).stdout, None);
        assert_eq!(ask_output("hermes", &ev, Decision::Ask).stdout, None);

        assert_eq!(event_from("hermes", &hook, None).unwrap()["summary"], "running `rm -rf /`");
        assert_eq!(event_from("hermes", &json!({"hook_event_name": "pre_llm_call", "tool_name": null}), None).unwrap()["summary"], "is thinking\u{2026}");
        let e = event_from(
            "hermes",
            &json!({"hook_event_name": "pre_approval_request", "tool_name": null, "extra": {"command": "sudo reboot", "description": "sudo", "session_key": "k"}}),
            None,
        )
        .unwrap();
        assert_eq!((e["summary"].as_str(), e["detail"].as_str(), e["session"].as_str()), (Some("needs your approval to run `sudo reboot`"), Some("sudo\nsudo reboot"), Some("k")));
        let e = event_from("hermes", &json!({"hook_event_name": "post_llm_call", "session_id": "s", "extra": {"assistant_response": "done!"}}), None).unwrap();
        assert_eq!((e["kind"].as_str(), e["detail"].as_str()), (Some("done"), Some("done!")));
        assert!(event_from("hermes", &json!({"hook_event_name": "on_session_start"}), None).is_none());
    }

    // ---------------------------------------------------------------- OpenCode

    #[test]
    fn opencode() {
        let hook = json!({"hook_event_name": "permission.asked", "cwd": "/w", "request": {
            "id": "per_1", "sessionID": "ses_1", "permission": "bash", "patterns": ["git push *"],
            "metadata": {"command": "git push origin main"}, "always": ["git push *"]}});
        let (body, _) = ask_request("opencode", &hook, None);
        assert_eq!(body, json!({"agent": "opencode", "tool": "Bash", "input": {"command": "git push origin main"}, "cwd": "/w", "session": "ses_1"}));
        assert_eq!(ask_output("opencode", "permission.asked", Decision::Allow), Out { stdout: Some("allow".into()), code: 0 });
        assert_eq!(ask_output("opencode", "permission.asked", Decision::Deny), Out { stdout: Some("deny".into()), code: 1 });
        assert_eq!(ask_output("opencode", "permission.asked", Decision::Ask), Out { stdout: Some("ask".into()), code: 2 });

        let edit = json!({"cwd": "/w", "request": {"permission": "edit", "patterns": ["a.ts"],
            "metadata": {"filepath": "/w/a.ts", "diff": "--- a\n+++ b\n@@\n-x\n+y\n+z\n"}}});
        let (body, _) = ask_request("opencode", &edit, None);
        assert_eq!((body["tool"].as_str(), body["input"]["file_path"].as_str()), (Some("Edit"), Some("/w/a.ts")));
        assert_eq!((body["input"]["old_string"].as_str(), body["input"]["new_string"].as_str()), (Some("x\n"), Some("y\nz\n")));
        let (body, _) = ask_request("opencode", &json!({"request": {"permission": "webfetch", "patterns": ["https://a.dev"], "metadata": {"url": "https://a.dev"}}}), None);
        assert_eq!((body["tool"].as_str(), body["input"]["url"].as_str()), (Some("WebFetch"), Some("https://a.dev")));
        let (body, _) = ask_request("opencode", &json!({"request": {"permission": "external_directory", "patterns": ["/etc/*"], "metadata": {"command": "cat /etc/x"}}}), None);
        assert_eq!((body["tool"].as_str(), body["input"]["patterns"][0].as_str()), (Some("external_directory"), Some("/etc/*")));

        let e = event_from("opencode", &json!({"hook_event_name": "tool.execute.before", "cwd": "/w", "tool": "edit", "sessionID": "s", "args": {"filePath": "/w/x.rs"}}), None).unwrap();
        assert_eq!((e["summary"].as_str(), e["session"].as_str()), (Some("editing `x.rs`"), Some("s")));
        let e = event_from("opencode", &json!({"hook_event_name": "session.status", "properties": {"sessionID": "s", "status": {"type": "busy"}}}), None).unwrap();
        assert_eq!(e["summary"], "is thinking\u{2026}");
        assert!(event_from("opencode", &json!({"hook_event_name": "session.status", "properties": {"status": {"type": "idle"}}}), None).is_none());
        assert_eq!(event_from("opencode", &json!({"hook_event_name": "session.idle", "properties": {"sessionID": "s"}}), None).unwrap()["kind"], "done");
        let e = event_from("opencode", &json!({"hook_event_name": "session.error", "properties": {"error": {"name": "ApiError", "data": {"message": "rate limited"}}}}), None).unwrap();
        assert_eq!((e["kind"].as_str(), e["detail"].as_str()), (Some("error"), Some("rate limited")));
    }

    #[test]
    fn word_outputs() {
        assert_eq!(word_output(Decision::Allow), Out { stdout: Some("allow".into()), code: 0 });
        assert_eq!(word_output(Decision::Deny), Out { stdout: Some("deny".into()), code: 1 });
        assert_eq!(word_output(Decision::Ask), Out { stdout: Some("ask".into()), code: 2 });
    }

    #[test]
    fn claude_sessions_subagents_and_questions() {
        let base = |ev: &str| json!({"hook_event_name": ev, "session_id": "s1", "cwd": "/w/orbi"});
        let start = event_from("claude-code", &base("SessionStart"), None).unwrap();
        assert_eq!((start["kind"].as_str(), start["phase"].as_str()), (Some("session"), Some("start")));
        assert_eq!(start["cwd"], "/w/orbi");
        assert_eq!(event_from("claude-code", &base("SessionEnd"), None).unwrap()["phase"], "end");

        let mut sub = base("SubagentStart");
        sub["agent_type"] = json!("Explore");
        let sub = event_from("claude-code", &sub, None).unwrap();
        assert_eq!((sub["phase"].as_str(), sub["summary"].as_str()), (Some("subagent_start"), Some("Explore")));

        let mut ask = base("PreToolUse");
        ask["tool_name"] = json!("AskUserQuestion");
        ask["tool_input"] = json!({"questions": [
            {"question": "Which database?", "header": "DB", "options": [{"label": "Postgres"}, {"label": "SQLite"}], "multiSelect": false},
            {"question": "Second?", "options": []}
        ]});
        let q = event_from("claude-code", &ask, None).unwrap();
        assert_eq!(q["kind"], "working");
        assert_eq!(q["question"]["text"], "Which database?");
        assert_eq!(q["question"]["options"], json!(["Postgres", "SQLite"]));
        assert_eq!(q["question"]["more"], 1);

        // A malformed question still reports activity, just without the card.
        let mut bad = base("PreToolUse");
        bad["tool_name"] = json!("AskUserQuestion");
        bad["tool_input"] = json!({"questions": "nope"});
        let b = event_from("claude-code", &bad, None).unwrap();
        assert!(b.get("question").is_none());
        // Ordinary events carry the working folder too.
        assert_eq!(event_from("claude-code", &base("UserPromptSubmit"), None).unwrap()["cwd"], "/w/orbi");
    }
}
