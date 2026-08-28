/**
 * First run.
 *
 * The vault has to exist before anything else is useful, so this is the only
 * screen shown until it does. It explains what the master password protects and
 * what happens if it is lost, because both are irreversible.
 */
import { useState } from "react";

import { BrandMark } from "../components/Icon";
import { ErrorNotice, Field, Notice } from "../components/ui";
import { api } from "../lib/api";
import { passwordHint } from "../lib/format";
import type { AppError, StartupStatus, VaultState } from "../lib/types";

export function Onboarding({
  startup,
  onCreated,
}: {
  startup: StartupStatus;
  onCreated: (state: VaultState) => void;
}) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const hint = passwordHint(password);
  const mismatch = confirm.length > 0 && confirm !== password;
  const canSubmit =
    !busy && hint.level > 0 && password === confirm && password.length > 0 && acknowledged;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      onCreated(await api.createVault(password));
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
      // The typed password is dropped from component state as soon as the call
      // returns, whichever way it went.
      setPassword("");
      setConfirm("");
    }
  };

  return (
    <div className="onboarding">
      <form className="card onboarding-card" onSubmit={submit}>
        <BrandMark className="onboarding-mark" size={52} />
        <div>
          <h1>Set up your workspace</h1>
          <p className="muted">
            Everything is stored beside the application file, in{" "}
            <span className="mono selectable">{startup.workspaceRoot ?? "this folder"}</span>.
            Move that folder and your whole workspace moves with it.
          </p>
        </div>

        <ol className="step-list">
          <li className="step done">
            <span className="step-number">✓</span>
            <div>
              <strong>Workspace ready</strong>
              <div className="faint">Folders created and storage initialised.</div>
            </div>
          </li>
          <li className="step current">
            <span className="step-number">2</span>
            <div>
              <strong>Create your vault</strong>
              <div className="faint">Passwords, one-time-password secrets and recovery codes.</div>
            </div>
          </li>
          <li className="step">
            <span className="step-number">3</span>
            <div>
              <strong>Install Thorium</strong>
              <div className="faint">Downloaded from the Browser page when you are ready.</div>
            </div>
          </li>
        </ol>

        <Field
          label="Master password"
          hint="At least 12 characters. A memorable passphrase of several words beats a short complicated one."
        >
          {(id) => (
            <input
              id={id}
              type="password"
              value={password}
              autoComplete="new-password"
              onChange={(event) => setPassword(event.target.value)}
              disabled={busy}
            />
          )}
        </Field>
        {hint.label ? (
          <span className={hint.level === 0 ? "hint" : "faint"}>{hint.label}</span>
        ) : null}

        <Field label="Confirm master password">
          {(id) => (
            <input
              id={id}
              type="password"
              value={confirm}
              autoComplete="new-password"
              onChange={(event) => setConfirm(event.target.value)}
              disabled={busy}
              aria-invalid={mismatch}
            />
          )}
        </Field>
        {mismatch ? <span className="hint">The two passwords do not match.</span> : null}

        <Notice tone="warning" title="There is no way to recover this password">
          The vault is encrypted with it. Nobody, including this application, can open the vault
          without it. Write it down and keep it somewhere safe.
        </Notice>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={acknowledged}
            onChange={(event) => setAcknowledged(event.target.checked)}
          />
          <span className="checkbox-text">
            I understand that losing this password means losing access to everything in the vault.
          </span>
        </label>

        {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}

        <button type="submit" className="button primary" disabled={!canSubmit}>
          {busy ? <span className="spinner" /> : null}
          Create vault
        </button>
      </form>
    </div>
  );
}
