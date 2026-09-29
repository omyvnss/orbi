import { useEffect, useId, useState } from "react";
import { errorText, setApiKey, testExplain, type ExplainProvider, type ExplainTest } from "../lib/tauri.js";
import type { SettingsApi } from "./Settings.js";
import { Button, Group, Icon, ICONS, PageHead, Row } from "./ui.js";

const PROVIDERS: { id: ExplainProvider; label: string; baseUrl: string; model: string; needsKey: boolean }[] = [
  { id: "ollama", label: "Ollama (local) — recommended", baseUrl: "http://127.0.0.1:11434/v1", model: "llama3.2", needsKey: false },
  { id: "openrouter", label: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "", needsKey: true },
  { id: "openai", label: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "", needsKey: true },
  { id: "custom", label: "Custom (OpenAI-compatible)", baseUrl: "", model: "", needsKey: true },
];

function isLocal(url: string): boolean {
  try {
    const h = new URL(url).hostname;
    return h === "localhost" || h === "127.0.0.1" || h === "[::1]" || h === "::1";
  } catch {
    return false;
  }
}

/** Text field that keeps its own draft and commits on blur / Enter. */
function CommitField({
  id,
  value,
  onCommit,
  placeholder,
  mono,
}: {
  id: string;
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    const v = draft.trim();
    if (v !== value) onCommit(v);
  };
  return (
    <input
      id={id}
      className={`s-input${mono ? " s-input-mono" : ""}`}
      value={draft}
      placeholder={placeholder}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") setDraft(value);
      }}
    />
  );
}

function KeyField({ api, providerLabel }: { api: SettingsApi; providerLabel: string }) {
  const { hasKey } = api.settings.explain;
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState<"save" | "clear" | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const id = useId();

  const run = async (key: string | null) => {
    setBusy(key === null ? "clear" : "save");
    setErr(null);
    try {
      api.replace(await setApiKey(key));
      setDraft("");
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Row
      label="API key"
      htmlFor={id}
      stack
      hint={
        hasKey ? (
          <span className="s-saved">
            <Icon d={ICONS.key} size={12} />
            Saved in Keychain for {providerLabel}
          </span>
        ) : (
          "Stored in the macOS Keychain, never in a file."
        )
      }
    >
      <form
        className="s-keyrow"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) void run(draft.trim());
        }}
      >
        <input
          id={id}
          type="password"
          className="s-input s-input-mono"
          value={draft}
          placeholder={hasKey ? "••••••••••••  (replace)" : "Paste key"}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
        />
        <Button type="submit" variant="primary" disabled={!draft.trim()} busy={busy === "save"}>
          Save
        </Button>
        {hasKey && (
          <Button onClick={() => void run(null)} busy={busy === "clear"}>
            Clear
          </Button>
        )}
      </form>
      {err && (
        <p className="s-inline-error" role="alert">
          <Icon d={ICONS.warn} size={12} />
          {err}
        </p>
      )}
    </Row>
  );
}

