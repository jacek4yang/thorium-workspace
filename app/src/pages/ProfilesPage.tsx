import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { BrowserProfile, WorkspaceError } from "../lib/types";

const EMPTY_FORM = {
  name: "",
  locale: "en-US",
  timezone: "America/Los_Angeles",
  pinnedVersion: "",
};

export default function ProfilesPage() {
  const [profiles, setProfiles] = useState<BrowserProfile[] | null>(null);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [form, setForm] = useState(EMPTY_FORM);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [listed, active] = await Promise.all([api.profilesList(), api.runningProfiles()]);
      setProfiles(listed);
      setRunning(new Set(active));
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [listed, activeSessions] = await Promise.all([
          api.profilesList(),
          api.runningProfiles(),
        ]);
        if (active) {
          setProfiles(listed);
          setRunning(new Set(activeSessions));
          setError(null);
        }
      } catch (thrown) {
        if (active) {
          setError(toError(thrown));
        }
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      setForm(EMPTY_FORM);
      await refresh();
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="profiles-heading">
      <h2 id="profiles-heading">Profiles</h2>

      <form
        className="card"
        onSubmit={(event) => {
          event.preventDefault();
          void run(async () => {
            const pinned = form.pinnedVersion.trim();
            await api.profileCreate({
              name: form.name,
              thoriumVersion: pinned
                ? { selection: "pinned", version: pinned }
                : { selection: "current" },
              startupUrls: [],
              locale: form.locale.trim() || null,
              timezone: form.timezone.trim() || null,
            });
          });
        }}
      >
        <h3>New profile</h3>
        <label>
          Name
          <input
            value={form.name}
            onChange={(event) => setForm({ ...form, name: event.target.value })}
            placeholder="Test Profile A"
            required
          />
        </label>
        <label>
          Locale (BCP-47, optional)
          <input
            value={form.locale}
            onChange={(event) => setForm({ ...form, locale: event.target.value })}
            placeholder="en-US"
          />
        </label>
        <label>
          Timezone (IANA, optional)
          <input
            value={form.timezone}
            onChange={(event) => setForm({ ...form, timezone: event.target.value })}
            placeholder="America/Los_Angeles"
          />
        </label>
        <label>
          Pin Thorium version (optional)
          <input
            value={form.pinnedVersion}
            onChange={(event) => setForm({ ...form, pinnedVersion: event.target.value })}
            placeholder="leave empty to follow Current"
          />
        </label>
        <button type="submit" disabled={busy}>
          Create profile
        </button>
      </form>

      {error && <p className="error" role="alert">{error.message}</p>}

      {profiles === null ? (
        <p className="muted">Loading profiles…</p>
      ) : profiles.length === 0 ? (
        <p className="muted">No profiles yet. Create your first profile above.</p>
      ) : (
        <ul className="profile-list">
          {profiles.map((profile) => (
            <li key={profile.id} className="card">
              <div className="profile-title">
                <strong>{profile.name}</strong>
                {running.has(profile.id) && <span className="badge">running</span>}
              </div>
              <dl>
                <dt>Thorium</dt>
                <dd>
                  {profile.thoriumVersion.selection === "pinned"
                    ? `pinned ${profile.thoriumVersion.version}`
                    : "current"}
                </dd>
                <dt>Locale</dt>
                <dd>{profile.locale ?? "—"}</dd>
                <dt>Timezone</dt>
                <dd>{profile.timezone ?? "—"}</dd>
                <dt>User data</dt>
                <dd className="mono">{profile.userDataRelPath}</dd>
              </dl>
              <div className="row">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      await api.profileLaunch(profile.id);
                    })
                  }
                >
                  Launch
                </button>
                {running.has(profile.id) && (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void run(() => api.profileStop(profile.id))}
                  >
                    Stop
                  </button>
                )}
                <button
                  type="button"
                  className="danger"
                  disabled={busy}
                  onClick={() => {
                    if (
                      window.confirm(
                        `Delete profile "${profile.name}" and its accounts? This cannot be undone.`,
                      )
                    ) {
                      void run(() => api.profileDelete(profile.id));
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
    </section>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
