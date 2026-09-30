//! Connect / disconnect Orbi to each agent's own hook config.
//!
//! Every edit is idempotent, keeps everything else in the file (key order
//! included), writes a timestamped backup next to the file before changing it,
//! writes atomically (temp file + rename, through symlinks), and refuses to
//! touch a file it can't parse. Orbi's entries are recognised by "orbi-hook"
//! appearing in their command/args, so disconnect removes only those.
//!
//! Every hook command goes through `/bin/sh` with the hook path passed as `$0`
//! (never interpolated into script text), so if Orbi.app is deleted the hook
//! silently does nothing: `[ -x "$0" ] && exec "$0" "$@"; exit 0`.
//!
//! Where each shape comes from: adapters/README.md.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub detected: bool,
    pub connected: bool,
    pub approvals: bool,
    pub status: bool,
    pub config_path: String,
    pub note: String,
}

pub fn list(hook: &Path) -> Vec<Integration> {
    list_in(&Env::real(), hook)
}

pub fn connect(id: &str, hook: &Path) -> Result<Integration, String> {
    connect_in(&Env::real(), id, hook)
}

pub fn disconnect(id: &str, hook: &Path) -> Result<Integration, String> {
    disconnect_in(&Env::real(), id, hook)
}

/// What Connect would change, without changing anything.
pub fn preview(id: &str, hook: &Path) -> Result<Preview, String> {
    preview_in(&Env::real(), id, hook)
}

/// One line of a Connect preview: `op` is "+", "-", " " (context) or "…"
/// (unchanged lines skipped).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub op: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub config_path: String,
    /// Nothing would change: already connected and up to date.
    pub unchanged: bool,
    /// The file doesn't exist yet and would be created.
    pub creates: bool,
    pub lines: Vec<DiffLine>,
}

/// Re-point connected integrations at `hook` (app moved or updated). Also
/// brings older Orbi entries up to the current set. Errors are ignored.
pub fn repair(hook: &Path) {
    repair_in(&Env::real(), hook)
}

// ------------------------------------------------------------------ registry

/// Marker every Orbi hook entry contains (the binary's file name).
const MARKER: &str = "orbi-hook";

/// `$0` is the hook path; missing/non-executable → exit 0 silently.
const WRAP: &str = r#"[ -x "$0" ] && exec "$0" "$@"; exit 0"#;

/// Seconds an agent waits for the ask hook. Orbi answers "ask" after its own
/// timeout (45 s default) and orbi-hook gives up at 110 s, so this is a net.
const ASK_TIMEOUT_SECS: u64 = 120;
const EVENT_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Claude,
    Codex,
    Gemini,
    Cursor,
    OpenCode,
    Hermes,
}

struct Spec {
    id: &'static str,
    name: &'static str,
    kind: Kind,
    approvals: bool,
    note: &'static str,
    /// Config file, relative to $HOME.
    config: &'static str,
    /// Any of these (relative to $HOME) existing means "installed".
    dirs: &'static [&'static str],
    binaries: &'static [&'static str],
    apps: &'static [&'static str],
}

const SPECS: &[Spec] = &[
    Spec {
        id: "claude-code",
        name: "Claude Code",
        kind: Kind::Claude,
        approvals: true,
        note: "Approve or deny from Orbi; the terminal prompt still works too",
        config: ".claude/settings.json",
        dirs: &[".claude"],
        binaries: &["claude"],
        apps: &[],
    },
    Spec {
        id: "codex",
        name: "Codex",
        kind: Kind::Codex,
        approvals: true,
        note: "Run /hooks in Codex once to trust Orbi's hooks",
        config: ".codex/hooks.json",
        dirs: &[".codex"],
        binaries: &["codex"],
        apps: &[],
    },
    Spec {
        id: "gemini",
        name: "Gemini CLI",
        kind: Kind::Gemini,
        approvals: false,
        note: "Status only \u{2014} Gemini hooks can block a tool but not approve one",
        config: ".gemini/settings.json",
        dirs: &[".gemini"],
        binaries: &["gemini"],
        apps: &[],
    },
    Spec {
        id: "opencode",
        name: "OpenCode",
        kind: Kind::OpenCode,
        approvals: true,
        note: "Answer in Orbi or in OpenCode \u{2014} whichever comes first",
        config: ".config/opencode/plugins/orbi.js",
        dirs: &[".config/opencode", ".opencode"],
        binaries: &["opencode"],
        apps: &[],
    },
    Spec {
        id: "cursor",
        name: "Cursor",
        kind: Kind::Cursor,
        approvals: false,
        note: "Status only \u{2014} Cursor's permission hooks run for every command",
        config: ".cursor/hooks.json",
        dirs: &[".cursor"],
        binaries: &["cursor-agent", "cursor"],
        apps: &["/Applications/Cursor.app"],
    },
    Spec {
        id: "hermes",
        name: "Hermes",
        kind: Kind::Hermes,
        approvals: false,
        note: "Status only \u{2014} Hermes asks once before running Orbi's hooks",
        config: ".hermes/config.yaml",
        dirs: &[".hermes"],
        binaries: &["hermes"],
        apps: &[],
    },
];

fn spec(id: &str) -> Result<&'static Spec, String> {
    SPECS.iter().find(|s| s.id == id).ok_or_else(|| format!("unknown integration \"{id}\""))
}

// ----------------------------------------------------------------------- env

struct Env {
    home: PathBuf,
    /// Directories searched for agent binaries.
    bin_dirs: Vec<PathBuf>,
    /// Whether `Spec::apps` (absolute, system-wide) are checked.
    check_apps: bool,
}

impl Env {
    fn real() -> Env {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let mut bin_dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin"].iter().map(PathBuf::from).collect();
        bin_dirs.extend(Env::home_bins(&home));
        if let Some(p) = std::env::var_os("PATH") {
            bin_dirs.extend(std::env::split_paths(&p));
        }
        Env { home, bin_dirs, check_apps: true }
    }

    fn home_bins(home: &Path) -> Vec<PathBuf> {
        [".local/bin", ".npm-global/bin", ".bun/bin", ".opencode/bin", ".cargo/bin", ".claude/local"]
            .iter()
            .map(|d| home.join(d))
            .collect()
    }

    #[cfg(test)]
    fn with_home(home: &Path) -> Env {
        Env { home: home.to_path_buf(), bin_dirs: Env::home_bins(home), check_apps: false }
    }
}

