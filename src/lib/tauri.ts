/**
 * Thin boundary around the Tauri shell. Every call is a no-op when the UI is
 * loaded as a plain browser tab, so `vite dev` in a browser still renders.
 */

export function isDesktopShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T = void>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  if (!isDesktopShell()) return undefined;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!isDesktopShell()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => handler(e.payload));
}

/** Everything the face renders, owned by Rust. See docs/PROTOCOL.md. */
export interface OrbiRequest {
  id: number;
  agent: string;
  agentLabel: string;
  tool: string;
  /** One line; backticked spans are code, e.g. "wants to run `npm test` in orbi/". */
  line: string;
  /** Full command / path / diff size, may be multi-line. */
  detail: string;
  warnings: string[];
  cwd: string;
  /** ms since epoch */
  createdAt: number;
}

export interface OrbiActivity {
  agent: string;
  agentLabel: string;
  kind: "working" | "done" | "error";
  summary: string;
  detail: string | null;
  /** ms since epoch */
  at: number;
}

export interface OrbiSnapshot {
  paused: boolean;
  timeoutSecs: number;
  /** Oldest first; queue[0] is what ⌃⌥A / ⌃⌥D answer. */
  queue: OrbiRequest[];
  activity: OrbiActivity | null;
}

/** Pull once on mount — events emitted before the listener attached are lost. */
export async function getState(): Promise<OrbiSnapshot | null> {
  return (await call<OrbiSnapshot>("get_state")) ?? null;
}

export function onOrbiState(handler: (s: OrbiSnapshot) => void): Promise<() => void> {
  return on<OrbiSnapshot>("orbi-state", handler);
}

/** Where the cursor is relative to the face, pushed from Rust's gaze loop. */
export interface Gaze {
  /** -1..1 from the face's centre. */
  x: number;
  y: number;
  /** 0 calm, 3 = the cursor is being scribbled over the face on purpose. */
  agitation: number;
  /** The cursor is over the face right now. */
  hovering: boolean;
}

/** Pull the current gaze once on mount: Rust only emits on change and Tauri
 * does not buffer events, so a late listener would otherwise wait forever. */
export async function getGaze(): Promise<Gaze | null> {
  return (await call<Gaze>("get_gaze")) ?? null;
}

export function onCursorGaze(handler: (g: Gaze) => void): Promise<() => void> {
  return on<Gaze>("cursor-gaze", handler);
}

/** ⌃⌥A was held back because the request at the front just changed. */
export function onNudge(handler: () => void): Promise<() => void> {
  return on<null>("orbi-nudge", () => handler());
}

export function onWidgetExpandedChange(handler: (expanded: boolean) => void): Promise<() => void> {
  return on<boolean>("widget-expanded", handler);
}

export function onWidgetEditModeChange(handler: (editing: boolean) => void): Promise<() => void> {
  return on<boolean>("widget-edit-mode", handler);
}

export async function requestCollapseWidget(): Promise<void> {
  await call("collapse_widget");
}

export async function saveLayout(layout: unknown): Promise<void> {
  await call("save_layout", { layout });
}

/** The saved layout, or null when nothing has been customised. */
export async function loadLayout(): Promise<unknown | null> {
  return (await call<unknown | null>("load_layout")) ?? null;
}

export async function resetLayout(): Promise<void> {
  await call("reset_layout");
}

export async function enterWidgetEditMode(): Promise<void> {
  await call("enter_widget_edit_mode");
}

export async function exitWidgetEditMode(): Promise<void> {
  await call("exit_widget_edit_mode");
}

/** Size the widget window to a laid-out height. The window is not resizable,
 * so a JS-side resize is a silent no-op on macOS; Rust does it instead. */
export async function fitWidgetHeight(height: number): Promise<void> {
  await call("fit_widget_height", { height });
}

export async function quitApp(): Promise<void> {
  await call("quit_app");
}

export async function getCursorPosition(): Promise<[number, number]> {
  return (await call<[number, number]>("get_cursor_position")) ?? [0, 0];
}

/* ---- settings window (docs/APP.md) ------------------------------------- */

export type ExplainMode = "rules" | "model";
export type ExplainProvider = "ollama" | "openrouter" | "openai" | "custom";

export interface Settings {
  timeoutSecs: number;
  launchAtLogin: boolean;
  paused: boolean;
  explain: {
    mode: ExplainMode;
    provider: ExplainProvider;
    baseUrl: string;
    model: string;
    hasKey: boolean;
  };
}

/** `explain` may be partial on update. */
export type SettingsPatch = Partial<Omit<Settings, "explain">> & {
  explain?: Partial<Omit<Settings["explain"], "hasKey">>;
};

export interface Integration {
  id: string;
  name: string;
  detected: boolean;
  connected: boolean;
  approvals: boolean;
  status: boolean;
  configPath: string;
  note: string;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  hookPath: string;
  port: number | null;
}

export interface ExplainTest {
  line: string;
  ms: number;
}

/* In a plain browser tab (#settings screenshots, design work) every command
 * resolves against this in-memory mock so the window is fully interactive. */
