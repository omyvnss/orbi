import { useEffect, useId, useRef, useState } from "react";
import { setPaused, type AppInfo } from "../lib/tauri.js";
import type { SettingsApi } from "./Settings.js";
import { Group, PageHead, Row, Toggle } from "./ui.js";

const MIN = 10;
const MAX = 300;

function TimeoutRow({ api }: { api: SettingsApi }) {
  const saved = api.settings.timeoutSecs;
  const [v, setV] = useState(saved);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const id = useId();
  useEffect(() => setV(saved), [saved]);
  useEffect(() => () => clearTimeout(timer.current), []);

  const change = (n: number) => {
    setV(n);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void api.apply({ timeoutSecs: n }), 350);
  };
  const pct = ((v - MIN) / (MAX - MIN)) * 100;
  const label = v >= 60 ? `${Math.floor(v / 60)}m${v % 60 ? ` ${v % 60}s` : ""}` : `${v}s`;

  return (
    <Row
      label="Approval timeout"
      htmlFor={id}
      hint="If you don't answer in time, your agent asks you in its own terminal. Orbi never approves on its own."
      stack
    >
      <div className="s-slider">
        <input
          id={id}
          type="range"
          min={MIN}
          max={MAX}
          step={5}
          value={v}
          aria-valuetext={label}
          style={{ ["--pct" as string]: `${pct}%` }}
          onChange={(e) => change(Number(e.target.value))}
        />
        <output htmlFor={id} className="s-slider-val">
          {label}
        </output>
      </div>
    </Row>
  );
}

const HOTKEYS: [string, string][] = [
  ["⌃⌥A", "Allow the request at the front of the queue"],
  ["⌃⌥D", "Deny it"],
  ["⌃⌥O", "Expand or collapse the details"],
  ["⌃⌥J", "Jump to the agent's terminal or app"],
];

export function GeneralSection({ api, info }: { api: SettingsApi; info: AppInfo | null }) {
  const s = api.settings;
  return (
    <>
      <PageHead title="General" sub="Timing, startup and the keys that answer your agents." />

      <Group>
        <TimeoutRow api={api} />
        <Row label="Pause Orbi" hint="While paused, agents use their own prompts. Nothing is queued.">
          <Toggle
            label="Pause Orbi"
            checked={s.paused}
            onChange={(paused) => void api.apply({ paused }, () => setPaused(paused))}
          />
        </Row>
        <Row label="Launch at login" hint="Start Orbi quietly when you log in to this Mac.">
          <Toggle
            label="Launch at login"
            checked={s.launchAtLogin}
            onChange={(launchAtLogin) => void api.apply({ launchAtLogin })}
          />
        </Row>
      </Group>

      <Group title="Hotkeys" footer="Global — they work from any app, without switching to the terminal.">
        {HOTKEYS.map(([k, d]) => (
          <div className="s-hotkey" key={k}>
            <span>{d}</span>
            <kbd className="s-kbd" aria-label={k.replace("⌃", "Control ").replace("⌥", "Option ")}>
              {[...k].map((c, i) => (
                <span key={i}>{c}</span>
              ))}
            </kbd>
          </div>
        ))}
      </Group>

      <Group title="About">
        <Row label="Version">
          <span className="s-value">{info ? info.version : "—"}</span>
        </Row>
        <Row label="License">
          <span className="s-value">MIT</span>
        </Row>
      </Group>
    </>
  );
}
