/**
 * Shared presentational pieces.
 *
 * Everything here is deliberately dumb: it renders what it is given and calls
 * back. No component in this file talks to the backend, and all styling goes
 * through the design tokens in styles.css.
 */
import { useEffect, useId, useRef } from "react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { localizedErrorMessage } from "../lib/errors";
import type { WorkspaceError } from "../lib/types";

/* ---- Buttons ----------------------------------------------------------- */

type ButtonVariant = "default" | "primary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "default" | "small";
  icon?: IconName;
}

/**
 * The single button control. Variants express action hierarchy: `primary`
 * marks the one recommended action, `ghost` keeps inline actions quiet, and
 * `danger` recolors destructive intent (pair it with a confirmation dialog).
 */
export function Button({ variant = "default", size, icon, children, className, ...rest }: ButtonProps) {
  const classes = [
    "button",
    variant !== "default" ? variant : "",
    size === "small" ? "small" : "",
    className ?? "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <button type="button" className={classes} {...rest}>
      {icon ? <Icon name={icon} size={size === "small" ? 13 : 15} /> : null}
      {children}
    </button>
  );
}

/* ---- Surfaces ----------------------------------------------------------- */

export function Card({
  title,
  subtitle,
  actions,
  children,
  className,
}: {
  title?: string;
  subtitle?: string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card${className ? ` ${className}` : ""}`}>
      {(title || actions) && (
        <div className="card-header">
          {title && <h2>{title}</h2>}
          {subtitle && <span className="card-subtitle">{subtitle}</span>}
          {actions && <span className="spacer">{actions}</span>}
        </div>
      )}
      {children}
    </section>
  );
}

/* ---- Data display ------------------------------------------------------- */

export function Badge({
  tone,
  icon,
  children,
}: {
  tone?: "success" | "warning" | "danger" | "accent";
  icon?: IconName;
  children: ReactNode;
}) {
  return (
    <span className={`badge${tone ? ` ${tone}` : ""}`}>
      {icon ? <Icon name={icon} size={11} /> : null}
      {children}
    </span>
  );
}

export function Stat({
  value,
  label,
  detail,
  icon,
}: {
  value: ReactNode;
  label: string;
  detail?: ReactNode;
  icon?: IconName;
}) {
  return (
    <div className="stat">
      {icon ? (
        <span className="stat-icon">
          <Icon name={icon} size={16} />
        </span>
      ) : null}
      <div className="stat-body">
        <span className="stat-value">{value}</span>
        <span className="stat-label">{label}</span>
        {detail ? <span className="faint">{detail}</span> : null}
      </div>
    </div>
  );
}

/* ---- Form fields -------------------------------------------------------- */

/**
 * A labelled form field. `children` receives the generated input id so the
 * label association stays explicit without leaking ids across the tree.
 */
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

/* ---- Feedback ----------------------------------------------------------- */

/** An error: localized explanation for known diagnostic codes, the original
 * backend message as detail, and the code itself for diagnosability. */
export function ErrorNotice({ error, onDismiss }: { error: WorkspaceError; onDismiss?: () => void }) {
  const { t } = useTranslation();
  const localized = localizedErrorMessage(error, t);
  const detail = localized !== error.message ? error.message : null;
  return (
    <div className="notice error" role="alert">
      <Icon name="alert" />
      <div className="notice-body">
        <div className="notice-title">{localized}</div>
        {detail ? <div className="muted">{detail}</div> : null}
        <code>{error.code}</code>
      </div>
      {onDismiss ? (
        <Button variant="ghost" size="small" onClick={onDismiss} aria-label={t("common.dismissError")}>
          <Icon name="x" size={13} />
        </Button>
      ) : null}
    </div>
  );
}

/** An informational or warning note. */
export function Notice({
  tone = "info",
  icon,
  title,
  children,
}: {
  tone?: "info" | "warning" | "error";
  icon?: IconName;
  title?: string;
  children: ReactNode;
}) {
  return (
    <div className={`notice ${tone}`}>
      <Icon name={icon ?? (tone === "info" ? "info" : "alert")} />
      <div className="notice-body">
        {title ? <div className="notice-title">{title}</div> : null}
        <div className="muted">{children}</div>
      </div>
    </div>
  );
}

/**
 * What to show when a list is empty. Always explains what the thing is and
 * offers the action that creates one: a blank panel with no explanation is
 * the most common way a good tool feels broken.
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

/** A spinner with an accessible loading label. */
export function Loading({ label }: { label?: string }) {
  const { t } = useTranslation();
  const text = label ?? t("common.loading");
  return (
    <div className="row" role="status">
      <span className="spinner" />
      <span className="muted">{text}</span>
    </div>
  );
}

/* ---- Dialogs ------------------------------------------------------------ */

/** A modal dialog. Closes on Escape and focuses its first control. */
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
  destructive = true,
  busy = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  destructive?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog
      title={title}
      onClose={onCancel}
      footer={
        <>
          <Button onClick={onCancel} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button
            variant={destructive ? "danger" : "primary"}
            className={destructive ? "solid" : undefined}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? <span className="spinner" /> : null}
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p>{message}</p>
    </Dialog>
  );
}

/* ---- Progress ------------------------------------------------------------ */

/** A progress bar; omit `fraction` for an indeterminate one. */
export function Progress({ fraction, label }: { fraction: number | null; label: string }) {
  const percent = fraction === null ? null : Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  return (
    <div className="stack-tight">
      <div className="row-wide">
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

/* ---- OTP ----------------------------------------------------------------- */

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

/* ---- Page scaffolding ------------------------------------------------------ */

/** The header strip at the top of every page. Its content shares the bounded,
 * centered geometry of the page canvas. */
export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div className="page-header-inner">
        <div className="grow" style={{ minWidth: 0 }}>
          <h1>{title}</h1>
          {subtitle ? <p className="page-header-subtitle">{subtitle}</p> : null}
        </div>
        {actions ? <div className="row">{actions}</div> : null}
      </div>
    </header>
  );
}

/** A collapsible content section with a keyboard-accessible toggle. */
export function Disclosure({
  label,
  meta,
  open,
  onToggle,
  children,
}: {
  label: string;
  meta?: ReactNode;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div>
      <button type="button" className="section-toggle" aria-expanded={open} onClick={onToggle}>
        <Icon name="chevron" size={12} className="chevron" />
        {label}
        {meta ? <span className="faint" style={{ textTransform: "none" }}>{meta}</span> : null}
      </button>
      {open ? <div className="section-body">{children}</div> : null}
    </div>
  );
}

/* ---- Settings composition ------------------------------------------------ */

/**
 * A grouped settings section: an uppercase group label ("General"),
 * followed by a raised card whose rows read as one continuous form.
 */
export function SettingSection({
  label,
  title,
  subtitle,
  children,
}: {
  label: string;
  title: string;
  subtitle?: string;
  children: ReactNode;
}) {
  return (
    <section>
      <h2 className="setting-section-label">{label}</h2>
      <div className="setting-section">
        <div style={{ padding: "var(--space-3) 0 var(--space-1)" }}>
          <h2 style={{ fontSize: 14 }}>{title}</h2>
          {subtitle ? <p className="setting-row-description">{subtitle}</p> : null}
        </div>
        {children}
      </div>
    </section>
  );
}

/**
 * One setting: title + description left, control right; stacks on narrow
 * windows. Rows inside a section are separated by hairlines.
 */
export function SettingRow({
  title,
  description,
  control,
  stacked = false,
}: {
  title: string;
  description?: string;
  control: ReactNode;
  /** Force the stacked (label above control) layout regardless of width. */
  stacked?: boolean;
}) {
  return (
    <div className={stacked ? "setting-row stacked" : "setting-row"}>
      <div className="setting-row-text">
        <div className="setting-row-title">{title}</div>
        {description ? <div className="setting-row-description">{description}</div> : null}
      </div>
      <div className="setting-row-control">{control}</div>
    </div>
  );
}
