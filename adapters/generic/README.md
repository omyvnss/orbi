# Orbi × any agent or script

Anything that can run a command can use `orbi-hook`. The binary lives inside
the app: `/Applications/Orbi.app/Contents/MacOS/orbi-hook` (Settings → About
shows the exact path).

## Status

```sh
orbi-hook event --agent my-agent --kind working --summary "running migrations"
orbi-hook event --agent my-agent --kind done    --summary "deployed to staging"
orbi-hook event --agent my-agent --kind error   --summary "tests failed" --detail "3 failures in api/"
```

| flag | meaning |
|---|---|
| `--agent` | id shown on the face (`my-agent` → "My Agent") |
| `--kind` | `working` · `done` · `error` (default `working`) |
| `--summary` | the one-line preview |
| `--detail` | optional longer text shown when expanded |
| `--session` | optional id to tell parallel runs apart |

It always exits 0 within about 1 s and prints nothing, whether or not Orbi is running.

## Gate an action

```sh
if orbi-hook ask --agent deploy --tool Bash --input '{"command":"./deploy.sh prod"}' --cwd "$PWD"; then
  ./deploy.sh prod
fi
```

It prints `allow`, `deny` or `ask` and exits **0 / 1 / 2**. `ask` (exit 2)
means Orbi couldn't get an answer: it timed out, it's paused, it isn't
running, or the server couldn't be verified. Treat it as "ask the user
another way", never as yes. `--input` is JSON; tools named like Claude Code's
(`Bash`, `Edit`, `Write`, `WebFetch`) get the best one-line explanation.

## Agent hook JSON on stdin

`orbi-hook ask|event --agent <id>` reads that agent's own hook JSON on stdin and
prints the output that agent expects. Ids: `claude-code`, `codex`, `gemini`,
`cursor`, `hermes`, `opencode` (see `../README.md`). Any other id is read as
Claude Code hook JSON (`tool_name`, `tool_input`, `cwd`, `session_id`).
