/**
 * The dashboard.
 *
 * Answers "what is the state of this workspace, and what should I do next?" in
 * one screen, and nothing else.
 */
import { api } from "../lib/api";
import { formatRelative } from "../lib/format";
import { useAsync } from "../lib/hooks";
import type { AppError, VaultState } from "../lib/types";
import type { PageId, ToastFn } from "../App";
import { Icon } from "../components/Icon";
import { EmptyState, ErrorNotice, Notice, Stat } from "../components/ui";

export function DashboardPage({
  vault,
  onToast,
  onNavigate,
}: {
  vault: VaultState | null;
  onToast: ToastFn;
  onNavigate: (page: PageId) => void;
}) {
  const profiles = useAsync(() => api.listProfiles(), []);
  const accounts = useAsync(() => api.listAccounts(), []);
  const versions = useAsync(() => api.listThoriumVersions(), []);

  const error = profiles.error ?? accounts.error ?? versions.error;
  const running = profiles.data?.filter((entry) => entry.status === "running").length ?? 0;
  const current = versions.data?.find((version) => version.isCurrent);
  const noBrowser = versions.data !== null && versions.data.length === 0;
  const noProfiles = profiles.data !== null && profiles.data.length === 0;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Dashboard</h1>
          <div className="subtitle">Everything in this workspace, at a glance.</div>
        </div>
        <div className="page-header-actions">
          <button
            type="button"
            className="button"
            onClick={() => {
              profiles.reload();
              accounts.reload();
              versions.reload();
            }}
          >
            <Icon name="refresh" />
            Refresh
          </button>
        </div>
      </header>

      <div className="page-body stack">
        {error ? <ErrorNotice error={error} /> : null}

        <div className="grid">
          <div className="card">
            <Stat value={profiles.data?.length ?? "–"} label="Browser profiles" />
            <p className="faint" style={{ marginTop: 8 }}>
              {running > 0 ? `${running} running now` : "None running"}
            </p>
          </div>
          <div className="card">
            <Stat value={accounts.data?.length ?? "–"} label="Accounts" />
            <p className="faint" style={{ marginTop: 8 }}>
              {vault?.state === "unlocked"
                ? `${vault.secret_count} secrets in the vault`
                : "Unlock the vault to use their secrets"}
            </p>
          </div>
          <div className="card">
            <Stat value={current?.version ?? "None"} label="Thorium version" />
            <p className="faint" style={{ marginTop: 8 }}>
              {current
                ? `Installed ${formatRelative(current.installedAt)}`
                : "No browser installed yet"}
            </p>
          </div>
        </div>

        {noBrowser ? (
          <Notice tone="warning" title="No browser is installed yet">
            Profiles need a Thorium build to launch.{" "}
            <button type="button" className="button small" onClick={() => onNavigate("browser")}>
              Install Thorium
            </button>
          </Notice>
        ) : null}

        {vault?.state === "locked" ? (
          <Notice tone="info" title="The vault is locked">
            Account metadata is still visible, but passwords and one-time codes need the master
            password.{" "}
            <button type="button" className="button small" onClick={() => onNavigate("vault")}>
              Unlock
            </button>
          </Notice>
        ) : null}

        <div className="card">
          <div className="card-header">
            <h2>Browser profiles</h2>
            <span className="spacer" />
            <button type="button" className="button small" onClick={() => onNavigate("profiles")}>
              Manage
            </button>
          </div>
          {noProfiles ? (
            <EmptyState
              icon="profiles"
              title="No profiles yet"
              description="A profile is one isolated browser: its own cookies, its own history, its own accounts. Create one for each identity you keep separate."
              action={
                <button
                  type="button"
                  className="button primary"
                  onClick={() => onNavigate("profiles")}
                >
                  <Icon name="plus" />
                  Create a profile
                </button>
              }
            />
          ) : (
            <div className="list">
              {(profiles.data ?? []).slice(0, 5).map((entry) => (
                <div className="list-item" key={entry.profile.id}>
                  <Icon name="browser" />
                  <div className="grow">
                    <div className="truncate">{entry.profile.name}</div>
                    <div className="faint">
                      {entry.profile.locale} · {entry.profile.timezone} · {entry.accountCount}{" "}
                      account{entry.accountCount === 1 ? "" : "s"}
                    </div>
                  </div>
                  <span className={`badge ${entry.status === "running" ? "success" : ""}`}>
                    {entry.status}
                  </span>
                  <button
                    type="button"
                    className="button small"
                    onClick={async () => {
                      try {
                        const outcome = await api.launchProfile(entry.profile.id);
                        onToast(
                          outcome.started
                            ? `Launched ${entry.profile.name}`
                            : `${entry.profile.name} is already running`,
                        );
                        profiles.reload();
                      } catch (caught) {
                        onToast((caught as AppError).message, "error");
                      }
                    }}
                  >
                    <Icon name="play" size={13} />
                    Launch
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
