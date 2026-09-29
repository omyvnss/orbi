# Orbi × Claude Code

**Approvals + status.** Orbi shows what Claude Code is doing. When Claude Code
is about to ask permission, Orbi shows the request too: answer with `⌃⌥A` / `⌃⌥D`,
or in the terminal as usual.

## Connect

Orbi → Settings → Integrations → Claude Code → Connect. This edits
`~/.claude/settings.json`:

- a `settings.json.orbi-backup-<time>` copy is written first,
- older Orbi entries are replaced and everything else stays as it was, key
  order included. Running it twice changes nothing,
- files that aren't valid JSON are left alone.

Disconnect removes only the handlers that mention `orbi-hook`. Restart
Claude Code sessions, or open `/hooks`, to pick up the change.

## What gets written

```json
"PermissionRequest": [ { "matcher": "*", "hooks": [ {
  "type": "command", "command": "/bin/sh",
  "args": ["-c", "[ -x \"$0\" ] && exec \"$0\" \"$@\"; exit 0",
           "/Applications/Orbi.app/Contents/MacOS/orbi-hook", "ask", "--agent", "claude-code"],
  "timeout": 120 } ] } ],
"UserPromptSubmit" / "PreToolUse" (matcher "*") / "Notification" / "Stop" / "StopFailure":
  same wrapper with ["event", "--agent", "claude-code"], "timeout": 5, "async": true
```

The `/bin/sh` wrapper gets the hook path as `$0`. If Orbi.app is gone, it
exits 0 silently.

| Hook | Orbi request | Face shows |
|---|---|---|
| `PermissionRequest` (sync) | `POST /ask`, waits for you | asking: "Claude Code wants to run `npm test` in orbi/" |
| `UserPromptSubmit` | `POST /event` working | "is thinking…" |
| `PreToolUse` | `POST /event` working | "running `npm test`", "editing `src/app.ts`", … |
| `Notification` | `POST /event` working | the notification text |
| `Stop` / `StopFailure` | `POST /event` done / error | "finished" / "stopped with an error" |

## Why `PermissionRequest`, not `PreToolUse`

From https://code.claude.com/docs/en/hooks:

> "PreToolUse hooks run before every tool call, whether or not it needs
> permission. PermissionRequest hooks run only when Claude Code is about to ask
> you for permission, or when it would otherwise auto-deny a call that can't
> prompt."

So Orbi asks exactly when Claude Code would, and your own allow/deny rules
still apply. PermissionRequest output:

```json
{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}
{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":"Denied in Orbi"}}}
```

For ask, timeout, or any error, orbi-hook prints nothing and exits 0, so Claude
Code's own prompt stays. If `orbi-hook ask` runs from a `PreToolUse` hook
instead, it prints the PreToolUse shape (`permissionDecision` +
`permissionDecisionReason`).

## Timeouts

Orbi answers "ask" after `timeout_secs` (45 s by default, in
`~/Library/Application Support/Orbi/config.json`). orbi-hook waits
`clamp(timeout_secs + 10, 55, 110)` s, and the hook `timeout` is 120 s, so
Orbi's fallback always arrives before Claude Code would kill the hook.

## Safety

- orbi-hook prints an allow only when a verified Orbi server returned
  `{"decision":"allow"}`. The server is verified with the HMAC handshake in
  `docs/APP.md`, and it only says allow when you press `⌃⌥A`.
- If Orbi isn't running, there's no token, the server is unverified, or
  anything else goes wrong, it prints nothing and exits 0 within milliseconds.
- In non-interactive sessions where no hook decides, Claude Code denies. Orbi
  can approve those from the face.
