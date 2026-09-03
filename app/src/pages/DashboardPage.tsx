// The dashboard answers three questions at a glance: what is the state of the
// workspace, what needs attention, and what can be launched quickly. It only
// reports what the backend actually exposes — no synthetic health checks.

import { useCallback, useEffect, useState } from "react";

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
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null);
  const [profiles, setProfiles] = useState<BrowserProfile[] | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [diagnostics, listed] = await Promise.all([api.diagnostics(), api.profilesList()]);
      setSnapshot(diagnostics);
      setProfiles(listed);
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  }, []);

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
      onToast(`Launched “${profile.name}”`);
    } catch (thrown) {
      onToast(toError(thrown).message, "error");
    } finally {
      setBusyId(null);
      void refresh();
    }
  };

  const stop = async (profile: BrowserProfile) => {
    setBusyId(profile.id);
    try {
      await api.profileStop(profile.id);
      onToast(`Stopped “${profile.name}”`);
    } catch (thrown) {
      onToast(toError(thrown).message, "error");
    } finally {
      setBusyId(null);
      void refresh();
    }
  };

  const health: HealthItem[] | null = snapshot
    ? [
        {
          ok: snapshot.vaultExists && snapshot.vaultLockState === "unlocked",
          label: "Vault",
          detail:
            !snapshot.vaultExists
              ? "Not created yet"
              : snapshot.vaultLockState === "unlocked"
                ? "Unlocked"
                : "Locked",
        },
        {
          ok: snapshot.currentThoriumVersion !== null,
          label: "Thorium",
          detail: snapshot.currentThoriumVersion ?? "Not installed",
        },
        {
          ok: snapshot.workspaceWritable,
          label: "Workspace",
          detail: snapshot.workspaceWritable ? "Writable" : "Not writable",
        },
        {
          ok: snapshot.schemaVersion > 0,
          label: "Database",
          detail: `Schema v${snapshot.schemaVersion}`,
        },
      ]
    : null;

  return (
    <>
      <PageHeader
        title="Dashboard"
        subtitle="Workspace state at a glance"
        actions={
          <Button onClick={() => void refresh()} icon="refresh">
            Refresh
          </Button>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {!snapshot || !profiles ? (
          <Loading label="Reading workspace state…" />
        ) : (
          <>
            <div className="grid-stats">
              <Card>
                <Stat
                  value={profiles.length}
                  label="Profiles"
                  detail={`${running.size} running`}
                />
              </Card>
              <Card>
                <Stat
                  value={running.size}
                  label="Running browsers"
                  detail={running.size === 0 ? "Idle" : "Supervised"}
                />
              </Card>
              <Card>
                <Stat
                  value={profiles.reduce((sum, profile) => sum + profile.accountIds.length, 0)}
                  label="Accounts"
                  detail={snapshot.vaultLockState === "unlocked" ? "Vault unlocked" : "Vault locked"}
                />
              </Card>
              <Card>
                <Stat
                  value={snapshot.currentThoriumVersion ?? "—"}
                  label="Thorium current"
                  detail={
                    snapshot.installedThoriumVersions.length > 0
                      ? `${snapshot.installedThoriumVersions.length} installed`
                      : "Not installed"
                  }
                />
              </Card>
            </div>

            {health && (
              <Card title="Workspace health" subtitle="Live checks against the running workspace">
                <ul className="stack-tight" style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {health.map((item) => (
                    <li key={item.label} className="row" style={{ justifyContent: "space-between" }}>
                      <span className="row" style={{ flexWrap: "nowrap" }}>
                        <Icon
                          name={item.ok ? "check" : "alert"}
                          size={15}
                          style={{ color: item.ok ? "var(--success)" : "var(--warning)" }}
                        />
                        <span>{item.label}</span>
                      </span>
                      <Badge tone={item.ok ? "success" : "warning"}>{item.detail}</Badge>
                    </li>
                  ))}
                </ul>
              </Card>
            )}

            {snapshot.vaultExists && snapshot.vaultLockState !== "unlocked" && (
              <Card>
                <div className="row-wide">
                  <span className="muted">
                    The Vault is {snapshot.vaultLockState}. Account passwords and 2FA are sealed
                    until it is unlocked.
                  </span>
                  <Button onClick={() => onNavigate("vault")} icon="unlock">
                    Unlock Vault
                  </Button>
                </div>
              </Card>
            )}

            {!snapshot.vaultExists && (
              <Card>
                <div className="row-wide">
                  <span className="muted">
                    Create your encrypted Vault to start storing account credentials.
                  </span>
                  <Button variant="primary" onClick={() => onNavigate("vault")} icon="vault">
                    Set up Vault
                  </Button>
                </div>
              </Card>
            )}

            {snapshot.currentThoriumVersion === null && (
              <Card>
                <div className="row-wide">
                  <span className="muted">
                    No Thorium browser is installed yet. Profiles need it before they can launch.
                  </span>
                  <Button onClick={() => onNavigate("browser")} icon="download">
                    Manage Browser
                  </Button>
                </div>
              </Card>
            )}

            <Card
              title="Profiles"
              subtitle="Launch or stop an isolated browser environment"
              actions={
                <Button size="small" onClick={() => onNavigate("profiles")}>
                  Manage profiles
                </Button>
              }
            >
              {profiles.length === 0 ? (
                <EmptyState
                  icon="profiles"
                  title="No profiles yet"
                  description="A profile is an isolated Thorium browser environment with its own User Data directory. Create one to get started."
                  action={
                    <Button variant="primary" icon="plus" onClick={() => onNavigate("profiles")}>
                      Create your first profile
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
                                Running
                              </Badge>
                            )}
                          </div>
                          <div className="faint truncate">
                            {describeSelection(profile)}
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
                            Stop
                          </Button>
                        ) : (
                          <Button
                            variant="primary"
                            size="small"
                            icon="play"
                            disabled={busyId !== null}
                            onClick={() => void launch(profile)}
                          >
                            Launch
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

function describeSelection(profile: BrowserProfile): string {
  return profile.thoriumVersion.selection === "pinned"
    ? `Thorium ${profile.thoriumVersion.version} (pinned)`
    : "Thorium (follows Current)";
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
