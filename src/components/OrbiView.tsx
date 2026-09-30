import { forwardRef } from "react";
import type { OrbiSnapshot } from "../lib/tauri.js";
import type { OrbiState } from "../lib/faceState.js";
import { OrbiFace } from "./OrbiFace.js";
import type { OrbiLayout } from "../lib/layout.js";
import { Preview } from "./Preview.js";

/** Pure rendering of face + preview. The live widget and #demo both use it. */
export const OrbiView = forwardRef<
  HTMLDivElement,
  {
    snapshot: OrbiSnapshot | null;
    face: OrbiState;
    expanded: boolean;
    gaze?: { x: number; y: number };
    layout?: OrbiLayout;
    notch?: { w: number; h: number } | null;
    tucked?: boolean;
  }
>(function OrbiView({ snapshot, face, expanded, gaze, layout, notch, tucked }, ref) {
  return (
    <div
      ref={ref}
      className="orbi-widget"
      data-expanded={expanded || undefined}
      style={layout ? { width: layout.width } : undefined}
    >
      <OrbiFace
        state={face}
        paused={snapshot?.paused ?? false}
        count={snapshot?.queue.length ?? 0}
        gaze={gaze}
        size={layout?.size}
        length={layout?.length}
        height={layout?.height}
        floating={(layout?.y ?? 0) > 2}
        notch={notch}
        tucked={tucked}
        sessions={snapshot?.sessions?.map((x) => x.state)}
      />
      <Preview snapshot={snapshot} face={face} expanded={expanded} />
    </div>
  );
});
