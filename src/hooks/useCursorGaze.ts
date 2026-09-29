import { useCallback, useEffect, useRef, useState } from "react";
import { getGaze, onCursorGaze, type Gaze } from "../lib/tauri.js";
import { GAZE_TRAVEL } from "../lib/motion.js";

/**
 * Makes the face look at the cursor, like something alive rather than a
 * tracking dot:
 *  - saccadic pursuit (fast hop, then hold) instead of a linear slide;
 *  - habituation: `interest` falls with sameness and recovers on novelty;
 *  - emotions decay rather than reset;
 *  - eye contact on hover, then a deliberate glance away;
 *  - idle wander so it is never perfectly still.
 *
 * Eye position is written straight to `--orbi-gaze-x/y` on the document
 * element every frame; React only hears about hover/interest (throttled).
 */

/** Idle wander amplitude, as a fraction of the full range. */
const WANDER = 0.3;

/** How long the cursor must be still before the eyes wander on their own. */
const WANDER_AFTER_MS = 8_000;

/** Direction reversals above which the cursor counts as deliberately waggled. */
const IRRITATION_THRESHOLD = 2;

/** How long a held gaze lasts before the face looks away. */
const EYE_CONTACT_MS = 2_800;

export type GazeReaction = "wake" | "irritated" | "annoyed";

export interface CursorGaze {
  hovering: boolean;
  /** 0..1 — high when something novel is happening, low once habituated. */
  interest: number;
  /** A one-shot reaction event, or null. */
  reaction: GazeReaction | null;
}

/** One step of saccadic pursuit: fast start, decelerating arrival (~3-4 frames). */
function stepTowards(current: number, target: number, urgency: number): number {
  const d = target - current;
  if (Math.abs(d) < 0.002) return target;
  const rate = Math.min(0.62, 0.3 + Math.abs(d) * 0.35 * urgency);
  return current + d * rate;
}

export function useCursorGaze(enabled = true, lookDown = false): CursorGaze {
  const [out, setOut] = useState<CursorGaze>({
    hovering: false,
    interest: 0.5,
    reaction: null,
  });

  const targetRef = useRef({ x: 0, y: 0 });
  const currentRef = useRef({ x: 0, y: 0 });
  const hoverRef = useRef(false);
  const interestRef = useRef(0.5);
  const irritationRef = useRef(0);
  const lastMoveRef = useRef(Date.now());
  const lastReactionRef = useRef(0);
  const wasHoveringRef = useRef(false);
  const contactSinceRef = useRef(0);
  const wanderRef = useRef({ x: 0, y: 0 });
  const prevTargetRef = useRef({ x: 0, y: 0 });
  const lookDownRef = useRef(lookDown);
  lookDownRef.current = lookDown;

  const fire = useCallback((id: GazeReaction, gapMs = 3_000) => {
    const now = Date.now();
    if (now - lastReactionRef.current < gapMs) return;
    lastReactionRef.current = now;
    setOut((o) => ({ ...o, reaction: id }));
  }, []);

  // ---- input ----------------------------------------------------------
  useEffect(() => {
    if (!enabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void getGaze().then((g) => {
      if (!cancelled && g) targetRef.current = { x: g.x, y: g.y };
    });

    void onCursorGaze((g: Gaze) => {
      if (cancelled) return;

      const prev = prevTargetRef.current;
      const novelty = Math.hypot(g.x - prev.x, g.y - prev.y);
      prevTargetRef.current = { x: g.x, y: g.y };

      targetRef.current = { x: g.x, y: g.y };
      hoverRef.current = g.hovering;
      lastMoveRef.current = Date.now();

      interestRef.current = Math.min(1, Math.max(0.15, interestRef.current + novelty * 1.6 - 0.02));

      if (g.agitation >= IRRITATION_THRESHOLD) {
        irritationRef.current = Math.min(1, irritationRef.current + 0.18);
        if (irritationRef.current > 0.75) fire("annoyed", 5_000);
        else if (irritationRef.current > 0.35) fire("irritated", 4_000);
      }

      if (g.hovering && !wasHoveringRef.current) {
        contactSinceRef.current = Date.now();
        interestRef.current = 1;
        fire("wake", 2_500);
      }
      wasHoveringRef.current = g.hovering;
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enabled, fire]);

  // ---- the animation loop ---------------------------------------------
  useEffect(() => {
    if (!enabled) return;
    let raf = 0;
    let lastPublish = 0;
    let lastWrote = { x: NaN, y: NaN };

    const frame = () => {
      raf = requestAnimationFrame(frame);
      const now = Date.now();
      const sinceMove = now - lastMoveRef.current;

      irritationRef.current = Math.max(0, irritationRef.current - 0.004);
      interestRef.current = Math.max(0.15, interestRef.current - 0.0015);

      let tx = targetRef.current.x;
      let ty = targetRef.current.y;

      if (sinceMove > WANDER_AFTER_MS) {
        // Idle: hop to a new spot every couple of seconds, hold in between.
        if (now % 2_400 < 20) {
          wanderRef.current = {
            x: (Math.random() * 2 - 1) * WANDER,
            y: (Math.random() * 2 - 1) * WANDER * 0.4,
          };
        }
        tx = wanderRef.current.x;
        ty = wanderRef.current.y;
      } else if (
        hoverRef.current &&
        contactSinceRef.current &&
        now - contactSinceRef.current > EYE_CONTACT_MS
      ) {
        // Eye contact held long enough — glance away, then re-latch.
        tx *= -0.35;
        ty = 0.28;
        if (now - contactSinceRef.current > EYE_CONTACT_MS + 900) {
          contactSinceRef.current = now;
        }
      }

      // Irritation adds a periodic tremor (not white noise, which reads as a glitch).
      if (irritationRef.current > 0.2) {
        tx += Math.sin(now / 55) * 0.12 * irritationRef.current;
        ty += Math.sin(now / 41) * 0.06 * irritationRef.current;
      }

      if (lookDownRef.current) {
        tx *= 0.25;
        ty = 0.85;
      }

      const urgency = 0.75 + interestRef.current * 0.5;
      currentRef.current = {
        x: stepTowards(currentRef.current.x, tx, urgency),
        y: stepTowards(currentRef.current.y, ty, urgency),
      };

      const { x, y } = currentRef.current;
      if (Math.abs(x - lastWrote.x) > 0.002 || Math.abs(y - lastWrote.y) > 0.002) {
        lastWrote = { x, y };
        const rootStyle = document.documentElement.style;
        rootStyle.setProperty("--orbi-gaze-x", `${(x * GAZE_TRAVEL).toFixed(2)}px`);
        // Half the vertical reach: the face is wider than it is tall.
        rootStyle.setProperty("--orbi-gaze-y", `${(y * GAZE_TRAVEL * 0.5).toFixed(2)}px`);
      }

      if (now - lastPublish > 120) {
        lastPublish = now;
        setOut((o) =>
          o.hovering === hoverRef.current && Math.abs(o.interest - interestRef.current) < 0.05
            ? o
            : { ...o, hovering: hoverRef.current, interest: interestRef.current },
        );
      }
    };

    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [enabled]);

  return out;
}
