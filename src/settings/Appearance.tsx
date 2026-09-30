import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { OrbiFace } from "../components/OrbiFace.js";
import {
  FACE_H,
  FACE_W,
  HEIGHT_RANGE,
  LENGTH_RANGE,
  SIZE_RANGE,
  parseLayout,
  windowWidth,
  type OrbiLayout,
} from "../lib/layout.js";
import { getScreenSize, loadLayout, resetLayout, saveLayout } from "../lib/tauri.js";
import type { OrbiState } from "../lib/faceState.js";
import { Button, Group, PageHead, Row, Segmented, Toggle } from "./ui.js";

/** Where the face sits, in screen points: its horizontal centre and its top. */
interface Spot {
  cx: number;
  y: number;
}
type Preset = "notch" | "left" | "right" | "custom";

const EDGE_SNAP = 18; // closer than this to the top edge → hug it
const NOTCH_SNAP = 44; // this close to the middle of the top edge → back in the notch
const INSET = 16; // corner presets keep this far from the side

function spotOf(l: OrbiLayout, screenW: number): Spot {
  return l.x === null || l.y === null ? { cx: screenW / 2, y: 0 } : { cx: l.x + l.width / 2, y: l.y };
}

function presetOf(s: Spot, screenW: number, faceW: number): Preset {
  if (s.y !== 0) return "custom";
  if (Math.abs(s.cx - screenW / 2) < 1) return "notch";
  if (Math.abs(s.cx - (INSET + faceW / 2)) < 1) return "left";
  if (Math.abs(s.cx - (screenW - INSET - faceW / 2)) < 1) return "right";
  return "custom";
}

