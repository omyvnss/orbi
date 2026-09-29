import { useEffect, useId, useRef, useState, type ReactNode } from "react";

/* Small, dependency-free controls for the settings window. */

export function Icon({ d, size = 16 }: { d: string; size?: number }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" className="s-icon">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export const ICONS = {
  agents: "M5.5 7a2 2 0 1 0 0-4 2 2 0 0 0 0 4ZM2 13c0-2 1.6-3.5 3.5-3.5S9 11 9 13M11 7.5a1.7 1.7 0 1 0 0-3.4M12 9.6c1.2.4 2 1.6 2 3.4",
  explain: "M3 4.5h10M3 8h7M3 11.5h4.5M12 10l.6 1.4L14 12l-1.4.6L12 14l-.6-1.4L10 12l1.4-.6Z",
  general: "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4ZM8 1.8v1.6M8 12.6v1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M1.8 8h1.6M12.6 8h1.6M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1",
  security: "M8 1.8 13 3.6v4c0 3.1-2.1 5.4-5 6.6-2.9-1.2-5-3.5-5-6.6v-4Z",
  check: "M3.5 8.2 6.6 11 12.5 5",
  copy: "M5.5 5.5V3.2c0-.4.3-.7.7-.7h6.6c.4 0 .7.3.7.7v6.6c0 .4-.3.7-.7.7h-2.3M3.2 5.5h6.6c.4 0 .7.3.7.7v6.6c0 .4-.3.7-.7.7H3.2a.7.7 0 0 1-.7-.7V6.2c0-.4.3-.7.7-.7Z",
  warn: "M8 2.2 14 13H2ZM8 6.5v2.8M8 11.2v.1",
  key: "M10 9.5a3.5 3.5 0 1 0-3.3-2.3L2.5 11.4v2h2v-1.4h1.4v-1.4h1.4l.8-.8A3.5 3.5 0 0 0 10 9.5ZM10.8 5.2h.01",
  plug: "M6 2v3M10 2v3M4.5 5h7v2.5a3.5 3.5 0 0 1-7 0ZM8 11v3",
  terminal: "M2.5 3.5h11v9h-11ZM5 6.5l2 1.5-2 1.5M8.5 10h2.5",
  refresh: "M13 8a5 5 0 1 1-1.5-3.6M13 2.5v2.8h-2.8",
} as const;

export function Spinner() {
  return <span className="s-spinner" aria-hidden="true" />;
}

export function Button({
  children,
  variant = "secondary",
  busy,
  className = "",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  busy?: boolean;
}) {
  return (
    <button
      type="button"
      className={`s-btn s-btn-${variant} ${className}`}
      aria-busy={busy || undefined}
      {...rest}
      disabled={rest.disabled || busy}
    >
      {busy && <Spinner />}
      {children}
    </button>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="s-switch"
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="s-switch-knob" />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const idx = options.findIndex((o) => o.value === value);
  const move = (d: number) => {
    const n = (idx + d + options.length) % options.length;
    onChange(options[n]!.value);
    refs.current[n]?.focus();
  };
  return (
    <div className="s-seg" role="radiogroup" aria-label={label}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={o.value === value ? 0 : -1}
          className="s-seg-opt"
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight" || e.key === "ArrowDown") {
              e.preventDefault();
              move(1);
            } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
              e.preventDefault();
              move(-1);
            }
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A grouped list, macOS-style: rounded container with hairline dividers. */
export function Group({ title, children, footer }: { title?: string; children: ReactNode; footer?: ReactNode }) {
  const id = useId();
  return (
    <section className="s-group" aria-labelledby={title ? id : undefined}>
      {title && (
        <h3 className="s-group-title" id={id}>
          {title}
        </h3>
      )}
      <div className="s-group-box">{children}</div>
      {footer && <p className="s-group-foot">{footer}</p>}
    </section>
  );
}

export function Row({
  label,
  hint,
  children,
  htmlFor,
  stack,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children?: ReactNode;
  htmlFor?: string;
  stack?: boolean;
}) {
  return (
    <div className={`s-row${stack ? " s-row-stack" : ""}`}>
      <div className="s-row-text">
        {htmlFor ? (
          <label className="s-row-label" htmlFor={htmlFor}>
            {label}
          </label>
        ) : (
          <span className="s-row-label">{label}</span>
        )}
        {hint && <span className="s-row-hint">{hint}</span>}
      </div>
      {children && <div className="s-row-ctl">{children}</div>}
    </div>
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (!done) return;
    const t = setTimeout(() => setDone(false), 1400);
    return () => clearTimeout(t);
  }, [done]);
  return (
    <button
      type="button"
      className="s-copy"
      aria-label={done ? "Copied" : label}
      title={done ? "Copied" : label}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(
          () => setDone(true),
          () => {},
        );
      }}
    >
      <Icon d={done ? ICONS.check : ICONS.copy} size={14} />
    </button>
  );
}

/** Modal confirm built on <dialog>: native focus trap, Esc to cancel. */
export function Confirm({
  open,
  title,
  body,
  confirmLabel,
  onConfirm,
  onCancel,
  busy,
}: {
  open: boolean;
  title: string;
  body: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  const id = useId();
  return (
    <dialog
      ref={ref}
      className="s-dialog"
      aria-labelledby={id}
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <h2 id={id} className="s-dialog-title">
        {title}
      </h2>
      <div className="s-dialog-body">{body}</div>
      <div className="s-dialog-actions">
        <Button onClick={onCancel} disabled={busy} autoFocus>
          Cancel
        </Button>
        <Button variant="danger" onClick={onConfirm} busy={busy}>
          {confirmLabel}
        </Button>
      </div>
    </dialog>
  );
}

export function PageHead({ title, sub }: { title: string; sub: ReactNode }) {
  return (
    <header className="s-head">
      <h1>{title}</h1>
      <p>{sub}</p>
    </header>
  );
}
