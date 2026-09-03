// The Vault page is deliberately calm. A locked vault is a normal state, not
// an error, so it gets neutral styling; danger styling is reserved for truly
// destructive operations. The three lifecycle states (uninitialized, locked,
// unlocked) each get their own focused layout.

import { useState } from "react";

import { Icon } from "../components/Icon";
import {
  Button,
  Card,
  Dialog,
  ErrorNotice,
  Field,
  PageHeader,
} from "../components/ui";
import { api } from "../lib/api";
import type { ToastFn } from "../lib/hooks";
import type { VaultStatus, WorkspaceSettings } from "../lib/types";
import { WorkspaceError } from "../lib/types";

function isPasswordUsable(password: string): boolean {
  return password.length >= 8 && password.length <= 200;
}

export default function VaultPage({
  vault,
  settings,
  onVaultChanged,
  onToast,
}: {
  vault: VaultStatus;
  settings: WorkspaceSettings | null;
  onVaultChanged: (status: VaultStatus) => void;
  onToast: ToastFn;
}) {
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [changeOpen, setChangeOpen] = useState(false);

  const run = async (action: () => Promise<void>, okMessage?: string) => {
    setBusy(true);
    try {
      await action();
      setPassword("");
      setConfirm("");
      const status = await api.vaultStatus();
      onVaultChanged(status);
      if (okMessage) onToast(okMessage);
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  const idleLock = settings?.vaultIdleLockMinutes ?? null;

  return (
    <>
      <PageHeader
        title="Vault"
        subtitle={
          vault.lockState === "unlocked"
            ? "Unlocked — secrets can be stored and revealed"
            : vault.lockState === "locked"
              ? "Locked — account secrets are sealed"
              : "Not created yet"
        }
      />
      <div className="page-body">
        <div className="onboarding" style={{ height: "auto", padding: 0 }}>
          <div className="stack" style={{ width: "min(520px, 100%)" }}>
            {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

            {vault.lockState === "missing" && (
              <Card
                title="Create your Vault"
                subtitle="One encrypted container for every secret in this workspace"
              >
                <div className="stack">
                  <p className="muted">
                    The Vault encrypts account passwords, OTP seeds, and recovery codes with
                    Argon2id + ChaCha20-Poly1305. The master password is never stored anywhere —
                    if you lose it, the secrets are unrecoverable.
                  </p>
                  <form
                    className="stack"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void run(async () => {
                        if (password !== confirm) {
                          throw new WorkspaceError(
                            "FRONTEND_MISMATCH",
                            "Passwords do not match.",
                          );
                        }
                        if (!isPasswordUsable(password)) {
                          throw new WorkspaceError(
                            "FRONTEND_WEAK_PASSWORD",
                            "Use at least 8 characters.",
                          );
                        }
                        await api.vaultCreate(password);
                      }, "Vault created");
                    }}
                  >
                    <Field label="Master password" hint="8–200 characters. Not recoverable.">
                      {(id) => (
                        <input
                          id={id}
                          type="password"
                          autoComplete="new-password"
                          value={password}
                          onChange={(event) => setPassword(event.target.value)}
                          required
                        />
                      )}
                    </Field>
                    <Field label="Confirm master password">
                      {(id) => (
                        <input
                          id={id}
                          type="password"
                          autoComplete="new-password"
                          value={confirm}
                          onChange={(event) => setConfirm(event.target.value)}
                          required
                        />
                      )}
                    </Field>
                    <Button variant="primary" type="submit" disabled={busy}>
                      {busy ? <span className="spinner" /> : null}
                      Create Vault
                    </Button>
                  </form>
                </div>
              </Card>
            )}

            {vault.lockState === "locked" && (
              <Card title="Vault is locked" subtitle="Enter the master password to unlock">
                <form
                  className="stack"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void run(() => api.vaultUnlock(password), "Vault unlocked");
                  }}
                >
                  <Field label="Master password">
                    {(id) => (
                      <input
                        id={id}
                        type="password"
                        autoComplete="current-password"
                        value={password}
                        onChange={(event) => setPassword(event.target.value)}
                        required
                        autoFocus
                      />
                    )}
                  </Field>
                  <Button variant="primary" type="submit" disabled={busy}>
                    {busy ? <span className="spinner" /> : null}
                    Unlock
                  </Button>
                  {idleLock !== null && (
                    <p className="faint">
                      The Vault locks automatically after {idleLock}{" "}
                      {idleLock === 1 ? "minute" : "minutes"} of inactivity
                      {settings?.vaultLockOnMinimize ? " and when the window is minimized" : ""}.
                    </p>
                  )}
                </form>
              </Card>
            )}

            {vault.lockState === "unlocked" && (
              <>
                <Card>
                  <div className="row-wide">
                    <div className="row" style={{ flexWrap: "nowrap" }}>
                      <Icon name="unlock" size={18} style={{ color: "var(--success)" }} />
                      <div>
                        <strong>Vault unlocked</strong>
                        <div className="faint">
                          {idleLock !== null
                            ? `Auto-locks after ${idleLock} ${
                                idleLock === 1 ? "minute" : "minutes"
                              } of inactivity`
                            : "Idle auto-lock is disabled in Settings"}
                          {settings?.vaultLockOnMinimize ? " · locks on minimize" : ""}
                        </div>
                      </div>
                    </div>
                    <Button icon="lock" disabled={busy} onClick={() => void run(() => api.vaultLock(), "Vault locked")}>
                      Lock now
                    </Button>
                  </div>
                </Card>

                <Card
                  title="Master password"
                  subtitle="Changing it re-encrypts the Vault under the new password"
                >
                  <Button onClick={() => setChangeOpen(true)} disabled={busy}>
                    Change master password…
                  </Button>
                  <p className="faint" style={{ marginTop: 12 }}>
                    While unlocked, secrets are available to the Accounts page. Locking removes
                    every revealed value from the interface immediately.
                  </p>
                </Card>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Rendered only while the vault is actually unlocked: if the backend
          locks the vault on its own schedule the dialog unmounts itself. */}
      {vault.lockState === "unlocked" && changeOpen && (
        <ChangePasswordDialog
          busy={busy}
          onClose={() => setChangeOpen(false)}
          onSubmit={(current, next) =>
            void run(
              async () => {
                await api.vaultChangePassword(current, next);
                setChangeOpen(false);
              },
              "Master password changed",
            )
          }
        />
      )}
    </>
  );
}

