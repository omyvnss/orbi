# Adapters — how Orbi plugs into each agent

Orbi's settings window connects each agent (`src-tauri/src/integrations.rs`).
Connecting edits that agent's own config to call the `orbi-hook` binary
bundled inside `Orbi.app`. Every edit keeps the rest of the file, writes a
`*.orbi-backup-<time>` copy first, and replaces the file atomically. Orbi
refuses to edit a file it can't parse. Disconnecting removes only entries that
mention `orbi-hook`.

Every command goes through `/bin/sh -c '[ -x "$0" ] && exec "$0" "$@"; exit 0' <hook> …`.
The hook path is passed as `$0` and never pasted into the script, so if
Orbi.app is deleted the hook does nothing: no output, no error, no delay.

| Agent | Approvals from Orbi | Status on the face | Mechanism | Config Orbi edits |
|---|---|---|---|---|
| Claude Code | **yes** | yes | `PermissionRequest` hook (sync) + async status hooks | `~/.claude/settings.json` |
| Codex CLI | **yes** | yes | `PermissionRequest` hook (sync) + async status hooks. You trust them once in `/hooks` | `~/.codex/hooks.json` |
| OpenCode | **yes** (Orbi or TUI, whichever answers first) | yes | generated JS plugin: `permission.asked` event → reply API | `~/.config/opencode/plugins/orbi.js` |
| Gemini CLI | no (hooks can block, not approve) | yes | `BeforeAgent`/`BeforeTool`/`AfterAgent`/`Notification` | `~/.gemini/settings.json` |
| Cursor | no (see below) | yes | `afterShellExecution`/`afterFileEdit`/`afterMCPExecution`/`stop` | `~/.cursor/hooks.json` |
| Hermes | no (hooks can block or escalate, not approve) | yes | shell hooks `pre_llm_call`/`pre_tool_call`/`pre_approval_request`/`post_llm_call` | `~/.hermes/config.yaml` |
| anything else | `orbi-hook ask --tool …` (exit 0/1/2) | `orbi-hook event --kind …` | [generic](generic/README.md) | none |

Orbi's own safety rules hold for every agent. It never approves anything by
itself. On a timeout or any doubt, the agent falls back to its own prompt.
The hook trusts only a server that passes the HMAC handshake in `docs/APP.md`.

Research date: 2026-09-26. Everything below comes from official docs or source.

---

## Claude Code — https://code.claude.com/docs/en/hooks

- Config: `~/.claude/settings.json` → `hooks.<Event>[] = {matcher?, hooks: [{type:"command", command, args?, timeout?, async?}]}`.
  Exec form: *"runs when `args` is present … spawns it directly with `args` as the argument vector."*
- Approvals: `PermissionRequest` *"runs when Claude Code is about to ask you for permission"*.
  stdin: `session_id, cwd, hook_event_name, tool_name, tool_input, permission_suggestions`.
  stdout: `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"|"deny","message"?}}}`.
  If the hook prints nothing and exits 0, Claude Code shows its normal prompt.
- Status: `UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`, `StopFailure` (all `async: true`).
- Details: [claude-code/README.md](claude-code/README.md).

## Codex CLI — https://developers.openai.com/codex/hooks

- Config: `~/.codex/hooks.json` (or `[hooks]` in `config.toml`), same three-level shape as Claude Code;
  `command` is a single shell string; `timeout` in seconds; `"async": true` for background hooks.
- Trust: *"Before a non-managed hook can run, Codex requires you to review and trust the exact hook
  definition … Use `/hooks` in the CLI."* Changing the path (e.g. moving Orbi.app) needs a re-trust.
- Approvals: *"`PermissionRequest` runs when Codex is about to ask for approval … It can allow the request,
  deny the request, or decline to decide and let the normal approval prompt continue."*
  stdin adds `turn_id, tool_name (Bash | apply_patch | mcp__…), tool_input.command`.
  stdout is the same as Claude Code: `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}`.
  *"If no matching hook decides, Codex uses the normal approval flow."*
