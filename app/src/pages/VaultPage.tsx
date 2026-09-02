import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { VaultStatus, WorkspaceError } from "../lib/types";

function isPasswordUsable(password: string): boolean {
  return password.length >= 8 && password.length <= 200;
}

export default function VaultPage() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.vaultStatus());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const loaded = await api.vaultStatus();
        if (active) {
          setStatus(loaded);
          setError(null);
        }
      } catch (thrown) {
        if (active) {
          setError(toError(thrown));
        }
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      setPassword("");
      setConfirm("");
      await refresh();
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  if (!status) {
    return <p className="muted">{error ? error.message : "Loading vault status…"}</p>;
  }

  if (status.lockState === "missing") {
    return (
      <section aria-labelledby="vault-heading">
        <h2 id="vault-heading">Create your Vault</h2>
        <p className="muted">
          The Vault encrypts account passwords, OTP seeds, and recovery codes
          inside this workspace. The master password is never stored.
        </p>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void run(async () => {
              if (password !== confirm) {
                throw new WorkspaceError("FRONTEND_MISMATCH", "Passwords do not match.");
              }
              if (!isPasswordUsable(password)) {
                throw new WorkspaceError(
                  "FRONTEND_WEAK_PASSWORD",
                  "Use at least 8 characters.",
                );
              }
              await api.vaultCreate(password);
            });
          }}
        >
          <label>
            Master password
            <input
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </label>
          <label>
            Confirm master password
            <input
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
              required
            />
          </label>
          <button type="submit" disabled={busy}>
            Create Vault
          </button>
        </form>
        {error && <p className="error" role="alert">{error.message}</p>}
      </section>
    );
  }

  if (status.lockState === "locked") {
    return (
      <section aria-labelledby="vault-heading">
        <h2 id="vault-heading">Vault is locked</h2>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void run(() => api.vaultUnlock(password));
          }}
        >
          <label>
            Master password
            <input
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </label>
          <button type="submit" disabled={busy}>
            Unlock
          </button>
        </form>
        {error && <p className="error" role="alert">{error.message}</p>}
      </section>
    );
  }

  return (
    <section aria-labelledby="vault-heading">
      <h2 id="vault-heading">Vault is unlocked</h2>
      <p className="muted">
        Account secrets can be stored and revealed while the Vault is unlocked.
        It locks automatically after the configured idle time.
      </p>
      <button
        type="button"
        disabled={busy}
        onClick={() => void run(() => api.vaultLock())}
      >
        Lock now
      </button>
      {error && <p className="error" role="alert">{error.message}</p>}
    </section>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