fn detected(env: &Env, s: &Spec) -> bool {
    s.dirs.iter().any(|d| env.home.join(d).is_dir())
        || s.binaries.iter().any(|b| env.bin_dirs.iter().any(|d| is_executable(&d.join(b))))
        || (env.check_apps && s.apps.iter().any(|a| Path::new(a).exists()))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn tilde(env: &Env, p: &Path) -> String {
    match p.strip_prefix(&env.home) {
        Ok(rest) if !env.home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

// ----------------------------------------------------------------- top level

fn info(env: &Env, s: &Spec, connected: bool) -> Integration {
    Integration {
        id: s.id.into(),
        name: s.name.into(),
        detected: detected(env, s),
        connected,
        approvals: s.approvals,
        status: true,
        config_path: tilde(env, &env.home.join(s.config)),
        note: s.note.into(),
    }
}

fn list_in(env: &Env, _hook: &Path) -> Vec<Integration> {
    SPECS.iter().map(|s| info(env, s, is_connected(env, s))).collect()
}

fn is_connected(env: &Env, s: &Spec) -> bool {
    let path = env.home.join(s.config);
    let Ok(text) = fs::read_to_string(&path) else { return false };
    match s.kind {
        Kind::OpenCode => text.contains(PLUGIN_MARKER),
        Kind::Hermes => !yaml_remove(&text).1.is_empty(),
        Kind::Cursor => parse_json(&text).map(|v| !cursor_ours(&v).is_empty()).unwrap_or(false),
        _ => parse_json(&text).map(|v| !nested_ours(&v).is_empty()).unwrap_or(false),
    }
}

fn connect_in(env: &Env, id: &str, hook: &Path) -> Result<Integration, String> {
    let s = spec(id)?;
    if !hook.is_absolute() {
        return Err("the hook path must be absolute".into());
    }
    let path = env.home.join(s.config);
    let hook = hook.to_string_lossy();
    match s.kind {
        Kind::OpenCode => {
            let js = opencode_plugin(&hook);
            match fs::read_to_string(&path) {
                Ok(old) if old == js => {}
                Ok(old) if !old.contains(PLUGIN_MARKER) => {
                    return Err(format!("{} exists and wasn't written by Orbi \u{2014} not touching it", tilde(env, &path)))
                }
                _ => write_atomic(&path, js.as_bytes())?,
            }
        }
        Kind::Hermes => edit_text(env, &path, |old| yaml_connect(old, &hermes_entries(&hook)))?,
        kind => edit_json(env, &path, kind, |root| json_connect(root, kind, &hook))?,
    }
    Ok(info(env, s, is_connected(env, s)))
}

fn disconnect_in(env: &Env, id: &str, _hook: &Path) -> Result<Integration, String> {
    let s = spec(id)?;
    let path = env.home.join(s.config);
    if path.exists() {
        match s.kind {
            Kind::OpenCode => {
                let old = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                if !old.contains(PLUGIN_MARKER) {
                    return Err(format!("{} wasn't written by Orbi \u{2014} not touching it", tilde(env, &path)));
                }
                fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            Kind::Hermes => edit_text(env, &path, |old| Ok(yaml_remove(old).0))?,
            kind => edit_json(env, &path, kind, |root| {
                if kind == Kind::Cursor {
                    cursor_remove(root);
                } else {
                    nested_remove(root);
                }
                Ok(())
            })?,
        }
    }
    Ok(info(env, s, is_connected(env, s)))
}

fn repair_in(env: &Env, hook: &Path) {
    for s in SPECS {
        if is_connected(env, s) {
            let _ = connect_in(env, s.id, hook);
        }
    }
}

// --------------------------------------------------------------- hook command

/// POSIX single-quote a word.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// One shell command string, for configs that only take `command: "<string>"`.
fn shell_command(hook: &str, args: &[&str]) -> String {
    let mut c = format!("/bin/sh -c {} {}", sq(WRAP), sq(hook));
    for a in args {
        c.push(' ');
        c.push_str(a);
    }
    c
}

/// Exec-form `(command, args)` for configs that support an argv.
fn exec_form(hook: &str, args: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["-c".to_string(), WRAP.to_string(), hook.to_string()];
    a.extend(args.iter().map(|s| s.to_string()));
    ("/bin/sh".into(), a)
}

fn mentions_marker(v: &Value) -> bool {
    let cmd = v.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(MARKER));
    let args = v
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|x| x.as_str().is_some_and(|x| x.contains(MARKER))));
    cmd || args
}

// ------------------------------------------------------ JSON configs (nested)

/// `(event, matcher, handler)` Orbi wants in a nested `hooks` config.
fn wanted(kind: Kind, hook: &str) -> Vec<(&'static str, Option<&'static str>, Value)> {
    let claude_handler = |args: &[&str], ask: bool| {
        let (command, args) = exec_form(hook, args);
        if ask {
            json!({ "type": "command", "command": command, "args": args, "timeout": ASK_TIMEOUT_SECS })
        } else {
            json!({ "type": "command", "command": command, "args": args, "timeout": EVENT_TIMEOUT_SECS, "async": true })
        }
    };
    match kind {
        Kind::Claude => {
            let ev = || claude_handler(&["event", "--agent", "claude-code"], false);
            vec![
                ("PermissionRequest", Some("*"), claude_handler(&["ask", "--agent", "claude-code"], true)),
                ("UserPromptSubmit", None, ev()),
                ("PreToolUse", Some("*"), ev()),
                // Tell Orbi a prompt was answered in the terminal.
                ("PostToolUse", Some("*"), ev()),
                ("PostToolUseFailure", Some("*"), ev()),
                ("Notification", None, ev()),
                ("Stop", None, ev()),
                ("StopFailure", None, ev()),
                // Sessions and sub-agents, so the face can list what's running.
                ("SessionStart", None, ev()),
                ("SessionEnd", None, ev()),
                ("SubagentStart", None, ev()),
                ("SubagentStop", None, ev()),
            ]
        }
        Kind::Codex => {
            let ev = || {
                json!({ "type": "command", "command": shell_command(hook, &["event", "--agent", "codex"]),
                        "timeout": EVENT_TIMEOUT_SECS, "async": true })
            };
            vec![
                (
                    "PermissionRequest",
                    Some("*"),
                    json!({ "type": "command", "command": shell_command(hook, &["ask", "--agent", "codex"]), "timeout": ASK_TIMEOUT_SECS }),
                ),
                ("UserPromptSubmit", None, ev()),
                ("PreToolUse", Some("*"), ev()),
                ("Stop", None, ev()),
            ]
        }
        Kind::Gemini => {
            // Gemini timeouts are milliseconds; hooks here are synchronous but
            // orbi-hook event returns in well under a second.
            let ev = || {
                json!({ "name": "orbi", "type": "command", "command": shell_command(hook, &["event", "--agent", "gemini"]),
                        "timeout": EVENT_TIMEOUT_SECS * 1000, "description": "Shows Gemini's status on Orbi" })
            };
            vec![("BeforeAgent", None, ev()), ("BeforeTool", Some("*"), ev()), ("AfterAgent", None, ev()), ("Notification", None, ev())]
        }
        _ => vec![],
    }
}

fn nested_ours(root: &Value) -> Vec<(String, Option<String>, Value)> {
    let mut out = Vec::new();
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else { return out };
    for (event, groups) in hooks {
        for g in groups.as_array().into_iter().flatten() {
            for h in g.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                if mentions_marker(h) {
                    let m = g.get("matcher").and_then(Value::as_str).map(str::to_string);
                    out.push((event.clone(), m, h.clone()));
                }
            }
        }
    }
    out
}