function Slider({
  label,
  hint,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  const id = useId();
  const pct = ((value - min) / (max - min)) * 100;
  const text = `${Math.round(value * 100)}%`;
  return (
    <Row label={label} hint={hint} htmlFor={id} stack>
      <div className="s-slider">
        <input
          id={id}
          type="range"
          min={min}
          max={max}
          step={0.05}
          value={value}
          aria-valuetext={text}
          style={{ ["--pct" as string]: `${pct}%` }}
          onChange={(e) => onChange(Number(e.target.value))}
        />
        <output htmlFor={id} className="s-slider-val">
          {text}
        </output>
      </div>
    </Row>
  );
}

export function AppearanceSection({ onError }: { onError: (msg: string) => void }) {
  const [layout, setLayout] = useState<OrbiLayout>(() => parseLayout(null));
  const [screen, setScreen] = useState({ w: 1512, h: 982 });
  const [ready, setReady] = useState(false);

  useEffect(() => {
    void Promise.all([loadLayout(), getScreenSize()]).then(([raw, s]) => {
      if (s) setScreen(s);
      setLayout(parseLayout(raw));
      setReady(true);
    });
  }, []);

  // Save shortly after the last change, so a slider drag is one write.
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(timer.current), []);
  const commit = useCallback(
    (next: OrbiLayout, delay = 220) => {
      setLayout(next);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        saveLayout(next).catch((e) => onError(`Couldn't save the layout — ${String(e)}`));
      }, delay);
    },
    [onError],
  );

  const faceW = FACE_W * layout.length * layout.size;
  const faceH = FACE_H * layout.height * layout.size;
  const spot = spotOf(layout, screen.w);
  const preset = presetOf(spot, screen.w, faceW);

  /** Rebuild the layout for a new size/length/spot. The notch is stored as "no position". */
  const withSpot = (base: OrbiLayout, s: Spot, fw: number): OrbiLayout => {
    const width = windowWidth(base.size, base.length);
    const clamped: Spot = {
      cx: Math.min(screen.w - fw / 2, Math.max(fw / 2, s.cx)),
      y: Math.min(screen.h - FACE_H * base.height * base.size - 8, Math.max(0, s.y)),
    };
    const inNotch = clamped.y === 0 && Math.abs(clamped.cx - screen.w / 2) < 1;
    return {
      ...base,
      width,
      x: inNotch ? null : Math.round(clamped.cx - width / 2),
      y: inNotch ? null : Math.round(clamped.y),
    };
  };

  const setSize = (size: number) => {
    const next = { ...layout, size };
    commit(withSpot(next, spot, FACE_W * next.length * size));
  };
  const setLength = (length: number) => {
    const next = { ...layout, length };
    commit(withSpot(next, spot, FACE_W * length * next.size));
  };
  const setHeight = (height: number) => commit(withSpot({ ...layout, height }, spot, faceW));
  const setPreset = (p: Preset) => {
    if (p === "custom") return;
    const cx = p === "notch" ? screen.w / 2 : p === "left" ? INSET + faceW / 2 : screen.w - INSET - faceW / 2;
    commit(withSpot(layout, { cx, y: 0 }, faceW), 0);
  };

  // ---- the drag map
  const mapRef = useRef<HTMLDivElement>(null);
  const [mapW, setMapW] = useState(480);
  useEffect(() => {
    const el = mapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setMapW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ready]);
  const k = mapW / screen.w; // map pixels per screen point
  const drag = useRef<{ dx: number; dy: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  const snap = (s: Spot): Spot => {
    let { cx, y } = s;
    if (y < EDGE_SNAP) y = 0;
    if (y === 0 && Math.abs(cx - screen.w / 2) < NOTCH_SNAP) cx = screen.w / 2;
    return { cx, y };
  };
  const fromPointer = (e: PointerEvent): Spot => {
    const r = mapRef.current!.getBoundingClientRect();
    const d = drag.current ?? { dx: 0, dy: 0 };
    return snap({ cx: (e.clientX - r.left - d.dx) / k, y: (e.clientY - r.top - d.dy) / k });
  };
  const onDown = (e: PointerEvent<HTMLButtonElement>) => {
    const r = mapRef.current!.getBoundingClientRect();
    drag.current = { dx: e.clientX - r.left - spot.cx * k, dy: e.clientY - r.top - spot.y * k };
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
  };
  const onMove = (e: PointerEvent<HTMLButtonElement>) => {
    if (!drag.current) return;
    // While dragging, only the map moves; the real face follows on release.
    setLayout(withSpot(layout, fromPointer(e), faceW));
  };
  const onUp = (e: PointerEvent<HTMLButtonElement>) => {
    if (!drag.current) return;
    const next = withSpot(layout, fromPointer(e), faceW);
    drag.current = null;
    setDragging(false);
    commit(next, 0);
  };
  const onKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    const step = e.shiftKey ? 50 : 10;
    const move: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const m = move[e.key];
    if (!m) return;
    e.preventDefault();
    commit(withSpot(layout, snap({ cx: spot.cx + m[0], y: spot.y + m[1] }), faceW));
  };

  const reset = () => {
    clearTimeout(timer.current);
    resetLayout()
      .then(() => setLayout(parseLayout(null)))
      .catch((e) => onError(`Couldn't reset — ${String(e)}`));
  };

  // A living preview: Orbi goes about its day while you adjust it.
  const [demo, setDemo] = useState<OrbiState>("idle");
  useEffect(() => {
    const cycle: OrbiState[] = ["idle", "working", "asking", "done"];
    let i = 0;
    const t = setInterval(() => setDemo(cycle[++i % cycle.length]!), 2200);
    return () => clearInterval(t);
  }, []);

  const where =
    preset === "notch"
      ? "In the notch"
      : spot.y === 0
        ? `Top edge · ${Math.round(spot.cx)} pt from the left`
        : `Floating · ${Math.round(spot.cx)}, ${Math.round(spot.y)} pt`;

  const floating = spot.y > 2;
  const chipW = Math.max(18, faceW * k);
  const chipH = Math.max(6, faceH * k);

  return (
    <>
      <PageHead title="Appearance" sub="Make Orbi your size, and put it where you'll see it." />

      <div className="s-appear-stage" aria-hidden="true">
        <div className="s-appear-bar" />
        <div className={`s-appear-face${floating ? " is-floating" : ""}`}>
          <OrbiFace state={demo} size={layout.size} length={layout.length} height={layout.height} floating={floating} />
        </div>
      </div>

      <Group title="Size and shape">
        <Slider
          label="Face size"
          hint="How big Orbi is — the eyes, the pill, everything."
          value={layout.size}
          min={SIZE_RANGE.min}
          max={SIZE_RANGE.max}
          onChange={setSize}
        />
        <Slider
          label="Width"
          hint="Stretch it sideways — short and round, or a long capsule."
          value={layout.length}
          min={LENGTH_RANGE.min}
          max={LENGTH_RANGE.max}
          onChange={setLength}
        />
        <Slider
          label="Height"
          hint="Stretch it down — how far Orbi hangs below the camera."
          value={layout.height}
          min={HEIGHT_RANGE.min}
          max={HEIGHT_RANGE.max}
          onChange={setHeight}
        />
      </Group>

      <Group
        title="Position"
        footer="Drag the face anywhere. Near the top it hugs the edge; near the middle it slides back into the notch. Arrow keys move it too (⇧ for bigger steps)."
      >
        <Row label="Place" hint={where}>
          <Segmented<Preset>
            label="Placement"
            value={preset}
            onChange={setPreset}
            options={[
              { value: "left", label: "Top left" },
              { value: "notch", label: "Notch" },
              { value: "right", label: "Top right" },
              ...(preset === "custom" ? [{ value: "custom" as const, label: "Custom" }] : []),
            ]}
          />
        </Row>
        <div className="s-map-wrap">
          <div
            ref={mapRef}
            className={`s-map${dragging ? " is-dragging" : ""}`}
            style={{ aspectRatio: `${screen.w} / ${screen.h}` }}
          >
            <div className="s-map-bar" />
            <div className="s-map-notch" />
            <i className={`s-map-guide${preset === "notch" ? " is-on" : ""}`} />
            <button
              type="button"
              className={`s-map-face${floating ? " is-floating" : ""}`}
              aria-label={`Orbi's position: ${where}. Drag, or use the arrow keys.`}
              style={{ left: spot.cx * k - chipW / 2, top: spot.y * k, width: chipW, height: chipH }}
              onPointerDown={onDown}
              onPointerMove={onMove}
              onPointerUp={onUp}
              onPointerCancel={onUp}
              onKeyDown={onKey}
            >
              <i />
              <i />
            </button>
          </div>
        </div>
      </Group>

      <Group title="Behaviour">
        <Row
          label="Tuck away when idle"
          hint="When no agent is doing anything, Orbi slips back into the notch. Hover the top of the screen to peek."
        >
          <Toggle label="Tuck away when idle" checked={layout.tuck} onChange={(tuck) => commit({ ...layout, tuck }, 0)} />
        </Row>
        <Row label="Sound" hint="A soft chime when an agent needs you, and a quieter one when it finishes.">
          <Toggle label="Sound" checked={layout.sound} onChange={(sound) => commit({ ...layout, sound }, 0)} />
        </Row>
        {layout.sound && (
          <Slider
            label="Volume"
            hint="Quiet by default — you'll notice it without it startling you."
            value={layout.volume}
            min={0.05}
            max={1}
            onChange={(volume) => commit({ ...layout, volume })}
          />
        )}
      </Group>

      <div className="s-appear-foot">
        <Button variant="ghost" onClick={reset}>
          Reset to default
        </Button>
      </div>
    </>
  );
}
