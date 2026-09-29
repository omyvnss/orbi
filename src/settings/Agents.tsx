import { useCallback, useEffect, useState } from "react";
import {
  connectIntegration,
  disconnectIntegration,
  errorText,
  listIntegrations,
  type AppInfo,
  type Integration,
} from "../lib/tauri.js";
import { Button, CopyButton, Icon, ICONS, PageHead } from "./ui.js";

/** Two-letter monogram; no third-party logos are bundled. */
function monogram(name: string): string {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length > 1) return (words[0]![0]! + words[1]![0]!).toUpperCase();
  return name.slice(0, 1).toUpperCase();
}

function AgentCard({
  it,
  busy,
  error,
  onToggle,
}: {
  it: Integration;
  busy: boolean;
  error?: string;
  onToggle: () => void;
}) {
  return (
    <li className="s-agent" data-connected={it.connected || undefined} data-missing={!it.detected || undefined}>
      <span className="s-agent-mono" aria-hidden="true">
        {monogram(it.name)}
      </span>
      <div className="s-agent-body">
        <div className="s-agent-top">
          <span className="s-agent-name">{it.name}</span>
          {it.connected ? (
            <span className="s-badge s-badge-on">
              <span className="s-dot" aria-hidden="true" />
              Connected
            </span>
          ) : it.detected ? (
            <span className="s-badge">Detected</span>
          ) : (
            <span className="s-badge s-badge-off">Not installed</span>
          )}
        </div>
        <div className="s-chips">
          {it.approvals ? (
            <span className="s-chip s-chip-amber">Approvals</span>
          ) : (
            <span className="s-chip">Status only</span>
          )}
          <code className="s-path" title={it.configPath}>
            {it.configPath}
          </code>
        </div>
        {it.note && <p className="s-agent-note">{it.note}</p>}
        {error && (
          <p className="s-inline-error" role="alert">
            <Icon d={ICONS.warn} size={12} />
            {error}
          </p>
        )}
      </div>
      <div className="s-agent-action">
        {!it.detected ? (
          <Button disabled aria-label={`${it.name} is not installed`}>
            Not installed
          </Button>
        ) : it.connected ? (
          <Button onClick={onToggle} busy={busy} aria-label={`Disconnect ${it.name}`}>
            {busy ? "Disconnecting" : "Disconnect"}
          </Button>
        ) : (
          <Button variant="primary" onClick={onToggle} busy={busy} aria-label={`Connect ${it.name}`}>
            {busy ? "Connecting" : "Connect"}
          </Button>
        )}
      </div>
    </li>
  );
}

function CustomCard({ hookPath }: { hookPath: string }) {
  const snippet = [
    `ORBI="${hookPath}"`,
    ``,
    `# show status on the face (working | done | error)`,
    `"$ORBI" event --agent my-bot --kind working --summary "running tests"`,
    ``,
    `# wait for ⌃⌥A / ⌃⌥D — stdin: tool_name, tool_input, cwd`,
    `echo '{"tool_name":"Bash","tool_input":{"command":"make deploy"}}' \\`,
    `  | "$ORBI" ask --agent my-bot`,
  ].join("\n");
  return (
    <section className="s-custom" aria-labelledby="s-custom-title">
      <div className="s-custom-head">
        <span className="s-agent-mono s-agent-mono-ghost" aria-hidden="true">
          <Icon d={ICONS.terminal} size={15} />
        </span>
        <div>
          <h3 id="s-custom-title" className="s-agent-name">
            Custom / any script
          </h3>
          <p className="s-agent-note">
            Anything that can run a command can drive Orbi. <code className="s-code-inline">event</code> updates the
            face; <code className="s-code-inline">ask</code> waits for ⌃⌥A / ⌃⌥D and never answers on its own.
          </p>
        </div>
      </div>
      <div className="s-snippet">
        <pre>{snippet}</pre>
        <CopyButton text={snippet} label="Copy snippet" />
      </div>
    </section>
  );
}

export function AgentsSection({ info }: { info: AppInfo | null }) {
  const [list, setList] = useState<Integration[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const refresh = useCallback(() => {
    listIntegrations().then(
      (l) => {
        setList(l);
        setListError(null);
      },
      (e) => setListError(errorText(e)),
    );
  }, []);

  useEffect(() => {
    refresh();
    // An agent may have been installed while the window was in the background.
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  const toggle = async (it: Integration) => {
    setBusy((b) => ({ ...b, [it.id]: true }));
    setErrors(({ [it.id]: _, ...rest }) => rest);
    try {
      const next = await (it.connected ? disconnectIntegration(it.id) : connectIntegration(it.id));
      setList((l) => l?.map((x) => (x.id === next.id ? next : x)) ?? l);
    } catch (e) {
      setErrors((m) => ({ ...m, [it.id]: errorText(e) }));
    } finally {
      setBusy(({ [it.id]: _, ...rest }) => rest);
    }
  };

  const connected = list?.filter((i) => i.connected).length ?? 0;
  const sorted = list
    ? [...list].sort((a, b) => Number(b.detected) - Number(a.detected))
    : null;

  return (
    <>
      <PageHead
        title="Agents"
        sub="Orbi adds a small hook to each agent's own config file. Disconnect removes it and leaves everything else untouched."
      />

      {list && connected === 0 && (
        <div className="s-banner">
          <span className="s-banner-orb" aria-hidden="true" />
          <div>
            <strong>Connect an agent to get started</strong>
            <p>Pick one below. Its next permission request shows up at the top of your screen.</p>
          </div>
        </div>
      )}

      {listError && (
        <p className="s-inline-error" role="alert">
          <Icon d={ICONS.warn} size={12} />
          {listError}
        </p>
      )}

      {sorted ? (
        <ul className="s-agents" aria-label="Agents">
          {sorted.map((it) => (
            <AgentCard key={it.id} it={it} busy={!!busy[it.id]} error={errors[it.id]} onToggle={() => void toggle(it)} />
          ))}
        </ul>
      ) : (
        !listError && (
          <ul className="s-agents" aria-hidden="true">
            {[0, 1, 2].map((i) => (
              <li key={i} className="s-agent s-skeleton" />
            ))}
          </ul>
        )
      )}

      {info && <CustomCard hookPath={info.hookPath} />}
    </>
  );
}
