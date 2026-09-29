# Orbi × Cursor

**Status only.** Orbi → Settings → Integrations → Cursor → Connect adds Orbi
to `~/.cursor/hooks.json` (backup first, other hooks kept). Cursor reloads
the file automatically.

| Event | Face shows |
|---|---|
| `afterShellExecution` | "ran `npm test`" |
| `afterFileEdit` | "edited `src/a.ts`" |
| `afterMCPExecution` | "using `create_issue` from linear" |
| `stop` | "finished" / "stopped" / "stopped with an error" |

**Why no approvals:** Cursor's `beforeShellExecution` / `beforeMCPExecution`
hooks do take `{"permission":"allow"|"deny"|"ask"}`, but they fire before
*every* command, not only the ones Cursor would ask about. Orbi would nag on
every `ls`. They also fail open: "Crashes, timeouts, and non-zero exit codes
other than `2` fail open by default … allows the action through"
(https://cursor.com/docs/agent/hooks). That clashes with Orbi's rule that it
never lets anything through by default.

If you still want Orbi to gate Cursor's shell commands, add this by hand:

```json
"beforeShellExecution": [{ "command": "'/Applications/Orbi.app/Contents/MacOS/orbi-hook' ask --agent cursor", "timeout": 120 }]
```

`orbi-hook` always prints valid JSON there, and prints `"ask"` when Orbi
can't answer. Expect Cursor to prompt for every command while Orbi is quit.
