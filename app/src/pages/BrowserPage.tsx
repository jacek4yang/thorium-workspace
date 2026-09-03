// Thorium lifecycle management. The install pipeline is only ever reported
// from real backend state: releases appear after a live discovery call, the
// download bar is driven by thorium://progress events, and "finalizing" covers
// the extract/promote phase which the backend does not stream separately.

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Icon } from "../components/Icon";
import {
  Badge,
  Button,
  Card,
  ConfirmDialog,
  EmptyState,
  ErrorNotice,
  Field,
  Loading,
  Notice,
  PageHeader,
  Progress,
} from "../components/ui";
import { api } from "../lib/api";
import type { ToastFn } from "../lib/hooks";
import type { DownloadProgress, ReleaseOption, ThoriumVersionInfo } from "../lib/types";
import { WorkspaceError } from "../lib/types";

const VARIANTS = ["AVX2", "AVX", "AVX512", "SSE4", "SSE3", "WIN32_SSE2"];

type InstallStage = "idle" | "download" | "finalize";

/** The visible pipeline. "Ready" is deliberately not a stage: after promote
 * the card is replaced by the updated Current version card, which is the real
 * evidence of readiness. Extract/Promote are not separately streamed by the
 * backend, so they share the honest "finalizing" state. */
const STAGES: { key: InstallStage; label: string }[] = [
  { key: "download", label: "Download" },
  { key: "finalize", label: "Extract" },
  { key: "finalize", label: "Promote" },
];