function TestRow() {
  const [busy, setBusy] = useState(false);
  const [res, setRes] = useState<{ ok: ExplainTest } | { err: string } | null>(null);
  const run = async () => {
    setBusy(true);
    setRes(null);
    try {
      setRes({ ok: await testExplain() });
    } catch (e) {
      setRes({ err: errorText(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Row label="Try it" hint="Sends a sample request through this model and shows the line it writes." stack>
      <div className="s-test">
        <Button onClick={() => void run()} busy={busy}>
          {busy ? "Testing" : "Test"}
        </Button>
        <div className="s-test-out" aria-live="polite">
          {res && "ok" in res && (
            <>
              <span className="s-test-line">
                <strong>Claude Code</strong>{" "}
                {res.ok.line.split("`").map((p, i) => (i % 2 ? <code key={i}>{p}</code> : <span key={i}>{p}</span>))}
              </span>
              <span className="s-test-ms">{res.ok.ms} ms</span>
            </>
          )}
          {res && "err" in res && (
            <span className="s-inline-error" role="alert">
              <Icon d={ICONS.warn} size={12} />
              {res.err}
            </span>
          )}
        </div>
      </div>
    </Row>
  );
}

export function ExplainSection({ api }: { api: SettingsApi }) {
  const ex = api.settings.explain;
  const provider = PROVIDERS.find((p) => p.id === ex.provider) ?? PROVIDERS[0]!;
  const ids = { provider: useId(), url: useId(), model: useId() };
  const local = ex.provider === "ollama" || isLocal(ex.baseUrl);

  return (
    <>
      <PageHead
        title="Explanations"
        sub="How Orbi writes the one line under the face. The rules line always shows first, so a model can never slow down an approval."
      />

      <div
        className="s-modecards"
        role="radiogroup"
        aria-label="Explanation mode"
        onKeyDown={(e) => {
          if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) return;
          e.preventDefault();
          const next = ex.mode === "rules" ? "model" : "rules";
          const group = e.currentTarget;
          void api.apply({ explain: { mode: next } });
          requestAnimationFrame(() =>
            (group.querySelector(`[data-mode="${next}"]`) as HTMLElement | null)?.focus(),
          );
        }}
      >
        {(
          [
            { v: "rules", t: "Rules", d: "Instant, private, free. Built-in templates for commands, edits and URLs." },
            { v: "model", t: "Model", d: "Your own model rewrites the line more naturally. Bring your own key." },
          ] as const
        ).map((m) => (
          <button
            key={m.v}
            type="button"
            role="radio"
            aria-checked={ex.mode === m.v}
            tabIndex={ex.mode === m.v ? 0 : -1}
            data-mode={m.v}
            className="s-modecard"
            onClick={() => ex.mode !== m.v && void api.apply({ explain: { mode: m.v } })}
          >
            <span className="s-radio" aria-hidden="true" />
            <span className="s-modecard-t">{m.t}</span>
            <span className="s-modecard-d">{m.d}</span>
          </button>
        ))}
      </div>

      {ex.mode === "model" && (
        <>
          <Group title="Provider">
            <Row label="Provider" htmlFor={ids.provider}>
              <div className="s-select-wrap">
                <select
                  id={ids.provider}
                  className="s-select"
                  value={ex.provider}
                  onChange={(e) => {
                    const p = PROVIDERS.find((x) => x.id === e.target.value)!;
                    void api.apply({
                      explain: {
                        provider: p.id,
                        ...(p.baseUrl ? { baseUrl: p.baseUrl } : {}),
                        model: p.model,
                      },
                    });
                  }}
                >
                  {PROVIDERS.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </div>
            </Row>
            <Row label="Base URL" htmlFor={ids.url}>
              <CommitField
                id={ids.url}
                mono
                value={ex.baseUrl}
                placeholder="https://…/v1"
                onCommit={(v) => void api.apply({ explain: { baseUrl: v } })}
              />
            </Row>
            <Row label="Model" htmlFor={ids.model}>
              <CommitField
                id={ids.model}
                mono
                value={ex.model}
                placeholder={ex.provider === "ollama" ? "llama3.2" : "model id"}
                onCommit={(v) => void api.apply({ explain: { model: v } })}
              />
            </Row>
            {provider.needsKey && <KeyField api={api} providerLabel={provider.label} />}
            <TestRow />
          </Group>

          {local ? (
            <p className="s-note">
              <Icon d={ICONS.security} size={14} />
              Runs on this Mac. Nothing leaves your machine.
            </p>
          ) : (
            <p className="s-note s-note-warn">
              <Icon d={ICONS.warn} size={14} />
              Commands and file paths are sent to this provider to write the line. Use Ollama to keep them on this Mac.
            </p>
          )}
        </>
      )}

      {ex.mode === "rules" && (
        <Group title="Examples">
          <div className="s-examples">
            <p>
              <strong>Claude Code</strong> wants to run <code>npm test</code> in <code>orbi/</code>
            </p>
            <p>
              <strong>Codex</strong> wants to edit <code>src/app.ts</code> (+12 −3 lines)
            </p>
            <p>
              <strong>Claude Code</strong> wants to run <code>rm -rf dist</code>
              <span className="s-warn-tag">deletes files</span>
            </p>
          </div>
        </Group>
      )}
    </>
  );
}
