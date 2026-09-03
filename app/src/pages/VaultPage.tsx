// The Vault page is deliberately calm. A locked vault is a normal state, not
// an error, so it gets neutral styling; danger styling is reserved for truly
// destructive operations. The three lifecycle states (uninitialized, locked,
// unlocked) each get their own focused layout.

import { useState } from "react";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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

  const subtitle =
    vault.lockState === "unlocked"
      ? t("vault.subtitleUnlocked")
      : vault.lockState === "locked"
        ? t("vault.subtitleLocked")
        : t("vault.subtitleMissing");

  return (
    <>
      <PageHeader title={t("vault.title")} subtitle={subtitle} />
      <div className="page-body">
        <div className="onboarding" style={{ height: "auto", padding: 0 }}>
          <div className="stack" style={{ width: "min(520px, 100%)" }}>
            {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

            {vault.lockState === "missing" && (
              <Card
                title={t("vault.create.title")}
                subtitle={t("vault.create.subtitle")}
              >
                <div className="stack">
                  <p className="muted">{t("vault.create.description")}</p>
                  <form
                    className="stack"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void run(async () => {
                        if (password !== confirm) {
                          throw new WorkspaceError("FRONTEND_MISMATCH", t("common.errors.FRONTEND_MISMATCH"));
                        }
                        if (!isPasswordUsable(password)) {
                          throw new WorkspaceError(
                            "FRONTEND_WEAK_PASSWORD",
                            t("common.errors.FRONTEND_WEAK_PASSWORD"),
                          );
                        }
                        await api.vaultCreate(password);
                      }, t("vault.toasts.created"));
                    }}
                  >
                    <Field label={t("vault.create.masterPassword")} hint={t("vault.create.masterPasswordHint")}>
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
                    <Field label={t("vault.create.confirm")}>
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
                      {t("vault.create.action")}
                    </Button>
                  </form>
                </div>
              </Card>
            )}

            {vault.lockState === "locked" && (
              <Card title={t("vault.locked.title")} subtitle={t("vault.locked.subtitle")}>
                <form
                  className="stack"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void run(() => api.vaultUnlock(password), t("vault.toasts.unlocked"));
                  }}
                >
                  <Field label={t("vault.locked.masterPassword")}>
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
                    {t("vault.locked.action")}
                  </Button>
                  {idleLock !== null && (
                    <p className="faint">
                      {t(
                        idleLock === 1
                          ? "vault.locked.autoLockHint"
                          : "vault.locked.autoLockHint_other",
                        { count: idleLock },
                      )}
                      {settings?.vaultLockOnMinimize ? t("vault.locked.minimizeSuffix") : ""}.
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
                        <strong>{t("vault.unlocked.title")}</strong>
                        <div className="faint">
                          {idleLock !== null
                            ? t(
                                idleLock === 1
                                  ? "vault.unlocked.autoLocks"
                                  : "vault.unlocked.autoLocks_other",
                                { count: idleLock },
                              )
                            : t("vault.unlocked.idleDisabled")}
                          {settings?.vaultLockOnMinimize ? t("vault.unlocked.locksOnMinimize") : ""}
                        </div>
                      </div>
                    </div>
                    <Button
                      icon="lock"
                      disabled={busy}
                      onClick={() => void run(() => api.vaultLock(), t("vault.toasts.locked"))}
                    >
                      {t("vault.unlocked.lockNow")}
                    </Button>
                  </div>
                </Card>

                <Card
                  title={t("vault.change.title")}
                  subtitle={t("vault.change.subtitle")}
                >
                  <Button onClick={() => setChangeOpen(true)} disabled={busy}>
                    {t("vault.change.action")}
                  </Button>
                  <p className="faint" style={{ marginTop: 12 }}>
                    {t("vault.change.whileUnlocked")}
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
              t("vault.toasts.changed"),
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
  const { t } = useTranslation();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");

  const usable = isPasswordUsable(next) && next === confirm && current.length > 0;

  return (
    <Dialog
      title={t("vault.change.dialogTitle")}
      description={t("vault.change.dialogDescription")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!usable || busy}
            onClick={() => onSubmit(current, next)}
          >
            {busy ? <span className="spinner" /> : null}
            {t("vault.change.changeAction")}
          </Button>
        </>
      }
    >
      <div className="stack">
        <Field label={t("vault.change.current")}>
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
        <Field label={t("vault.change.new")} hint={t("vault.change.newHint")}>
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
        <Field label={t("vault.change.confirm")}>
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
          <p className="faint">{t("vault.change.lengthWarning")}</p>
        )}
      </div>
    </Dialog>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", localizedFallback(thrown));
}

/** Backend errors are already localized by the caller's `run`; anything
 * else falls back to the string form. */
function localizedFallback(thrown: unknown): string {
  return thrown instanceof WorkspaceError ? thrown.message : String(thrown);
}
