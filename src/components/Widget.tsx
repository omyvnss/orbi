import { useEffect, useRef, useState } from "react";
import { useCursorGaze } from "../hooks/useCursorGaze.js";
import { useOrbiMood } from "../hooks/useOrbiMood.js";
import { useOrbiState } from "../hooks/useOrbiState.js";
import { fitWidgetHeight, onNudge, onWidgetExpandedChange } from "../lib/tauri.js";
import { OrbiView } from "./OrbiView.js";

/** The widget window: live state, hover/⌃⌥O expansion, and window fitting. */
export default function Widget() {
  const { snapshot, face } = useOrbiState();
  const gaze = useCursorGaze(true, face === "asking");
  useOrbiMood({ calm: snapshot?.paused ?? false });

  const [pinned, setPinned] = useState(false);
  const [domHover, setDomHover] = useState(false);
  const expanded = pinned || gaze.hovering || domHover;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onWidgetExpandedChange((v) => setPinned(v)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // A held-back ⌃⌥A: a short shake says "that one changed — look again".
  const [nudge, setNudge] = useState(0);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onNudge(() => setNudge((n) => n + 1)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Rust owns window size; tell it how tall the content is. It clamps to the
  // collapsed minimum, so shrinking below that is harmless.
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = 0;
    const ro = new ResizeObserver(() => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h !== last) {
        last = h;
        void fitWidgetHeight(h);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return (
    <div
      key={nudge}
      className={nudge ? "orbi-nudge" : undefined}
      onMouseEnter={() => setDomHover(true)}
      onMouseLeave={() => setDomHover(false)}
    >
      <OrbiView ref={ref} snapshot={snapshot} face={face} expanded={expanded} />
    </div>
  );
}
