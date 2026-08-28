/**
 * Pure formatting helpers.
 *
 * Kept free of React and of Tauri so they can be unit tested directly, which is
 * where the UI's own correctness is actually verifiable.
 */
import type { InstallProgress, ServiceKind, VaultState } from "./types";

/** Formats a byte count for a human. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** Formats a Unix epoch second count as a local date and time. */
export function formatTimestamp(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || seconds <= 0) return "never";
  return new Date(seconds * 1000).toLocaleString();
}

/** Formats a Unix epoch second count as a local date. */
export function formatDate(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || seconds <= 0) return "never";
  return new Date(seconds * 1000).toLocaleDateString();
}

/** A short relative description, for "installed 3 days ago". */
export function formatRelative(seconds: number, now = Date.now() / 1000): string {
  if (seconds <= 0) return "never";
  const delta = Math.max(0, Math.round(now - seconds));
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)} min ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)} h ago`;
  const days = Math.floor(delta / 86400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/** The human name of a service kind. */
export function serviceLabel(service: ServiceKind): string {
  switch (service.kind) {
    case "git_hub":
      return "GitHub";
    case "microsoft":
      return "Microsoft";
    default:
      return service.label || "Other";
  }
}

/** A one-line summary of vault state for the header. */
export function vaultSummary(state: VaultState): string {
  switch (state.state) {
    case "uninitialized":
      return "No vault yet";
    case "locked":
      return "Locked";
    default:
      return state.secret_count === 1 ? "Unlocked, 1 secret" : `Unlocked, ${state.secret_count} secrets`;
  }
}

/** Formats an install progress stage as a sentence and a 0-1 fraction. */
export function describeProgress(progress: InstallProgress): {
  label: string;
  fraction: number | null;
} {
  switch (progress.stage) {
    case "resolving":
      return { label: "Looking up the latest release…", fraction: null };
    case "downloading": {
      const fraction =
        progress.total && progress.total > 0 ? progress.received / progress.total : null;
      const of = progress.total ? ` of ${formatBytes(progress.total)}` : "";
      return { label: `Downloading ${formatBytes(progress.received)}${of}…`, fraction };
    }
    case "verifying":
      return { label: "Verifying the download…", fraction: null };
    case "extracting":
      return {
        label: `Extracting ${progress.done} of ${progress.total} files…`,
        fraction: progress.total > 0 ? progress.done / progress.total : null,
      };
    case "activating":
      return { label: "Activating the new version…", fraction: 0.99 };
    default:
      return { label: `Installed ${progress.version}`, fraction: 1 };
  }
}

/** Groups a code into readable blocks: `123456` becomes `123 456`. */
export function groupCode(code: string): string {
  if (code.length === 6) return `${code.slice(0, 3)} ${code.slice(3)}`;
  if (code.length === 8) return `${code.slice(0, 4)} ${code.slice(4)}`;
  return code;
}

/**
 * A rough strength hint for a master password.
 *
 * Deliberately advisory only: the backend enforces the actual minimum, and a
 * strength meter that blocks submission trains people to game the meter.
 */
export function passwordHint(value: string): { level: 0 | 1 | 2 | 3; label: string } {
  const length = [...value].length;
  if (length === 0) return { level: 0, label: "" };
  if (length < 12) return { level: 0, label: "Too short — at least 12 characters" };
  const variety =
    Number(/[a-z]/.test(value)) +
    Number(/[A-Z]/.test(value)) +
    Number(/[0-9]/.test(value)) +
    Number(/[^A-Za-z0-9]/.test(value));
  if (length >= 20 || (length >= 16 && variety >= 3)) return { level: 3, label: "Strong" };
  if (length >= 14 || variety >= 3) return { level: 2, label: "Good" };
  return { level: 1, label: "Acceptable" };
}