/// Remove every Orbi handler (and groups / events left empty by that).
fn nested_remove(root: &mut Value) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return false };
    let mut changed = false;
    let mut empty_events = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        let mut touched = false;
        for g in groups.iter_mut() {
            if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                let n = hs.len();
                hs.retain(|h| !mentions_marker(h));
                touched |= hs.len() != n;
            }
        }
        if touched {
            changed = true;
            groups.retain(|g| !matches!(g.get("hooks").and_then(Value::as_array), Some(h) if h.is_empty()));
            if groups.is_empty() {
                empty_events.push(event.clone());
            }
        }
    }
    for e in empty_events {
        hooks.shift_remove(&e);
    }
    if changed && hooks.is_empty() {
        if let Some(o) = root.as_object_mut() {
            o.shift_remove("hooks");
        }
    }
    changed
}

fn json_connect(root: &mut Value, kind: Kind, hook: &str) -> Result<(), String> {
    if kind == Kind::Cursor {
        return cursor_connect(root, hook);
    }
    let want = wanted(kind, hook);
    let have = nested_ours(root);
    let same = have.len() == want.len()
        && have.iter().zip(&want).all(|((e, m, h), (we, wm, wh))| e == we && m.as_deref() == *wm && h == wh);
    if same {
        return Ok(());
    }
    nested_remove(root);
    let obj = root.as_object_mut().ok_or("the config file is not a JSON object")?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("\"hooks\" in the config is not an object")?;
    for (event, matcher, handler) in want {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let groups = groups.as_array_mut().ok_or_else(|| format!("hooks.{event} is not a list"))?;
        let mut g = Map::new();
        if let Some(m) = matcher {
            g.insert("matcher".into(), json!(m));
        }
        g.insert("hooks".into(), json!([handler]));
        groups.push(Value::Object(g));
    }
    Ok(())
}

// ------------------------------------------------------ JSON configs (Cursor)

fn cursor_wanted(hook: &str) -> Vec<(&'static str, Value)> {
    ["afterShellExecution", "afterFileEdit", "afterMCPExecution", "stop"]
        .iter()
        .map(|e| (*e, json!({ "command": shell_command(hook, &["event", "--agent", "cursor"]), "timeout": EVENT_TIMEOUT_SECS })))
        .collect()
}

fn cursor_ours(root: &Value) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    if let Some(hooks) = root.get("hooks").and_then(Value::as_object) {
        for (event, list) in hooks {
            for h in list.as_array().into_iter().flatten() {
                if mentions_marker(h) {
                    out.push((event.clone(), h.clone()));
                }
            }
        }
    }
    out
}

fn cursor_remove(root: &mut Value) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return false };
    let mut changed = false;
    let mut empty = Vec::new();
    for (event, list) in hooks.iter_mut() {
        if let Some(l) = list.as_array_mut() {
            let n = l.len();
            l.retain(|h| !mentions_marker(h));
            if l.len() != n {
                changed = true;
                if l.is_empty() {
                    empty.push(event.clone());
                }
            }
        }
    }
    for e in empty {
        hooks.shift_remove(&e);
    }
    changed
}

fn cursor_connect(root: &mut Value, hook: &str) -> Result<(), String> {
    let want = cursor_wanted(hook);
    let have = cursor_ours(root);
    if have.len() == want.len() && have.iter().zip(&want).all(|((e, h), (we, wh))| e == we && h == wh) {
        return Ok(());
    }
    cursor_remove(root);
    let obj = root.as_object_mut().ok_or("the config file is not a JSON object")?;
    if !obj.contains_key("version") {
        // `version` first, like Cursor's own examples.
        let mut fresh = Map::new();
        fresh.insert("version".into(), json!(1));
        for (k, v) in std::mem::take(obj) {
            fresh.insert(k, v);
        }
        *obj = fresh;
    }
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("\"hooks\" in the config is not an object")?;
    for (event, handler) in want {
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        list.as_array_mut().ok_or_else(|| format!("hooks.{event} is not a list"))?.push(handler);
    }
    Ok(())
}

// ------------------------------------------------------------------ JSON I/O

