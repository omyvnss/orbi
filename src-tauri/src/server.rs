//! The local HTTP server agent hooks talk to. See `docs/PROTOCOL.md`.
//!
//! A localhost server that can approve shell commands is a security surface,
//! so: bound to 127.0.0.1 only, every request needs the bearer token from
//! `~/.orbi/token`, browser-originated requests (any `Origin` header) and
//! foreign `Host` headers are refused, and bodies are size-capped.
//!
//! Orbi never answers `allow` on its own. The only paths to `allow`/`deny`
//! are `answer()`, called from the ⌃⌥A / ⌃⌥D hotkeys. Everything else —
//! timeout, pause, shutdown, bad input — ends in `ask`, which hands the
//! decision back to the agent's own prompt.

use std::{
    collections::VecDeque,
    fs,
    io::Read,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    explain,
    http::{self, Request},
};

pub const DEFAULT_PORT: u16 = 47821;
pub const DEFAULT_TIMEOUT_SECS: u64 = 45;
const MAX_BODY: u64 = 256 * 1024;
/// Upper bound on requests handled at once, so a misbehaving client can't
/// spawn threads without limit.
const MAX_IN_FLIGHT: usize = 64;
/// Of those, how many may still be sending their request. Long-polling
/// `/ask`s don't count, so slow senders can't starve real requests.
const MAX_READING: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    id: u64,
    agent: String,
    agent_label: String,
    tool: String,
    line: String,
    detail: String,
    warnings: Vec<String>,
    cwd: String,
    created_at: u64,
}

struct Pending {
    view: PendingView,
    /// The agent's session id, so a later "tool finished" event from the same
    /// session can withdraw a prompt answered in the terminal.
    session: Option<String>,
    app_bundle: Option<String>,
    term: Option<Term>,
    tx: mpsc::Sender<(Decision, &'static str)>,
}

/// Which terminal tab an agent runs in, as reported by orbi-hook and checked
/// here character by character: it ends up as an argument to `osascript`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    program: String,
    /// iTerm2 session UUID.
    iterm: Option<String>,
    /// Apple Terminal tty, `/dev/ttysNNN`.
    tty: Option<String>,
}

/// A question the agent asked (Claude Code `AskUserQuestion`). Display only:
/// Orbi never answers it — the user does, in the terminal.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    text: String,
    options: Vec<String>,
    /// How many further questions follow this one.
    more: u64,
}

/// One running agent session, named after its project folder.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    key: String,
    agent: String,
    agent_label: String,
    project: String,
    /// ready | working | asking | question | done | error
    state: String,
    summary: String,
    subagents: u32,
    question: Option<Question>,
    updated_at: u64,
}

struct Session {
    view: SessionView,
    agent: String,
    id: String,
    term: Option<Term>,
    app_bundle: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    agent: String,
    agent_label: String,
    kind: String,
    summary: String,
    detail: Option<String>,
    at: u64,
    #[serde(skip)]
    app_bundle: Option<String>,
    #[serde(skip)]
    term: Option<Term>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    paused: bool,
    timeout_secs: u64,
    queue: Vec<PendingView>,
    activity: Option<Activity>,
    sessions: Vec<SessionView>,
}

#[derive(Default)]
pub struct Core {
    queue: VecDeque<Pending>,
    activity: Option<Activity>,
    next_id: u64,
    paused: bool,
    timeout_secs: u64,
    /// The most recent app any agent reported running in, for ⌃⌥J.
    last_app_bundle: Option<String>,
    last_term: Option<Term>,
    /// Running sessions, newest last.
    sessions: Vec<Session>,
    /// The request at the front of the queue and when it got there. ⌃⌥A
    /// only approves a front that has been on screen long enough to read.
    front_id: Option<u64>,
    front_since: Option<Instant>,
}

/// How long a request must sit, unchanged, at the front of the queue before
/// ⌃⌥A can approve it. Stops a keypress meant for one request landing on the
/// one that replaced it (the first timed out, was withdrawn, or a double
/// press).
const FRONT_SETTLE: Duration = Duration::from_millis(700);

/// Records when the front of the queue changes. Called under the lock before
/// anything reads the front.
fn sync_front(core: &mut Core) {
    let id = core.queue.front().map(|p| p.view.id);
    if id != core.front_id {
        core.front_id = id;
        core.front_since = id.map(|_| Instant::now());
    }
}

pub struct CoreState(pub Mutex<Core>);

