import { useEffect, useState } from "react";
import { getState, onOrbiState, type OrbiSnapshot } from "../lib/tauri.js";
import { deriveFaceState, nextRelaxAt, type OrbiState } from "../lib/faceState.js";

/**
 * The Rust core's snapshot plus the face state derived from it. Pulls once on
 * mount, then follows `orbi-state`. A timer re-derives when done/error/working
 * are due to relax, since nothing is emitted at that moment.
 */
export function useOrbiState(): { snapshot: OrbiSnapshot | null; face: OrbiState } {
  const [snapshot, setSnapshot] = useState<OrbiSnapshot | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let gotEvent = false;

    void onOrbiState((s) => {
      gotEvent = true;
      setNow(Date.now());
      setSnapshot(s);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    void getState().then((s) => {
      // An event that raced ahead of the pull is newer; keep it.
      if (!cancelled && s && !gotEvent) {
        setNow(Date.now());
        setSnapshot(s);
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const at = nextRelaxAt(snapshot, now);
    if (at === null) return;
    const t = window.setTimeout(() => setNow(Date.now()), at - Date.now() + 20);
    return () => window.clearTimeout(t);
  }, [snapshot, now]);

  return { snapshot, face: deriveFaceState(snapshot, now) };
}

/** Re-render every `ms` while `active`; returns the current time. */
export function useNow(active: boolean, ms = 1_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const t = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(t);
  }, [active, ms]);
  return now;
}
