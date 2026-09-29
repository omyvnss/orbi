# Orbi protocol

How agent hooks talk to Orbi, and how Orbi's Rust core talks to its face.

## Discovery & auth

- Orbi listens on `127.0.0.1` only. Default port **47821**; if that is taken it
  binds a random free port. The port in use is written to
  `~/Library/Application Support/Orbi/port` on every launch.
- On first launch Orbi writes a random 256-bit hex token to
  `~/Library/Application Support/Orbi/token` (mode `0600`, directory `0700`).
  It is reused on later launches and can be rotated from Settings.
- Hooks verify they are talking to the real Orbi before sending the token —
  see "Server authenticity" in `docs/APP.md`.
- Every request must carry `Authorization: Bearer <token>`. Missing or wrong
  token → `401`.
- Requests carrying an `Origin` header are rejected (`403`) — browsers always
  send one, hooks never do. The `Host` header must be `127.0.0.1:<port>` or
  `localhost:<port>` (DNS-rebinding guard).
- Bodies over 256 KB → `413`.

## `POST /event` — non-blocking status update

```json
{ "agent": "claude-code", "kind": "working|done|error", "summary": "…",
  "detail": "optional longer text", "session": "optional session id",
  "app_bundle": "optional, e.g. com.apple.Terminal" }
```

Returns `204` immediately.

## `POST /ask` — permission request, long-polls

```json
{ "agent": "claude-code", "tool": "Bash", "input": { "command": "npm test" },
  "cwd": "/path/to/project", "session": "optional",
  "app_bundle": "optional — the terminal/app the agent runs in" }
```

Blocks until the user answers or the timeout (default 45 s, set in
Settings → General) passes. Response:

```json
{ "decision": "allow" | "deny" | "ask", "reason": "…" }
```

- `allow` / `deny` only ever come from the user pressing `⌃⌥A` / `⌃⌥D`.
- `ask` means "fall back to the agent's own prompt": timeout, Orbi paused, or
  anything unexpected. **Orbi never returns `allow` on its own.**
- Requests are queued and answered strictly in arrival order (oldest first).

## `GET /health`

Authenticated. Returns `{"ok":true,"version":"0.1.0"}`.

## Hook contract (`orbi-hook`)

If the token / port files are missing, the connection is refused, the server
fails the proof check, or anything at all goes wrong, the hook prints nothing and exits `0` within a
few hundred ms, so the agent behaves exactly as if Orbi were not installed.

## Core → face (Tauri)

- Command `get_state` → `OrbiSnapshot`.
- Event `orbi-state` → `OrbiSnapshot`, emitted on every change.

```ts
type OrbiSnapshot = {
  paused: boolean;
  timeoutSecs: number;
  queue: Array<{
    id: number;
    agent: string;        // "claude-code"
    agentLabel: string;   // "Claude Code"
    tool: string;         // "Bash"
    line: string;         // "wants to run `npm test` in orbi/"
    detail: string;       // full command / path / diff size, multi-line ok
    warnings: string[];   // e.g. ["deletes files"], empty when nothing risky
    cwd: string;
    createdAt: number;    // ms since epoch
  }>;
  // The most recent /event from any agent, or null.
  activity: null | {
    agent: string;
    agentLabel: string;
    kind: "working" | "done" | "error";
    summary: string;
    detail: string | null;
    at: number;           // ms since epoch
  };
};
```

Face state is derived by the frontend: queue non-empty → `asking`;
otherwise `activity.kind` (`done`/`error` relax to `idle` after a few
seconds, `working` relaxes to `idle` after ~60 s without a new event);
otherwise `idle`.

## Hotkeys

| key | action |
|---|---|
| `⌃⌥A` | allow the oldest pending request |
| `⌃⌥D` | deny it |
| `⌃⌥O` | expand / collapse the preview (event `widget-expanded`) |
| `⌃⌥J` | bring the agent's app (`app_bundle`) to the front |
