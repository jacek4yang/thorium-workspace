/**
 * The Profiles page.
 *
 * A profile is one isolated browser. The list makes the isolation visible: each
 * row shows the locale and timezone it will present, and whether it is running.
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Icon } from "../components/Icon";
import { ConfirmDialog, Dialog, EmptyState, ErrorNotice, Field, Notice } from "../components/ui";
import { api, events } from "../lib/api";
import { useAsync } from "../lib/hooks";
import type {
  Account,
  AppError,
  BrowserProfile,
  BrowserProfileDraft,
  InstalledVersion,
  ProfileView,
} from "../lib/types";
import type { PageId, ToastFn } from "../App";

function emptyDraft(): BrowserProfileDraft {
  return {
    name: "",
    thorium: { mode: "current" },
    startup_urls: [],
    locale: "en-US",
    timezone: "UTC",
    account_ids: [],
    notes: "",
  };
}

function toDraft(profile: BrowserProfile): BrowserProfileDraft {
  return {
    name: profile.name,
    thorium: profile.thorium,
    startup_urls: profile.startup_urls,
    locale: profile.locale,
    timezone: profile.timezone,
    account_ids: profile.account_ids,
    notes: profile.notes,
  };
}

export function ProfilesPage({
  onToast,
  onNavigate,
}: {
  onToast: ToastFn;
  locked: boolean;
  onNavigate: (page: PageId) => void;
}) {
  const profiles = useAsync(() => api.listProfiles(), []);
  const accounts = useAsync(() => api.listAccounts(), []);
  const versions = useAsync(() => api.listThoriumVersions(), []);
  const timezones = useAsync(() => api.listTimezones(), []);
  const [editing, setEditing] = useState<{ id: string | null; draft: BrowserProfileDraft } | null>(
    null,
  );
  const [deleting, setDeleting] = useState<ProfileView | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen(events.profilesChanged, () => profiles.reload());
    return () => {
      void unlisten.then((off) => off());
    };
  }, [profiles]);

  const list = profiles.data ?? [];
  const noBrowser = (versions.data?.length ?? 0) === 0;

  const launch = async (view: ProfileView) => {
    setBusyId(view.profile.id);
    try {
      const outcome = await api.launchProfile(view.profile.id);
      onToast(
        outcome.started
          ? `Launched ${view.profile.name}`
          : outcome.focused
            ? `${view.profile.name} is already running — brought it to the front`
            : `${view.profile.name} is already running`,
      );
      profiles.reload();
    } catch (caught) {
      onToast((caught as AppError).message, "error");
    } finally {
      setBusyId(null);
    }
  };

  const stop = async (view: ProfileView) => {
    setBusyId(view.profile.id);
    try {
      await api.stopProfile(view.profile.id);
      onToast(`Stopped ${view.profile.name}`);
      profiles.reload();
    } catch (caught) {
      onToast((caught as AppError).message, "error");
    } finally {
      setBusyId(null);
    }
  };

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Profiles</h1>
          <div className="subtitle">
            Each profile is an independent browser with its own data directory.
          </div>
        </div>
        <div className="page-header-actions">
          <button
            type="button"
            className="button primary"
            onClick={() => setEditing({ id: null, draft: emptyDraft() })}
          >
            <Icon name="plus" />
            New profile
          </button>
        </div>
      </header>

      <div className="page-body stack">
        {profiles.error ? <ErrorNotice error={profiles.error} /> : null}

        {noBrowser && list.length > 0 ? (
          <Notice tone="warning" title="No browser installed">
            These profiles cannot launch until a Thorium version is installed.{" "}
            <button type="button" className="button small" onClick={() => onNavigate("browser")}>
              Install Thorium
            </button>
          </Notice>
        ) : null}

        {list.length === 0 && !profiles.loading ? (
          <EmptyState
            icon="profiles"
            title="No profiles yet"
            description="Create one profile per identity you keep separate. Two profiles never share cookies, history or logged-in sessions, because each one owns its own browser data directory."
            action={
              <button
                type="button"
                className="button primary"
                onClick={() => setEditing({ id: null, draft: emptyDraft() })}
              >
                <Icon name="plus" />
                Create a profile
              </button>
            }
          />
        ) : (
          <div className="grid">
            {list.map((view) => {
              const running = view.status === "running" || view.status === "starting";
              return (
                <div className="card stack" key={view.profile.id}>
                  <div className="row">
                    <Icon name="browser" size={18} />
                    <div className="grow" style={{ minWidth: 0 }}>
                      <h2 className="truncate">{view.profile.name}</h2>
                      <div className="faint">
                        {view.profile.thorium.mode === "pinned"
                          ? `Pinned to ${view.profile.thorium.version}`
                          : "Follows the current version"}
                      </div>
                    </div>
                    <span className={`badge ${running ? "success" : ""}`}>{view.status}</span>
                  </div>

                  <div className="row" style={{ gap: 6 }}>
                    <span className="tag">{view.profile.locale}</span>
                    <span className="tag">{view.profile.timezone}</span>
                    <span className="tag">
                      {view.accountCount} account{view.accountCount === 1 ? "" : "s"}
                    </span>
                    {view.profile.startup_urls.length > 0 ? (
                      <span className="tag">
                        {view.profile.startup_urls.length} startup page
                        {view.profile.startup_urls.length === 1 ? "" : "s"}
                      </span>
                    ) : null}
                  </div>

                  {view.profile.notes ? (
                    <p className="faint selectable">{view.profile.notes}</p>
                  ) : null}

                  <div className="row">
                    {running ? (
                      <button
                        type="button"
                        className="button"
                        onClick={() => void stop(view)}
                        disabled={busyId === view.profile.id}
                      >
                        {busyId === view.profile.id ? (
                          <span className="spinner" />
                        ) : (
                          <Icon name="stop" size={13} />
                        )}
                        Stop
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="button primary"
                        onClick={() => void launch(view)}
                        disabled={busyId === view.profile.id || noBrowser}
                        title={noBrowser ? "Install Thorium first" : undefined}
                      >
                        {busyId === view.profile.id ? (
                          <span className="spinner" />
                        ) : (
                          <Icon name="play" size={13} />
                        )}
                        Launch
                      </button>
                    )}
                    <button
                      type="button"
                      className="button"
                      onClick={() =>
                        setEditing({ id: view.profile.id, draft: toDraft(view.profile) })
                      }
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      className="button danger"
                      onClick={() => setDeleting(view)}
                      disabled={running}
                      title={running ? "Stop the profile first" : "Delete this profile"}
                    >
                      <Icon name="trash" size={13} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {editing ? (
        <ProfileDialog
          draft={editing.draft}
          isNew={editing.id === null}
          accounts={accounts.data ?? []}
          versions={versions.data ?? []}
          timezones={timezones.data ?? []}
          onClose={() => setEditing(null)}
          onSave={async (draft) => {
            try {
              if (editing.id === null) {
                await api.createProfile(draft);
                onToast(`Created ${draft.name}`);
              } else {
                await api.updateProfile(editing.id, draft);
                onToast(`Saved ${draft.name}`);
              }
              setEditing(null);
              profiles.reload();
            } catch (caught) {
              throw caught as AppError;
            }
          }}
        />
      ) : null}

      {deleting ? (
        <DeleteProfileDialog
          view={deleting}
          onCancel={() => setDeleting(null)}
          onConfirm={async (deleteData) => {
            try {
              await api.deleteProfile(deleting.profile.id, deleteData);
              onToast(`Deleted ${deleting.profile.name}`);
              profiles.reload();
            } catch (caught) {
              onToast((caught as AppError).message, "error");
            } finally {
              setDeleting(null);
            }
          }}
        />
      ) : null}
    </>
  );
}

function ProfileDialog({
  draft: initial,
  isNew,
  accounts,
  versions,
  timezones,
  onClose,
  onSave,
}: {
  draft: BrowserProfileDraft;
  isNew: boolean;
  accounts: Account[];
  versions: InstalledVersion[];
  timezones: string[];
  onClose: () => void;
  onSave: (draft: BrowserProfileDraft) => Promise<void>;
}) {
  const [draft, setDraft] = useState(initial);
  const [urls, setUrls] = useState(initial.startup_urls.join("\n"));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSave({
        ...draft,
        startup_urls: urls
          .split("\n")
          .map((line) => line.trim())
          .filter(Boolean),
      });
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={isNew ? "New profile" : "Edit profile"}
      description="A profile's browser data directory is fixed when it is created, so renaming it later never moves or merges browser state."
      onClose={onClose}
      wide
      footer={
        <>
          <button type="button" className="button" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="button primary"
            onClick={save}
            disabled={busy || !draft.name.trim()}
          >
            {busy ? <span className="spinner" /> : null}
            {isNew ? "Create profile" : "Save changes"}
          </button>
        </>
      }
    >
      <Field label="Name">
        {(id) => (
          <input
            id={id}
            type="text"
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            placeholder="Work, Personal, Client A…"
          />
        )}
      </Field>

      <div className="row" style={{ gap: 16, alignItems: "flex-start" }}>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Field label="Locale" hint="Applied to the browser and reported to pages.">
            {(id) => (
              <input
                id={id}
                type="text"
                value={draft.locale ?? ""}
                onChange={(event) => setDraft({ ...draft, locale: event.target.value })}
                placeholder="en-US"
                list="locale-suggestions"
              />
            )}
          </Field>
          <datalist id="locale-suggestions">
            {["en-US", "en-GB", "pl-PL", "de-DE", "fr-FR", "es-ES", "ja-JP", "pt-BR"].map((tag) => (
              <option key={tag} value={tag} />
            ))}
          </datalist>
        </div>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Field label="Timezone" hint="An IANA name, for example Europe/Warsaw.">
            {(id) => (
              <input
                id={id}
                type="text"
                value={draft.timezone ?? ""}
                onChange={(event) => setDraft({ ...draft, timezone: event.target.value })}
                placeholder="UTC"
                list="timezone-suggestions"
              />
            )}
          </Field>
          <datalist id="timezone-suggestions">
            {timezones.map((zone) => (
              <option key={zone} value={zone} />
            ))}
          </datalist>
        </div>
      </div>

      <Field label="Thorium version">
        {(id) => (
          <select
            id={id}
            value={draft.thorium.mode === "pinned" ? draft.thorium.version : "__current__"}
            onChange={(event) =>
              setDraft({
                ...draft,
                thorium:
                  event.target.value === "__current__"
                    ? { mode: "current" }
                    : { mode: "pinned", version: event.target.value },
              })
            }
          >
            <option value="__current__">Follow the current version</option>
            {versions.map((version) => (
              <option key={version.version} value={version.version}>
                Pin to {version.version}
              </option>
            ))}
          </select>
        )}
      </Field>

      <Field label="Startup pages" hint="One URL per line. http, https and about: only.">
        {(id) => (
          <textarea
            id={id}
            value={urls}
            onChange={(event) => setUrls(event.target.value)}
            placeholder="https://example.com/"
            spellCheck={false}
          />
        )}
      </Field>

      <Field label="Accounts in this profile" hint="Which stored accounts belong to this identity.">
        {(id) => (
          <div
            id={id}
            className="list"
            style={{ maxHeight: 180, overflowY: "auto" }}
            role="group"
            aria-label="Accounts in this profile"
          >
            {accounts.length === 0 ? (
              <p className="faint">No accounts yet. Add them on the Accounts page.</p>
            ) : (
              accounts.map((account) => (
                <label className="checkbox" key={account.id}>
                  <input
                    type="checkbox"
                    checked={draft.account_ids.includes(account.id)}
                    onChange={(event) =>
                      setDraft({
                        ...draft,
                        account_ids: event.target.checked
                          ? [...draft.account_ids, account.id]
                          : draft.account_ids.filter((id) => id !== account.id),
                      })
                    }
                  />
                  <span className="checkbox-text">
                    <strong>{account.display_name}</strong>
                    <span className="faint">{account.username ?? account.email ?? ""}</span>
                  </span>
                </label>
              ))
            )}
          </div>
        )}
      </Field>

      <Field label="Notes">
        {(id) => (
          <textarea
            id={id}
            value={draft.notes}
            onChange={(event) => setDraft({ ...draft, notes: event.target.value })}
            style={{ minHeight: 60 }}
            placeholder="What this profile is for. Not a place for passwords."
          />
        )}
      </Field>

      {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
    </Dialog>
  );
}

function DeleteProfileDialog({
  view,
  onCancel,
  onConfirm,
}: {
  view: ProfileView;
  onCancel: () => void;
  onConfirm: (deleteBrowserData: boolean) => Promise<void>;
}) {
  const [deleteData, setDeleteData] = useState(false);
  const [busy, setBusy] = useState(false);

  return (
    <ConfirmDialog
      title={`Delete ${view.profile.name}?`}
      message="The profile and its account associations are removed. The accounts themselves are not deleted."
      confirmLabel="Delete profile"
      destructive
      busy={busy}
      onCancel={onCancel}
      onConfirm={() => {
        setBusy(true);
        void onConfirm(deleteData).finally(() => setBusy(false));
      }}
    >
      <label className="checkbox" style={{ marginTop: 12 }}>
        <input
          type="checkbox"
          checked={deleteData}
          onChange={(event) => setDeleteData(event.target.checked)}
        />
        <span className="checkbox-text">
          <strong>Also delete this profile's browser data</strong>
          <span className="faint">
            Cookies, history, saved sessions and cache. This cannot be undone, and it is usually
            several hundred megabytes.
          </span>
        </span>
      </label>
    </ConfirmDialog>
  );
}
