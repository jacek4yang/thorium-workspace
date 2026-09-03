// Profiles are the core product object: one profile owns one isolated Thorium
// User Data directory. This page treats them as first-class cards with the
// running state, environment, and actions always visible.

import { useCallback, useEffect, useState } from "react";

import {
  Badge,
  Button,
  Card,
  ConfirmDialog,
  Dialog,
  EmptyState,
  ErrorNotice,
  Field,
  Loading,
  PageHeader,
} from "../components/ui";
import { api } from "../lib/api";
import type { ToastFn } from "../lib/hooks";
import type { BrowserProfile, ThoriumVersionInfo } from "../lib/types";
import { WorkspaceError } from "../lib/types";

const EMPTY_FORM = {
  name: "",
  locale: "",
  timezone: "",
  startupUrls: "",
  pinSelection: "current",
  pinnedVersion: "",
};

export default function ProfilesPage({ onToast }: { onToast: ToastFn }) {
  const [profiles, setProfiles] = useState<BrowserProfile[] | null>(null);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [installed, setInstalled] = useState<ThoriumVersionInfo[]>([]);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [editorTarget, setEditorTarget] = useState<BrowserProfile | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<BrowserProfile | null>(null);
  const [showCreate, setShowCreate] = useState(false);

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
        if (active) setError(toError(thrown));
      }
    })();
    void api
      .thoriumInstalled()
      .then((versions) => {
        if (active) setInstalled(versions);
      })
      .catch(() => {
        /* version pinning list is best-effort; leave empty */
      });
    return () => {
      active = false;
    };
  }, []);

  /** Runs an action against the backend, surfacing failures as toasts.
   * Returns whether it succeeded so dialogs stay open on failure. */
  const run = async (action: () => Promise<void>, okMessage?: string): Promise<boolean> => {
    setBusy(true);
    try {
      await action();
      if (okMessage) onToast(okMessage);
      await refresh();
      setError(null);
      return true;
    } catch (thrown) {
      onToast(toError(thrown).message, "error");
      return false;
    } finally {
      setBusy(false);
    }
  };

  const launch = (profile: BrowserProfile) => {
    setBusyId(profile.id);
    void run(
      async () => {
        await api.profileLaunch(profile.id);
      },
      `Launched “${profile.name}”`,
    ).finally(() => setBusyId(null));
  };

  const stop = (profile: BrowserProfile) => {
    setBusyId(profile.id);
    void run(
      () => api.profileStop(profile.id),
      `Stopped “${profile.name}”`,
    ).finally(() => setBusyId(null));
  };

  return (
    <>
      <PageHeader
        title="Profiles"
        subtitle="Isolated Thorium browser environments, each with its own User Data directory"
        actions={
          <>
            <Button onClick={() => void refresh()} icon="refresh" disabled={busy}>
              Refresh
            </Button>
            <Button variant="primary" icon="plus" onClick={() => setShowCreate(true)}>
              New profile
            </Button>
          </>
        }
      />
      <div className="page-body stack">
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {profiles === null ? (
          <Loading label="Loading profiles…" />
        ) : profiles.length === 0 ? (
          <EmptyState
            icon="profiles"
            title="No profiles yet"
            description="A profile is an isolated Thorium browser environment with its own User Data directory, locale, timezone, and accounts. Nothing is shared between profiles."
            action={
              <Button variant="primary" icon="plus" onClick={() => setShowCreate(true)}>
                Create your first profile
              </Button>
            }
          />
        ) : (
          <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {profiles.map((profile) => (
              <li key={profile.id}>
                <ProfileCard
                  profile={profile}
                  running={running.has(profile.id)}
                  installed={installed}
                  busy={busy}
                  busySelf={busyId === profile.id}
                  onLaunch={() => launch(profile)}
                  onStop={() => stop(profile)}
                  onEdit={() => setEditorTarget(profile)}
                  onDelete={() => setDeleteTarget(profile)}
                />
              </li>
            ))}
          </ul>
        )}
      </div>

      {(showCreate || editorTarget) && (
        <ProfileDialog
          profile={editorTarget}
          installed={installed}
          busy={busy}
          onClose={() => {
            setShowCreate(false);
            setEditorTarget(null);
          }}
          onSubmit={async (input, profile) => {
            const ok = profile
              ? await run(
                  async () => {
                    await api.profileUpdate({ ...profile, ...input });
                  },
                  `Saved “${input.name}”`,
                )
              : await run(async () => {
                  await api.profileCreate(input);
                }, `Created “${input.name}”`);
            if (ok) {
              setShowCreate(false);
              setEditorTarget(null);
            }
          }}
        />
      )}

      {deleteTarget && (
        <ConfirmDialog
          title={`Delete “${deleteTarget.name}”?`}
          message={`This permanently deletes the profile, its accounts, passwords, 2FA factors, and recovery codes. The User Data directory on disk is kept. This cannot be undone.`}
          confirmLabel="Delete profile"
          busy={busy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget;
            setDeleteTarget(null);
            void run(
              () => api.profileDelete(target.id),
              `Deleted “${target.name}”`,
            );
          }}
        />
      )}
    </>
  );
}

