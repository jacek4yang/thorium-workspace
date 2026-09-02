import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import {
  DownloadProgress,
  ReleaseOption,
  ThoriumVersionInfo,
  WorkspaceError,
} from "../lib/types";

export default function BrowserPage() {
  const [installed, setInstalled] = useState<ThoriumVersionInfo[] | null>(null);
  const [releases, setReleases] = useState<ReleaseOption[] | null>(null);
  const [variantFilter, setVariantFilter] = useState("AVX2");
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<WorkspaceError | null>(null);

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
        if (active) setInstalled(listed);
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
    setBusyTo(null);
    try {
      setReleases(await api.thoriumDiscover());
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

  const setBusyTo = (value: boolean | null) => {
    if (value === null) {
      setInstalling(false);
      setProgress(null);
    } else {
      setInstalling(value);
    }
  };

  const install = async (release: ReleaseOption) => {
    setInstalling(true);
    setProgress({ downloaded: 0, total: release.sizeBytes });
    try {
      await api.thoriumInstall(release);
      await refresh();
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setInstalling(false);
      setProgress(null);
    }
  };

  return (
    <section aria-labelledby="browser-heading">
      <h2 id="browser-heading">Browser (Thorium)</h2>

      {installed === null ? (
        <p className="muted">Loading installed versions…</p>
      ) : installed.length === 0 ? (
        <p className="muted">
          No Thorium version installed. Check upstream releases and install one
          below.
        </p>
      ) : (
        <ul className="profile-list">
          {installed.map((version) => (
            <li key={version.version} className="card">
              <div className="profile-title">
                <strong>{version.version}</strong>
                {version.isCurrent && <span className="badge">current</span>}
                {version.variant && <span className="badge">{version.variant}</span>}
              </div>
              <div className="row">
                {!version.isCurrent && (
                  <button
                    type="button"
                    onClick={() =>
                      void api
                        .thoriumSetCurrent(version.version)
                        .then(refresh)
                        .catch((thrown) => setError(toError(thrown)))
                    }
                  >
                    Set current
                  </button>
                )}
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    if (
                      window.confirm(
                        `Delete Thorium ${version.version}? The current version and versions used by running profiles are protected.`,
                      )
                    ) {
                      void api
                        .thoriumDelete(version.version)
                        .then(refresh)
                        .catch((thrown) => setError(toError(thrown)));
                    }
                  }}
                >
                  Delete
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {error && <p className="error" role="alert">{error.message}</p>}

      <h3>Upstream releases</h3>
      <div className="row">
        <label>
          Variant
          <select
            value={variantFilter}
            onChange={(event) => setVariantFilter(event.target.value)}
          >
            <option value="AVX2">AVX2</option>
            <option value="AVX">AVX</option>
            <option value="AVX512">AVX512</option>
            <option value="SSE4">SSE4</option>
            <option value="SSE3">SSE3</option>
            <option value="WIN32_SSE2">WIN32_SSE2</option>
          </select>
        </label>
        <button type="button" onClick={() => void discover()} disabled={installing}>
          Check upstream releases
        </button>
      </div>

      {installing && progress && (
        <div>
          <p>
            Downloading… {(progress.downloaded / 1_000_000).toFixed(1)} MB
            {progress.total > 0 &&
              ` of ${(progress.total / 1_000_000).toFixed(1)} MB`}
          </p>
          <progress
            value={progress.total > 0 ? progress.downloaded : undefined}
            max={progress.total > 0 ? progress.total : undefined}
          />
        </div>
      )}

      {releases === null ? (
        <p className="muted">Press “Check upstream releases” to search GitHub.</p>
      ) : releases.length === 0 ? (
        <p className="muted">No portable Windows releases found upstream.</p>
      ) : (
        <ul className="plain">
          {dedupe(releases, variantFilter).map((release) => (
            <li key={release.url}>
              <span className="mono">
                {release.version} · {release.variant} ·{" "}
                {(release.sizeBytes / 1_000_000).toFixed(0)} MB · {release.repo}
              </span>{" "}
              <button
                type="button"
                disabled={installing}
                onClick={() => void install(release)}
              >
                Install
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** Keeps the newest release per version for the selected variant. */
function dedupe(
  releases: ReleaseOption[],
  variant: string,
): ReleaseOption[] {
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

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
