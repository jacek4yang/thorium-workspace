// Diagnostics: safe, redaction-pinned facts about the running workspace,
// grouped so a developer can scan them quickly. The copy action puts a
// plain-text report on the clipboard; the backend guarantees these fields
// never contain secret material.

import { useCallback, useEffect, useState } from "react";

import {
  Badge,
  Button,
  Card,
  ErrorNotice,
  Loading,
  PageHeader,
} from "../components/ui";
import { api } from "../lib/api";
import type { ToastFn } from "../lib/hooks";
import type { DiagnosticsSnapshot } from "../lib/types";
import { WorkspaceError } from "../lib/types";

interface Row {
  label: string;
  value: string;
  mono?: boolean;
  tone?: "success" | "warning" | "danger";
}

export default function DiagnosticsPage({ onToast }: { onToast: ToastFn }) {
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
        if (active) setError(toError(thrown));
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const copyReport = async () => {
    if (!snapshot) return;
    try {
      await navigator.clipboard.writeText(renderReport(snapshot));
      onToast("Diagnostic report copied — safe to share");
    } catch {
      onToast("Could not access the clipboard", "error");
    }
  };

  return (
    <>
      <PageHeader
        title="Diagnostics"
        subtitle="Safe runtime facts; this report never contains secret values"
        actions={
          <>
            <Button icon="clipboard" disabled={!snapshot} onClick={() => void copyReport()}>
              Copy report
            </Button>
            <Button icon="refresh" onClick={() => void refresh()}>
              Refresh
            </Button>
          </>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}
        {!snapshot ? (
          <Loading label="Loading diagnostics…" />
        ) : (
          <div className="grid">
            <Card title="Workspace">
              <Rows
                rows={[
                  { label: "Path", value: snapshot.workspacePath, mono: true },
                  {
                    label: "Writable",
                    value: snapshot.workspaceWritable ? "Yes" : "No",
                    tone: snapshot.workspaceWritable ? "success" : "danger",
                  },
                  { label: "Schema version", value: `v${snapshot.schemaVersion}` },
                ]}
              />
            </Card>

            <Card title="Vault">
              <Rows
                rows={[
                  {
                    label: "Exists",
                    value: snapshot.vaultExists ? "Yes" : "Not created",
                  },
                  {
                    label: "State",
                    value: snapshot.vaultExists ? snapshot.vaultLockState : "—",
                    tone:
                      snapshot.vaultLockState === "unlocked"
                        ? "success"
                        : snapshot.vaultExists
                          ? "warning"
                          : undefined,
                  },
                  {
                    label: "Idle lock",
                    value: snapshot.idleLockMinutes
                      ? `${snapshot.idleLockMinutes} min`
                      : "disabled",
                  },
                ]}
              />
            </Card>

            <Card title="Thorium">
              <Rows
                rows={[
                  {
                    label: "Installed versions",
                    value:
                      snapshot.installedThoriumVersions.length === 0
                        ? "None"
                        : snapshot.installedThoriumVersions.join(", "),
                    mono: snapshot.installedThoriumVersions.length > 0,
                  },
                  {
                    label: "Current",
                    value: snapshot.currentThoriumVersion ?? "Not selected",
                    mono: snapshot.currentThoriumVersion !== null,
                    tone: snapshot.currentThoriumVersion ? "success" : "warning",
                  },
                ]}
              />
            </Card>

            <Card title="Runtime">
              <Rows
                rows={[
                  {
                    label: "Running profiles",
                    value:
                      snapshot.runningProfiles.length === 0
                        ? "None"
                        : `${snapshot.runningProfiles.length}`,
                  },
                  {
                    label: "Clipboard clear",
                    value: `${snapshot.clipboardClearSeconds} s`,
                  },
                ]}
              />
              {snapshot.runningProfiles.length > 0 && (
                <p className="faint" style={{ marginTop: 8 }}>
                  Profile IDs: <span className="mono">{snapshot.runningProfiles.join(", ")}</span>
                </p>
              )}
            </Card>
          </div>
        )}
        <p className="faint">
          These values are redacted at the Rust boundary. A copied report is safe to attach to a
          bug report.
        </p>
      </div>
    </>
  );
}

function Rows({ rows }: { rows: Row[] }) {
  return (
    <dl style={{ margin: 0, display: "grid", rowGap: 8 }}>
      {rows.map((row) => (
        <div key={row.label} className="row-wide">
          <dt className="muted" style={{ minWidth: 130 }}>
            {row.label}
          </dt>
          <dd style={{ margin: 0, textAlign: "right" }}>
            {row.tone ? (
              <Badge tone={row.tone}>{row.value}</Badge>
            ) : (
              <span className={row.mono ? "mono selectable" : "selectable"}>{row.value}</span>
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function renderReport(snapshot: DiagnosticsSnapshot): string {
  return [
    "Thorium Workspace diagnostic report",
    "-----------------------------------",
    `workspace path:        ${snapshot.workspacePath}`,
    `workspace writable:    ${snapshot.workspaceWritable}`,
    `schema version:        ${snapshot.schemaVersion}`,
    `vault exists:          ${snapshot.vaultExists}`,
    `vault lock state:      ${snapshot.vaultExists ? snapshot.vaultLockState : "missing"}`,
    `vault idle lock:       ${snapshot.idleLockMinutes ? `${snapshot.idleLockMinutes} min` : "disabled"}`,
    `thorium installed:     ${snapshot.installedThoriumVersions.join(", ") || "none"}`,
    `thorium current:       ${snapshot.currentThoriumVersion ?? "none"}`,
    `running profiles:      ${snapshot.runningProfiles.length}`,
    `clipboard clear:       ${snapshot.clipboardClearSeconds} s`,
  ].join("\n");
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
