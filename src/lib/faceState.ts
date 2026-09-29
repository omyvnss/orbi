import type { OrbiSnapshot } from "./tauri.js";

export type OrbiState = "idle" | "working" | "asking" | "done" | "error";

/** done / error hold this long after the event, then relax to idle. */
export const SETTLE_MS = 4_000;
/** working without a fresh event for this long relaxes to idle. */
export const WORKING_STALE_MS = 60_000;

function expiry(s: OrbiSnapshot | null): number | null {
  const a = s?.activity;
  if (!a) return null;
  return a.at + (a.kind === "working" ? WORKING_STALE_MS : SETTLE_MS);
}

/** queue non-empty → asking; else a fresh activity's kind; else idle. */
export function deriveFaceState(s: OrbiSnapshot | null, now: number): OrbiState {
  if (!s) return "idle";
  if (s.queue.length > 0) return "asking";
  const until = expiry(s);
  if (s.activity && until !== null && now < until) return s.activity.kind;
  return "idle";
}

/** When the derived state will next change on its own, or null. */
export function nextRelaxAt(s: OrbiSnapshot | null, now: number): number | null {
  if (!s || s.queue.length > 0) return null;
  const until = expiry(s);
  return until !== null && until > now ? until : null;
}
