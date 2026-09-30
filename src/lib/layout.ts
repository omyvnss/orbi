import type { CSSProperties } from "react";

/**
 * The user's saved widget layout: face width, window position, and small
 * per-part offsets applied as CSS custom properties (`--orbi-<part>-dx` etc).
 * Persisted by Rust via `save_layout` / `load_layout`.
 */

export const PART_NAMES = ["eyes", "orbit", "glow"] as const;
export type PartName = (typeof PART_NAMES)[number];

export interface PartAdjust {
  /** Offset in the SVG's viewBox units, so it is render-size independent. */
  dx: number;
  dy: number;
  /** Multiplier, 1 = untouched. */
  s: number;
}

export interface OrbiLayout {
  /** Widget window width in logical points (derived from size × length). */
  width: number;
  /** Overall size of the face, 1 = default. Set in Settings → Appearance. */
  size: number;
  /** How wide the pill stretches sideways, 1 = default. */
  length: number;
  /** How far the pill hangs down below the camera, 1 = default. */
  height: number;
  /** Shrink back into the notch when no agent is doing anything. */
  tuck: boolean;
  /** Soft sounds when an agent needs you or finishes. */
  sound: boolean;
  /** 0..1 */
  volume: number;
  /** Widget window position in logical points, or null for default top-centre. */
  x: number | null;
  y: number | null;
  parts: Record<PartName, PartAdjust>;
}

const NEUTRAL: PartAdjust = { dx: 0, dy: 0, s: 1 };

/** Matches the Rust side's collapsed window width. */
export const DEFAULT_WIDTH = 320;

/** The face's drawing size in points (see OrbiFace). */
export const FACE_W = 150;
export const FACE_H = 40;

export const SIZE_RANGE = { min: 0.75, max: 1.6 } as const;
export const LENGTH_RANGE = { min: 0.7, max: 2 } as const;
export const HEIGHT_RANGE = { min: 0.8, max: 2.5 } as const;

/** Window width that fits a face of this size and length, plus room for the
 * asking-state widening and the preview card. */
export function windowWidth(size: number, length: number): number {
  return Math.max(DEFAULT_WIDTH, Math.ceil((FACE_W * length + 16) * size + 40));
}

export function defaultLayout(): OrbiLayout {
  return {
    width: DEFAULT_WIDTH,
    size: 1,
    length: 1,
    height: 1,
    tuck: true,
    sound: true,
    volume: 0.5,
    x: null,
    y: null,
    parts: Object.fromEntries(PART_NAMES.map((p) => [p, { ...NEUTRAL }])) as Record<
      PartName,
      PartAdjust
    >,
  };
}

/** Coerce whatever is on disk into a valid layout. A malformed field costs
 * that one field, never a face that fails to render. */
export function parseLayout(raw: unknown): OrbiLayout {
  const base = defaultLayout();
  if (!raw || typeof raw !== "object") return base;
  const o = raw as Record<string, unknown>;

  const num = (v: unknown, fallback: number, min: number, max: number) =>
    typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;

  base.size = num(o.size, 1, SIZE_RANGE.min, SIZE_RANGE.max);
  base.length = num(o.length, 1, LENGTH_RANGE.min, LENGTH_RANGE.max);
  base.height = num(o.height, 1, HEIGHT_RANGE.min, HEIGHT_RANGE.max);
  base.tuck = typeof o.tuck === "boolean" ? o.tuck : true;
  base.sound = typeof o.sound === "boolean" ? o.sound : true;
  base.volume = num(o.volume, 0.5, 0, 1);
  base.width = windowWidth(base.size, base.length);
  base.x = typeof o.x === "number" && Number.isFinite(o.x) ? o.x : null;
  base.y = typeof o.y === "number" && Number.isFinite(o.y) ? o.y : null;

  const parts = (o.parts ?? {}) as Record<string, unknown>;
  for (const name of PART_NAMES) {
    const p = (parts[name] ?? {}) as Record<string, unknown>;
    base.parts[name] = {
      dx: num(p.dx, 0, -100, 100),
      dy: num(p.dy, 0, -40, 40),
      s: num(p.s, 1, 0.2, 3),
    };
  }
  return base;
}

/** CSS custom properties for non-neutral parts only. */
export function layoutVars(layout: OrbiLayout): CSSProperties {
  const vars: Record<string, string> = {};
  for (const name of PART_NAMES) {
    const p = layout.parts[name];
    if (p.dx !== 0) vars[`--orbi-${name}-dx`] = `${p.dx}px`;
    if (p.dy !== 0) vars[`--orbi-${name}-dy`] = `${p.dy}px`;
    if (p.s !== 1) vars[`--orbi-${name}-s`] = `${p.s}`;
  }
  return vars as CSSProperties;
}

export function isNeutral(p: PartAdjust): boolean {
  return p.dx === 0 && p.dy === 0 && p.s === 1;
}

export const PART_LABELS: Record<PartName, string> = {
  eyes: "Eyes",
  orbit: "Orbit dot",
  glow: "Glow",
};
