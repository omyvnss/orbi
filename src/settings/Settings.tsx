import { useCallback, useEffect, useRef, useState } from "react";
import {
  errorText,
  getAppInfo,
  getSettings,
  onSettingsChanged,
  updateSettings,
  type AppInfo,
  type Settings as SettingsT,
  type SettingsPatch,
} from "../lib/tauri.js";
import { Icon, ICONS } from "./ui.js";
import { AgentsSection } from "./Agents.js";
import { ExplainSection } from "./Explain.js";
import { GeneralSection } from "./General.js";
import { SecuritySection } from "./Security.js";
import "./settings.css";

export type SectionId = "agents" | "explain" | "general" | "security";

const SECTIONS: { id: SectionId; label: string; icon: string }[] = [
  { id: "agents", label: "Agents", icon: ICONS.agents },
  { id: "explain", label: "Explanations", icon: ICONS.explain },
  { id: "general", label: "General", icon: ICONS.general },
  { id: "security", label: "Security", icon: ICONS.security },
];

function sectionFromHash(): SectionId {
  const s = window.location.hash.split("/")[1];
  return SECTIONS.some((x) => x.id === s) ? (s as SectionId) : "agents";
}

/** Settings plus an optimistic `apply` that reverts on failure. */
export interface SettingsApi {
  settings: SettingsT;
  apply: (patch: SettingsPatch, run?: () => Promise<SettingsT>) => Promise<void>;
  replace: (s: SettingsT) => void;
}

function merge(s: SettingsT, p: SettingsPatch): SettingsT {
  return { ...s, ...p, explain: { ...s.explain, ...p.explain } } as SettingsT;
}

/** The little orb mark in the sidebar: the face's two round eyes. */
/** Orbi's logo (the Orbit mark, light version for the dark sidebar). Source: brand/orbi-mark-light.svg */
function Mark() {
  return (
    <svg viewBox="60 290 904 440" width="30" height="15" aria-hidden="true" className="s-mark">
      <defs>
        <linearGradient id="s-mark-ring" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#e2ad55" /><stop offset=".6" stopColor="#efc57a" /><stop offset="1" stopColor="#f6e2b8" />
        </linearGradient>
      </defs>
      <g transform="translate(512 512) rotate(-14)">
        <path d="M -440 0 A 440 138 0 0 1 440 0" fill="none" stroke="url(#s-mark-ring)" strokeWidth="30" strokeLinecap="round" opacity=".6" />
      </g>
      <rect x="142" y="336" width="740" height="352" rx="176" fill="#ecebe4" />
      <circle cx="382" cy="500" r="66" fill="#0b0b0c" />
      <circle cx="642" cy="500" r="66" fill="#0b0b0c" />
      <g transform="translate(512 512) rotate(-14)">
        <path d="M 440 0 A 440 138 0 0 1 -440 0" fill="none" stroke="url(#s-mark-ring)" strokeWidth="30" strokeLinecap="round" />
        <circle cx="286" cy="104" r="42" fill="#f6e2b8" />
      </g>
    </svg>
  );
}

export default function Settings() {
  const [section, setSection] = useState<SectionId>(sectionFromHash);
  const [settings, setSettings] = useState<SettingsT | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const navRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const mainRef = useRef<HTMLElement>(null);

  useEffect(() => {
    document.title = "Orbi Settings";
    getSettings().then(setSettings, (e) => setLoadError(errorText(e)));
    getAppInfo().then(setInfo, () => {});
    let cancelled = false;
    let off: (() => void) | undefined;
    void onSettingsChanged(setSettings).then((fn) => {
      if (cancelled) fn();
      else off = fn;
    });
    const onHash = () => setSection(sectionFromHash());
    window.addEventListener("hashchange", onHash);
    return () => {
      cancelled = true;
      off?.();
      window.removeEventListener("hashchange", onHash);
    };
  }, []);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const go = (id: SectionId) => {
    setSection(id);
    history.replaceState(null, "", `#settings/${id}`);
    mainRef.current?.scrollTo({ top: 0 });
  };

  const apply = useCallback<SettingsApi["apply"]>(async (patch, run) => {
    let before: SettingsT | null = null;
    setSettings((s) => {
      before = s;
      return s ? merge(s, patch) : s;
    });
    try {
      setSettings(await (run ? run() : updateSettings(patch)));
    } catch (e) {
      if (before) setSettings(before);
      setToast(`Couldn't save — ${errorText(e)}`);
    }
  }, []);

  const api: SettingsApi | null = settings ? { settings, apply, replace: setSettings } : null;

  return (
    <div className="s-root">
      <aside className="s-side">
        <div className="s-brand">
          <Mark />
          <span>Orbi</span>
        </div>
        <nav
          className="s-nav"
          role="tablist"
          aria-orientation="vertical"
          aria-label="Settings sections"
          onKeyDown={(e) => {
            const i = SECTIONS.findIndex((s) => s.id === section);
            let n = -1;
            if (e.key === "ArrowDown") n = (i + 1) % SECTIONS.length;
            else if (e.key === "ArrowUp") n = (i - 1 + SECTIONS.length) % SECTIONS.length;
            else if (e.key === "Home") n = 0;
            else if (e.key === "End") n = SECTIONS.length - 1;
            if (n < 0) return;
            e.preventDefault();
            go(SECTIONS[n]!.id);
            navRefs.current[n]?.focus();
          }}
        >
          {SECTIONS.map((s, i) => (
            <button
              key={s.id}
              ref={(el) => {
                navRefs.current[i] = el;
              }}
              type="button"
              role="tab"
              id={`tab-${s.id}`}
              aria-controls="s-panel"
              aria-selected={section === s.id}
              tabIndex={section === s.id ? 0 : -1}
              className="s-nav-item"
              onClick={() => go(s.id)}
            >
              <Icon d={s.icon} />
              {s.label}
            </button>
          ))}
        </nav>
        <div className="s-side-foot">
          {settings?.paused ? (
            <span className="s-pill s-pill-paused">Paused</span>
          ) : (
            <span className="s-pill s-pill-live">
              <span className="s-dot" aria-hidden="true" />
              Watching
            </span>
          )}
          {info && <span className="s-version">v{info.version}</span>}
        </div>
      </aside>

      <main className="s-main" ref={mainRef} id="s-panel" role="tabpanel" aria-labelledby={`tab-${section}`}>
        {loadError ? (
          <div className="s-empty">
            <p>Orbi couldn't load its settings.</p>
            <p className="s-muted">{loadError}</p>
          </div>
        ) : !api ? (
          <div className="s-loading" aria-busy="true" />
        ) : (
          <div className="s-page" key={section}>
            {section === "agents" && <AgentsSection info={info} />}
            {section === "explain" && <ExplainSection api={api} />}
            {section === "general" && <GeneralSection api={api} info={info} />}
            {section === "security" && <SecuritySection info={info} onError={setToast} />}
          </div>
        )}
      </main>

      <div className="s-toast-region" role="status" aria-live="polite">
        {toast && (
          <div className="s-toast">
            <Icon d={ICONS.warn} size={14} />
            {toast}
          </div>
        )}
      </div>
    </div>
  );
}
