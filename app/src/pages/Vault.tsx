/**
 * The vault page.
 *
 * Locking, unlocking, changing the master password, and the settings that decide
 * when the vault locks itself.
 */
import { useState } from "react";

import { Icon } from "../components/Icon";
import { ErrorNotice, Field, Notice } from "../components/ui";
import { api } from "../lib/api";
import { formatTimestamp, passwordHint } from "../lib/format";
import type { AppError, VaultSettings, VaultState, WorkspaceSettings } from "../lib/types";
import type { ToastFn } from "../App";

export function VaultPage({
  vault,
  settings,
  onToast,
  onVaultChanged,
  onSettingsChanged,
}: {
  vault: VaultState | null;
  settings: WorkspaceSettings | null;
  onToast: ToastFn;
  onVaultChanged: (state: VaultState) => void;
  onSettingsChanged: (settings: WorkspaceSettings) => void;
}) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [changing, setChanging] = useState(false);

  const unlocked = vault?.state === "unlocked";

  const unlock = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onVaultChanged(await api.unlockVault(password));
      onToast("Vault unlocked");
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
      setPassword("");
    }
  };

  const updateVaultSettings = async (patch: Partial<VaultSettings>) => {
    if (!settings) return;
    const next: WorkspaceSettings = { ...settings, vault: { ...settings.vault, ...patch } };
    try {
      onSettingsChanged(await api.setSettings(next));
    } catch (caught) {
      onToast((caught as AppError).message, "error");
    }
  };

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Vault</h1>
          <div className="subtitle">
            Passwords, one-time-password secrets and recovery codes, encrypted at rest.
          </div>
        </div>
        <div className="page-header-actions">
          {unlocked ? (
            <button
              type="button"
              className="button"
              onClick={async () => {
                onVaultChanged(await api.lockVault());
                onToast("Vault locked");
              }}
            >
              <Icon name="lock" />
              Lock now
            </button>
          ) : null}
        </div>
      </header>

      <div className="page-body stack">
        {!unlocked ? (
          <form className="card stack" onSubmit={unlock} style={{ maxWidth: 520 }}>
            <div className="row">
              <Icon name="lock" size={20} />
              <h2>The vault is locked</h2>
            </div>
            <p className="muted">
              {vault?.state === "locked" && vault.reason === "idle"
                ? "It locked itself after a period of inactivity."
                : vault?.state === "locked" && vault.reason === "minimized"
                  ? "It locked when the window was minimised."
                  : "Enter your master password to use account secrets."}
            </p>
            <Field label="Master password">
              {(id) => (
                <input
                  id={id}
                  type="password"
                  value={password}
                  autoComplete="current-password"
                  onChange={(event) => setPassword(event.target.value)}
                  disabled={busy}
                />
              )}
            </Field>
            {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
            <button type="submit" className="button primary" disabled={busy || !password}>
              {busy ? <span className="spinner" /> : <Icon name="unlock" />}
              Unlock
            </button>
          </form>
        ) : (
          <div className="card stack">
            <div className="row">
              <Icon name="unlock" size={20} />
              <div className="grow">
                <h2>Unlocked</h2>
                <p className="faint">
                  {vault.secret_count} secret{vault.secret_count === 1 ? "" : "s"} · unlocked at{" "}
                  {formatTimestamp(vault.unlocked_at)}
                  {vault.idle_lock_seconds
                    ? ` · locks after ${Math.round(vault.idle_lock_seconds / 60)} min idle`
                    : " · no idle lock"}
                </p>
              </div>
            </div>
          </div>
        )}

        {settings ? (
          <div className="card stack">
            <div className="card-header">
              <h2>Locking</h2>
            </div>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={settings.vault.idle_lock_enabled}
                onChange={(event) =>
                  void updateVaultSettings({ idle_lock_enabled: event.target.checked })
                }
              />
              <span className="checkbox-text">
                <strong>Lock after a period of inactivity</strong>
                <span className="faint">
                  The vault closes itself so a workspace left open does not stay open.
                </span>
              </span>
            </label>
            <Field label="Idle timeout" hint="Between 30 seconds and 24 hours.">
              {(id) => (
                <select
                  id={id}
                  value={settings.vault.idle_lock_seconds}
                  disabled={!settings.vault.idle_lock_enabled}
                  onChange={(event) =>
                    void updateVaultSettings({ idle_lock_seconds: Number(event.target.value) })
                  }
                >
                  <option value={60}>1 minute</option>
                  <option value={300}>5 minutes</option>
                  <option value={600}>10 minutes</option>
                  <option value={1800}>30 minutes</option>
                  <option value={3600}>1 hour</option>
                  <option value={14400}>4 hours</option>
                </select>
              )}
            </Field>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={settings.vault.lock_on_minimize}
                onChange={(event) =>
                  void updateVaultSettings({ lock_on_minimize: event.target.checked })
                }
              />
              <span className="checkbox-text">
                <strong>Lock when the window is minimised</strong>
                <span className="faint">
                  Useful on a shared machine; it means re-entering the master password often.
                </span>
              </span>
            </label>
          </div>
        ) : null}

        <div className="card stack">
          <div className="card-header">
            <h2>Master password</h2>
            <span className="spacer" />
            <button
              type="button"
              className="button small"
              onClick={() => setChanging(true)}
              disabled={!unlocked}
            >
              <Icon name="key" size={13} />
              Change
            </button>
          </div>
          <p className="muted">
            Changing it re-encrypts the vault under the new password. A copy of the old file is kept
            beside it as <span className="mono">workspace.twvault.bak</span> until the next change,
            so an interrupted re-key is recoverable.
          </p>
          {!unlocked ? <p className="faint">Unlock the vault first.</p> : null}
        </div>

        <div className="card stack">
          <div className="card-header">
            <h2>Housekeeping</h2>
          </div>
          <p className="muted">
            Deleting an account normally removes its secrets too. If a delete was interrupted, this
            removes anything left behind that nothing points at any more.
          </p>
          <div>
            <button
              type="button"
              className="button"
              disabled={!unlocked}
              onClick={async () => {
                try {
                  const removed = await api.collectOrphanedSecrets();
                  onToast(
                    removed === 0
                      ? "No orphaned secrets found"
                      : `Removed ${removed} orphaned secret${removed === 1 ? "" : "s"}`,
                  );
                } catch (caught) {
                  onToast((caught as AppError).message, "error");
                }
              }}
            >
              <Icon name="trash" />
              Remove orphaned secrets
            </button>
          </div>
        </div>
      </div>

      {changing ? (
        <ChangePasswordDialog
          onClose={() => setChanging(false)}
          onDone={() => {
            setChanging(false);
            onToast("Master password changed");
          }}
        />
      ) : null}
    </>
  );
}

