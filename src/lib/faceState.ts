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

/** An unanswered question keeps Orbi's attention this long at most. */
export const QUESTION_STALE_MS = 10 * 60_000;

/** The newest session still waiting on an answer to its question. */
export function openQuestion(s: OrbiSnapshot | null, now: number) {
  return s?.sessions?.find((x) => x.state === "question" && x.question && now - x.updatedAt < QUESTION_STALE_MS) ?? null;
}

/** queue non-empty → asking; an open question → asking; else a fresh activity's kind; else idle. */
export function deriveFaceState(s: OrbiSnapshot | null, now: number): OrbiState {
  if (!s) return "idle";
  if (s.queue.length > 0) return "asking";
  if (openQuestion(s, now)) return "asking";
  const until = expiry(s);
  if (s.activity && until !== null && now < until) return s.activity.kind;
  return "idle";
}

/** When the derived state will next change on its own, or null. */
export function nextRelaxAt(s: OrbiSnapshot | null, now: number): number | null {
  if (!s || s.queue.length > 0) return null;
  const q = openQuestion(s, now);
  if (q) return q.updatedAt + QUESTION_STALE_MS;
  const until = expiry(s);
  return until !== null && until > now ? until : null;
}
