import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { DiagnosticsSnapshot, WorkspaceError } from "../lib/types";

export default function DashboardPage() {
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [profileCount, setProfileCount] = useState<number | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [diagnostics, profiles] = await Promise.all([
          api.diagnostics(),
          api.profilesList(),
        ]);
        if (active) {
          setSnapshot(diagnostics);
          setProfileCount(profiles.length);
        }
      } catch (thrown) {
        if (active) setError(toError(thrown));
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  if (error) {
    return <p className="error" role="alert">{error.message}</p>;
  }
  if (!snapshot) {
    return <p className="muted">Loading…</p>;
  }

  return (
    <section aria-labelledby="dashboard-heading">
      <h2 id="dashboard-heading">Dashboard</h2>
      <div className="card">
        <dl>
          <dt>Vault</dt>
          <dd>
            {snapshot.vaultExists ? snapshot.vaultLockState : "not created"}
          </dd>
          <dt>Profiles</dt>
          <dd>{profileCount ?? "…"}</dd>
          <dt>Running profiles</dt>
          <dd>{snapshot.runningProfiles.length}</dd>
          <dt>Thorium</dt>
          <dd>{snapshot.currentThoriumVersion ?? "not installed"}</dd>
          <dt>Workspace</dt>
          <dd className="mono">{snapshot.workspacePath}</dd>
        </dl>
      </div>
      <p className="muted">
        Use Profiles to manage browser profiles, Accounts for credentials and
        2FA, Browser for Thorium installs, and Diagnostics for safe runtime
        details.
      </p>
    </section>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
