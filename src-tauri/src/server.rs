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
    tx: mpsc::Sender<(Decision, &'static str)>,
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
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    paused: bool,
    timeout_secs: u64,
    queue: Vec<PendingView>,
    activity: Option<Activity>,
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

fn snapshot(core: &Core) -> Snapshot {
    Snapshot {
        paused: core.paused,
        timeout_secs: core.timeout_secs,
        queue: core.queue.iter().map(|p| p.view.clone()).collect(),
        activity: core.activity.clone(),
    }
}

fn emit_state(app: &AppHandle) {
    let snap = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        sync_front(&mut core);
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
    let bundle = {
        let state = app.state::<CoreState>();
        let core = state.0.lock().unwrap();
        core.queue
            .front()
            .and_then(|p| p.app_bundle.clone())
            .or_else(|| core.activity.as_ref().and_then(|a| a.app_bundle.clone()))
            .or_else(|| core.last_app_bundle.clone())
    };
    if let Some(bundle) = bundle {
        // Validated at intake to [A-Za-z0-9.-]; passed as an argument, never
        // through a shell.
        let _ = Command::new("/usr/bin/open").arg("-b").arg(bundle).spawn();
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

fn handle_event(app: &AppHandle, body: &Value) {
    let agent = str_field(body, "agent", 40).unwrap_or_else(|| "agent".into());
    let kind = match body.get("kind").and_then(Value::as_str) {
        Some(k @ ("working" | "done" | "error")) => k.to_string(),
        _ => return,
    };
    let app_bundle = bundle_field(body);
    let session = str_field(body, "session", 200);
    let settles = kind != "working" || body.get("settles").and_then(Value::as_bool) == Some(true);
    let released: Vec<Pending> = {
        let state = app.state::<CoreState>();
        let mut core = state.0.lock().unwrap();
        if app_bundle.is_some() {
            core.last_app_bundle = app_bundle.clone();
        }
        // The same session moved on, so anything it was waiting on was
        // answered in the terminal. Only matches a known session: without
        // one, an event can't prove which prompt it belongs to.
        let mut released = Vec::new();
        if settles && session.is_some() {
            let (gone, keep): (VecDeque<Pending>, VecDeque<Pending>) = core
                .queue
                .drain(..)
                .partition(|p| p.view.agent == agent && p.session == session);
            core.queue = keep;
            released.extend(gone);
        }
        core.activity = Some(Activity {
            agent_label: explain::agent_label(&agent),
            agent,
            kind,
            summary: explain::visible(&str_field(body, "summary", 300).unwrap_or_default()),
            detail: str_field(body, "detail", 4000).map(|d| explain::visible(&d)),
            at: now_ms(),
            app_bundle,
        });
        released
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
}
