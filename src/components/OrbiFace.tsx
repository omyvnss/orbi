import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useId, type CSSProperties } from "react";
import { GAZE_TRAVEL, SPRING, SPRING_MOMENTUM, FADE_ONLY } from "../lib/motion.js";
import type { OrbiState } from "../lib/faceState.js";

export type { OrbiState };

/** Face drawing space. The pill is rendered 1:1 in points. */
const W = 150;
const H = 40;
const EYE_R = 6.5;
const EYE_DX = 13; // each eye's distance from centre
const CX = W / 2;
const CY = H / 2;

/** The orbit the working dot travels: an ellipse around both eyes. */
const ORBIT = { rx: 36, ry: 13 };
const ORBIT_PATH =
  `M${CX + ORBIT.rx},${CY} A${ORBIT.rx},${ORBIT.ry} 0 1,1 ${CX - ORBIT.rx},${CY} ` +
  `A${ORBIT.rx},${ORBIT.ry} 0 1,1 ${CX + ORBIT.rx},${CY}`;

const LABELS: Record<OrbiState, string> = {
  idle: "Orbi is idle",
  working: "Orbi: agent is working",
  asking: "Orbi: agent needs you",
  done: "Orbi: agent is done",
  error: "Orbi: agent hit an error",
};

type EyeShape = "orb" | "happy" | "cross" | "shut";

function eyeShape(state: OrbiState, paused: boolean): EyeShape {
  if (paused) return "shut";
  if (state === "done") return "happy";
  if (state === "error") return "cross";
  return "orb";
}

function Eye({ cx, shape, gradId, reduce }: { cx: number; shape: EyeShape; gradId: string; reduce: boolean }) {
  const t = reduce ? FADE_ONLY : SPRING;
  const enter = reduce ? { opacity: 0 } : { opacity: 0, scale: 0.4 };
  const common = {
    initial: enter,
    animate: { opacity: 1, scale: 1 },
    exit: enter,
    transition: t,
  };
  return (
    <g transform={`translate(${cx} ${CY})`}>
      <AnimatePresence initial={false}>
        {shape === "orb" && <motion.circle key="orb" r={EYE_R} fill={`url(#${gradId})`} {...common} />}
        {shape === "happy" && (
          <motion.path key="happy" className="orbi-eye-stroke" d="M-5.5 2 Q0 -5.5 5.5 2" {...common} />
        )}
        {shape === "cross" && (
          <motion.path key="cross" className="orbi-eye-stroke" d="M-3.6 -3.6 L3.6 3.6 M3.6 -3.6 L-3.6 3.6" {...common} />
        )}
        {shape === "shut" && <motion.path key="shut" className="orbi-eye-stroke" d="M-5 1 H5" {...common} />}
      </AnimatePresence>
    </g>
  );
}

/** The orbiting dot plus two fading trail beads. Static when motion is reduced. */
function Orbit({ reduce }: { reduce: boolean }) {
  if (reduce) {
    return (
      <g className="orbi-orbit">
        <ellipse className="orbi-orbit-track" cx={CX} cy={CY} rx={ORBIT.rx} ry={ORBIT.ry} />
        <circle className="orbi-orbit-dot" cx={CX + ORBIT.rx * 0.7} cy={CY - ORBIT.ry * 0.71} r={2.2} />
      </g>
    );
  }
  const beads = [
    { r: 2.3, lead: 0.14, o: 1 },
    { r: 1.6, lead: 0.07, o: 0.45 },
    { r: 1.1, lead: 0, o: 0.2 },
  ];
  return (
    <g className="orbi-orbit">
      <ellipse className="orbi-orbit-track" cx={CX} cy={CY} rx={ORBIT.rx} ry={ORBIT.ry} />
      {beads.map((b) => (
        <circle key={b.r} className="orbi-orbit-dot" r={b.r} opacity={b.o}>
          <animateMotion dur="1.8s" begin={`-${b.lead}s`} repeatCount="indefinite" path={ORBIT_PATH} />
        </circle>
      ))}
    </g>
  );
}

/**
 * Orbi's face: a dark pill flush with the top of the screen, two round orb
 * eyes, a dot that orbits while an agent works, and a warm glow when it
 * needs an answer.
 *
 * Eye position comes from `--orbi-gaze-x/y` (written to :root by
 * `useCursorGaze`); blink/squash from `--orbi-rig-eye-sx/sy` (`useOrbiMood`).
 * `gaze` overrides the live value for static previews.
 */
export function OrbiFace({
  state,
  paused = false,
  count = 0,
  gaze,
  layoutVars,
  className = "",
}: {
  state: OrbiState;
  paused?: boolean;
  /** Pending requests; a badge shows the ones behind the current one. */
  count?: number;
  /** −1..1 from centre. Only for static previews; the live widget omits it. */
  gaze?: { x: number; y: number };
  layoutVars?: CSSProperties;
  className?: string;
}) {
  const reduce = useReducedMotion() ?? false;
  const gradId = `orbi-eye-${useId().replace(/:/g, "")}`;
  const asking = state === "asking" && !paused;
  const shape = eyeShape(state, paused);
  const behind = Math.max(0, count - 1);

  return (
    <motion.div
      className={`orbi-face ${className}`}
      data-state={paused ? "paused" : state}
      role="img"
      aria-label={paused ? "Orbi is paused" : LABELS[state]}
      initial={false}
      // Widens a touch when it needs you, like the island making room.
      animate={{ width: asking ? W + 16 : W, opacity: paused ? 0.55 : 1 }}
      transition={reduce ? FADE_ONLY : SPRING_MOMENTUM}
      style={
        {
          height: H,
          ...layoutVars,
          ...(gaze
            ? {
                "--orbi-gaze-x": `${(gaze.x * GAZE_TRAVEL).toFixed(2)}px`,
                "--orbi-gaze-y": `${(gaze.y * GAZE_TRAVEL * 0.5).toFixed(2)}px`,
              }
            : null),
        } as CSSProperties
      }
    >
      <div className="orbi-face-glow" aria-hidden="true" />
      <div className="orbi-face-shell" aria-hidden="true">
        <div className="orbi-face-tint" />
        <svg className="orbi-face-art" viewBox={`0 0 ${W} ${H}`} width={W} height={H}>
          <defs>
            <radialGradient id={gradId} cx="38%" cy="32%" r="75%">
              <stop offset="0%" stopColor="var(--orbi-eye-hi)" />
              <stop offset="55%" stopColor="var(--orbi-eye)" />
              <stop offset="100%" stopColor="var(--orbi-eye-lo)" />
            </radialGradient>
          </defs>
          {state === "working" && !paused && <Orbit reduce={reduce} />}
          <g className="orbi-eyes">
            <motion.g
              className="orbi-eyes-pose"
              initial={false}
              animate={{ scale: asking ? 1.2 : 1 }}
              transition={reduce ? FADE_ONLY : SPRING}
            >
              <Eye cx={CX - EYE_DX} shape={shape} gradId={gradId} reduce={reduce} />
              <Eye cx={CX + EYE_DX} shape={shape} gradId={gradId} reduce={reduce} />
            </motion.g>
          </g>
        </svg>
        <AnimatePresence>
          {asking && behind > 0 && (
            <motion.span
              key="badge"
              className="orbi-badge"
              initial={{ opacity: 0, scale: 0.6 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.6 }}
              transition={reduce ? FADE_ONLY : SPRING}
            >
              +{behind}
            </motion.span>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}