const mock = {
  settings: {
    timeoutSecs: 45,
    launchAtLogin: false,
    paused: false,
    explain: {
      mode: "rules",
      provider: "ollama",
      baseUrl: "http://127.0.0.1:11434/v1",
      model: "llama3.2",
      hasKey: false,
    },
  } as Settings,
  integrations: [
    { id: "claude-code", name: "Claude Code", detected: true, connected: true, approvals: true, status: true,
      configPath: "~/.claude/settings.json", note: "Approve and deny tool calls with ⌃⌥A / ⌃⌥D" },
    { id: "codex", name: "Codex", detected: true, connected: false, approvals: false, status: true,
      configPath: "~/.codex/config.toml", note: "Status only — Codex has no approval hook" },
    { id: "gemini", name: "Gemini CLI", detected: true, connected: false, approvals: true, status: true,
      configPath: "~/.gemini/settings.json", note: "BeforeTool hook answers approvals" },
    { id: "opencode", name: "OpenCode", detected: false, connected: false, approvals: true, status: true,
      configPath: "~/.config/opencode/plugin/orbi.js", note: "Installs a small plugin file" },
    { id: "cursor", name: "Cursor", detected: true, connected: true, approvals: true, status: true,
      configPath: "~/.cursor/hooks.json", note: "Agent shell and edit hooks" },
    { id: "hermes", name: "Hermes", detected: false, connected: false, approvals: false, status: true,
      configPath: "~/.hermes/config.yaml", note: "Status only — shows working and done" },
  ] as Integration[],
  info: {
    version: "0.1.0",
    dataDir: "~/Library/Application Support/Orbi",
    hookPath: "/Applications/Orbi.app/Contents/MacOS/orbi-hook",
    port: 47821,
  } as AppInfo,
};
const mockListeners = new Set<(s: Settings) => void>();

function wait(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function mockSettingsUpdate(next: Settings): Promise<Settings> {
  mock.settings = next;
  mockListeners.forEach((fn) => fn(next));
  return structuredClone(next);
}

async function cmd<T>(name: string, args: Record<string, unknown> | undefined, fallback: () => Promise<T>): Promise<T> {
  if (!isDesktopShell()) return fallback();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(name, args);
}

export function getSettings(): Promise<Settings> {
  return cmd("get_settings", undefined, async () => structuredClone(mock.settings));
}

export function updateSettings(patch: SettingsPatch): Promise<Settings> {
  return cmd("update_settings", { patch }, () =>
    mockSettingsUpdate({
      ...mock.settings,
      ...patch,
      explain: { ...mock.settings.explain, ...patch.explain },
    } as Settings),
  );
}

export function setPaused(paused: boolean): Promise<Settings> {
  return cmd("set_paused", { paused }, () => mockSettingsUpdate({ ...mock.settings, paused }));
}

/** Stores the key for the current provider in the Keychain; `null` clears it. */
export function setApiKey(key: string | null): Promise<Settings> {
  return cmd("set_api_key", { key }, async () => {
    await wait(300);
    return mockSettingsUpdate({ ...mock.settings, explain: { ...mock.settings.explain, hasKey: key !== null } });
  });
}

/** Rejects with a human-readable error string. */
export function testExplain(): Promise<ExplainTest> {
  return cmd("test_explain", undefined, async () => {
    await wait(700);
    const s = mock.settings.explain;
    if (s.mode === "model" && s.provider !== "ollama" && !s.hasKey) throw "No API key saved for this provider";
    return { line: "wants to delete the build folder `dist/` inside orbi/", ms: 612 };
  });
}

export function listIntegrations(): Promise<Integration[]> {
  return cmd("list_integrations", undefined, async () => structuredClone(mock.integrations));
}

function mockToggle(id: string, connected: boolean): () => Promise<Integration> {
  return async () => {
    await wait(650);
    const it = mock.integrations.find((i) => i.id === id);
    if (!it) throw `Unknown agent: ${id}`;
    if (!it.detected) throw `${it.name} is not installed`;
    it.connected = connected;
    return { ...it };
  };
}

export function connectIntegration(id: string): Promise<Integration> {
  return cmd("connect_integration", { id }, mockToggle(id, true));
}

export function disconnectIntegration(id: string): Promise<Integration> {
  return cmd("disconnect_integration", { id }, mockToggle(id, false));
}

export function regenerateToken(): Promise<null> {
  return cmd("regenerate_token", undefined, async () => {
    await wait(400);
    return null;
  });
}

export function getAppInfo(): Promise<AppInfo> {
  return cmd("get_app_info", undefined, async () => ({ ...mock.info }));
}

export async function openSettings(): Promise<void> {
  if (!isDesktopShell()) {
    window.open(`${location.pathname}#settings`, "_blank");
    return;
  }
  await call("open_settings");
}

export function onSettingsChanged(handler: (s: Settings) => void): Promise<() => void> {
  if (!isDesktopShell()) {
    mockListeners.add(handler);
    return Promise.resolve(() => mockListeners.delete(handler));
  }
  return on<Settings>("settings-changed", handler);
}

/** Tauri rejects with a plain string; normalise anything to a message. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong";
}
