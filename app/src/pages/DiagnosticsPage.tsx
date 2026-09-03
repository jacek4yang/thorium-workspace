// Diagnostics: safe, redaction-pinned facts about the running workspace,
// grouped so a developer can scan them quickly. The copy action puts a
// plain-text report on the clipboard; the backend guarantees these fields
// never contain secret material. Codes, paths, and version numbers stay
// untranslated.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);

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

  const refresh = async () => {
    try {
      setSnapshot(await api.diagnostics());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

  const copyReport = async () => {
    if (!snapshot) return;
    try {
      await navigator.clipboard.writeText(renderReport(snapshot, t));
      onToast(t("diagnostics.copiedToast"));
    } catch {
      onToast(t("diagnostics.clipboardFailedToast"), "error");
    }
  };

  return (
    <>
      <PageHeader
        title={t("diagnostics.title")}
        subtitle={t("diagnostics.subtitle")}
        actions={
          <>
            <Button icon="clipboard" disabled={!snapshot} onClick={() => void copyReport()}>
              {t("diagnostics.copyReport")}
            </Button>
            <Button icon="refresh" onClick={() => void refresh()}>
              {t("common.refresh")}
            </Button>
          </>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}
        {!snapshot ? (
          <Loading label={t("diagnostics.loading")} />
        ) : (
          <div className="grid" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(380px, 1fr))" }}>
            <Card title={t("diagnostics.groups.workspace")}>
              <Rows
                rows={[
                  { label: t("diagnostics.rows.path"), value: snapshot.workspacePath, mono: true },
                  {
                    label: t("diagnostics.rows.writable"),
                    value: snapshot.workspaceWritable
                      ? t("diagnostics.rows.existsYes")
                      : t("diagnostics.rows.existsNo"),
                    tone: snapshot.workspaceWritable ? "success" : "danger",
                  },
                  {
                    label: t("diagnostics.rows.schemaVersion"),
                    value: t("diagnostics.rows.schemaValue", { version: snapshot.schemaVersion }),
                  },
                ]}
              />
            </Card>

            <Card title={t("diagnostics.groups.vault")}>
              <Rows
                rows={[
                  {
                    label: t("diagnostics.rows.exists"),
                    value: snapshot.vaultExists
                      ? t("diagnostics.rows.existsYes")
                      : t("diagnostics.rows.existsNo"),
                  },
                  {
                    label: t("diagnostics.rows.state"),
                    value: snapshot.vaultExists ? snapshot.vaultLockState : "—",
                    tone:
                      snapshot.vaultLockState === "unlocked"
                        ? "success"
                        : snapshot.vaultExists
                          ? "warning"
                          : undefined,
                  },
                  {
                    label: t("diagnostics.rows.idleLock"),
                    value: snapshot.idleLockMinutes
                      ? t("diagnostics.rows.minutes", { count: snapshot.idleLockMinutes })
                      : t("diagnostics.rows.disabled"),
                  },
                ]}
              />
            </Card>

            <Card title={t("diagnostics.groups.thorium")}>
              <Rows
                rows={[
                  {
                    label: t("diagnostics.rows.installedVersions"),
                    value:
                      snapshot.installedThoriumVersions.length === 0
                        ? t("common.none")
                        : snapshot.installedThoriumVersions.join(", "),
                    mono: snapshot.installedThoriumVersions.length > 0,
                  },
                  {
                    label: t("diagnostics.rows.current"),
                    value: snapshot.currentThoriumVersion ?? t("diagnostics.rows.notSelected"),
                    mono: snapshot.currentThoriumVersion !== null,
                    tone: snapshot.currentThoriumVersion ? "success" : "warning",
                  },
                ]}
              />
            </Card>

            <Card title={t("diagnostics.groups.runtime")}>
              <Rows
                rows={[
                  {
                    label: t("diagnostics.rows.runningProfiles"),
                    value:
                      snapshot.runningProfiles.length === 0
                        ? t("common.none")
                        : `${snapshot.runningProfiles.length}`,
                  },
                  {
                    label: t("diagnostics.rows.clipboardClear"),
                    value: t("diagnostics.rows.seconds", { count: snapshot.clipboardClearSeconds }),
                  },
                ]}
              />
              {snapshot.runningProfiles.length > 0 && (
                <p className="faint" style={{ marginTop: 8 }}>
                  {t("diagnostics.rows.profileIds")}{" "}
                  <span className="mono">{snapshot.runningProfiles.join(", ")}</span>
                </p>
              )}
            </Card>
          </div>
        )}
        <p className="faint">{t("diagnostics.footer")}</p>
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

function renderReport(
  snapshot: DiagnosticsSnapshot,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  return [
    "Thorium Workspace diagnostic report",
    "-----------------------------------",
    `${t("diagnostics.rows.path")}:              ${snapshot.workspacePath}`,
    `${t("diagnostics.rows.writable")}:          ${snapshot.workspaceWritable}`,
    `${t("diagnostics.rows.schemaVersion")}:     v${snapshot.schemaVersion}`,
    `${t("diagnostics.rows.exists")}:            ${snapshot.vaultExists}`,
    `${t("diagnostics.rows.state")}:             ${snapshot.vaultExists ? snapshot.vaultLockState : "missing"}`,
    `${t("diagnostics.rows.idleLock")}:          ${
      snapshot.idleLockMinutes
        ? t("diagnostics.rows.minutes", { count: snapshot.idleLockMinutes })
        : t("diagnostics.rows.disabled")
    }`,
    `${t("diagnostics.rows.installedVersions")}: ${snapshot.installedThoriumVersions.join(", ") || "none"}`,
    `${t("diagnostics.rows.current")}:           ${snapshot.currentThoriumVersion ?? "none"}`,
    `${t("diagnostics.rows.runningProfiles")}:   ${snapshot.runningProfiles.length}`,
    `${t("diagnostics.rows.clipboardClear")}:    ${t("diagnostics.rows.seconds", { count: snapshot.clipboardClearSeconds })}`,
  ].join("\n");
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}