function ChangePasswordDialog({
  busy,
  onClose,
  onSubmit,
}: {
  busy: boolean;
  onClose: () => void;
  onSubmit: (current: string, next: string) => void;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");

  const usable = isPasswordUsable(next) && next === confirm && current.length > 0;

  return (
    <Dialog
      title="Change master password"
      description="The Vault is re-encrypted under the new password. Keep a backup of the old password until you have verified the change."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!usable || busy} onClick={() => onSubmit(current, next)}>
            {busy ? <span className="spinner" /> : null}
            Change password
          </Button>
        </>
      }
    >
      <div className="stack">
        <Field label="Current master password">
          {(id) => (
            <input
              id={id}
              type="password"
              autoComplete="current-password"
              value={current}
              onChange={(event) => setCurrent(event.target.value)}
            />
          )}
        </Field>
        <Field label="New master password" hint="8–200 characters">
          {(id) => (
            <input
              id={id}
              type="password"
              autoComplete="new-password"
              value={next}
              onChange={(event) => setNext(event.target.value)}
            />
          )}
        </Field>
        <Field label="Confirm new master password">
          {(id) => (
            <input
              id={id}
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
            />
          )}
        </Field>
        {next.length > 0 && !isPasswordUsable(next) && (
          <p className="faint">Use between 8 and 200 characters.</p>
        )}
      </div>
    </Dialog>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