function ProfileCard({
  profile,
  running,
  installed,
  busy,
  busySelf,
  onLaunch,
  onStop,
  onEdit,
  onDelete,
}: {
  profile: BrowserProfile;
  running: boolean;
  installed: ThoriumVersionInfo[];
  busy: boolean;
  busySelf: boolean;
  onLaunch: () => void;
  onStop: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const currentVersion = installed.find((entry) => entry.isCurrent)?.version;
  const thoriumLabel =
    profile.thoriumVersion.selection === "pinned"
      ? `Thorium ${profile.thoriumVersion.version} · pinned`
      : currentVersion
        ? `Thorium ${currentVersion} · follows Current`
        : "Thorium · follows Current (not installed)";
  const environment = [profile.locale, profile.timezone].filter(Boolean).join(" · ");

  return (
    <Card>
      <div className="row-wide" style={{ alignItems: "flex-start", flexWrap: "nowrap", gap: "var(--space-4)" }}>
        <div className="grow stack-tight">
          <div className="row" style={{ flexWrap: "nowrap" }}>
            <span
              aria-hidden="true"
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                flex: "none",
                background: running ? "var(--success)" : "var(--text-tertiary)",
              }}
            />
            <strong style={{ fontSize: 15 }}>{profile.name}</strong>
            {running ? (
              <Badge tone="success" icon="play">
                Running
              </Badge>
            ) : (
              <Badge>Stopped</Badge>
            )}
          </div>
          <div className="muted">{thoriumLabel}</div>
          <div className="faint">
            {environment ? `${environment} · ` : ""}
            {profile.accountIds.length} {profile.accountIds.length === 1 ? "account" : "accounts"}
            {profile.startupUrls.length > 0
              ? ` · ${profile.startupUrls.length} startup URL${profile.startupUrls.length === 1 ? "" : "s"}`
              : ""}
          </div>
          <div className="faint mono truncate selectable" title={profile.userDataRelPath}>
            {profile.userDataRelPath}
          </div>
        </div>
        <div className="row" style={{ flexWrap: "nowrap", alignItems: "flex-start" }}>
          <Button size="small" icon="edit" disabled={busy} onClick={onEdit}>
            Edit
          </Button>
          {running ? (
            <Button size="small" icon="stop" disabled={busy} onClick={onStop}>
              Stop
            </Button>
          ) : (
            <Button
              variant="primary"
              size="small"
              icon="play"
              disabled={busy || busySelf}
              onClick={onLaunch}
            >
              Launch
            </Button>
          )}
          <Button size="small" variant="danger" icon="trash" disabled={busy} onClick={onDelete}>
            Delete
          </Button>
        </div>
      </div>
    </Card>
  );
}