impl CoreState {
    pub fn new() -> Self {
        CoreState(Mutex::new(Core { timeout_secs: DEFAULT_TIMEOUT_SECS, ..Default::default() }))
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Most sessions shown at once.
const MAX_SESSIONS: usize = 8;
/// A finished or failed session lingers this long, then leaves the face.
const SETTLED_TTL_MS: u64 = 5 * 60 * 1000;
/// Anything silent this long is presumed gone (terminal closed, crash).
const QUIET_TTL_MS: u64 = 30 * 60 * 1000;

fn prune_sessions(core: &mut Core, now: u64) {
    core.sessions.retain(|s| {
        let age = now.saturating_sub(s.view.updated_at);
        let settled = matches!(s.view.state.as_str(), "done" | "error");
        age < QUIET_TTL_MS && !(settled && age >= SETTLED_TTL_MS)
    });
}

fn snapshot(core: &Core) -> Snapshot {
    let mut sessions: Vec<SessionView> = core
        .sessions
        .iter()
        .rev()
        .take(MAX_SESSIONS)
        .map(|s| {
            let mut v = s.view.clone();
            // A permission prompt from this session outranks everything else.
            if core.queue.iter().any(|p| p.view.agent == s.agent && p.session.as_deref() == Some(s.id.as_str())) {
                v.state = "asking".into();
            }
            v
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    Snapshot {
        paused: core.paused,
        timeout_secs: core.timeout_secs,
        queue: core.queue.iter().map(|p| p.view.clone()).collect(),
        activity: core.activity.clone(),
        sessions,
    }
}

fn emit_state(app: &AppHandle) {
    let snap = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        sync_front(&mut core);
        prune_sessions(&mut core, now_ms());
        snapshot(&core)
    };
    let _ = app.emit("orbi-state", snap);
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> Snapshot {
    let state = app.state::<CoreState>();
    let core = state.0.lock().unwrap();
    snapshot(&core)
}

/// Answers the oldest pending request. The only way `allow`/`deny` ever
/// reaches an agent. Returns false when nothing was waiting.
pub fn answer(app: &AppHandle, decision: Decision) -> bool {
    let reason = match decision {
        Decision::Allow => "Approved in Orbi",
        Decision::Deny => "Denied in Orbi",
        Decision::Ask => "Handed back to the terminal by Orbi",
    };
    let pending = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        sync_front(&mut core);
        let settled = core.front_since.is_some_and(|t| t.elapsed() >= FRONT_SETTLE);
        if decision == Decision::Allow && !settled {
            None
        } else {
            core.queue.pop_front()
        }
    };
    let Some(pending) = pending else {
        // Nothing to answer, or the front just changed: ask for another look
        // rather than approve something the user may not have read.
        let _ = app.emit("orbi-nudge", ());
        return false;
    };
    // The waiting request may have timed out in the same instant; then the
    // receiver is gone and this is a harmless no-op.
    let _ = pending.tx.send((decision, reason));
    emit_state(app);
    true
}

/// Tray "Pause Orbi". Pausing releases everything waiting back to the
/// agents' own prompts, and new requests get `ask` straight away.
pub fn set_paused(app: &AppHandle, paused: bool) {
    let released: Vec<Pending> = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        core.paused = paused;
        if paused { core.queue.drain(..).collect() } else { Vec::new() }
    };
    for p in released {
        let _ = p.tx.send((Decision::Ask, "Orbi is paused"));
    }
    emit_state(app);
}

/// ⌃⌥J — brings the app the relevant agent runs in to the front: the oldest
/// pending request's, else the latest activity's, else the last one seen.
pub fn jump_to_agent(app: &AppHandle) {
    let (bundle, term) = {
        let state = app.state::<CoreState>();
        let core = state.0.lock().unwrap();
        // The request on screen, else a session waiting on a question, else
        // whatever was active last.
        let questioned = core.sessions.iter().rev().find(|s| s.view.question.is_some());
        if let Some(p) = core.queue.front() {
            (p.app_bundle.clone().or_else(|| core.last_app_bundle.clone()), p.term.clone())
        } else if let Some(s) = questioned {
            (s.app_bundle.clone().or_else(|| core.last_app_bundle.clone()), s.term.clone())
        } else if let Some(a) = core.activity.as_ref() {
            (a.app_bundle.clone().or_else(|| core.last_app_bundle.clone()), a.term.clone().or_else(|| core.last_term.clone()))
        } else {
            (core.last_app_bundle.clone(), core.last_term.clone())
        }
    };
    // osascript can wait on the one-time Automation prompt: never on this thread.
    std::thread::spawn(move || {
        if term.as_ref().is_some_and(focus_tab) {
            return;
        }
        if let Some(bundle) = bundle {
            // Validated at intake to [A-Za-z0-9.-]; passed as an argument, never
            // through a shell.
            let _ = Command::new("/usr/bin/open").arg("-b").arg(bundle).spawn();
        }
    });
}

const TERMINAL_SCRIPT: &[&str] = &[
    "on run argv",
    "set wanted to item 1 of argv",
    "tell application \"Terminal\"",
    "repeat with w in windows",
    "repeat with t in tabs of w",
    "if tty of t is wanted then",
    "set selected of t to true",
    "set index of w to 1",
    "activate",
    "return \"ok\"",
    "end if",
    "end repeat",
    "end repeat",
    "end tell",
    "return \"missing\"",
    "end run",
];

const ITERM_SCRIPT: &[&str] = &[
    "on run argv",
    "set wanted to item 1 of argv",
    "tell application \"iTerm2\"",
    "repeat with w in windows",
    "repeat with t in tabs of w",
    "repeat with s in sessions of t",
    "if unique id of s is wanted then",
    "select w",
    "select t",
    "select s",
    "activate",
    "return \"ok\"",
    "end if",
    "end repeat",
    "end repeat",
    "end repeat",
    "end tell",
    "return \"missing\"",
    "end run",
];

/// Brings the exact terminal tab forward. The tab id travels as an `argv`
/// item, never inside the script text. False when it can't (no permission,
/// tab closed, other terminal) so the caller falls back to opening the app.
fn focus_tab(term: &Term) -> bool {
    let (script, arg) = match (term.program.as_str(), &term.iterm, &term.tty) {
        ("iTerm.app", Some(id), _) => (ITERM_SCRIPT, id),
        ("Apple_Terminal", _, Some(tty)) => (TERMINAL_SCRIPT, tty),
        _ => return false,
    };
    let mut cmd = Command::new("/usr/bin/osascript");
    for line in script {
        cmd.arg("-e").arg(line);
    }
    cmd.arg(arg);
    match cmd.output() {
        Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok",
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- files

/// `~/Library/Application Support/Orbi` — token, port, config. The one place
/// Orbi keeps anything outside its own app bundle. `ORBI_DATA_DIR` overrides
/// it for tests.
pub fn data_dir() -> Result<PathBuf, String> {
    let dir = match std::env::var_os("ORBI_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => {
            let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
            PathBuf::from(home).join("Library/Application Support/Orbi")
        }
    };
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    set_mode(&dir, 0o700);
    Ok(dir)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

fn is_valid_token(t: &str) -> bool {
    t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Reads `~/.orbi/token`, creating it (0600) on first launch.
fn load_or_create_token(dir: &Path) -> Result<String, String> {
    let path = dir.join("token");
    if let Ok(existing) = fs::read_to_string(&path) {
        let t = existing.trim().to_string();
        if is_valid_token(&t) {
            set_mode(&path, 0o600);
            return Ok(t);
        }
    }
    write_new_token(&path)
}

fn random_hex(n: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; n];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("cannot read /dev/urandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn write_new_token(path: &Path) -> Result<String, String> {
    let token = random_hex(32)?;
    // Create with 0600 from the start so the token is never world-readable.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Written to a temp file then renamed, so a hook never reads a
        // half-written token.
        let tmp = path.with_extension("tmp");
        let _ = fs::remove_file(&tmp);
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(token.as_bytes()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    fs::write(path, &token).map_err(|e| e.to_string())?;
    Ok(token)
}

/// The live token. Hooks re-read the file on every call, so rotating it
/// takes effect for the very next request.
pub struct TokenState(pub Mutex<String>);

static PORT: OnceLock<u16> = OnceLock::new();

/// Removes the port file on quit, so a hook never dials a port some other
/// process may have taken since.
pub fn remove_port_file() {
    if let Ok(dir) = data_dir() {
        let _ = fs::remove_file(dir.join("port"));
    }
}

/// The port the server is listening on, once it is.
pub fn port() -> Option<u16> {
    PORT.get().copied()
}

/// Settings › Security › Regenerate token.
pub fn regenerate_token(app: &AppHandle) -> Result<(), String> {
    let path = data_dir()?.join("token");
    let token = write_new_token(&path)?;
    *app.state::<TokenState>().0.lock().unwrap() = token;
    Ok(())
}

pub fn set_timeout(app: &AppHandle, secs: u64) {
    app.state::<CoreState>().0.lock().unwrap().timeout_secs = secs.clamp(5, 600);
    emit_state(app);
}

pub fn is_paused(app: &AppHandle) -> bool {
    app.state::<CoreState>().0.lock().unwrap().paused
}

/// Replaces a pending request's one-line explanation (the BYOK model's
/// rewrite arriving after the rules line was already shown).
pub fn set_line(app: &AppHandle, id: u64, line: String) {
    let changed = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        match core.queue.iter_mut().find(|p| p.view.id == id) {
            Some(p) => {
                p.view.line = line;
                true
            }
            None => false,
        }
    };
    if changed {
        emit_state(app);
    }
}

// ---------------------------------------------------------------- server

/// Starts the server on its own thread. Errors are logged, never fatal: an
/// Orbi without a server just shows an idle face, and hooks fall through.
pub fn start(app: AppHandle) {
    let dir = match data_dir() {
        Ok(d) => d,
        Err(e) => return eprintln!("[orbi] server disabled: {e}"),
    };
    let token = match load_or_create_token(&dir) {
        Ok(t) => t,
        Err(e) => return eprintln!("[orbi] server disabled: {e}"),
    };
    *app.state::<TokenState>().0.lock().unwrap() = token;

    let listener = TcpListener::bind(("127.0.0.1", DEFAULT_PORT))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)));
    let listener = match listener {
        Ok(l) => l,
        Err(e) => return eprintln!("[orbi] server disabled: cannot bind: {e}"),
    };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else {
        return eprintln!("[orbi] server disabled: no local address");
    };
    if let Err(e) = fs::write(dir.join("port"), format!("{port}\n")) {
        eprintln!("[orbi] cannot write port file: {e}");
    }
    let _ = PORT.set(port);
    eprintln!("[orbi] listening on 127.0.0.1:{port}");

    let in_flight: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
    let reading: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                in_flight.fetch_sub(1, Ordering::SeqCst);
                http::reject(stream, 503);
                continue;
            }
            if reading.fetch_add(1, Ordering::SeqCst) >= MAX_READING {
                reading.fetch_sub(1, Ordering::SeqCst);
                in_flight.fetch_sub(1, Ordering::SeqCst);
                http::reject(stream, 503);
                continue;
            }
            let app = app.clone();
            std::thread::spawn(move || {
                let parsed = http::read_request(stream, MAX_BODY as usize);
                reading.fetch_sub(1, Ordering::SeqCst);
                match parsed {
                    Ok(req) => handle(&app, req, port),
                    Err((stream, status)) => http::reject(stream, status),
                }
                in_flight.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
}

/// Equal-length comparison that doesn't exit early on the first mismatch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Origin/Host guards, then the bearer token.
fn check_request(req: &Request, token: &str, port: u16) -> Result<(), (u16, &'static str)> {
    if req.header("Origin").is_some() {
        return Err((403, "browser requests are not allowed"));
    }
    let host_ok = req.header("Host").is_some_and(|h| {
        h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")
    });
    if !host_ok {
        return Err((403, "bad host"));
    }
    let presented = req
        .header("Authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(presented.trim().as_bytes(), token.as_bytes()) {
        return Err((401, "bad token"));
    }
    Ok(())
}

fn parse_body(req: &Request) -> Result<Value, (u16, &'static str)> {
    serde_json::from_slice(&req.body).map_err(|_| (400, "invalid json"))
}

/// HMAC-SHA256(token, msg) as lowercase hex.
fn proof(token: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("any key length");
    mac.update(msg.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// A client nonce: 32–128 hex chars, or nothing.
fn nonce_of(req: &Request) -> Option<String> {
    req.header("X-Orbi-Nonce")
        .map(str::trim)
        .filter(|n| (32..=128).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// Sends a response, signed when the client sent a nonce so the hook can
/// tell real Orbi from something squatting on its port (docs/APP.md).
fn respond(req: Request, token: &str, nonce: Option<&str>, status: u16, body: String) {
    match nonce {
        Some(n) => {
            let p = proof(token, &format!("resp\n{n}\n{body}"));
            req.respond(status, &[("X-Orbi-Proof", &p)], &body);
        }
        None => req.respond(status, &[], &body),
    }
}

fn handle(app: &AppHandle, req: Request, port: u16) {
    let token = app.state::<TokenState>().0.lock().unwrap().clone();
    let nonce = nonce_of(&req);

    // `/hello` is the one unauthenticated route: it proves Orbi knows the
    // token before the hook sends it. Origin/Host guards still apply.
    if req.method == "GET" && req.path == "/hello" {
        match (check_request(&req, &token, port), &nonce) {
            (Err((403, m)), _) => respond(req, &token, None, 403, json!({ "error": m }).to_string()),
            (_, Some(n)) => {
                let p = proof(&token, &format!("hello\n{port}\n{n}"));
                req.respond(200, &[("X-Orbi-Proof", &p)], &json!({"ok": true}).to_string());
            }
            (_, None) => {
                respond(req, &token, None, 400, json!({"error": "nonce required"}).to_string())
            }
        }
        return;
    }

    if let Err((status, msg)) = check_request(&req, &token, port) {
        // Unauthenticated: never sign, so a guessed request learns nothing.
        respond(req, &token, None, status, json!({ "error": msg }).to_string());
        return;
    }
    let (status, body) = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => {
            (200, json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}).to_string())
        }
        ("POST", "/event") => match parse_body(&req) {
            Ok(body) => {
                handle_event(app, &body);
                (204, String::new())
            }
            Err((s, m)) => (s, json!({ "error": m }).to_string()),
        },
        ("POST", "/ask") => match parse_body(&req) {
            Ok(body) => match handle_ask(app, &body, || req.peer_closed()) {
                Some((decision, reason)) => {
                    (200, json!({"decision": decision.as_str(), "reason": reason}).to_string())
                }
                // Nobody is listening any more; nothing to send.
                None => return,
            },
            Err((s, m)) => (s, json!({ "error": m }).to_string()),
        },
        _ => (404, json!({"error": "not found"}).to_string()),
    };
    respond(req, &token, nonce.as_deref(), status, body);
}

/// Truncates to at most `max` characters, on a char boundary.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn str_field(body: &Value, key: &str, max: usize) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(|s| clip(s.trim(), max)).filter(|s| !s.is_empty())
}

/// A macOS bundle id, or nothing. Anything else is dropped so it can never
/// reach `open` as something other than a bundle id.
fn bundle_field(body: &Value) -> Option<String> {
    body.get("app_bundle").and_then(Value::as_str).filter(|b| {
        !b.is_empty()
            && b.len() <= 200
            && !b.starts_with('-')
            && b.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
    }).map(str::to_string)
}

/// A terminal tab reference, or nothing. Every part is checked against a
/// strict pattern because it is later handed to `osascript` as an argument.
fn term_field(body: &Value) -> Option<Term> {
    let t = body.get("term")?;
    let program = t.get("program")?.as_str()?;
    if program.is_empty() || program.len() > 40 || !program.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) {
        return None;
    }
    // ITERM_SESSION_ID is "w0t1p0:<UUID>"; keep the UUID.
    let iterm = t
        .get("iterm")
        .and_then(Value::as_str)
        .map(|v| v.rsplit(':').next().unwrap_or(""))
        .filter(|id| id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'))
        .map(str::to_string);
    let tty = t
        .get("tty")
        .and_then(Value::as_str)
        .filter(|v| {
            v.strip_prefix("/dev/ttys")
                .is_some_and(|n| !n.is_empty() && n.len() <= 4 && n.bytes().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_string);
    Some(Term { program: program.to_string(), iterm, tty })
}

fn question_field(body: &Value) -> Option<Question> {
    let q = body.get("question")?;
    let text = str_field(q, "text", 200)?;
    let options: Vec<String> = q
        .get("options")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(4).filter_map(Value::as_str).map(|o| clip(o.trim(), 40)).filter(|o| !o.is_empty()).collect())
        .unwrap_or_default();
    let more = q.get("more").and_then(Value::as_u64).unwrap_or(0).min(9);
    Some(Question { text: explain::visible(&text), options: options.iter().map(|o| explain::visible(o)).collect(), more })
}

/// The last folder of a working directory: "~/code/orbi" → "orbi".
fn project_of(cwd: &str) -> String {
    let name = cwd.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    explain::visible(&clip(name, 40))
}

/// The session for `(agent, id)`, created if new. Oldest sessions make room.
fn touch_session<'a>(core: &'a mut Core, agent: &str, id: &str, cwd: Option<&str>, now: u64) -> &'a mut Session {
    if let Some(i) = core.sessions.iter().position(|s| s.agent == agent && s.id == id) {
        // Newest last, so the face lists the most recent first.
        let s = core.sessions.remove(i);
        core.sessions.push(s);
    } else {
        if core.sessions.len() >= MAX_SESSIONS * 2 {
            core.sessions.remove(0);
        }
        core.sessions.push(Session {
            view: SessionView {
                key: format!("{agent}:{}", clip(id, 60)),
                agent: agent.to_string(),
                agent_label: explain::agent_label(agent),
                project: String::new(),
                state: "ready".into(),
                summary: String::new(),
                subagents: 0,
                question: None,
                updated_at: now,
            },
            agent: agent.to_string(),
            id: id.to_string(),
            term: None,
            app_bundle: None,
        });
    }
    let s = core.sessions.last_mut().expect("just pushed");
    if let Some(c) = cwd.filter(|c| !c.is_empty()) {
        s.view.project = project_of(c);
    }
    if s.view.project.is_empty() {
        s.view.project = s.view.agent_label.clone();
    }
    s.view.updated_at = now;
    s
}

/// Applies one `/event` to Orbi's state. Returns the prompts it proved were
/// already answered elsewhere, for the caller to release. Pure, so it's tested
/// without a running app.
fn apply_event(core: &mut Core, body: &Value, now: u64) -> Vec<Pending> {
    let agent = str_field(body, "agent", 40).unwrap_or_else(|| "agent".into());
    let app_bundle = bundle_field(body);
    let term = term_field(body);
    let session = str_field(body, "session", 200);
    let cwd = str_field(body, "cwd", 1000);
    if app_bundle.is_some() {
        core.last_app_bundle = app_bundle.clone();
    }
    if term.is_some() {
        core.last_term = term.clone();
    }
    let release = |core: &mut Core, agent: &str, session: &Option<String>| -> Vec<Pending> {
        let (gone, keep): (VecDeque<Pending>, VecDeque<Pending>) =
            core.queue.drain(..).partition(|p| p.view.agent == agent && p.session == *session);
        core.queue = keep;
        gone.into_iter().collect()
    };

    // Session lifecycle: bookkeeping only, the face's activity is untouched.
    if let Some(phase) = body.get("phase").and_then(Value::as_str) {
        let Some(id) = session.clone() else { return Vec::new() };
        let mut released = Vec::new();
        match phase {
            "start" => {
                let s = touch_session(core, &agent, &id, cwd.as_deref(), now);
                if matches!(s.view.state.as_str(), "done" | "error") {
                    s.view.state = "ready".into();
                    s.view.summary.clear();
                }
                s.view.question = None;
                if term.is_some() {
                    s.term = term;
                }
                if app_bundle.is_some() {
                    s.app_bundle = app_bundle;
                }
            }
            "end" => {
                core.sessions.retain(|s| !(s.agent == agent && s.id == id));
                // The session is gone, so nothing can still be waiting on it.
                released = release(core, &agent, &session);
            }
            "subagent_start" | "subagent_stop" => {
                let kind = str_field(body, "summary", 40).map(|k| explain::visible(&k));
                let s = touch_session(core, &agent, &id, cwd.as_deref(), now);
                if phase == "subagent_start" {
                    s.view.subagents = (s.view.subagents + 1).min(99);
                    s.view.state = "working".into();
                    s.view.summary = match kind {
                        Some(k) => format!("running a sub-agent: {k}"),
                        None => "running a sub-agent".into(),
                    };
                } else {
                    s.view.subagents = s.view.subagents.saturating_sub(1);
                }
            }
            _ => {}
        }
        return released;
    }

    let kind = match body.get("kind").and_then(Value::as_str) {
        Some(k @ ("working" | "done" | "error")) => k.to_string(),
        _ => return Vec::new(),
    };
    let settles = kind != "working" || body.get("settles").and_then(Value::as_bool) == Some(true);
    // The same session moved on, so anything it was waiting on was answered
    // in the terminal. Only matches a known session: without one, an event
    // can't prove which prompt it belongs to.
    let released = if settles && session.is_some() { release(core, &agent, &session) } else { Vec::new() };
    let summary = explain::visible(&str_field(body, "summary", 300).unwrap_or_default());
    let question = question_field(body);

    if let Some(id) = session.as_deref() {
        let s = touch_session(core, &agent, id, cwd.as_deref(), now);
        s.view.state = if question.is_some() { "question".into() } else { kind.clone() };
        s.view.summary = summary.clone();
        s.view.question = question;
        if kind != "working" {
            s.view.subagents = 0;
        }
        if term.is_some() {
            s.term = term.clone();
        }
        if app_bundle.is_some() {
            s.app_bundle = app_bundle.clone();
        }
    }
    core.activity = Some(Activity {
        agent_label: explain::agent_label(&agent),
        agent,
        kind,
        summary,
        detail: str_field(body, "detail", 4000).map(|d| explain::visible(&d)),
        at: now,
        app_bundle,
        term,
    });
    released
}

fn handle_event(app: &AppHandle, body: &Value) {
    let released = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        apply_event(&mut core, body, now_ms())
    };
    for p in released {
        let _ = p.tx.send((Decision::Ask, "Answered in the terminal"));
    }
    emit_state(app);
}

/// Queues a request and waits for the user. `None` means the hook went away
/// first (answered in the terminal, agent interrupted): the request is
/// withdrawn from the face and there is no one to reply to.
fn handle_ask(
    app: &AppHandle,
    body: &Value,
    peer_closed: impl Fn() -> bool,
) -> Option<(Decision, &'static str)> {
    let agent = str_field(body, "agent", 40).unwrap_or_else(|| "agent".into());
    let tool = str_field(body, "tool", 100).unwrap_or_else(|| "unknown".into());
    let cwd = str_field(body, "cwd", 1000).unwrap_or_default();
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    let app_bundle = bundle_field(body);
    let term = term_field(body);
    let session = str_field(body, "session", 200);
    let explained = explain::explain(&tool, &input, &cwd);
    let (rules_line, rules_detail) = (explained.line.clone(), explained.detail.clone());

    let (tx, rx) = mpsc::channel();
    let tool_for_model = tool.clone();
    let (id, label_for_model, timeout) = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        if core.paused {
            return Some((Decision::Ask, "Orbi is paused"));
        }
        if app_bundle.is_some() {
            core.last_app_bundle = app_bundle.clone();
        }
        if term.is_some() {
            core.last_term = term.clone();
        }
        if let Some(id) = session.as_deref() {
            let s = touch_session(&mut core, &agent, id, Some(cwd.as_str()), now_ms());
            s.view.question = None;
            if term.is_some() {
                s.term = term.clone();
            }
            if app_bundle.is_some() {
                s.app_bundle = app_bundle.clone();
            }
        }
        core.next_id += 1;
        let id = core.next_id;
        let label = explain::agent_label(&agent);
        core.queue.push_back(Pending {
            view: PendingView {
                id,
                agent_label: label.clone(),
                agent,
                tool,
                line: explained.line,
                detail: explained.detail,
                warnings: explained.warnings,
                cwd,
                created_at: now_ms(),
            },
            session,
            app_bundle,
            term,
            tx,
        });
        (id, label, Duration::from_secs(core.timeout_secs))
    };
    emit_state(app);
    // Optional BYOK rewrite. Runs on its own thread and only ever replaces
    // the line already on screen; the answer never waits for it.
    crate::settings::spawn_rewrite(app, id, label_for_model, tool_for_model, rules_line, rules_detail);

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(answer) => return Some(answer),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Some((Decision::Ask, "Asking in the terminal"))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let gone = peer_closed();
        if gone || std::time::Instant::now() >= deadline {
            withdraw(app, id);
            // An answer can land between the check and the removal.
            if let Ok(answer) = rx.try_recv() {
                return if gone { None } else { Some(answer) };
            }
            return if gone {
                None
            } else {
                Some((Decision::Ask, "No answer in Orbi; asking in the terminal"))
            };
        }
    }
}

