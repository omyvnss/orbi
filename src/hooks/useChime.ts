import { useEffect, useRef } from "react";
import type { OrbiState } from "../lib/faceState.js";

/**
 * Orbi's sounds, synthesised on the spot (no audio files): a soft two-note
 * chime when an agent needs you, and a quieter rising pair when one finishes.
 * Nothing plays for routine work.
 */
export function useChime(face: OrbiState, enabled: boolean, volume: number) {
  const ctx = useRef<AudioContext | null>(null);
  const prev = useRef<OrbiState>(face);

  useEffect(() => {
    const was = prev.current;
    prev.current = face;
    if (!enabled || volume <= 0 || was === face) return;
    if (face !== "asking" && face !== "done") return;

    try {
      ctx.current ??= new AudioContext();
      const ac = ctx.current;
      void ac.resume();
      const peak = 0.09 * volume;
      const tone = (freq: number, at: number, dur: number, gain = peak) => {
        const o = ac.createOscillator();
        const g = ac.createGain();
        o.type = "sine";
        o.frequency.value = freq;
        g.gain.setValueAtTime(0, at);
        g.gain.linearRampToValueAtTime(gain, at + 0.012);
        g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
        o.connect(g).connect(ac.destination);
        o.start(at);
        o.stop(at + dur + 0.05);
      };
      const t = ac.currentTime + 0.01;
      if (face === "asking") {
        tone(880, t, 0.5);
        tone(1318.5, t + 0.12, 0.8);
      } else {
        tone(659.3, t, 0.22, peak * 0.6);
        tone(987.8, t + 0.09, 0.4, peak * 0.6);
      }
    } catch {
      /* no audio device — stay quiet */
    }
  }, [face, enabled, volume]);
}