function ChangePasswordDialog({
  onClose,
  onDone,
}: {
  onClose: () => void;
  onDone: () => void;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const hint = passwordHint(next);
  const canSubmit = !busy && current.length > 0 && hint.level > 0 && next === confirm;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.changeMasterPassword(current, next);
      onDone();
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
      setCurrent("");
      setNext("");
      setConfirm("");
    }
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label="Change master password">
        <div className="dialog-header">
          <h2>Change master password</h2>
          <p className="muted">The vault is backed up before it is re-encrypted.</p>
        </div>
        <div className="dialog-body">
          <Field label="Current master password">
            {(id) => (
              <input
                id={id}
                type="password"
                value={current}
                autoComplete="current-password"
                onChange={(event) => setCurrent(event.target.value)}
              />
            )}
          </Field>
          <Field label="New master password" hint="At least 12 characters.">
            {(id) => (
              <input
                id={id}
                type="password"
                value={next}
                autoComplete="new-password"
                onChange={(event) => setNext(event.target.value)}
              />
            )}
          </Field>
          {hint.label ? <span className="faint">{hint.label}</span> : null}
          <Field label="Confirm new master password">
            {(id) => (
              <input
                id={id}
                type="password"
                value={confirm}
                autoComplete="new-password"
                onChange={(event) => setConfirm(event.target.value)}
              />
            )}
          </Field>
          {confirm && confirm !== next ? (
            <span className="hint">The two passwords do not match.</span>
          ) : null}
          {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
          <Notice tone="warning">
            The new password cannot be recovered either. Update wherever you keep it written down.
          </Notice>
        </div>
        <div className="dialog-footer">
          <button type="button" className="button" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="button primary" onClick={submit} disabled={!canSubmit}>
            {busy ? <span className="spinner" /> : null}
            Change password
          </button>
        </div>
      </div>
    </div>
  );
}