fn parse_json(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// Indentation the file already uses (first indented line), default 2 spaces.
fn detect_indent(text: &str) -> String {
    text.lines()
        .map(|l| &l[..l.len() - l.trim_start().len()])
        .find(|ws| !ws.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "  ".into())
}

fn to_json_text(v: &Value, indent: &str) -> Result<String, String> {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    v.serialize(&mut ser).map_err(|e| e.to_string())?;
    buf.push(b'\n');
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn edit_json(env: &Env, path: &Path, kind: Kind, f: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<(), String> {
    edit_text(env, path, |old| json_transform(env, path, kind, old, f))
}

/// The new text of a JSON config after `f` — shared by the real edit and the
/// Connect preview, so the preview is exactly what gets written.
fn json_transform(env: &Env, path: &Path, kind: Kind, old: &str, f: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<String, String> {
    let mut v = parse_json(old)
        .map_err(|e| format!("{} isn't valid JSON ({e}) \u{2014} Orbi won't touch it", tilde(env, path)))?;
    if !v.is_object() {
        return Err(format!("{} isn't a JSON object \u{2014} Orbi won't touch it", tilde(env, path)));
    }
    let before = v.clone();
    f(&mut v)?;
    if v == before && !old.trim().is_empty() {
        return Ok(old.to_string());
    }
    if kind == Kind::Cursor && old.trim().is_empty() && v == json!({}) {
        return Ok(old.to_string());
    }
    to_json_text(&v, &detect_indent(old))
}

// ------------------------------------------------------------ Connect preview

fn preview_in(env: &Env, id: &str, hook: &Path) -> Result<Preview, String> {
    let s = spec(id)?;
    if !hook.is_absolute() {
        return Err("the hook path must be absolute".into());
    }
    let path = env.home.join(s.config);
    let target = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let (old, existed) = match fs::read(&target) {
        Ok(b) => (String::from_utf8(b).map_err(|_| format!("{} isn't UTF-8 text \u{2014} Orbi won't touch it", tilde(env, &path)))?, true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => return Err(format!("can't read {}: {e}", tilde(env, &path))),
    };
    let hook = hook.to_string_lossy();
    // The same transforms `connect_in` applies.
    let new = match s.kind {
        Kind::OpenCode => {
            let js = opencode_plugin(&hook);
            if existed && old != js && !old.contains(PLUGIN_MARKER) {
                return Err(format!("{} exists and wasn't written by Orbi \u{2014} not touching it", tilde(env, &path)));
            }
            js
        }
        Kind::Hermes => yaml_connect(&old, &hermes_entries(&hook))?,
        kind => json_transform(env, &path, kind, &old, |root| json_connect(root, kind, &hook))?,
    };
    Ok(Preview {
        config_path: tilde(env, &path),
        unchanged: new == old,
        creates: !existed,
        lines: if new == old { Vec::new() } else { diff_lines(&old, &new) },
    })
}

/// Lines of context kept around each change.
const CONTEXT: usize = 2;
/// Above this many lines per side, skip the line diff and show the result's
/// changed region only (configs are never this big in practice).
const MAX_DIFF_LINES: usize = 3000;

/// A minimal line diff (LCS), unchanged runs collapsed to a few lines of
/// context. Context and removed lines that look like they hold a secret are
/// masked: the preview only needs to show what Orbi adds.
fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut ops: Vec<(char, &str)> = Vec::new();
    if a.len() > MAX_DIFF_LINES || b.len() > MAX_DIFF_LINES {
        ops.extend(b.iter().map(|l| ('+', *l)));
    } else {
        let (n, m) = (a.len(), b.len());
        let mut dp = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a[i] == b[j] {
                ops.push((' ', a[i]));
                i += 1;
                j += 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                ops.push(('-', a[i]));
                i += 1;
            } else {
                ops.push(('+', b[j]));
                j += 1;
            }
        }
        ops.extend(a[i..].iter().map(|l| ('-', *l)));
        ops.extend(b[j..].iter().map(|l| ('+', *l)));
    }
    // Keep changes plus CONTEXT lines either side; mark the gaps.
    let keep: Vec<bool> = (0..ops.len())
        .map(|k| {
            let lo = k.saturating_sub(CONTEXT);
            let hi = (k + CONTEXT + 1).min(ops.len());
            ops[lo..hi].iter().any(|(op, _)| *op != ' ')
        })
        .collect();
    let mut out = Vec::new();
    let mut gap = false;
    for (k, (op, text)) in ops.iter().enumerate() {
        if !keep[k] {
            gap = true;
            continue;
        }
        if gap {
            out.push(DiffLine { op: "…".into(), text: String::new() });
            gap = false;
        }
        let shown = if *op == '+' { text.to_string() } else { mask_secret(text) };
        let shown: String = shown.chars().take(300).collect();
        out.push(DiffLine { op: op.to_string(), text: shown });
    }
    if gap {
        out.push(DiffLine { op: "…".into(), text: String::new() });
    }
    out
}

/// `"apiKey": "sk-…"` → `"apiKey": "••••••"`. Anything whose key mentions a
/// secret word keeps its key and loses its value.
fn mask_secret(line: &str) -> String {
    const WORDS: &[&str] = &["key", "token", "secret", "password", "passwd", "auth", "credential", "bearer", "cookie", "session"];
    let Some(cut) = line.find([':', '=']) else { return line.to_string() };
    let key = line[..cut].to_ascii_lowercase();
    if !WORDS.iter().any(|w| key.contains(w)) {
        return line.to_string();
    }
    let value = line[cut + 1..].trim();
    if value.is_empty() || value == "{" || value == "[" {
        return line.to_string();
    }
    let trail = if value.ends_with(',') { "," } else { "" };
    format!("{} \"\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\"{}", &line[..=cut], trail)
}

// ------------------------------------------------------------ file plumbing

/// Read (missing = ""), transform, and write only if the text changed: backup
/// first, then atomic replace. Symlinked configs are edited at their target.
fn edit_text(env: &Env, path: &Path, f: impl FnOnce(&str) -> Result<String, String>) -> Result<(), String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (old, existed) = match fs::read(&target) {
        Ok(b) => (String::from_utf8(b).map_err(|_| format!("{} isn't UTF-8 text \u{2014} Orbi won't touch it", tilde(env, path)))?, true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => return Err(format!("can't read {}: {e}", tilde(env, path))),
    };
    let new = f(&old)?;
    if new == old {
        return Ok(());
    }
    if existed {
        backup(&target)?;
    }
    write_atomic(&target, new.as_bytes())
}

fn backup(path: &Path) -> Result<PathBuf, String> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut dest = path.with_file_name(format!("{name}.orbi-backup-{secs}"));
    let mut i = 1;
    while dest.exists() {
        dest = path.with_file_name(format!("{name}.orbi-backup-{secs}-{i}"));
        i += 1;
    }
    fs::copy(path, &dest).map_err(|e| format!("backup failed: {e}"))?;
    prune_backups(path, &name);
    Ok(dest)
}

/// Keeps the oldest backup (the file as it was before Orbi ever touched it)
/// and the newest; deletes the ones in between so copies of a config that
/// may hold secrets don't pile up.
fn prune_backups(path: &Path, name: &str) {
    let Some(dir) = path.parent() else { return };
    let prefix = format!("{name}.orbi-backup-");
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut found: Vec<(u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let rest = file.strip_prefix(&prefix)?;
            let secs = rest.split('-').next()?.parse::<u64>().ok()?;
            Some((secs, e.path()))
        })
        .collect();
    found.sort();
    if found.len() > 2 {
        for (_, p) in &found[1..found.len() - 1] {
            let _ = fs::remove_file(p);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("bad config path")?;
    fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.orbi-tmp-{}", std::process::id()));
    let res = (|| {
        // Created with the original file's mode (or 0600) from the start, so
        // a config holding secrets is never briefly readable by other users.
        #[cfg(unix)]
        let mut f = {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mode = fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o600);
            let _ = fs::remove_file(&tmp);
            fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?
        };
        #[cfg(not(unix))]
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(|e| format!("can't write {}: {e}", path.display()))
}

// ---------------------------------------------------------- OpenCode plugin

const PLUGIN_MARKER: &str = "Generated by Orbi (orbi-hook)";

/// OpenCode's `permission.ask` plugin hook is never called in current
/// releases, so the plugin listens for `permission.asked` events and answers
/// through OpenCode's own reply API. OpenCode's prompt stays up meanwhile;
/// whichever answer comes first wins, and a TUI answer cancels Orbi's.
fn opencode_plugin(hook: &str) -> String {
    let hook_js = serde_json::to_string(hook).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"// {marker}. Orbi shows OpenCode's status and lets you answer its
// permission prompts with a hotkey. Manage it from Orbi's settings; Orbi
// rewrites or removes this file.
import {{ spawn }} from "node:child_process"
import {{ accessSync, constants }} from "node:fs"

const HOOK = {hook_js}

function run(args, input, done) {{
  try {{
    accessSync(HOOK, constants.X_OK)
  }} catch {{
    return null
  }}
  let out = ""
  const child = spawn(HOOK, args, {{ stdio: ["pipe", "pipe", "ignore"] }})
  child.stdout.on("data", (d) => (out += d))
  child.on("error", () => done && done(""))
  child.on("close", () => done && done(out.trim()))
  child.stdin.on("error", () => {{}})
  child.stdin.end(JSON.stringify(input))
  return child
}}

export const OrbiPlugin = async ({{ client, directory }}) => {{
  const pending = new Map()

  const reply = async (req, answer) => {{
    try {{
      if (client?.permission?.reply) {{
        await client.permission.reply({{ requestID: req.id, reply: answer }})
        return
      }}
    }} catch {{}}
    try {{
      await client?.postSessionIdPermissionsPermissionId?.({{
        path: {{ id: req.sessionID, permissionID: req.id }},
        body: {{ response: answer }},
      }})
    }} catch {{}}
  }}

  const status = (input) => run(["event", "--agent", "opencode"], {{ cwd: directory, ...input }})

  return {{
    event: async ({{ event }}) => {{
      try {{
        const p = event?.properties ?? {{}}
        if (event.type === "permission.asked" || event.type === "permission.updated") {{
          if (!p.id || pending.has(p.id)) return
          const child = run(["ask", "--agent", "opencode"], {{ hook_event_name: "permission.asked", cwd: directory, request: p }}, (answer) => {{
            if (!pending.has(p.id)) return
            pending.delete(p.id)
            if (answer === "allow") reply(p, "once")
            else if (answer === "deny") reply(p, "reject")
          }})
          if (child) pending.set(p.id, child)
        }} else if (event.type === "permission.replied") {{
          const id = p.requestID ?? p.permissionID
          const child = pending.get(id)
          if (child) {{
            pending.delete(id)
            child.kill()
          }}
        }} else if (event.type === "session.status" || event.type === "session.idle" || event.type === "session.error") {{
          status({{ hook_event_name: event.type, properties: p }})
        }}
      }} catch {{}}
    }},
    "tool.execute.before": async (input, output) => {{
      try {{
        status({{ hook_event_name: "tool.execute.before", tool: input?.tool, sessionID: input?.sessionID, args: output?.args }})
      }} catch {{}}
    }},
  }}
}}
"#,
        marker = PLUGIN_MARKER,
        hook_js = hook_js
    )
}

// ------------------------------------------------------------ Hermes (YAML)
//
// No YAML parser here: a careful line edit of the top-level `hooks:` block
// (block style only). Anything unexpected → refuse rather than guess.

const HERMES_EVENTS: &[&str] = &["pre_llm_call", "pre_tool_call", "pre_approval_request", "post_llm_call"];

fn hermes_entries(hook: &str) -> Vec<(&'static str, String)> {
    HERMES_EVENTS.iter().map(|e| (*e, shell_command(hook, &["event", "--agent", "hermes"]))).collect()
}

/// YAML double-quoted scalar.
fn yq(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

fn is_content(l: &str) -> bool {
    let t = l.trim();
    !t.is_empty() && !t.starts_with('#')
}

/// A mapping-key line that can own children indented at `d`.
fn is_parent_of(l: &str, d: usize) -> bool {
    let i = indent_of(l);
    is_content(l) && !l.trim_start().starts_with("- ") && (i < d || (i == d && d > 0 && l.trim_end().ends_with(':')))
}

/// `key:` possibly followed by a value / comment → the value text ("" if none).
fn key_value<'a>(trimmed: &'a str, key: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(key)?.strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let v = rest.trim();
    Some(if v.starts_with('#') { "" } else { v })
}

/// Remove Orbi's list items (`- command: "...orbi-hook..."`) and any event
/// keys / `hooks:` they leave empty. Returns (text, removed item commands).
fn yaml_remove(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut drop = vec![false; lines.len()];
    let mut removed = Vec::new();
    let mut parents = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim_start();
        if t.starts_with("- ") && t[2..].trim_start().starts_with("command:") && l.contains(MARKER) {
            let d = indent_of(l);
            removed.push(t.to_string());
            drop[i] = true;
            let mut j = i + 1;
            while j < lines.len() && is_content(lines[j]) && indent_of(lines[j]) > d {
                drop[j] = true;
                j += 1;
            }
            // parent key: nearest earlier key line at a smaller indent (or the
            // same indent, for "indentless" sequences)
            if let Some(p) = (0..i).rev().find(|&k| is_parent_of(lines[k], d)) {
                parents.push(p);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if removed.is_empty() {
        return (text.to_string(), removed);
    }
    // Drop parents (event keys, then `hooks:`) that no longer have children.
    for _ in 0..2 {
        let mut next = Vec::new();
        for &p in &parents {
            if drop[p] {
                continue;
            }
            let pi = indent_of(lines[p]);
            let has_child = ((p + 1)..lines.len())
                .filter(|&k| !drop[k] && is_content(lines[k]))
                .take(1)
                .any(|k| indent_of(lines[k]) > pi || (indent_of(lines[k]) == pi && lines[k].trim_start().starts_with("- ")));
            let bare = lines[p].trim_end().ends_with(':');
            if !has_child && bare {
                drop[p] = true;
                if let Some(pp) = (0..p).rev().find(|&k| !drop[k] && is_parent_of(lines[k], pi)) {
                    next.push(pp);
                }
            }
        }
        parents = next;
    }
    let out: Vec<&str> = lines.iter().zip(&drop).filter(|(_, d)| !**d).map(|(l, _)| *l).collect();
    (out.join("\n"), removed)
}

fn yaml_connect(text: &str, entries: &[(&str, String)]) -> Result<String, String> {
    if text.lines().any(|l| l.starts_with('\t') || (l.starts_with(' ') && l.trim_start_matches(' ').starts_with('\t'))) {
        return Err("config.yaml uses tabs for indentation \u{2014} Orbi won't touch it".into());
    }
    // Already exactly right? Leave the file alone.
    let (base, had) = yaml_remove(text);
    let want: Vec<String> = entries.iter().map(|(_, c)| format!("- command: {}", yq(c))).collect();
    if had == want {
        let mut ok = true;
        for (ev, _) in entries {
            ok &= text.contains(&format!("{ev}:"));
        }
        if ok {
            return Ok(text.to_string());
        }
    }
    let mut lines: Vec<String> = base.split('\n').map(str::to_string).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let item = |li: usize, cmd: &str| vec![format!("{}- command: {}", " ".repeat(li), yq(cmd)), format!("{}  timeout: 5", " ".repeat(li))];

    let hooks_idx = lines.iter().position(|l| key_value(l, "hooks").is_some());
    let h = match hooks_idx {
        None => {
            lines.push("hooks:".into());
            lines.len() - 1
        }
        Some(h) => {
            match key_value(&lines[h], "hooks").unwrap_or("") {
                "" => {}
                "{}" | "null" | "~" => lines[h] = "hooks:".into(),
                _ => return Err("config.yaml has an inline `hooks:` value \u{2014} Orbi won't touch it".into()),
            }
            h
        }
    };
    for (event, cmd) in entries {
        // Block of `hooks:` = following lines until the next top-level key.
        let end = ((h + 1)..lines.len()).find(|&k| is_content(&lines[k]) && indent_of(&lines[k]) == 0).unwrap_or(lines.len());
        let ci = ((h + 1)..end).find(|&k| is_content(&lines[k])).map(|k| indent_of(&lines[k])).unwrap_or(2).max(1);
        let found = ((h + 1)..end).find(|&k| indent_of(&lines[k]) == ci && key_value(lines[k].trim_start(), event).is_some());
        // How far this file indents a sequence under its key (2, 4, or 0 for
        // "indentless" sequences) — copied from the first one in the block.
        let step = ((h + 1)..end)
            .find(|&k| is_content(&lines[k]) && lines[k].trim_start().starts_with("- "))
            .map(|k| indent_of(&lines[k]).saturating_sub(ci))
            .unwrap_or(2);
        match found {
            Some(k) => {
                match key_value(lines[k].trim_start(), event).unwrap_or("") {
                    "" => {}
                    "[]" | "null" | "~" => lines[k] = format!("{}{}:", " ".repeat(ci), event),
                    _ => return Err(format!("config.yaml has an inline `{event}:` value \u{2014} Orbi won't touch it")),
                }
                let li = ((k + 1)..end)
                    .find(|&j| is_content(&lines[j]))
                    .filter(|&j| indent_of(&lines[j]) >= ci && lines[j].trim_start().starts_with("- "))
                    .map(|j| indent_of(&lines[j]))
                    .unwrap_or(ci + step);
                let at = k + 1;
                for (n, l) in item(li, cmd).into_iter().enumerate() {
                    lines.insert(at + n, l);
                }
            }
            None => {
                let last = ((h + 1)..end).rev().find(|&k| is_content(&lines[k]) || (lines[k].trim_start().starts_with('#') && indent_of(&lines[k]) > 0)).unwrap_or(h);
                let mut block = vec![format!("{}{}:", " ".repeat(ci), event)];
                block.extend(item(ci + step, cmd));
                for (n, l) in block.into_iter().enumerate() {
                    lines.insert(last + 1 + n, l);
                }
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

// -------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const HOOK: &str = "/Applications/Orbi.app/Contents/MacOS/orbi-hook";
    const HOOK2: &str = "/Volumes/Test Disk/Application Support/Orbi's Build/orbi-hook";

    fn temp_home() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("orbi-integ-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    fn backups(dir: &Path) -> usize {
        fs::read_dir(dir).map(|r| r.filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".orbi-backup-")).count()).unwrap_or(0)
    }

    fn get(v: &[Integration], id: &str) -> Integration {
        v.iter().find(|i| i.id == id).unwrap().clone()
    }

    #[test]
    fn shell_quoting_survives_spaces_and_quotes() {
        let c = shell_command(HOOK2, &["event", "--agent", "codex"]);
        assert_eq!(
            c,
            r#"/bin/sh -c '[ -x "$0" ] && exec "$0" "$@"; exit 0' '/Volumes/Test Disk/Application Support/Orbi'\''s Build/orbi-hook' event --agent codex"#
        );
        // Run it for real: a missing hook must exit 0 silently.
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(&c).output().unwrap();
        assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
        // And an existing one gets its args intact.
        let home = temp_home();
        let fake = home.join("dir with 'q' and space").join("orbi-hook");
        fs::create_dir_all(fake.parent().unwrap()).unwrap();
        fs::write(&fake, "#!/bin/sh\nprintf '%s|' \"$@\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let c = shell_command(&fake.to_string_lossy(), &["ask", "--agent", "codex"]);
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(&c).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "ask|--agent|codex|");
        // Exec form too.
        let (cmd, args) = exec_form(&fake.to_string_lossy(), &["event", "--agent", "claude-code"]);
        let out = std::process::Command::new(cmd).args(&args).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "event|--agent|claude-code|");
        let (cmd, args) = exec_form("/nope/orbi-hook", &["event"]);
        let out = std::process::Command::new(cmd).args(&args).output().unwrap();
        assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
    }

    #[test]
    fn detection_and_listing() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let l = list_in(&env, Path::new(HOOK));
        assert_eq!(l.len(), 6);
        assert!(l.iter().all(|i| !i.detected && !i.connected && i.status));
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        fs::write(home.join(".local/bin/opencode"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(home.join(".local/bin/opencode"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let l = list_in(&env, Path::new(HOOK));
        assert!(get(&l, "codex").detected && get(&l, "opencode").detected && !get(&l, "gemini").detected);
        assert_eq!(get(&l, "claude-code").config_path, "~/.claude/settings.json");
        let v = serde_json::to_value(get(&l, "gemini")).unwrap();
        assert_eq!(v["configPath"], "~/.gemini/settings.json");
        assert_eq!(v["approvals"], false);
        assert!(connect_in(&env, "nope", Path::new(HOOK)).is_err());
        assert!(connect_in(&env, "codex", Path::new("relative/orbi-hook")).is_err());
    }

    #[test]
    fn claude_connect_disconnect_roundtrip() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let p = home.join(".claude/settings.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let original = r#"{
    "model": "opus",
    "hooks": {
        "PreToolUse": [
            { "matcher": "Bash", "hooks": [ { "type": "command", "command": "my-linter" } ] }
        ],
        "Stop": [
            { "hooks": [ { "type": "command", "command": "$HOME/.orbi/bin/orbi-hook", "args": ["event"] } ] }
        ]
    },
    "zeta": true
}
"#;
        fs::write(&p, original).unwrap();
        let i = connect_in(&env, "claude-code", Path::new(HOOK)).unwrap();
        assert!(i.connected && i.detected && i.approvals);
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        // key order kept, other hooks kept, legacy Orbi entry replaced, 4-space indent kept
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["model", "hooks", "zeta"]);
        assert!(read(&p).contains("\n    \"model\""));
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "my-linter");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let ask = &v["hooks"]["PermissionRequest"][0];
        assert_eq!(ask["matcher"], "*");
        assert_eq!(ask["hooks"][0]["command"], "/bin/sh");
        assert_eq!(ask["hooks"][0]["args"], json!(["-c", WRAP, HOOK, "ask", "--agent", "claude-code"]));
        assert_eq!(ask["hooks"][0]["timeout"], 120);
        assert_eq!(v["hooks"]["PreToolUse"][1]["hooks"][0]["async"], true);
        assert_eq!(backups(p.parent().unwrap()), 1);

        // idempotent: no rewrite, no new backup
        let before = read(&p);
        connect_in(&env, "claude-code", Path::new(HOOK)).unwrap();
        assert_eq!(read(&p), before);
        assert_eq!(backups(p.parent().unwrap()), 1);

        // repair re-points to a new path
        repair_in(&env, Path::new(HOOK2));
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(v["hooks"]["PermissionRequest"][0]["hooks"][0]["args"][2], HOOK2);
        assert!(!read(&p).contains(HOOK));

        let i = disconnect_in(&env, "claude-code", Path::new(HOOK2)).unwrap();
        assert!(!i.connected);
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(
            v,
            json!({"model": "opus", "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "my-linter"}]}]}, "zeta": true})
        );
        // disconnect again is a no-op
        disconnect_in(&env, "claude-code", Path::new(HOOK2)).unwrap();
    }

    #[test]
    fn refuses_invalid_files() {
        let home = temp_home();
        let env = Env::with_home(&home);
        for (dir, file, id) in [(".claude", "settings.json", "claude-code"), (".gemini", "settings.json", "gemini"), (".cursor", "hooks.json", "cursor")] {
            let p = home.join(dir).join(file);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "{ // comment\n \"a\": 1 }").unwrap();
            let e = connect_in(&env, id, Path::new(HOOK)).unwrap_err();
            assert!(e.contains("isn't valid JSON"), "{e}");
            assert_eq!(read(&p), "{ // comment\n \"a\": 1 }");
            fs::write(&p, "[1,2]").unwrap();
            assert!(connect_in(&env, id, Path::new(HOOK)).unwrap_err().contains("isn't a JSON object"));
            assert_eq!(backups(p.parent().unwrap()), 0);
        }
    }

    #[test]
    fn creates_missing_files_and_follows_symlinks() {
        let home = temp_home();
        let env = Env::with_home(&home);
        connect_in(&env, "codex", Path::new(HOOK)).unwrap();
        let p = home.join(".codex/hooks.json");
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        let cmd = v["hooks"]["PermissionRequest"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, shell_command(HOOK, &["ask", "--agent", "codex"]));
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["async"], true);
        assert_eq!(backups(&home.join(".codex")), 0);
        disconnect_in(&env, "codex", Path::new(HOOK)).unwrap();
        assert_eq!(read(&p).trim(), "{}");

        #[cfg(unix)]
        {
            let real = home.join("dotfiles/settings.json");
            fs::create_dir_all(real.parent().unwrap()).unwrap();
            fs::write(&real, "{\"a\": 1}\n").unwrap();
            fs::create_dir_all(home.join(".gemini")).unwrap();
            std::os::unix::fs::symlink(&real, home.join(".gemini/settings.json")).unwrap();
            connect_in(&env, "gemini", Path::new(HOOK)).unwrap();
            assert!(fs::symlink_metadata(home.join(".gemini/settings.json")).unwrap().file_type().is_symlink());
            let v: Value = serde_json::from_str(&read(&real)).unwrap();
            assert_eq!(v["a"], 1);
            assert_eq!(v["hooks"]["BeforeTool"][0]["matcher"], "*");
            assert_eq!(v["hooks"]["BeforeTool"][0]["hooks"][0]["timeout"], 5000);
            assert!(list_in(&env, Path::new(HOOK)).iter().any(|i| i.id == "gemini" && i.connected));
        }
    }

    #[test]
    fn codex_keeps_other_hooks() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let p = home.join(".codex/hooks.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, r#"{"description":"mine","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#).unwrap();
        connect_in(&env, "codex", Path::new(HOOK)).unwrap();
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(v["description"], "mine");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert!(v["hooks"]["Stop"][1]["hooks"][0]["command"].as_str().unwrap().contains(MARKER));
        disconnect_in(&env, "codex", Path::new(HOOK)).unwrap();
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(v, json!({"description":"mine","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}));
    }

    #[test]
    fn cursor_flat_hooks() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let p = home.join(".cursor/hooks.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, r#"{"hooks":{"stop":[{"command":"./hooks/audit.sh"}]}}"#).unwrap();
        let i = connect_in(&env, "cursor", Path::new(HOOK)).unwrap();
        assert!(i.connected && !i.approvals);
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(v.as_object().unwrap().keys().next().unwrap(), "version");
        assert_eq!(v["hooks"]["stop"][0]["command"], "./hooks/audit.sh");
        assert_eq!(v["hooks"]["stop"][1]["command"], shell_command(HOOK, &["event", "--agent", "cursor"]));
        assert!(v["hooks"].get("beforeShellExecution").is_none());
        repair_in(&env, Path::new(HOOK2));
        assert!(read(&p).contains("Orbi'\\\\''s Build"));
        disconnect_in(&env, "cursor", Path::new(HOOK2)).unwrap();
        let v: Value = serde_json::from_str(&read(&p)).unwrap();
        assert_eq!(v, json!({"version": 1, "hooks": {"stop": [{"command": "./hooks/audit.sh"}]}}));
    }

    #[test]
    fn opencode_plugin_file() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let p = home.join(".config/opencode/plugins/orbi.js");
        let i = connect_in(&env, "opencode", Path::new(HOOK2)).unwrap();
        assert!(i.connected);
        let js = read(&p);
        assert!(js.contains(PLUGIN_MARKER));
        assert!(js.contains(r#"const HOOK = "/Volumes/Test Disk/Application Support/Orbi's Build/orbi-hook""#));
        assert!(js.contains("permission.asked") && js.contains("\"--agent\", \"opencode\""));
        repair_in(&env, Path::new(HOOK));
        assert!(read(&p).contains(&format!("const HOOK = \"{HOOK}\"")));
        disconnect_in(&env, "opencode", Path::new(HOOK)).unwrap();
        assert!(!p.exists());
        // someone else's orbi.js is never touched
        fs::write(&p, "export const Mine = async () => ({})\n").unwrap();
        assert!(connect_in(&env, "opencode", Path::new(HOOK)).is_err());
        assert!(disconnect_in(&env, "opencode", Path::new(HOOK)).is_err());
        assert_eq!(read(&p), "export const Mine = async () => ({})\n");
    }

    #[test]
    fn hermes_yaml_with_existing_hooks() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let p = home.join(".hermes/config.yaml");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let original = "model: x\nhooks:\n  outbound:\n    - name: dash\n      url: http://127.0.0.1:1/e\n  pre_tool_call:\n    - matcher: \"terminal\"\n      command: \"~/.hermes/agent-hooks/block.sh\"\n# trailing comment\ndisplay:\n  theme: dark\n";
        fs::write(&p, original).unwrap();
        let i = connect_in(&env, "hermes", Path::new(HOOK2)).unwrap();
        assert!(i.connected);
        let t = read(&p);
        let cmd = yq(&shell_command(HOOK2, &["event", "--agent", "hermes"]));
        assert!(t.contains(&format!("  pre_tool_call:\n    - command: {cmd}\n      timeout: 5\n    - matcher: \"terminal\"")), "{t}");
        assert!(t.contains(&format!("  post_llm_call:\n    - command: {cmd}\n      timeout: 5\n")), "{t}");
        assert!(t.starts_with("model: x\nhooks:\n  outbound:\n    - name: dash\n"));
        assert!(t.ends_with("# trailing comment\ndisplay:\n  theme: dark\n"), "{t}");
        assert_eq!(backups(p.parent().unwrap()), 1);
        // idempotent
        connect_in(&env, "hermes", Path::new(HOOK2)).unwrap();
        assert_eq!(read(&p), t);
        // repair
        repair_in(&env, Path::new(HOOK));
        assert!(read(&p).contains(HOOK) && !read(&p).contains("Orbi'"));
        // disconnect restores the original exactly
        disconnect_in(&env, "hermes", Path::new(HOOK)).unwrap();
        assert_eq!(read(&p), original);
    }

    #[test]
    fn hermes_yaml_without_hooks_and_edge_cases() {
        let original = "model: x\n";
        let t = yaml_connect(original, &hermes_entries(HOOK)).unwrap();
        assert!(t.starts_with("model: x\nhooks:\n  pre_llm_call:\n    - command: "));
        assert_eq!(yaml_remove(&t).0, "model: x\n");
        let t = yaml_connect("hooks: {}\n", &hermes_entries(HOOK)).unwrap();
        assert!(t.starts_with("hooks:\n  pre_llm_call:\n"));
        assert!(yaml_connect("hooks: {a: 1}\n", &hermes_entries(HOOK)).is_err());
        assert!(yaml_connect("hooks:\n  pre_tool_call: [{command: x}]\n", &hermes_entries(HOOK)).is_err());
        assert!(yaml_connect("a:\n\tb: 1\n", &hermes_entries(HOOK)).is_err());
        // 4-space style is followed
        let t = yaml_connect("hooks:\n    pre_llm_call:\n        - command: mine\n", &hermes_entries(HOOK)).unwrap();
        assert!(t.contains("    pre_llm_call:\n        - command: \"/bin/sh"), "{t}");
        assert!(t.contains("        - command: mine\n"));
        assert!(t.contains("    post_llm_call:\n        - command: "), "{t}");
        assert_eq!(yaml_remove(&t).0, "hooks:\n    pre_llm_call:\n        - command: mine\n");
        // "indentless" sequences are followed too
        let src = "hooks:\n  pre_tool_call:\n  - command: mine\n    timeout: 3\nother: 1\n";
        let t = yaml_connect(src, &hermes_entries(HOOK)).unwrap();
        assert!(t.contains("  pre_tool_call:\n  - command: \"/bin/sh"), "{t}");
        assert!(t.contains("  post_llm_call:\n  - command: "), "{t}");
        assert!(t.ends_with("    timeout: 5\nother: 1\n"), "{t}");
        assert_eq!(yaml_remove(&t).0, src);
        // empty file
        let t = yaml_connect("", &hermes_entries(HOOK)).unwrap();
        assert!(t.starts_with("hooks:\n  pre_llm_call:\n"));
        assert_eq!(yaml_remove(&t).0.trim(), "");
    }

    #[test]
    fn preview_shows_exactly_what_connect_writes_and_writes_nothing() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let hook = Path::new("/Applications/Orbi.app/Contents/MacOS/orbi-hook");
        let cfg = home.join(".claude/settings.json");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(&cfg, "{\n  \"env\": {\n    \"ANTHROPIC_API_KEY\": \"sk-secret-123\"\n  },\n  \"model\": \"opus\"\n}\n").unwrap();
        let before = fs::read_to_string(&cfg).unwrap();

        let p = preview_in(&env, "claude-code", hook).unwrap();
        assert!(!p.unchanged && !p.creates);
        assert_eq!(fs::read_to_string(&cfg).unwrap(), before, "preview must not write");
        let text: String = p.lines.iter().map(|l| format!("{}{}\n", l.op, l.text)).collect();
        assert!(text.contains("SessionStart") && text.contains("PermissionRequest"));
        assert!(!text.contains("sk-secret-123"), "secrets in context lines are masked:\n{text}");
        assert!(p.lines.iter().all(|l| l.op != "-" || !l.text.contains("sk-")));

        connect_in(&env, "claude-code", hook).unwrap();
        let after = preview_in(&env, "claude-code", hook).unwrap();
        assert!(after.unchanged && after.lines.is_empty(), "connected and current → nothing to change");
        // The secret is still there, untouched.
        assert!(fs::read_to_string(&cfg).unwrap().contains("sk-secret-123"));
    }

    #[test]
    fn preview_of_a_new_file_and_of_a_foreign_plugin() {
        let home = temp_home();
        let env = Env::with_home(&home);
        let hook = Path::new("/Applications/Orbi.app/Contents/MacOS/orbi-hook");
        let p = preview_in(&env, "claude-code", hook).unwrap();
        assert!(p.creates && p.lines.iter().all(|l| l.op == "+"));
        assert!(!home.join(".claude/settings.json").exists());

        let plugin = home.join(".config/opencode/plugins/orbi.js");
        fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        fs::write(&plugin, "// someone else's plugin\n").unwrap();
        assert!(preview_in(&env, "opencode", hook).is_err());
        assert!(preview_in(&env, "nope", hook).is_err());
        assert!(preview_in(&env, "claude-code", Path::new("relative/hook")).is_err());
    }

    #[test]
    fn secrets_are_masked_and_diffs_collapse() {
        assert_eq!(mask_secret(r#"    "GITHUB_TOKEN": "ghp_abc","#), "    \"GITHUB_TOKEN\": \"\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\",");
        assert_eq!(mask_secret(r#"  "model": "opus""#), r#"  "model": "opus""#);
        assert_eq!(mask_secret(r#"  "auth": {"#), r#"  "auth": {"#);
        let old: String = (0..40).map(|i| format!("line {i}\n")).collect();
        let new = old.replace("line 20\n", "line 20\nadded\n");
        let d = diff_lines(&old, &new);
        assert_eq!(d.iter().filter(|l| l.op == "+").count(), 1);
        assert_eq!(d.iter().filter(|l| l.op == " ").count(), 4);
        assert_eq!(d.first().unwrap().op, "…");
        assert_eq!(d.last().unwrap().op, "…");
    }
}
