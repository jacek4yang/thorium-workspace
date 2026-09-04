// The dashboard answers three questions at a glance: what is the state of the
// workspace, what needs attention, and what can be launched quickly. It only
// reports what the backend actually exposes — no synthetic health checks.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Icon } from "../components/Icon";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  ErrorNotice,
  Loading,
  PageHeader,
  Stat,
} from "../components/ui";
import { api } from "../lib/api";
import { localizedErrorMessage } from "../lib/errors";
import type { SectionId } from "../components/Sidebar";
import type { ToastFn } from "../lib/hooks";
import type { BrowserProfile, DiagnosticsSnapshot } from "../lib/types";
import { WorkspaceError } from "../lib/types";

interface HealthItem {
  ok: boolean;
  label: string;
  detail: string;
}

export default function DashboardPage({
  onNavigate,
  onToast,
}: {
  onNavigate: (section: SectionId) => void;
  onToast: ToastFn;
}) {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [profiles, setProfiles] = useState<BrowserProfile[] | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [diagnostics, listed] = await Promise.all([api.diagnostics(), api.profilesList()]);
        if (active) {
          setSnapshot(diagnostics);
          setProfiles(listed);
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

  const running = new Set(snapshot?.runningProfiles ?? []);

  const launch = async (profile: BrowserProfile) => {
    setBusyId(profile.id);
    try {
      await api.profileLaunch(profile.id);
      onToast(t("dashboard.profilesCard.launchedToast", { name: profile.name }));
    } catch (thrown) {
      onToast(localizedErrorMessage(toError(thrown), t), "error");
    } finally {
      setBusyId(null);
      void refresh();
    }
  };

  const stop = async (profile: BrowserProfile) => {
    setBusyId(profile.id);
    try {
      await api.profileStop(profile.id);
      onToast(t("dashboard.profilesCard.stoppedToast", { name: profile.name }));
    } catch (thrown) {
      onToast(localizedErrorMessage(toError(thrown), t), "error");
    } finally {
      setBusyId(null);
      void refresh();
    }
  };

  const refresh = async () => {
    try {
      const [diagnostics, listed] = await Promise.all([api.diagnostics(), api.profilesList()]);
      setSnapshot(diagnostics);
      setProfiles(listed);
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

  const health: HealthItem[] | null = snapshot
    ? [
        {
          ok: snapshot.vaultExists && snapshot.vaultLockState === "unlocked",
          label: t("dashboard.health.vault"),
          detail: !snapshot.vaultExists
            ? t("dashboard.health.notCreated")
            : snapshot.vaultLockState === "unlocked"
              ? t("dashboard.health.unlocked")
              : t("dashboard.health.locked"),
        },
        {
          ok: snapshot.currentThoriumVersion !== null,
          label: t("dashboard.health.thorium"),
          detail: snapshot.currentThoriumVersion ?? t("dashboard.health.notCreated"),
        },
        {
          ok: snapshot.workspaceWritable,
          label: t("dashboard.health.workspace"),
          detail: snapshot.workspaceWritable
            ? t("dashboard.health.writable")
            : t("dashboard.health.notWritable"),
        },
        {
          ok: snapshot.schemaVersion > 0,
          label: t("dashboard.health.database"),
          detail: t("dashboard.health.schema", { version: snapshot.schemaVersion }),
        },
      ]
    : null;

  return (
    <>
      <PageHeader
        title={t("dashboard.title")}
        subtitle={t("dashboard.subtitle")}
        actions={
          <Button onClick={() => void refresh()} icon="refresh">
            {t("common.refresh")}
          </Button>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {!snapshot || !profiles ? (
          <Loading label={t("dashboard.loading")} />
        ) : (
          <>
            <div className="grid-stats">
              <Card>
                <Stat
                  icon="profiles"
                  value={profiles.length}
                  label={t("dashboard.stats.profiles")}
                  detail={t("dashboard.stats.running", { count: running.size })}
                />
              </Card>
              <Card>
                <Stat
                  icon="play"
                  value={running.size}
                  label={t("dashboard.stats.runningBrowsers")}
                  detail={
                    running.size === 0
                      ? t("dashboard.stats.idle")
                      : t("dashboard.stats.supervised")
                  }
                />
              </Card>
              <Card>
                <Stat
                  icon="accounts"
                  value={profiles.reduce((sum, profile) => sum + profile.accountIds.length, 0)}
                  label={t("dashboard.stats.accounts")}
                  detail={
                    snapshot.vaultLockState === "unlocked"
                      ? t("dashboard.stats.vaultUnlocked")
                      : t("dashboard.stats.vaultLocked")
                  }
                />
              </Card>
              <Card>
                <Stat
                  icon="browser"
                  value={snapshot.currentThoriumVersion ?? "—"}
                  label={t("dashboard.stats.thoriumCurrent")}
                  detail={
                    snapshot.installedThoriumVersions.length > 0
                      ? t("dashboard.stats.installedCount", {
                          count: snapshot.installedThoriumVersions.length,
                        })
                      : t("common.notInstalled")
                  }
                />
              </Card>
            </div>

            {health && (
              <Card
                title={t("dashboard.health.title")}
                subtitle={t("dashboard.health.subtitle")}
              >
                <div
                  className="grid"
                  style={{ gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))" }}
                >
                  {health.map((item) => (
                    <div key={item.label} className="list-row">
                      <Icon
                        name={item.ok ? "check" : "alert"}
                        size={16}
                        style={{
                          color: item.ok ? "var(--success)" : "var(--warning)",
                          flex: "none",
                        }}
                      />
                      <span className="grow">{item.label}</span>
                      <Badge tone={item.ok ? "success" : "warning"}>{item.detail}</Badge>
                    </div>
                  ))}
                </div>
              </Card>
            )}

            {snapshot.vaultExists && snapshot.vaultLockState !== "unlocked" && (
              <Card>
                <div className="row-wide">
                  <span className="muted">{t("dashboard.vaultLockedCard.message")}</span>
                  <Button onClick={() => onNavigate("vault")} icon="unlock">
                    {t("dashboard.vaultLockedCard.action")}
                  </Button>
                </div>
              </Card>
            )}

            {!snapshot.vaultExists && (
              <Card>
                <div className="row-wide">
                  <span className="muted">{t("dashboard.vaultMissingCard.message")}</span>
                  <Button variant="primary" onClick={() => onNavigate("vault")} icon="vault">
                    {t("dashboard.vaultMissingCard.action")}
                  </Button>
                </div>
              </Card>
            )}

            {snapshot.currentThoriumVersion === null && (
              <Card>
                <div className="row-wide">
                  <span className="muted">{t("dashboard.noThoriumCard.message")}</span>
                  <Button onClick={() => onNavigate("browser")} icon="download">
                    {t("dashboard.noThoriumCard.action")}
                  </Button>
                </div>
              </Card>
            )}

            <Card
              title={t("dashboard.profilesCard.title")}
              subtitle={t("dashboard.profilesCard.subtitle")}
              actions={
                <Button size="small" onClick={() => onNavigate("profiles")}>
                  {t("dashboard.profilesCard.manage")}
                </Button>
              }
            >
              {profiles.length === 0 ? (
                <EmptyState
                  icon="profiles"
                  title={t("dashboard.profilesCard.emptyTitle")}
                  description={t("dashboard.profilesCard.emptyDescription")}
                  action={
                    <Button variant="primary" icon="plus" onClick={() => onNavigate("profiles")}>
                      {t("dashboard.profilesCard.emptyAction")}
                    </Button>
                  }
                />
              ) : (
                <ul className="stack-tight" style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {profiles.map((profile) => {
                    const isRunning = running.has(profile.id);
                    return (
                      <li key={profile.id} className="list-row">
                        <div className="grow" style={{ minWidth: 0 }}>
                          <div className="row" style={{ flexWrap: "nowrap" }}>
                            <strong className="truncate">{profile.name}</strong>
                            {isRunning && (
                              <Badge tone="success" icon="play">
                                {t("dashboard.profilesCard.running")}
                              </Badge>
                            )}
                          </div>
                          <div className="faint truncate">
                            {describeSelection(profile, t)}
                            {(profile.locale || profile.timezone) && " · "}
                            {[profile.locale, profile.timezone].filter(Boolean).join(" · ")}
                          </div>
                        </div>
                        {isRunning ? (
                          <Button
                            size="small"
                            icon="stop"
                            disabled={busyId !== null}
                            onClick={() => void stop(profile)}
                          >
                            {t("dashboard.profilesCard.stop")}
                          </Button>
                        ) : (
                          <Button
                            variant="primary"
                            size="small"
                            icon="play"
                            disabled={busyId !== null}
                            onClick={() => void launch(profile)}
                          >
                            {t("dashboard.profilesCard.launch")}
                          </Button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </Card>
          </>
        )}
      </div>
    </>
  );
}

function describeSelection(
  profile: BrowserProfile,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  return profile.thoriumVersion.selection === "pinned"
    ? t("dashboard.profilesCard.pinned", { version: profile.thoriumVersion.version })
    : t("dashboard.profilesCard.followsCurrent");
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
