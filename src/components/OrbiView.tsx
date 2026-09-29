import { forwardRef } from "react";
import type { OrbiSnapshot } from "../lib/tauri.js";
import type { OrbiState } from "../lib/faceState.js";
import { OrbiFace } from "./OrbiFace.js";
import { Preview } from "./Preview.js";

/** Pure rendering of face + preview. The live widget and #demo both use it. */
export const OrbiView = forwardRef<
  HTMLDivElement,
  {
    snapshot: OrbiSnapshot | null;
    face: OrbiState;
    expanded: boolean;
    gaze?: { x: number; y: number };
  }
>(function OrbiView({ snapshot, face, expanded, gaze }, ref) {
  return (
    <div ref={ref} className="orbi-widget" data-expanded={expanded || undefined}>
      <OrbiFace state={face} paused={snapshot?.paused ?? false} count={snapshot?.queue.length ?? 0} gaze={gaze} />
      <Preview snapshot={snapshot} face={face} expanded={expanded} />
    </div>
  );
});
