/**
 * Shared presentational pieces.
 *
 * Everything here is deliberately dumb: it renders what it is given and calls
 * back. No component in this file talks to the backend.
 */
import { useEffect, useId, useRef } from "react";
import type { ReactNode } from "react";

import type { AppError } from "../lib/types";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

/** A labelled form field. */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children(id)}
      {hint ? <span className="hint">{hint}</span> : null}
    </div>
  );
}

/** An error, rendered with its diagnostic code and remedy. */
export function ErrorNotice({ error, onDismiss }: { error: AppError; onDismiss?: () => void }) {
  return (
    <div className="notice error" role="alert">
      <Icon name="alert" />
      <div className="notice-body">
        <div className="notice-title">{error.message}</div>
        {error.remedy ? <p className="muted">{error.remedy}</p> : null}
        <code>{error.code}</code>
      </div>
      {onDismiss ? (
        <button type="button" className="button ghost small" onClick={onDismiss}>
          Dismiss
        </button>
      ) : null}
    </div>
  );
}

/** An informational or warning note. */
export function Notice({
  tone = "info",
  title,
  children,
}: {
  tone?: "info" | "warning" | "error";
  title?: string;
  children: ReactNode;
}) {
  return (
    <div className={`notice ${tone}`}>
      <Icon name={tone === "info" ? "check" : "alert"} />
      <div className="notice-body">
        {title ? <div className="notice-title">{title}</div> : null}
        <div className="muted">{children}</div>
      </div>
    </div>
  );
}

/**
 * What to show when a list is empty.
 *
 * Always explains what the thing is and offers the action that creates one: a
 * blank panel with no explanation is the most common way a good tool feels
 * broken.
 */
export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: IconName;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <Icon name={icon} size={28} />
      <h2>{title}</h2>
      <p>{description}</p>
      {action}
    </div>
  );
}

/** A modal dialog. Closes on Escape and traps initial focus. */
export function Dialog({
  title,
  description,
  onClose,
  footer,
  wide,
  children,
}: {
  title: string;
  description?: string;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    // Focus the first control so the dialog is usable from the keyboard alone.
    const first = ref.current?.querySelector<HTMLElement>(
      "input, select, textarea, button:not([data-autofocus-skip])",
    );
    first?.focus();
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className={wide ? "dialog wide" : "dialog"}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        ref={ref}
      >
        <div className="dialog-header">
          <h2 id={titleId}>{title}</h2>
          {description ? <p className="muted">{description}</p> : null}
        </div>
        <div className="dialog-body">{children}</div>
        {footer ? <div className="dialog-footer">{footer}</div> : null}
      </div>
    </div>
  );
}

/** A confirmation dialog for a destructive action. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  destructive,
  busy,
  onConfirm,
  onCancel,
  children,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  destructive?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  children?: ReactNode;
}) {
  return (
    <Dialog
      title={title}
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className={destructive ? "button danger" : "button primary"}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? <span className="spinner" /> : null}
            {confirmLabel}
          </button>
        </>
      }
    >
      <p>{message}</p>
      {children}
    </Dialog>
  );
}

/** A progress bar; omit `fraction` for an indeterminate one. */
export function Progress({ fraction, label }: { fraction: number | null; label: string }) {
  const percent = fraction === null ? null : Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  return (
    <div className="stack" style={{ gap: 6 }}>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="muted">{label}</span>
        {percent !== null ? <span className="mono faint">{percent}%</span> : null}
      </div>
      <div
        className="progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        aria-label={label}
      >
        <div
          className={percent === null ? "progress-bar indeterminate" : "progress-bar"}
          style={percent === null ? undefined : { width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

/** The countdown ring beside a live TOTP code. */
export function CodeRing({ remaining, period }: { remaining: number; period: number }) {
  const radius = 10;
  const circumference = 2 * Math.PI * radius;
  const fraction = period > 0 ? Math.min(1, Math.max(0, remaining / period)) : 0;
  return (
    <svg
      className={`code-ring${remaining <= 5 ? " expiring" : ""}`}
      viewBox="0 0 26 26"
      role="img"
      aria-label={`${remaining} seconds remaining`}
    >
      <circle className="track" cx="13" cy="13" r={radius} />
      <circle
        className="value"
        cx="13"
        cy="13"
        r={radius}
        strokeDasharray={circumference}
        strokeDashoffset={circumference * (1 - fraction)}
        strokeLinecap="round"
      />
    </svg>
  );
}

/** A single statistic on the dashboard. */
export function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div className="stat">
      <span className="stat-value">{value}</span>
      <span className="stat-label">{label}</span>
    </div>
  );
}

/** A small icon button with an accessible name. */
export function IconButton({
  icon,
  label,
  onClick,
  disabled,
  tone,
}: {
  icon: IconName;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  tone?: "danger";
}) {
  return (
    <button
      type="button"
      className={`button ghost small${tone === "danger" ? " danger" : ""}`}
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
    >
      <Icon name={icon} size={14} />
    </button>
  );
}
