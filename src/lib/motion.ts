import type { Transition } from "framer-motion";

/**
 * One motion vocabulary for the whole app. Values follow Apple's
 * "Designing Fluid Interfaces": critically damped by default, bounce only
 * after a gesture that carried real momentum. Animate transform/opacity only.
 * Handle reduced motion at the call site with framer's `useReducedMotion()`.
 */

/** Default UI spring — critically damped, no overshoot. */
export const SPRING: Transition = { type: "spring", bounce: 0, duration: 0.34 };

/** For small elements that should feel immediate. */
export const SPRING_SNAP: Transition = { type: "spring", bounce: 0, duration: 0.22 };

/** The only place bounce is allowed. */
export const SPRING_MOMENTUM: Transition = { type: "spring", bounce: 0.2, duration: 0.4 };

/** Reduced motion: a short cross-fade instead of movement. */
export const FADE_ONLY: Transition = { duration: 0.16 };

/**
 * How far the eyes travel at full gaze, in the face SVG's viewBox units.
 * Shared because `useCursorGaze` writes the CSS variable and `OrbiFace`
 * renders from it.
 */
export const GAZE_TRAVEL = 7;