- Status: `UserPromptSubmit`, `PreToolUse`, `Stop` (`last_assistant_message`).
  The older `notify = [...]` program (`agent-turn-complete`) is still supported by `orbi-hook codex-notify`
  (https://developers.openai.com/codex/config-advanced).

## Gemini CLI — https://geminicli.com/docs/hooks/reference/

- Config: `~/.gemini/settings.json` → `hooks.<Event>[] = {matcher?, hooks: [{type:"command", command, name?, timeout (ms)}]}`.
  `command` is a shell string.
- `BeforeTool` output: *"`decision`: Set to `"deny"` (or `"block"`) to prevent the tool from executing."*
  There is no "approve". In source, a hook `allow` doesn't skip Gemini's policy/confirmation step
  (`packages/core/src/scheduler/scheduler.ts`: only `ask` and block change the flow).
  Orbi can't approve for Gemini, so it only shows status. `orbi-hook ask --agent gemini`
  still prints `{"decision":"deny",…}` on deny and nothing otherwise, for anyone who wires it by hand.
- Status: `BeforeAgent` (`prompt`), `BeforeTool` (`tool_name`, `tool_input`), `AfterAgent` (`prompt_response`),
  `Notification` (`notification_type: "ToolPermission"`, `message`; *"cannot … grant permissions"*).
- stdout must be JSON or empty. Empty output means "no decision".

## OpenCode — https://opencode.ai/docs/plugins/

- Plugins: JS/TS files in `~/.config/opencode/plugins/` are loaded at startup. A plugin gets
  `{ client, directory, serverUrl, $ }` and returns hooks: `event`, `tool.execute.before`, ….
- The typed `"permission.ask"` hook exists but is never called in current releases
  (https://github.com/anomalyco/opencode/issues/7006, /47674). Orbi's plugin uses the
  `permission.asked` **event** instead (`{id, sessionID, permission, patterns, metadata}`, from
  `packages/schema/src/v1/permission.ts`). It answers through OpenCode's reply API:
  `POST /permission/:requestID/reply {reply: "once"|"reject"}` (`client.permission.reply`). On older
  servers it uses `POST /session/:id/permissions/:permissionID {response}` instead.
  OpenCode's own prompt stays up. Whichever answer comes first wins. A `permission.replied` event
  from the TUI cancels Orbi's pending ask.
- Details: [opencode/README.md](opencode/README.md).

## Cursor — https://cursor.com/docs/agent/hooks

- Config: `~/.cursor/hooks.json` → `{"version":1,"hooks":{"<event>":[{"command","timeout"?,"matcher"?}]}}`.
- `beforeShellExecution` / `beforeMCPExecution` output: `{"permission":"allow"|"deny"|"ask", "user_message", "agent_message"}`.
  **Why Orbi doesn't use it for approvals:** it fires *"before any shell command"*, so Orbi would ask
  about every command, not only the ones Cursor would prompt for. It also fails open:
  *"Crashes, timeouts, and non-zero exit codes other than `2` fail open by default … allows the
  action through"*, and *"invalid JSON … blocks the action"*. So Cursor gets status only.
  If you wire `orbi-hook ask --agent cursor` to it by hand, it always prints valid JSON, and when Orbi
  can't answer it prints `"ask"`. For `preToolUse`, which can't `ask`, it prints `deny`.
- Status: `afterShellExecution` (`command`), `afterFileEdit` (`file_path`), `afterMCPExecution`,
  `stop` (`status: completed|aborted|error`).

## Hermes (Nous Research) — https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks

- Shell hooks in `~/.hermes/config.yaml`: `hooks: {<event>: [{matcher?, command, timeout?}]}`.
  `command` *"runs via shlex.split, shell=False"*. stdin JSON: `hook_event_name, tool_name, tool_input,
  session_id, cwd, extra{…}`.
- Consent: *"Each unique (event, command) pair prompts the user for approval the first time Hermes sees it"*
  (or `--accept-hooks` / `HERMES_ACCEPT_HOOKS=1` / `hooks_auto_accept: true`). Orbi doesn't pre-approve.
- `pre_tool_call` can return `{"action":"block"}` or `{"action":"approve"}`, which *"escalates the call
  to the existing human-approval gate"* (it does not auto-allow). `pre_approval_request`
  is *"observer-only; they cannot veto or pre-answer the approval"*. So Orbi can't approve for Hermes,
  and Hermes gets status only. Orbi's face shows "needs your approval to run `…`" when Hermes asks.
- Status: `pre_llm_call`, `pre_tool_call`, `pre_approval_request` (`extra.command`, `extra.description`),
  `post_llm_call` (`extra.assistant_response`).

## Not integrated

Amp, Cline/Kilo, Aider and others: no hook mechanism was verified against official docs, so they get
nothing agent-specific. Anything that can run a command can use the [generic](generic/README.md) CLI.