/** Create/edit dialog. `profile === null` means create. */
function ProfileDialog({
  profile,
  installed,
  busy,
  onClose,
  onSubmit,
}: {
  profile: BrowserProfile | null;
  installed: ThoriumVersionInfo[];
  busy: boolean;
  onClose: () => void;
  onSubmit: (
    input: {
      name: string;
      thoriumVersion: { selection: "current" } | { selection: "pinned"; version: string };
      startupUrls: string[];
      locale: string | null;
      timezone: string | null;
    },
    profile: BrowserProfile | null,
  ) => Promise<void>;
}) {
  const [form, setForm] = useState(() =>
    profile
      ? {
          name: profile.name,
          locale: profile.locale ?? "",
          timezone: profile.timezone ?? "",
          startupUrls: profile.startupUrls.join("\n"),
          pinSelection: profile.thoriumVersion.selection,
          pinnedVersion:
            profile.thoriumVersion.selection === "pinned"
              ? profile.thoriumVersion.version
              : (installed.find((entry) => entry.isCurrent)?.version ?? ""),
        }
      : EMPTY_FORM,
  );

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const pinned = form.pinSelection === "pinned" ? form.pinnedVersion.trim() : "";
    void onSubmit(
      {
        name: form.name.trim(),
        thoriumVersion:
          form.pinSelection === "pinned" && pinned
            ? { selection: "pinned", version: pinned }
            : { selection: "current" },
        startupUrls: form.startupUrls
          .split("\n")
          .map((line) => line.trim())
          .filter((line) => line.length > 0 && /^https?:\/\//i.test(line)),
        locale: form.locale.trim() || null,
        timezone: form.timezone.trim() || null,
      },
      profile,
    );
  };

  const pinnable = installed.map((entry) => entry.version);

  return (
    <Dialog
      title={profile ? `Edit “${profile.name}”` : "New profile"}
      description={
        profile
          ? "Changes apply to the next launch of this profile."
          : "Each profile gets its own User Data directory; nothing is shared between profiles."
      }
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || form.name.trim().length === 0} form="profile-form" type="submit">
            {profile ? "Save changes" : "Create profile"}
          </Button>
        </>
      }
    >
      <form id="profile-form" className="stack" onSubmit={submit}>
        <Field label="Name">
          {(id) => (
            <input
              id={id}
              value={form.name}
              onChange={(event) => setForm({ ...form, name: event.target.value })}
              placeholder="Personal"
              required
              autoFocus
            />
          )}
        </Field>
        <div className="form-grid">
          <Field label="Locale" hint="BCP-47, applied via Chromium language settings">
            {(id) => (
              <input
                id={id}
                value={form.locale}
                onChange={(event) => setForm({ ...form, locale: event.target.value })}
                placeholder="en-US"
              />
            )}
          </Field>
          <Field label="Timezone" hint="IANA name, e.g. America/Los_Angeles">
            {(id) => (
              <input
                id={id}
                value={form.timezone}
                onChange={(event) => setForm({ ...form, timezone: event.target.value })}
                placeholder="America/Los_Angeles"
              />
            )}
          </Field>
        </div>
        <Field label="Startup URLs" hint="One URL per line, http(s) only">
          {(id) => (
            <textarea
              id={id}
              rows={3}
              value={form.startupUrls}
              onChange={(event) => setForm({ ...form, startupUrls: event.target.value })}
              placeholder={"https://github.com\nhttps://mail.protection.outlook.com"}
            />
          )}
        </Field>
        <div className="form-grid">
          <Field label="Thorium version">
            {(id) => (
              <select
                id={id}
                value={form.pinSelection}
                onChange={(event) =>
                  setForm({
                    ...form,
                    pinSelection: event.target.value as "current" | "pinned",
                  })
                }
              >
                <option value="current">Follow Current</option>
                {pinnable.length > 0 && <option value="pinned">Pin a version…</option>}
              </select>
            )}
          </Field>
          {form.pinSelection === "pinned" && (
            <Field label="Pinned version">
              {(id) => (
                <select
                  id={id}
                  value={form.pinnedVersion}
                  onChange={(event) => setForm({ ...form, pinnedVersion: event.target.value })}
                >
                  {pinnable.length === 0 && <option value="">No versions installed</option>}
                  {pinnable.map((version) => (
                    <option key={version} value={version}>
                      {version}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          )}
        </div>
        {pinnable.length === 0 && (
          <p className="faint">
            No Thorium versions are installed yet; the profile will use the current version once
            one is installed. Install Thorium from the Browser section.
          </p>
        )}
      </form>
    </Dialog>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