/// Removes a request from the queue if it is still there.
fn withdraw(app: &AppHandle, id: u64) {
    let removed = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        let before = core.queue.len();
        core.queue.retain(|p| p.view.id != id);
        core.queue.len() != before
    };
    if removed {
        emit_state(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn clip_respects_chars() {
        assert_eq!(clip("héllo", 10), "héllo");
        assert_eq!(clip("héllo", 2), "hé…");
    }

    #[test]
    fn bundle_ids_are_validated() {
        let ok = json!({"app_bundle": "com.apple.Terminal"});
        assert_eq!(bundle_field(&ok).as_deref(), Some("com.apple.Terminal"));
        for bad in ["", "-a", "com.x; rm -rf /", "a/b", "../x"] {
            assert_eq!(bundle_field(&json!({ "app_bundle": bad })), None, "{bad}");
        }
    }

    #[test]
    fn proof_matches_rfc4231() {
        // RFC 4231 test case 2.
        assert_eq!(
            proof("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn front_settle_is_tracked() {
        let mut core = Core::default();
        sync_front(&mut core);
        assert_eq!(core.front_id, None);
        let (tx, _rx) = mpsc::channel();
        core.queue.push_back(Pending {
            view: PendingView {
                id: 7,
                agent: "a".into(),
                agent_label: "A".into(),
                tool: "Bash".into(),
                line: String::new(),
                detail: String::new(),
                warnings: vec![],
                cwd: String::new(),
                created_at: 0,
            },
            session: None,
            app_bundle: None,
            term: None,
            tx,
        });
        sync_front(&mut core);
        let first = core.front_since.unwrap();
        assert_eq!(core.front_id, Some(7));
        sync_front(&mut core);
        assert_eq!(core.front_since.unwrap(), first, "unchanged front keeps its time");
        core.queue.clear();
        sync_front(&mut core);
        assert_eq!((core.front_id, core.front_since), (None, None));
    }

    #[test]
    fn token_format() {
        assert!(is_valid_token(&"a".repeat(64)));
        assert!(!is_valid_token(&"g".repeat(64)));
        assert!(!is_valid_token("abc"));
    }

    #[test]
    fn terminal_tabs_are_validated() {
        let t = |v: Value| term_field(&json!({ "term": v }));
        let uuid = "2C7A5E0B-1D3F-4A6B-9C8D-0E1F2A3B4C5D";
        assert_eq!(
            t(json!({"program": "iTerm.app", "iterm": format!("w0t1p0:{uuid}")})),
            Some(Term { program: "iTerm.app".into(), iterm: Some(uuid.into()), tty: None })
        );
        assert_eq!(t(json!({"program": "Apple_Terminal", "tty": "/dev/ttys004"})).unwrap().tty.as_deref(), Some("/dev/ttys004"));
        for bad_tty in ["/dev/ttys", "/dev/ttys12345", "/dev/tty.x", "ttys004", "/dev/ttys00\"; x"] {
            assert_eq!(t(json!({"program": "Apple_Terminal", "tty": bad_tty})).unwrap().tty, None, "{bad_tty}");
        }
        assert_eq!(t(json!({"program": "iTerm.app", "iterm": "w0t1p0:\" & do shell script \"x"})).unwrap().iterm, None);
        assert_eq!(t(json!({"program": "a b"})), None);
        assert_eq!(t(json!({"program": ""})), None);
        assert_eq!(term_field(&json!({})), None);
    }

    #[test]
    fn sessions_track_lifecycle_subagents_and_questions() {
        let mut core = Core::default();
        let ev = |v: Value| v;
        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "session", "phase": "start", "session": "s1", "cwd": "/w/orbi"})), 1);
        assert_eq!(core.sessions.len(), 1);
        assert_eq!((core.sessions[0].view.project.as_str(), core.sessions[0].view.state.as_str()), ("orbi", "ready"));
        // Lifecycle never changes the face's activity.
        assert!(core.activity.is_none());

        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "session", "phase": "subagent_start", "session": "s1", "summary": "Explore"})), 2);
        assert_eq!(core.sessions[0].view.subagents, 1);
        assert_eq!(core.sessions[0].view.summary, "running a sub-agent: Explore");

        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "working", "summary": "has a question for you", "session": "s1",
            "question": {"text": "Which DB?", "options": ["Postgres", "SQLite"], "more": 0}})), 3);
        let s = &core.sessions[0].view;
        assert_eq!(s.state, "question");
        assert_eq!(s.question.as_ref().unwrap().options, vec!["Postgres".to_string(), "SQLite".to_string()]);

        // The next event from that session clears the question.
        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "working", "summary": "running `ls`", "session": "s1"})), 4);
        assert!(core.sessions[0].view.question.is_none());

        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "done", "summary": "finished", "session": "s1"})), 5);
        assert_eq!((core.sessions[0].view.state.as_str(), core.sessions[0].view.subagents), ("done", 0));

        // A second project; newest first in the snapshot.
        apply_event(&mut core, &ev(json!({"agent": "codex", "kind": "working", "summary": "x", "session": "t9", "cwd": "/w/web"})), 6);
        let snap = snapshot(&core);
        assert_eq!(snap.sessions.iter().map(|s| s.project.as_str()).collect::<Vec<_>>(), vec!["web", "orbi"]);

        // Settled sessions leave after a while; quiet ones after longer.
        prune_sessions(&mut core, 5 + SETTLED_TTL_MS);
        assert_eq!(core.sessions.len(), 1);
        prune_sessions(&mut core, 6 + QUIET_TTL_MS);
        assert!(core.sessions.is_empty());

        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "session", "phase": "start", "session": "s2"})), 7);
        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "session", "phase": "end", "session": "s2"})), 8);
        assert!(core.sessions.is_empty());
        // No session id: lifecycle events are ignored.
        apply_event(&mut core, &ev(json!({"agent": "claude-code", "kind": "session", "phase": "start"})), 9);
        assert!(core.sessions.is_empty());
    }

    #[test]
    fn session_end_releases_its_prompts_to_the_terminal() {
        let mut core = Core::default();
        let (tx, rx) = mpsc::channel();
        core.queue.push_back(Pending {
            view: PendingView {
                id: 1,
                agent: "claude-code".into(),
                agent_label: "Claude Code".into(),
                tool: "Bash".into(),
                line: String::new(),
                detail: String::new(),
                warnings: vec![],
                cwd: String::new(),
                created_at: 0,
            },
            session: Some("s1".into()),
            app_bundle: None,
            term: None,
            tx,
        });
        let released = apply_event(&mut core, &json!({"agent": "claude-code", "kind": "session", "phase": "end", "session": "s1"}), 1);
        assert_eq!(released.len(), 1);
        assert!(core.queue.is_empty());
        drop(rx);
    }

    #[test]
    fn projects_and_questions_are_bounded() {
        assert_eq!(project_of("/Users/x/code/orbi/"), "orbi");
        assert_eq!(project_of(""), "");
        let q = question_field(&json!({"question": {"text": "  ", "options": []}}));
        assert!(q.is_none());
        let long: Vec<String> = (0..10).map(|i| format!("opt{i}")).collect();
        let q = question_field(&json!({"question": {"text": "Pick", "options": long, "more": 99}})).unwrap();
        assert_eq!((q.options.len(), q.more), (4, 9));
    }
}
