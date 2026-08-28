/**
 * The Browser page: installing and selecting Thorium versions.
 *
 * The upstream release page is linked next to every action, so a user can always
 * see for themselves what is about to be downloaded.
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "../components/Icon";
import { ConfirmDialog, EmptyState, ErrorNotice, Notice, Progress } from "../components/ui";
import { api, events } from "../lib/api";
import { describeProgress, formatBytes, formatRelative } from "../lib/format";
import { useAsync } from "../lib/hooks";
import type { AppError, AvailableRelease, InstallProgress, InstalledVersion } from "../lib/types";
import type { ToastFn } from "../App";

const CHANNEL_LABELS: Record<string, string> = {
  windows_avx2: "Windows x64 (AVX2)",
  windows_avx: "Windows x64 (AVX)",
  windows_sse3: "Windows x64 (SSE3)",
  windows_arm64: "Windows on ARM (arm64)",
};

export function BrowserPage({ onToast }: { onToast: ToastFn }) {
  const versions = useAsync(() => api.listThoriumVersions(), []);
  const [available, setAvailable] = useState<AvailableRelease | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [removing, setRemoving] = useState<InstalledVersion | null>(null);

  useEffect(() => {
    const unlisten = listen<{ progress: InstallProgress }>(
      events.thoriumInstallProgress,
      (event) => setProgress(event.payload.progress),
    );
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen(events.thoriumChanged, () => versions.reload());
    return () => {
      void unlisten.then((off) => off());
    };
  }, [versions]);

  const check = async () => {
    setChecking(true);
    setError(null);
    try {
      setAvailable(await api.checkForThoriumUpdate());
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setChecking(false);
    }
  };

  const install = async () => {
    setInstalling(true);
    setError(null);
    setProgress({ stage: "resolving" });
    try {
      const installation = await api.installThorium(null);
      onToast(`Installed Thorium ${installation.version}`);
      setAvailable(null);
      versions.reload();
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setInstalling(false);
      setProgress(null);
    }
  };

  const list = versions.data ?? [];
  const describedProgress = progress ? describeProgress(progress) : null;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Browser</h1>
          <div className="subtitle">
            Portable Thorium builds, downloaded and managed inside this workspace.
          </div>
        </div>
        <div className="page-header-actions">
          <button type="button" className="button" onClick={check} disabled={checking || installing}>
            {checking ? <span className="spinner" /> : <Icon name="refresh" />}
            Check for updates
          </button>
          <button
            type="button"
            className="button primary"
            onClick={install}
            disabled={installing || checking}
          >
            {installing ? <span className="spinner" /> : <Icon name="download" />}
            {list.length === 0 ? "Install Thorium" : "Install latest"}
          </button>
        </div>
      </header>

      <div className="page-body stack">
        {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
        {versions.error ? <ErrorNotice error={versions.error} /> : null}

        {installing && describedProgress ? (
          <div className="card">
            <Progress fraction={describedProgress.fraction} label={describedProgress.label} />
            <p className="faint" style={{ marginTop: 10 }}>
              The download is verified and unpacked into a staging folder first. Nothing replaces
              your current version until it has been checked.
            </p>
          </div>
        ) : null}

        {available ? (
          <div className="card stack">
            <div className="card-header">
              <h2>{available.alreadyInstalled ? "Already installed" : "Update available"}</h2>
              <span className="spacer" />
              <span className="badge accent">{available.tag}</span>
            </div>
            <p className="muted">
              {available.name} · <span className="mono">{available.assetName}</span> ·{" "}
              {formatBytes(available.assetSizeBytes)}
            </p>
            <div className="row">
              <button
                type="button"
                className="button small"
                onClick={() => void openUrl(available.htmlUrl)}
              >
                <Icon name="external" size={13} />
                View the release upstream
              </button>
              {!available.alreadyInstalled ? (
                <button
                  type="button"
                  className="button primary small"
                  onClick={install}
                  disabled={installing}
                >
                  <Icon name="download" size={13} />
                  Install {available.tag}
                </button>
              ) : null}
            </div>
          </div>
        ) : null}

        <div className="card">
          <div className="card-header">
            <h2>Installed versions</h2>
            <span className="spacer" />
            {list.length > 1 ? (
              <button
                type="button"
                className="button small"
                onClick={async () => {
                  try {
                    const version = await api.rollbackThorium();
                    onToast(`Switched back to ${version}`);
                    versions.reload();
                  } catch (caught) {
                    onToast((caught as AppError).message, "error");
                  }
                }}
              >
                <Icon name="refresh" size={13} />
                Roll back
              </button>
            ) : null}
          </div>

          {list.length === 0 ? (
            <EmptyState
              icon="browser"
              title="No Thorium installed"
              description="Thorium is downloaded at runtime rather than bundled, so this application stays small and the browser stays current. Installing one takes a few minutes on a normal connection."
              action={
                <button type="button" className="button primary" onClick={install} disabled={installing}>
                  <Icon name="download" />
                  Install Thorium
                </button>
              }
            />
          ) : (
            <div className="scroll-x">
              <table>
                <thead>
                  <tr>
                    <th>Version</th>
                    <th>Channel</th>
                    <th>Installed</th>
                    <th>Status</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {list.map((version) => (
                    <tr key={version.version}>
                      <td>
                        <span className="mono">{version.version}</span>
                        {!version.presentOnDisk ? (
                          <span className="badge danger" style={{ marginLeft: 8 }}>
                            files missing
                          </span>
                        ) : null}
                      </td>
                      <td className="muted">
                        {CHANNEL_LABELS[version.channel] ?? version.channel}
                      </td>
                      <td className="muted">{formatRelative(version.installedAt)}</td>
                      <td>
                        {version.isCurrent ? (
                          <span className="badge success">current</span>
                        ) : version.inUse ? (
                          <span className="badge warning">in use</span>
                        ) : version.pinnedByProfiles > 0 ? (
                          <span className="badge">
                            pinned by {version.pinnedByProfiles}
                          </span>
                        ) : (
                          <span className="faint">available</span>
                        )}
                      </td>
                      <td>
                        <div className="row" style={{ justifyContent: "flex-end", gap: 6 }}>
                          {!version.isCurrent ? (
                            <button
                              type="button"
                              className="button small"
                              onClick={async () => {
                                try {
                                  await api.setCurrentThorium(version.version);
                                  onToast(`${version.version} is now the current version`);
                                  versions.reload();
                                } catch (caught) {
                                  onToast((caught as AppError).message, "error");
                                }
                              }}
                            >
                              Make current
                            </button>
                          ) : null}
                          <button
                            type="button"
                            className="button danger small"
                            onClick={() => setRemoving(version)}
                            disabled={version.inUse || version.pinnedByProfiles > 0}
                            title={
                              version.inUse
                                ? "A running profile is using this version"
                                : version.pinnedByProfiles > 0
                                  ? "A profile is pinned to this version"
                                  : "Remove this version"
                            }
                          >
                            <Icon name="trash" size={13} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <Notice tone="info" title="How updates work here">
          Installing a newer version never deletes the one you have. The previous build stays on
          disk so you can roll back, and a profile pinned to a specific version keeps using it until
          you change that profile.
        </Notice>
      </div>

      {removing ? (
        <ConfirmDialog
          title={`Remove Thorium ${removing.version}?`}
          message="The files for this version are deleted from the workspace. Browser profiles and their data are not affected."
          confirmLabel="Remove version"
          destructive
          onCancel={() => setRemoving(null)}
          onConfirm={async () => {
            try {
              await api.removeThorium(removing.version);
              onToast(`Removed ${removing.version}`);
              versions.reload();
            } catch (caught) {
              onToast((caught as AppError).message, "error");
            } finally {
              setRemoving(null);
            }
          }}
        />
      ) : null}
    </>
  );
}
