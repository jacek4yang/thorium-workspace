// Thorium lifecycle management. The install pipeline is only ever reported
// from real backend state: releases appear after a live discovery call, the
// download bar is driven by thorium://progress events, and "finalizing" covers
// the extract/promote phase which the backend does not stream separately.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
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
const STAGES: {
  key: InstallStage;
  labelKey: "browser.stages.download" | "browser.stages.extract" | "browser.stages.promote";
}[] = [
  { key: "download", labelKey: "browser.stages.download" },
  { key: "finalize", labelKey: "browser.stages.extract" },
  { key: "finalize", labelKey: "browser.stages.promote" },
];

export default function BrowserPage({ onToast }: { onToast: ToastFn }) {
  const { t } = useTranslation();
  const [installed, setInstalled] = useState<ThoriumVersionInfo[] | null>(null);
  const [releases, setReleases] = useState<ReleaseOption[] | null>(null);
  const [variantFilter, setVariantFilter] = useState("AVX2");
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [stage, setStage] = useState<InstallStage>("idle");
  const [installingVersion, setInstallingVersion] = useState<string | null>(null);
  const [discoverBusy, setDiscoverBusy] = useState(false);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ThoriumVersionInfo | null>(null);
  const [proxyConfigured, setProxyConfigured] = useState<boolean | null>(null);

  useEffect(() => {
    void api
      .settingsGet()
      .then((settings) => setProxyConfigured(Boolean(settings.downloadProxy?.trim())))
      .catch(() => setProxyConfigured(null));
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

  const refresh = async () => {
    try {
      setInstalled(await api.thoriumInstalled());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

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
      onToast(
        t("browser.toasts.installed", { version: release.version, variant: release.variant }),
      );
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
      onToast(t("browser.toasts.current", { version }));
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
        title={t("browser.title")}
        subtitle={t("browser.subtitle")}
        actions={
          <Button onClick={() => void refresh()} icon="refresh" disabled={installing}>
            {t("common.refresh")}
          </Button>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {installing && (
          <Card title={t("browser.installing", { version: installingVersion ?? "" })}>
            <div className="stack-tight">
              <div className="stages">
                {STAGES.map((entry, index) => {
                  const activeIndex = STAGES.findIndex((s) => s.key === stage);
                  const state = index < activeIndex ? "done" : index === activeIndex ? "active" : "";
                  return (
                    <span
                      key={`${entry.labelKey}-${index}`}
                      style={{ display: "inline-flex", alignItems: "center", gap: 8 }}
                    >
                      {index > 0 && <Icon name="chevron" size={12} className="stage-arrow" />}
                      <span className={`stage ${state}`}>{t(entry.labelKey)}</span>
                    </span>
                  );
                })}
              </div>
              {stage === "download" && progress && (
                <Progress
                  fraction={progress.total > 0 ? progress.downloaded / progress.total : null}
                  label={
                    progress.total > 0
                      ? t("browser.downloadedOf", {
                          downloaded: (progress.downloaded / 1_000_000).toFixed(1),
                          total: (progress.total / 1_000_000).toFixed(1),
                        })
                      : `${(progress.downloaded / 1_000_000).toFixed(1)} MB`
                  }
                />
              )}
              {stage === "finalize" && (
                <Progress fraction={null} label={t("browser.finalize")} />
              )}
            </div>
          </Card>
        )}

        {installed === null ? (
          <Loading />
        ) : (
          <>
            <Card
              title={t("browser.current.title")}
              subtitle={t("browser.current.subtitle")}
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
                      <span className="faint">
                        {t("browser.current.installedWhen", {
                          when: formatWhen(current.installedAt),
                        })}
                      </span>
                    )}
                  </div>
                </div>
              ) : (
                <EmptyState
                  icon="download"
                  title={t("browser.current.emptyTitle")}
                  description={t("browser.current.emptyDescription")}
                />
              )}
            </Card>

            {others.length > 0 && (
              <Card
                title={t("browser.others.title")}
                subtitle={t("browser.others.subtitle")}
              >
                <table>
                  <thead>
                    <tr>
                      <th>{t("browser.others.columnVersion")}</th>
                      <th>{t("browser.others.columnVariant")}</th>
                      <th>{t("browser.others.columnInstalled")}</th>
                      <th style={{ textAlign: "right" }}>{t("browser.others.columnActions")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {others.map((version) => (
                      <tr key={version.version}>
                        <td className="mono">{version.version}</td>
                        <td>{version.variant ?? "—"}</td>
                        <td className="faint">
                          {version.installedAt ? formatWhen(version.installedAt) : "—"}
                        </td>
                        <td>
                          <div
                            className="row"
                            style={{ justifyContent: "flex-end", flexWrap: "nowrap" }}
                          >
                            <Button size="small" onClick={() => void setCurrent(version.version)}>
                              {t("browser.others.setCurrent")}
                            </Button>
                            <Button
                              size="small"
                              variant="danger"
                              icon="trash"
                              onClick={() => setDeleteTarget(version)}
                            >
                              {t("common.delete")}
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
          title={t("browser.install.title")}
          subtitle={t("browser.install.subtitle")}
        >
          <div className="stack">
            {proxyConfigured === false && (
              <Notice tone="info" icon="info">
                {t("browser.install.directDownloadHint")}
              </Notice>
            )}
            <div className="row">
              <Field label={t("browser.install.variant")} hint={t("browser.install.variantHint")}>
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
                  {discoverBusy ? t("browser.install.checking") : t("browser.install.check")}
                </Button>
              </div>
            </div>

            {filtered === null ? (
              <p className="faint">{t("browser.install.notChecked")}</p>
            ) : filtered.length === 0 ? (
              <Notice tone="warning" title={t("browser.install.noneFound")}>
                {t("browser.install.noneFoundDescription", { variant: variantFilter })}
              </Notice>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>{t("browser.install.columnVersion")}</th>
                    <th>{t("browser.install.columnSize")}</th>
                    <th>{t("browser.install.columnSource")}</th>
                    <th style={{ textAlign: "right" }}>{t("browser.install.columnAction")}</th>
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
                            {t("browser.install.install")}
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
          title={t("browser.deleteDialog.title", { version: deleteTarget.version })}
          message={t("browser.deleteDialog.message")}
          confirmLabel={t("browser.deleteDialog.confirm")}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget;
            setDeleteTarget(null);
            void (async () => {
              try {
                await api.thoriumDelete(target.version);
                onToast(t("browser.toasts.deleted", { version: target.version }));
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
