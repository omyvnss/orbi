import { useState } from "react";
import { useOrbiMood } from "../hooks/useOrbiMood.js";
import type { OrbiRequest, OrbiSnapshot } from "../lib/tauri.js";
import type { OrbiState } from "../lib/faceState.js";
import { OrbiView } from "./OrbiView.js";

/** index.html#demo — every state side by side with mock snapshots, for visual QA. */

function buildCases() {
  const now = Date.now();

  const rmReq: OrbiRequest = {
    id: 1,
    agent: "claude-code",
    agentLabel: "Claude Code",
    tool: "Bash",
    line: "wants to run `rm -rf dist` in orbi/",
    detail: "rm -rf dist",
    warnings: ["deletes files"],
    cwd: "~/code/orbi",
    createdAt: now - 12_000,
  };

  const queue3: OrbiRequest[] = [
    rmReq,
    {
      id: 2,
      agent: "codex",
      agentLabel: "Codex",
      tool: "Edit",
      line: "wants to edit `src/app.ts` (+12 −3 lines)",
      detail: "src/app.ts  +12 −3",
      warnings: [],
      cwd: "~/code/web",
      createdAt: now - 6_000,
    },
    {
      id: 3,
      agent: "claude-code",
      agentLabel: "Claude Code",
      tool: "WebFetch",
      line: "wants to open `docs.rs`",
      detail: "https://docs.rs/serde/latest/serde/",
      warnings: [],
      cwd: "~/code/orbi",
      createdAt: now - 2_000,
    },
  ];

  const base: OrbiSnapshot = { paused: false, timeoutSecs: 45, queue: [], activity: null };

  const activity = (kind: "working" | "done" | "error", summary: string, detail: string | null = null) => ({
    ...base,
    activity: { agent: "claude-code", agentLabel: "Claude Code", kind, summary, detail, at: now },
  });

  const cases: Array<{
    title: string;
    face: OrbiState;
    snap: OrbiSnapshot;
    expanded?: boolean;
    gaze?: { x: number; y: number };
    tucked?: boolean;
  }> = [
    { title: "idle", face: "idle", snap: base, gaze: { x: -0.4, y: 0.1 } },
    { title: "idle · tucked into the notch", face: "idle", snap: base, tucked: true },
    { title: "working", face: "working", snap: activity("working", "running `pnpm test`") },
    { title: "asking · queue of 3", face: "asking", snap: { ...base, queue: queue3 }, gaze: { x: 0, y: 0.85 } },
    { title: "done", face: "done", snap: activity("done", "finished · 14 files changed") },
    { title: "error", face: "error", snap: activity("error", "hook failed: `tsc` exited 2") },
    { title: "paused", face: "idle", snap: { ...base, paused: true } },
    {
      title: "asking · expanded (hover / ⌃⌥O)",
      face: "asking",
      snap: {
        ...base,
        queue: [{ ...rmReq, detail: "rm -rf dist && rm -rf node_modules/.cache\n# clears the build output before a clean rebuild" }, ...queue3.slice(1)],
      },
      expanded: true,
      gaze: { x: 0, y: 0.85 },
    },
    {
      title: "question · answer in the terminal",
      face: "asking",
      snap: {
        ...base,
        sessions: [
          {
            key: "claude-code:s1", agent: "claude-code", agentLabel: "Claude Code", project: "orbi", state: "question",
            summary: "has a question for you", subagents: 0, updatedAt: now,
            question: { text: "Which database should the queue use?", options: ["Postgres", "SQLite", "Keep in memory"], more: 1 },
          },
        ],
      },
      gaze: { x: 0, y: 0.85 },
    },
    {
      title: "3 sessions · expanded",
      face: "working",
      expanded: true,
      snap: {
        ...activity("working", "running `pnpm test`"),
        sessions: [
          { key: "a", agent: "claude-code", agentLabel: "Claude Code", project: "orbi", state: "working", summary: "running `pnpm test`", subagents: 2, question: null, updatedAt: now },
          { key: "b", agent: "codex", agentLabel: "Codex", project: "web", state: "done", summary: "finished", subagents: 0, question: null, updatedAt: now - 60_000 },
          { key: "c", agent: "opencode", agentLabel: "OpenCode", project: "api", state: "ready", summary: "", subagents: 0, question: null, updatedAt: now - 120_000 },
        ],
      },
    },
    {
      title: "asking · single, no risk",
      face: "asking",
      snap: {
        ...base,
        queue: [
          {
            ...rmReq,
            line: "wants to run `npm test` in orbi/",
            detail: "npm test",
            warnings: [],
          },
        ],
      },
      gaze: { x: 0, y: 0.85 },
    },
  ];
  return cases;
}

export default function Demo() {
  useOrbiMood();
  // Built on mount so the mock timeouts are fresh on every load.
  const [CASES] = useState(buildCases);
  // #demo?notch simulates a 14" MacBook's camera housing (185 × 32 pt) drawn
  // over the tile, so you can see exactly what the notch hides.
  const notch = window.location.hash.includes("notch") ? { w: 185, h: 32 } : null;
  return (
    <main className="demo">
      <header className="demo-head">
        <h1>Orbi · states</h1>
        <p>Each tile is the 320pt widget window, flush under a mock menu bar.</p>
      </header>
      <div className="demo-grid">
        {CASES.map((c) => (
          <figure key={c.title} className="demo-cell" data-tall={c.expanded || undefined}>
            <div className="demo-screen">
              <div className="demo-menubar" />
              <div className="demo-window">
                <OrbiView
                  snapshot={c.snap}
                  face={c.face}
                  expanded={c.expanded ?? false}
                  gaze={c.gaze ?? { x: 0, y: 0 }}
                  notch={notch}
                  tucked={c.tucked ?? false}
                />
              </div>
              {notch && <div className="demo-notch" style={{ width: notch.w, height: notch.h }} />}
            </div>
            <figcaption>{c.title}</figcaption>
          </figure>
        ))}
      </div>
    </main>
  );
}
