# Orbi × Hermes

**Status only.** Orbi → Settings → Integrations → Hermes → Connect adds shell
hooks to `~/.hermes/config.yaml` (backup first). It is a careful line edit of
the top-level `hooks:` block: your other hooks, `outbound:` targets and
comments are kept. Inline/flow-style `hooks:` or tab indentation → Orbi
refuses and changes nothing.

```yaml
hooks:
  pre_tool_call:
    - command: "/bin/sh -c '[ -x \"$0\" ] && exec \"$0\" \"$@\"; exit 0' '/Applications/Orbi.app/Contents/MacOS/orbi-hook' event --agent hermes"
      timeout: 5
  # same for pre_llm_call, pre_approval_request, post_llm_call
```

| Event | Face shows |
|---|---|
| `pre_llm_call` | "is thinking…" |
| `pre_tool_call` | "running `…`", "editing `…`", … |
| `pre_approval_request` | "needs your approval to run `sudo …`" |
| `post_llm_call` | "finished" |

Hermes asks once per hook before running it ("Each unique (event, command)
pair prompts the user for approval the first time"). Say yes, or run
`hermes --accept-hooks`. Non-interactive runs (gateway, cron) need
`HERMES_ACCEPT_HOOKS=1`.

**Why no approvals:** `pre_approval_request` hooks are "observer-only; they
cannot veto or pre-answer the approval". `pre_tool_call` can block or
*escalate* to Hermes' own approval gate, but never approve
(https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks).
Approve in Hermes itself. Orbi tells you when it's waiting.
