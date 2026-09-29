# Orbi × Gemini CLI

**Status only.** Orbi → Settings → Integrations → Gemini CLI → Connect adds
Orbi's hooks to `~/.gemini/settings.json` (backup first; other settings kept;
files with comments or invalid JSON are left alone).

| Event | Face shows |
|---|---|
| `BeforeAgent` | "is thinking…" |
| `BeforeTool` (matcher `*`) | "running `npm test`", "editing `src/a.ts`", … |
| `Notification` (`ToolPermission`) | the permission message |
| `AfterAgent` | "finished" |

Each entry is `{"name":"orbi","type":"command","command":"/bin/sh -c '…' '<orbi-hook>' event --agent gemini","timeout":5000}`
(Gemini timeouts are milliseconds). Manage them with `/hooks panel` in Gemini.

**Why no approvals:** Gemini's `BeforeTool` hook can deny a tool or force a
prompt, but it can't approve one. Gemini's policy and confirmation step still
runs after the hook ("`decision`: Set to `"deny"` … to prevent the tool from
executing", https://geminicli.com/docs/hooks/reference/). Answering in Orbi
would mean answering twice. Approve in Gemini's own prompt.
