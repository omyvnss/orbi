import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useState, type ReactNode } from "react";
import { openSettings, type OrbiRequest, type OrbiSnapshot } from "../lib/tauri.js";
import type { OrbiState } from "../lib/faceState.js";
import { useNow } from "../hooks/useOrbiState.js";
import { SPRING, FADE_ONLY } from "../lib/motion.js";

/** "wants to run `npm test` in orbi/" → text with mono chips for backticked spans. */
function RichLine({ text }: { text: string }) {
  const parts = text.split("`");
  return (
    <>
      {parts.map((p, i) =>
        i % 2 === 1 ? (
          <code key={i} className="orbi-chip">
            {p}
          </code>
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
    </>
  );
}

function Warn({ children }: { children: ReactNode }) {
  return (
    <span className="orbi-warn">
      <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true">
        <path d="M6 1.2 11 10.4H1Z" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
        <path d="M6 4.6v2.6M6 8.7v.1" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      </svg>
      {children}
    </span>
  );
}

/** Subtle way into Settings, only while the card is expanded (hovered). */
function SettingsGear() {
  return (
    <button
      type="button"
      className="orbi-gear"
      aria-label="Open Orbi settings"
      title="Settings"
      onClick={() => void openSettings()}
    >
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
        <path
          d="M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4ZM8 1.8v1.6M8 12.6v1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M1.8 8h1.6M12.6 8h1.6M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );
}

function Keys({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="orbi-key">
      <kbd>{keys}</kbd>
      {label}
    </span>
  );
}

function secondsLeft(req: OrbiRequest, timeoutSecs: number, now: number): number {
  return Math.max(0, Math.ceil((req.createdAt + timeoutSecs * 1000 - now) / 1000));
}

/** Hairline that depletes over the timeout. The offset is fixed at mount (keyed
 * per request) so re-renders never shift the running CSS animation. */
function TimerBar({ createdAt, timeoutSecs }: { createdAt: number; timeoutSecs: number }) {
  const [elapsed] = useState(() => Math.max(0, (Date.now() - createdAt) / 1000));
  return (
    <div
      className="orbi-timer"
      style={{ animationDuration: `${timeoutSecs}s`, animationDelay: `-${elapsed.toFixed(2)}s` }}
      aria-hidden="true"
    />
  );
}

function AskingBody({ snap, expanded }: { snap: OrbiSnapshot; expanded: boolean }) {
  const req = snap.queue[0]!;
  const now = useNow(expanded);
  const rest = snap.queue.slice(1);

  return (
    <>
      <div className="orbi-line">
        <span className="orbi-text">
          <strong className="orbi-agent">{req.agentLabel}</strong> <RichLine text={req.line} />
        </span>
      </div>
      <div className="orbi-hints">
        <Keys keys="⌃⌥A" label="Allow" />
        <Keys keys="⌃⌥D" label="Deny" />
        {/* The risk sits here so the line keeps its full width for the command. */}
        {req.warnings[0] ? (
          <span className="orbi-hint-end">
            <Warn>{req.warnings[0]}</Warn>
          </span>
        ) : (
          !expanded && <span className="orbi-hint-end">⌃⌥O details</span>
        )}
        {expanded && <SettingsGear />}
      </div>

      {expanded && (
        <div className="orbi-detail">
          <pre className="orbi-code">{req.detail || req.line.replace(/`/g, "")}</pre>
          <dl className="orbi-meta">
            <dt>in</dt>
            <dd className="orbi-mono">{req.cwd}</dd>
            <dt>tool</dt>
            <dd>{req.tool}</dd>
            {req.warnings.length > 0 && (
              <>
                <dt>risk</dt>
                <dd className="orbi-warn-list">
                  {req.warnings.map((w) => (
                    <Warn key={w}>{w}</Warn>
                  ))}
                </dd>
              </>
            )}
            <dt>time</dt>
            <dd>
              falls back to the terminal in <span className="orbi-num">{secondsLeft(req, snap.timeoutSecs, now)}s</span>
            </dd>
          </dl>
          {rest.length > 0 && (
            <div className="orbi-queue">
              <span className="orbi-queue-head">{rest.length} more waiting</span>
              {rest.map((r) => (
                <span key={r.id} className="orbi-queue-item">
                  {r.agentLabel} · {r.tool}
                </span>
              ))}
            </div>
          )}
        </div>
      )}

      <TimerBar key={req.id} createdAt={req.createdAt} timeoutSecs={snap.timeoutSecs} />
    </>
  );
}

function ActivityBody({ snap, face, expanded }: { snap: OrbiSnapshot; face: OrbiState; expanded: boolean }) {
  const a = snap.activity!;
  return (
    <>
      <div className="orbi-line">
        <span className={`orbi-status orbi-status-${face}`} aria-hidden="true" />
        <span className="orbi-text">
          <strong className="orbi-agent">{a.agentLabel}</strong> <RichLine text={a.summary} />
        </span>
        {expanded && <SettingsGear />}
      </div>
      {expanded && a.detail && (
        <div className="orbi-detail">
          <pre className="orbi-code">{a.detail}</pre>
        </div>
      )}
    </>
  );
}

type Mode = "paused" | "asking" | "activity" | "idle-hover" | null;

function modeOf(snap: OrbiSnapshot | null, face: OrbiState, expanded: boolean): Mode {
  if (snap?.paused) return "paused";
  if (face === "asking" && snap && snap.queue.length > 0) return "asking";
  if (face !== "idle" && snap?.activity) return "activity";
  return expanded ? "idle-hover" : null;
}

/** The card under the face: one line collapsed, full detail when expanded. */
export function Preview({
  snapshot,
  face,
  expanded,
}: {
  snapshot: OrbiSnapshot | null;
  face: OrbiState;
  expanded: boolean;
}) {
  const reduce = useReducedMotion() ?? false;
  const mode = modeOf(snapshot, face, expanded);

  return (
    <AnimatePresence initial={false}>
      {mode && (
        <motion.div
          key="card"
          className="orbi-card"
          data-mode={mode}
          data-face={face}
          role="status"
          aria-live="polite"
          initial={reduce ? { opacity: 0 } : { opacity: 0, y: -6, scale: 0.97 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={reduce ? { opacity: 0 } : { opacity: 0, y: -6, scale: 0.97 }}
          transition={reduce ? FADE_ONLY : SPRING}
        >
          {mode === "paused" && (
            <div className="orbi-line">
              <span className="orbi-status orbi-status-paused" aria-hidden="true" />
              <span className="orbi-text">
                <strong className="orbi-agent">Paused</strong> agents use their own prompts
              </span>
              {expanded && <SettingsGear />}
            </div>
          )}
          {mode === "asking" && <AskingBody snap={snapshot!} expanded={expanded} />}
          {mode === "activity" && <ActivityBody snap={snapshot!} face={face} expanded={expanded} />}
          {mode === "idle-hover" && (
            <div className="orbi-line">
              <span className="orbi-status orbi-status-idle" aria-hidden="true" />
              <span className="orbi-text orbi-muted">No agent needs you</span>
              <SettingsGear />
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
