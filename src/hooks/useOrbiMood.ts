import { useEffect, useRef, useState } from "react";

/**
 * The face's mood as four continuous numbers, driving CSS custom properties
 * (`--orbi-rig-*`) on the document element every frame:
 *
 *   - valence   −1..1 — how well things are going (brow/eye tilt).
 *   - arousal    0..1 — how worked up (eye stretch).
 *   - energy     0..1 — how fresh; falls while ignored (droop, blink rate).
 *   - attention  0..1 — how locked on (head tilt).
 *
 * Everything decays toward rest rather than resetting. `calm` damps the whole
 * rig toward stillness.
 */

export interface MoodInput {
  valenceBump?: number;
  arousalBump?: number;
  /** Damp motion to almost nothing. */
  calm?: boolean;
  /** Milliseconds since the user last engaged with the face. */
  idleMs?: number;
}

export interface Mood {
  valence: number;
  arousal: number;
  energy: number;
  attention: number;
  /** Push the mood — call on real events, not per frame. */
  feel: (input: { valence?: number; arousal?: number }) => void;
}

/** Irregular blink timing: real blinks cluster, a metronome reads as a machine. */
const BLINK_MIN_MS = 2_600;
const BLINK_MAX_MS = 7_400;
const BLINK_DOWN_MS = 70;
const BLINK_UP_MS = 110;

/** How squashed the eye gets at the bottom of a blink. */
const BLINK_DEPTH = 0.18;

export function useOrbiMood(input: MoodInput = {}): Mood {
  const [snapshot, setSnapshot] = useState({
    valence: 0,
    arousal: 0.3,
    energy: 0.8,
    attention: 0.5,
  });

  const m = useRef({ valence: 0, arousal: 0.3, energy: 0.8, attention: 0.5 });
  const nextBlinkRef = useRef(Date.now() + 3_000);
  const blinkStartRef = useRef(0);
  const inputRef = useRef(input);
  inputRef.current = input;

  const feelRef = useRef((d: { valence?: number; arousal?: number }) => {
    const s = m.current;
    s.valence = Math.max(-1, Math.min(1, s.valence + (d.valence ?? 0)));
    s.arousal = Math.max(0, Math.min(1, s.arousal + (d.arousal ?? 0)));
  });

  useEffect(() => {
    let raf = 0;
    let lastPublish = 0;

    const frame = () => {
      raf = requestAnimationFrame(frame);
      const now = Date.now();
      const { calm = false, idleMs = 0 } = inputRef.current;
      const s = m.current;

      // Different decay rates on purpose: excitement fades fast, a mood lingers.
      s.arousal = Math.max(calm ? 0.05 : 0.15, s.arousal - 0.0022);
      s.valence *= 0.9985;

      const bored = Math.min(1, idleMs / 120_000);
      s.energy += (1 - bored * 0.75 - s.energy) * 0.004;
      s.attention += ((calm ? 0.25 : 1 - bored) - s.attention) * 0.01;

      // ---- blink ----------------------------------------------------------
      let blinkSy = 1;
      if (blinkStartRef.current) {
        const t = now - blinkStartRef.current;
        if (t < BLINK_DOWN_MS) {
          blinkSy = 1 - (t / BLINK_DOWN_MS) * (1 - BLINK_DEPTH);
        } else if (t < BLINK_DOWN_MS + BLINK_UP_MS) {
          // Opens slower than it closes, like a real eyelid.
          const u = (t - BLINK_DOWN_MS) / BLINK_UP_MS;
          blinkSy = BLINK_DEPTH + u * (1 - BLINK_DEPTH);
        } else {
          blinkStartRef.current = 0;
          const span = BLINK_MIN_MS + Math.random() * (BLINK_MAX_MS - BLINK_MIN_MS);
          nextBlinkRef.current = now + span * (0.5 + s.energy * 0.8);
        }
      } else if (now >= nextBlinkRef.current) {
        blinkStartRef.current = now;
      }

      // ---- the rig --------------------------------------------------------
      const gain = calm ? 0.25 : 1;
      const openness = 1 + (s.arousal * 0.14 + s.attention * 0.08 - (1 - s.energy) * 0.16) * gain;
      const sy = openness * blinkSy;
      const sx = 1 + (1 - sy) * 0.35;
      const headTilt = (s.attention * 3.2 * Math.sign(s.valence || 1) + s.valence * 1.8) * gain;
      const browTilt = -s.valence * 5.5 * gain;
      const eyeTilt = s.valence * 1.6 * gain;

      // Written to :root, not React state — the blink needs per-frame writes
      // and a re-render per frame would be wasted work.
      const rootStyle = document.documentElement.style;
      rootStyle.setProperty("--orbi-rig-eye-sx", sx.toFixed(3));
      rootStyle.setProperty("--orbi-rig-eye-sy", sy.toFixed(3));
      rootStyle.setProperty("--orbi-rig-eye-tilt", `${eyeTilt.toFixed(2)}deg`);
      rootStyle.setProperty("--orbi-rig-brow-tilt", `${browTilt.toFixed(2)}deg`);
      rootStyle.setProperty("--orbi-rig-head-tilt", `${headTilt.toFixed(2)}deg`);

      if (now - lastPublish > 500) {
        lastPublish = now;
        setSnapshot({ ...s });
      }
    };

    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  return { ...snapshot, feel: feelRef.current };
}
