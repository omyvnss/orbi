# Orbi app contract (v1.1)

Single self-contained `Orbi.app`. No second app, no `~/.orbi`, no build tools
needed by users.

## Files

- **Data dir:** `~/Library/Application Support/Orbi/` (mode 0700). Override for
  tests with env `ORBI_DATA_DIR`.
  - `token` (0600) — 64 hex chars
  - `port` — the TCP port in use
  - `config.json` — settings (below)
- **Hook binary:** bundled inside the app as a Tauri sidecar
  (`Orbi.app/Contents/MacOS/orbi-hook`; in dev it sits next to the main
  binary in `src-tauri/target/<profile>/`). The app finds it with
  `current_exe().parent().join("orbi-hook")`. Integrations always point at
  that absolute path. On every launch the app re-points already-connected
  integrations if the path changed (app moved/updated).

## Server authenticity (anti port-squatting)

A squatter on Orbi's port (another local user, while Orbi is down) could
otherwise answer `allow`. So the hook never trusts an unproven server:

1. `GET /hello` with header `X-Orbi-Nonce: <32 hex>` — no token. Server replies
   `200 {"ok":true}` with header
   `X-Orbi-Proof: hex(HMAC-SHA256(key = token ascii bytes, msg = "hello\n" + port + "\n" + nonce))`,
   where `port` is the port the hook dialled (so a squatter can't relay the
   nonce to the real Orbi on another port). Hook waits at most 250 ms.
   Hook verifies (constant-time); mismatch/missing → fall through silently.
2. Only then the real request, with `Authorization: Bearer <token>` and a fresh
   `X-Orbi-Nonce`. Every response carries
   `X-Orbi-Proof: hex(HMAC-SHA256(token, "resp\n" + nonce + "\n" + body))`.
   Hook verifies before trusting `allow`/`deny`; mismatch → treat as `ask`.

Crates: `hmac` + `sha2` (RustCrypto).

Other guards: ⌃⌥A only approves a request that has been at the front of the
queue, unchanged, for 0.7 s (otherwise the face shakes); the whole HTTP request
must arrive within 2 s; settings-changing commands are accepted only from the
Settings window; one Orbi instance at a time; the port file is removed on quit.

## config.json

```json
{
  "timeout_secs": 45,
  "launch_at_login": false,
  "explain": {
    "mode": "rules",            // "rules" | "model"
    "provider": "ollama",       // "ollama" | "openrouter" | "openai" | "custom"
    "base_url": "http://127.0.0.1:11434/v1",
    "model": "llama3.2"
  },
  "onboarded": false
}
```
API keys are **never** in config.json — they live in the macOS Keychain
(service `dev.orbi.app`, account `byok-<provider>`).

## Tauri commands (settings window, label `settings`, url `index.html#settings`)

```ts
type Settings = {
  timeoutSecs: number; launchAtLogin: boolean; paused: boolean;
  explain: { mode: "rules" | "model"; provider: "ollama" | "openrouter" | "openai" | "custom";
             baseUrl: string; model: string; hasKey: boolean };
};
type Integration = {
  id: string;            // "claude-code" | "codex" | "gemini" | "opencode" | "cursor" | "hermes" | ...
  name: string;          // "Claude Code"
  detected: boolean;     // the agent is installed on this Mac
  connected: boolean;    // Orbi's hooks are present in its config
  approvals: boolean;    // Orbi can approve/deny for this agent (not just show status)
  status: boolean;       // Orbi shows working/done for this agent
  configPath: string;    // file Orbi edits, "~"-abbreviated
  note: string;          // one short line, e.g. "Status only — Codex has no approval hook"
};
type AppInfo = { version: string; dataDir: string; hookPath: string; port: number | null };
```

| command | args | returns |
|---|---|---|
| `get_settings` | – | `Settings` |
| `update_settings` | `{ patch: Partial<Settings> }` (explain may be partial) | `Settings` |
| `set_api_key` | `{ key: string \| null }` (for current provider) | `Settings` |
| `test_explain` | – | `{ line: string, ms: number }` or error string |
| `list_integrations` | – | `Integration[]` |
| `connect_integration` | `{ id }` | `Integration` |
| `disconnect_integration` | `{ id }` | `Integration` |
| `regenerate_token` | – | `null` |
| `get_app_info` | – | `AppInfo` |
| `open_settings` | – | opens/focuses the settings window |
| `set_paused` | `{ paused: boolean }` | `Settings` |

Event `settings-changed` → `Settings`.

## Rust module APIs (src-tauri/src)

```rust
// integrations.rs
pub struct Integration { /* serde camelCase, fields as the TS type */ }
pub fn list(hook: &std::path::Path) -> Vec<Integration>;
pub fn connect(id: &str, hook: &std::path::Path) -> Result<Integration, String>;
pub fn disconnect(id: &str, hook: &std::path::Path) -> Result<Integration, String>;
pub fn repair(hook: &std::path::Path);   // re-point connected integrations to `hook`

// byok.rs
pub struct ModelConfig { pub provider: String, pub base_url: String, pub model: String }
pub fn key_get(provider: &str) -> Option<String>;          // Keychain
pub fn key_set(provider: &str, key: Option<&str>) -> Result<(), String>;
/// Rewrites the rule-based line more naturally. Blocking, hard 4 s timeout.
/// Must never be the reason a request waits: callers show the rules line first.
pub fn rewrite(cfg: &ModelConfig, key: Option<&str>, agent_label: &str, tool: &str,
               rules_line: &str, detail: &str) -> Result<String, String>;
```
