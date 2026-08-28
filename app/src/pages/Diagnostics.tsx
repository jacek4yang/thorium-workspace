/**
 * Diagnostics.
 *
 * Answers "why is this not working?" without answering "what are this user's
 * passwords?". The copyable report is redacted before it is copied: paths are
 * reduced to their last component and profile names are omitted, because a
 * support log gets pasted in public.
 */
import { Icon } from "../components/Icon";
import { ErrorNotice, Notice } from "../components/ui";
import { api } from "../lib/api";
import { useAsync } from "../lib/hooks";
import type { AppError } from "../lib/types";
import type { ToastFn } from "../App";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <tr>
      <th style={{ width: 220, paddingTop: 8 }}>{label}</th>
      <td className="selectable">{value}</td>
    </tr>
  );
}

function YesNo({ value }: { value: boolean }) {
  return (
    <span className={`badge ${value ? "success" : "warning"}`}>{value ? "yes" : "no"}</span>
  );
}

export function DiagnosticsPage({ onToast }: { onToast: ToastFn }) {
  const report = useAsync(() => api.diagnostics(), []);
  const data = report.data;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Diagnostics</h1>
          <div className="subtitle">
            Everything support needs, and nothing that identifies you.
          </div>
        </div>
        <div className="page-header-actions">
          <button type="button" className="button" onClick={report.reload}>
            <Icon name="refresh" />
            Refresh
          </button>
          <button
            type="button"
            className="button primary"
            onClick={async () => {
              try {
                await api.copyDiagnostics();
                onToast("Redacted report copied");
              } catch (caught) {
                onToast((caught as AppError).message, "error");
              }
            }}
          >
            <Icon name="copy" />
            Copy report
          </button>
        </div>
      </header>

      <div className="page-body stack">
        {report.error ? <ErrorNotice error={report.error} /> : null}

        {data ? (
          <>
            <Notice tone="info" title="What the copied report contains">
              Versions, paths reduced to their last folder name, counts and yes/no answers. It never
              contains a password, a one-time-password secret, a recovery code or a profile name.
            </Notice>

            <div className="card">
              <div className="card-header">
                <h2>Workspace</h2>
              </div>
              <table>
                <tbody>
                  <Row label="Application version" value={<span className="mono">{data.appVersion}</span>} />
                  <Row label="Platform" value={data.platform} />
                  <Row
                    label="Windows process supervision"
                    value={<YesNo value={data.windowsSupervision} />}
                  />
                  <Row
                    label="Workspace folder"
                    value={<span className="mono truncate">{data.workspaceRoot}</span>}
                  />
                  <Row label="Folder writable" value={<YesNo value={data.workspaceWritable} />} />
                  <Row
                    label="Instance name"
                    value={<span className="mono">{data.instanceName}</span>}
                  />
                  <Row label="Stale files cleaned at start" value={`${data.staleFilesRemoved} runtime, ${data.staleStagingRemoved} staging`} />
                </tbody>
              </table>
            </div>

            <div className="card">
              <div className="card-header">
                <h2>Storage and vault</h2>
              </div>
              <table>
                <tbody>
                  <Row label="Schema version" value={data.schemaVersion} />
                  <Row
                    label="Database integrity"
                    value={
                      <span className={`badge ${data.databaseIntegrity === "ok" ? "success" : "danger"}`}>
                        {data.databaseIntegrity}
                      </span>
                    }
                  />
                  <Row label="Vault" value={<span className="badge">{data.vaultState}</span>} />
                  <Row label="Vault format" value={data.vaultFormatVersion ?? "—"} />
                  <Row
                    label="Key derivation memory"
                    value={
                      data.vaultKdfMemoryKib
                        ? `${Math.round(data.vaultKdfMemoryKib / 1024)} MiB (Argon2id)`
                        : "—"
                    }
                  />
                  <Row label="Secrets stored" value={data.vaultSecretCount ?? "locked"} />
                  <Row label="Accounts" value={data.accountCount} />
                  <Row label="Second factors" value={data.factorCount} />
                </tbody>
              </table>
            </div>

            <div className="card">
              <div className="card-header">
                <h2>Browser</h2>
              </div>
              <table>
                <tbody>
                  <Row label="Update channel" value={<span className="mono">{data.thoriumChannel}</span>} />
                  <Row
                    label="Installed versions"
                    value={
                      data.thoriumVersions.length === 0 ? (
                        <span className="faint">none</span>
                      ) : (
                        <span className="mono">{data.thoriumVersions.join(", ")}</span>
                      )
                    }
                  />
                  <Row
                    label="Current version"
                    value={<span className="mono">{data.thoriumCurrent ?? "none"}</span>}
                  />
                  <Row
                    label="Executable"
                    value={
                      <span className="mono truncate">{data.thoriumExecutable ?? "none"}</span>
                    }
                  />
                </tbody>
              </table>
            </div>

            <div className="card">
              <div className="card-header">
                <h2>Profiles</h2>
              </div>
              {data.profiles.length === 0 ? (
                <p className="faint">No profiles.</p>
              ) : (
                <div className="scroll-x">
                  <table>
                    <thead>
                      <tr>
                        <th>Profile</th>
                        <th>Status</th>
                        <th>Locale</th>
                        <th>Timezone</th>
                        <th>Data</th>
                        <th>Control channel</th>
                        <th>Overrides</th>
                      </tr>
                    </thead>
                    <tbody>
                      {data.profiles.map((profile) => (
                        <tr key={profile.id}>
                          <td className="truncate">{profile.name}</td>
                          <td>
                            <span
                              className={`badge ${profile.status === "running" ? "success" : ""}`}
                            >
                              {profile.status}
                            </span>
                          </td>
                          <td className="mono">{profile.locale}</td>
                          <td className="mono">{profile.timezone}</td>
                          <td>
                            <YesNo value={profile.userDataPresent} />
                          </td>
                          <td>
                            <YesNo value={profile.cdpActive} />
                          </td>
                          <td>
                            <YesNo value={profile.emulationActive} />
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>

            <div className="card">
              <div className="card-header">
                <h2>Settings in effect</h2>
              </div>
              <table>
                <tbody>
                  <Row label="Theme" value={data.theme} />
                  <Row
                    label="Clipboard clearing"
                    value={`${data.clipboardClearEnabled ? "on" : "off"} (${data.clipboardClearSeconds}s)`}
                  />
                  <Row
                    label="Vault idle lock"
                    value={`${data.vaultIdleLockEnabled ? "on" : "off"} (${data.vaultIdleLockSeconds}s)`}
                  />
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <div className="row">
            <span className="spinner" />
            <span className="muted">Collecting diagnostics…</span>
          </div>
        )}
      </div>
    </>
  );
}
