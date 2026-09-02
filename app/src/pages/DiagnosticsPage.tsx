import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { DiagnosticsSnapshot, WorkspaceError } from "../lib/types";

export default function DiagnosticsPage() {
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.diagnostics());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const loaded = await api.diagnostics();
        if (active) {
          setSnapshot(loaded);
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

  if (error) {
    return <p className="error" role="alert">{error.message}</p>;
  }
  if (!snapshot) {
    return <p className="muted">Loading diagnostics…</p>;
  }

  return (
    <section aria-labelledby="diagnostics-heading">
      <h2 id="diagnostics-heading">Diagnostics</h2>
      <dl className="diagnostics">
        <dt>Workspace path</dt>
        <dd className="mono">{snapshot.workspacePath}</dd>
        <dt>Workspace writable</dt>
        <dd>{snapshot.workspaceWritable ? "yes" : "no"}</dd>
        <dt>Schema version</dt>
        <dd>{snapshot.schemaVersion}</dd>
        <dt>Vault</dt>
        <dd>
          {snapshot.vaultExists ? snapshot.vaultLockState : "missing"}
        </dd>
        <dt>Installed Thorium versions</dt>
        <dd>
          {snapshot.installedThoriumVersions.length === 0
            ? "none"
            : snapshot.installedThoriumVersions.join(", ")}
        </dd>
        <dt>Current Thorium version</dt>
        <dd>{snapshot.currentThoriumVersion ?? "not selected"}</dd>
        <dt>Running profiles</dt>
        <dd>
          {snapshot.runningProfiles.length === 0 ? "none" : snapshot.runningProfiles.join(", ")}
        </dd>
        <dt>Vault idle lock</dt>
        <dd>{snapshot.idleLockMinutes ? `${snapshot.idleLockMinutes} min` : "disabled"}</dd>
        <dt>Clipboard clear</dt>
        <dd>{snapshot.clipboardClearSeconds} s</dd>
      </dl>
      <button type="button" onClick={() => void refresh()}>
        Refresh
      </button>
      <p className="muted">
        Diagnostics never contain secret values. A copied report is safe to share.
      </p>
    </section>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
