import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useState, type ReactNode } from "react";
import { openSettings, type OrbiRequest, type OrbiSession, type OrbiSnapshot } from "../lib/tauri.js";
import { openQuestion, type OrbiState } from "../lib/faceState.js";
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

/** Claude asked a multiple-choice question. Orbi shows it; the answer goes in
 * the terminal (⌃⌥J jumps to the exact tab), so the terminal is never held up. */
function QuestionBody({ s, expanded }: { s: OrbiSession; expanded: boolean }) {
  const q = s.question!;
  return (
    <>
      <div className="orbi-line">
        <span className="orbi-text">
          <strong className="orbi-agent">{s.agentLabel}</strong> asks <span className="orbi-muted">· {s.project}</span>
        </span>
        {expanded && <SettingsGear />}
      </div>
      <p className="orbi-question">{q.text}</p>
      {q.options.length > 0 && (
        <div className="orbi-options">
          {q.options.map((o) => (
            <span key={o} className="orbi-option">
              {o}
            </span>
          ))}
        </div>
      )}
      <div className="orbi-hints">
        <Keys keys="⌃⌥J" label="Answer in terminal" />
        {q.more > 0 && <span className="orbi-hint-end">+{q.more} more</span>}
      </div>
    </>
  );
}

const STATE_WORD: Record<OrbiSession["state"], string> = {
  ready: "ready",
  working: "working",
  asking: "needs you",
  question: "has a question",
  done: "done",
  error: "error",
};

/** Every running session, when the card is open. */
function SessionList({ sessions }: { sessions: OrbiSession[] }) {
  return (
    <ul className="orbi-sessions" aria-label="Running sessions">
      {sessions.map((s) => (
        <li key={s.key} className="orbi-session" data-state={s.state}>
          <span className="orbi-session-dot" aria-hidden="true" />
          <span className="orbi-session-name">{s.project}</span>
          <span className="orbi-session-meta">
            {s.agentLabel} · {s.summary ? <RichLine text={s.summary} /> : STATE_WORD[s.state]}
            {s.subagents > 0 && <span className="orbi-session-sub"> · {s.subagents} sub-agent{s.subagents > 1 ? "s" : ""}</span>}
          </span>
        </li>
      ))}
    </ul>
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

type Mode = "paused" | "asking" | "question" | "activity" | "idle-hover" | null;

function modeOf(snap: OrbiSnapshot | null, face: OrbiState, expanded: boolean, now: number): Mode {
  if (snap?.paused) return "paused";
  if (face === "asking" && snap && snap.queue.length > 0) return "asking";
  if (face === "asking" && openQuestion(snap, now)) return "question";
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
  const now = useNow(true, 15_000);
  const mode = modeOf(snapshot, face, expanded, now);
  const question = mode === "question" ? openQuestion(snapshot, now) : null;
  const sessions = snapshot?.sessions ?? [];
  // The session list only when open, and only when it adds something: more
  // than one session, or one the card isn't already about.
  const showSessions = expanded && mode !== "paused" && sessions.length > (mode === "idle-hover" ? 0 : 1);

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
          {mode === "question" && question && <QuestionBody s={question} expanded={expanded} />}
          {mode === "activity" && <ActivityBody snap={snapshot!} face={face} expanded={expanded} />}
          {mode === "idle-hover" && (
            <div className="orbi-line">
              <span className="orbi-status orbi-status-idle" aria-hidden="true" />
              <span className="orbi-text orbi-muted">No agent needs you</span>
              <SettingsGear />
            </div>
          )}
          {showSessions && <SessionList sessions={sessions} />}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
