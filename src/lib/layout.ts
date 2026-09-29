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
  /** Rendered face width in logical points. */
  width: number;
  /** Widget window position in logical points, or null for default top-centre. */
  x: number | null;
  y: number | null;
  parts: Record<PartName, PartAdjust>;
}

const NEUTRAL: PartAdjust = { dx: 0, dy: 0, s: 1 };

/** Matches the Rust side's collapsed window width. */
export const DEFAULT_WIDTH = 320;

export function defaultLayout(): OrbiLayout {
  return {
    width: DEFAULT_WIDTH,
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

  base.width = num(o.width, DEFAULT_WIDTH, 80, 600);
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
