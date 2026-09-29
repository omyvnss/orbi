# Orbi × Codex CLI

**Approvals + status.** Codex hooks use the same shapes as Claude Code
(https://developers.openai.com/codex/hooks).

## Connect

Orbi → Settings → Integrations → Codex → Connect. This writes Orbi's entries
into `~/.codex/hooks.json`, keeping any other hooks, with a backup first. Then
**open `/hooks` in Codex once and trust them**:

> "Before a non-managed hook can run, Codex requires you to review and trust
> the exact hook definition. Codex records trust against the hook's current
> hash, so new or changed hooks are marked for review and skipped until trusted."

If Orbi.app moves, Orbi updates the path, and Codex asks you to trust the hooks again.

## What gets written

| Event | Command (`/bin/sh` wrapper, path passed as `$0`) | Notes |
|---|---|---|
| `PermissionRequest` (matcher `*`) | `… ask --agent codex` | sync, `timeout: 120` |
| `UserPromptSubmit`, `PreToolUse` (matcher `*`), `Stop` | `… event --agent codex` | `async: true`, `timeout: 5` |

- `PermissionRequest` *"runs when Codex is about to ask for approval … It
  doesn't run for commands that don't need approval."* Orbi prints
  `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"|"deny"}}}`.
  For ask or timeout it prints nothing, and *"if no matching hook decides,
  Codex uses the normal approval flow."*
- `apply_patch` requests appear on the face as file edits with +/− line counts.

## Legacy `notify`

`orbi-hook codex-notify` still handles Codex's `notify` program
(`agent-turn-complete` → "finished"). The app doesn't write it, because the
hooks above cover it. If you added it by hand earlier, you can remove it from
`~/.codex/config.toml`.