export default function BrowserPage({ onToast }: { onToast: ToastFn }) {
  const [installed, setInstalled] = useState<ThoriumVersionInfo[] | null>(null);
  const [releases, setReleases] = useState<ReleaseOption[] | null>(null);
  const [variantFilter, setVariantFilter] = useState("AVX2");
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [stage, setStage] = useState<InstallStage>("idle");
  const [installingVersion, setInstallingVersion] = useState<string | null>(null);
  const [discoverBusy, setDiscoverBusy] = useState(false);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ThoriumVersionInfo | null>(null);

  const refresh = useCallback(async () => {
    try {
      setInstalled(await api.thoriumInstalled());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const listed = await api.thoriumInstalled();
        if (active) {
          setInstalled(listed);
          setError(null);
        }
      } catch (thrown) {
        if (active) setError(toError(thrown));
      }
    })();
    const unlisten = listen<DownloadProgress>("thorium://progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      active = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  const discover = async () => {
    setDiscoverBusy(true);
    try {
      setReleases(await api.thoriumDiscover());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setDiscoverBusy(false);
    }
  };

  const install = async (release: ReleaseOption) => {
    setError(null);
    setStage("download");
    setInstallingVersion(release.version);
    setProgress({ downloaded: 0, total: release.sizeBytes });
    try {
      await api.thoriumInstall(release);
      onToast(`Thorium ${release.version} (${release.variant}) installed`);
      await refresh();
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setStage("idle");
      setInstallingVersion(null);
      setProgress(null);
    }
  };

  const setCurrent = async (version: string) => {
    try {
      await api.thoriumSetCurrent(version);
      onToast(`Thorium ${version} is now Current`);
      await refresh();
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

  const current = installed?.find((entry) => entry.isCurrent) ?? null;
  const others = installed?.filter((entry) => !entry.isCurrent) ?? [];
  const installing = stage !== "idle";
  const filtered = releases ? dedupe(releases, variantFilter) : null;

  return (
    <>
      <PageHeader
        title="Browser"
        subtitle="Portable Thorium installations, managed beside this workspace"
        actions={
          <Button onClick={() => void refresh()} icon="refresh" disabled={installing}>
            Refresh
          </Button>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {installing && (
          <Card title={`Installing Thorium ${installingVersion ?? ""}`}>
            <div className="stack-tight">
              <div className="stages">
                {STAGES.map((entry, index) => {
                  const activeIndex = STAGES.findIndex((s) => s.key === stage);
                  const state = index < activeIndex ? "done" : index === activeIndex ? "active" : "";
                  return (
                    <span key={`${entry.label}-${index}`} style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                      {index > 0 && (
                        <Icon name="chevron" size={12} className="stage-arrow" />
                      )}
                      <span className={`stage ${state}`}>{entry.label}</span>
                    </span>
                  );
                })}
              </div>
              {stage === "download" && progress && (
                <Progress
                  fraction={progress.total > 0 ? progress.downloaded / progress.total : null}
                  label={`${(progress.downloaded / 1_000_000).toFixed(1)} MB${
                    progress.total > 0 ? ` of ${(progress.total / 1_000_000).toFixed(1)} MB` : ""
                  } downloaded`}
                />
              )}
              {stage === "finalize" && (
                <Progress fraction={null} label="Extracting and promoting the staged install…" />
              )}
            </div>
          </Card>
        )}

        {installed === null ? (
          <Loading label="Loading installed versions…" />
        ) : (
          <>
            <Card
              title="Current version"
              subtitle="Profiles that follow Current use this version"
            >
              {current ? (
                <div className="row-wide">
                  <div className="row" style={{ flexWrap: "nowrap" }}>
                    <Icon name="check" size={18} style={{ color: "var(--success)" }} />
                    <strong style={{ fontSize: 16 }} className="mono">
                      {current.version}
                    </strong>
                    {current.variant && <Badge tone="accent">{current.variant}</Badge>}
                    {current.installedAt && (
                      <span className="faint">installed {formatWhen(current.installedAt)}</span>
                    )}
                  </div>
                </div>
              ) : (
                <EmptyState
                  icon="download"
                  title="No Thorium version installed"
                  description="Discover upstream portable releases below and install the variant matching your CPU. The install is staged and atomically promoted, so a failed update never destroys the last working version."
                />
              )}
            </Card>

            {others.length > 0 && (
              <Card title="Other installed versions" subtitle="Previous versions are kept as a known-good fallback">
                <table>
                  <thead>
                    <tr>
                      <th>Version</th>
                      <th>Variant</th>
                      <th>Installed</th>
                      <th style={{ textAlign: "right" }}>Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {others.map((version) => (
                      <tr key={version.version}>
                        <td className="mono">{version.version}</td>
                        <td>{version.variant ?? "—"}</td>
                        <td className="faint">{version.installedAt ? formatWhen(version.installedAt) : "—"}</td>
                        <td>
                          <div className="row" style={{ justifyContent: "flex-end", flexWrap: "nowrap" }}>
                            <Button size="small" onClick={() => void setCurrent(version.version)}>
                              Set current
                            </Button>
                            <Button
                              size="small"
                              variant="danger"
                              icon="trash"
                              onClick={() => setDeleteTarget(version)}
                            >
                              Delete
                            </Button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </Card>
            )}
          </>
        )}

        <Card
          title="Install a new version"
          subtitle="Discovery queries the live upstream release list"
        >
          <div className="stack">
            <div className="row">
              <Field label="Variant" hint="Pick the newest instruction set your CPU supports">
                {(id) => (
                  <select
                    id={id}
                    value={variantFilter}
                    onChange={(event) => setVariantFilter(event.target.value)}
                  >
                    {VARIANTS.map((variant) => (
                      <option key={variant} value={variant}>
                        {variant}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
              <div style={{ alignSelf: "flex-end" }}>
                <Button
                  onClick={() => void discover()}
                  disabled={installing || discoverBusy}
                  icon="refresh"
                >
                  {discoverBusy ? "Checking…" : "Check upstream releases"}
                </Button>
              </div>
            </div>

            {filtered === null ? (
              <p className="faint">
                Press “Check upstream releases” to list portable Windows builds.
              </p>
            ) : filtered.length === 0 ? (
              <Notice tone="warning" title="No releases found">
                No portable Windows {variantFilter} builds were found upstream. Try another
                variant.
              </Notice>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>Version</th>
                    <th>Size</th>
                    <th>Source</th>
                    <th style={{ textAlign: "right" }}>Action</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((release) => (
                    <tr key={release.url}>
                      <td className="mono">{release.version}</td>
                      <td className="faint">{(release.sizeBytes / 1_000_000).toFixed(0)} MB</td>
                      <td className="faint mono">{release.repo}</td>
                      <td>
                        <div className="row" style={{ justifyContent: "flex-end" }}>
                          <Button
                            variant="primary"
                            size="small"
                            icon="download"
                            disabled={installing}
                            onClick={() => void install(release)}
                          >
                            Install
                          </Button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      </div>

      {deleteTarget && (
        <ConfirmDialog
          title={`Delete Thorium ${deleteTarget.version}?`}
          message="The current version and versions used by running profiles are protected and cannot be deleted. Deleting a fallback version cannot be undone without re-downloading it."
          confirmLabel="Delete version"
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget;
            setDeleteTarget(null);
            void (async () => {
              try {
                await api.thoriumDelete(target.version);
                onToast(`Deleted Thorium ${target.version}`);
                await refresh();
              } catch (thrown) {
                setError(toError(thrown));
              }
            })();
          }}
        />
      )}
    </>
  );
}

/** Keeps the newest release per version for the selected variant. */
function dedupe(releases: ReleaseOption[], variant: string): ReleaseOption[] {
  const seen = new Set<string>();
  const result: ReleaseOption[] = [];
  for (const release of releases) {
    if (release.variant !== variant) continue;
    if (seen.has(release.version)) continue;
    seen.add(release.version);
    result.push(release);
  }
  return result;
}

function formatWhen(iso: string): string {
  const parsed = new Date(iso);
  return Number.isNaN(parsed.getTime()) ? iso : parsed.toLocaleDateString();
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
