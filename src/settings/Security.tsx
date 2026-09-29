import { useState } from "react";
import { errorText, regenerateToken, type AppInfo } from "../lib/tauri.js";
import { Button, Confirm, CopyButton, Group, Icon, ICONS, PageHead, Row } from "./ui.js";

const GUARANTEES: [string, string][] = [
  ["Never auto-approves", "Allow only ever comes from you pressing ⌃⌥A."],
  ["Silence means your agent asks", "No answer before the timeout? The agent falls back to its own prompt — never to allow."],
  ["Off means off", "If Orbi isn't running, agents behave exactly as if it were never installed."],
  ["Local only", "The server listens on 127.0.0.1, needs a secret token, and proves itself to every hook before its answer is trusted."],
  ["Screen shares", "Orbi asks macOS to keep the face out of screen shares, but recent macOS versions can still capture it. Quit Orbi before sharing a screen that might show a sensitive command."],
];

export function SecuritySection({ info, onError }: { info: AppInfo | null; onError: (m: string) => void }) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  const regen = async () => {
    setBusy(true);
    try {
      await regenerateToken();
      setDone(true);
      setConfirm(false);
    } catch (e) {
      onError(errorText(e));
      setConfirm(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <PageHead title="Security" sub="Orbi can approve shell commands, so it's built like something that can." />

      <Group title="What Orbi guarantees">
        <ul className="s-guarantees">
          {GUARANTEES.map(([t, d]) => (
            <li key={t}>
              <span className="s-check" aria-hidden="true">
                <Icon d={ICONS.check} size={12} />
              </span>
              <div>
                <strong>{t}</strong>
                <span>{d}</span>
              </div>
            </li>
          ))}
        </ul>
      </Group>

      <Group title="Local data">
        <Row label="Data folder" hint="Token, port and settings. Readable only by you." stack>
          <div className="s-pathfield">
            <code>{info?.dataDir ?? "—"}</code>
            {info && <CopyButton text={info.dataDir} label="Copy path" />}
          </div>
        </Row>
        <Row label="Port">
          <span className="s-value s-mono">{info?.port != null ? `127.0.0.1:${info.port}` : "Not listening"}</span>
        </Row>
        <Row
          label="Access token"
          hint={
            done
              ? "New token in place. Connected agents picked it up automatically."
              : "Rotate it if you think another program has read it. Connected agents keep working — no reconnecting."
          }
        >
          <Button variant="danger" onClick={() => setConfirm(true)}>
            <Icon d={ICONS.refresh} size={14} />
            Regenerate
          </Button>
        </Row>
      </Group>

      <Confirm
        open={confirm}
        busy={busy}
        title="Regenerate the access token?"
        body={
          <>
            <p>The old token stops working immediately.</p>
            <p>Connected agents read the new token on their next request, so there's nothing to reconnect.</p>
          </>
        }
        confirmLabel="Regenerate"
        onConfirm={() => void regen()}
        onCancel={() => setConfirm(false)}
      />
    </>
  );
}
