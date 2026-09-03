// Accounts belong to Browser Profiles. The page is a two-level list: pick a
// profile, then manage its account records. Secret operations route through
// the Vault; when it is locked the page says so and hides nothing important,
// but every reveal/copy path is disabled and any revealed material is dropped.

import { useCallback, useEffect, useState } from "react";

import AccountCard from "./AccountCard";
import {
  Button,
  Card,
  ConfirmDialog,
  Dialog,
  EmptyState,
  ErrorNotice,
  Field,
  Loading,
  Notice,
  PageHeader,
} from "../components/ui";
import { api } from "../lib/api";
import type { SectionId } from "../components/Sidebar";
import type { ToastFn } from "../lib/hooks";
import type { Account, AccountInput, BrowserProfile, ServiceKind } from "../lib/types";
import { WorkspaceError } from "../lib/types";

const SERVICES: { id: ServiceKind; label: string }[] = [
  { id: { kind: "github" }, label: "GitHub" },
  { id: { kind: "microsoft" }, label: "Microsoft" },
  { id: { kind: "google" }, label: "Google" },
  { id: { kind: "gitlab" }, label: "GitLab" },
];

const EMPTY_ACCOUNT_FORM = {
  displayName: "",
  service: "github",
  customLabel: "",
  username: "",
  email: "",
  loginUrl: "",
  tags: "",
  notes: "",
};

type AccountForm = typeof EMPTY_ACCOUNT_FORM;

function formToInput(form: AccountForm): AccountInput {
  return {
    displayName: form.displayName.trim(),
    serviceKind:
      form.service === "custom"
        ? { kind: "custom", label: form.customLabel.trim() || "Custom" }
        : ({ kind: form.service } as ServiceKind),
    username: form.username.trim() || null,
    email: form.email.trim() || null,
    loginUrl: form.loginUrl.trim() || null,
    tags: form.tags
      .split(",")
      .map((tag) => tag.trim())
      .filter((tag) => tag.length > 0),
    notes: form.notes,
  };
}

export default function AccountsPage({
  locked,
  onNavigate,
  onToast,
}: {
  locked: boolean;
  onNavigate: (section: SectionId) => void;
  onToast: ToastFn;
}) {
  const [profiles, setProfiles] = useState<BrowserProfile[] | null>(null);
  const [profileId, setProfileId] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Account | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Account | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const listed = await api.profilesList();
        if (active) {
          setProfiles(listed);
          setProfileId(listed[0]?.id ?? null);
        }
      } catch (thrown) {
        if (active) setError(toError(thrown));
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  const loadAccounts = useCallback(async (id: string) => {
    const listed = await api.accountsList(id);
    setAccounts(listed);
  }, []);

  useEffect(() => {
    if (!profileId) return;
    let active = true;
    void (async () => {
      try {
        const listed = await api.accountsList(profileId);
        if (active) {
          setAccounts(listed);
          setError(null);
        }
      } catch (thrown) {
        if (active) setError(toError(thrown));
      }
    })();
    return () => {
      active = false;
    };
  }, [profileId]);

  const reload = useCallback(async () => {
    if (profileId) await loadAccounts(profileId);
  }, [profileId, loadAccounts]);

  if (profiles === null) {
    return (
      <>
        <PageHeader title="Accounts" subtitle="Credentials and second factors per profile" />
        <div className="page-body">
          <Loading label="Loading profiles…" />
        </div>
      </>
    );
  }

  if (profiles.length === 0) {
    return (
      <>
        <PageHeader
          title="Accounts"
          subtitle="Credentials and second factors per profile"
        />
        <div className="page-body">
          <EmptyState
            icon="accounts"
            title="No profiles yet"
            description="Accounts belong to a Browser Profile. Create a profile first, then add its accounts here."
            action={
              <Button variant="primary" icon="plus" onClick={() => onNavigate("profiles")}>
                Go to Profiles
              </Button>
            }
          />
        </div>
      </>
    );
  }

  const selectedProfile = profiles.find((profile) => profile.id === profileId);

  return (
    <>
      <PageHeader
        title="Accounts"
        subtitle={selectedProfile ? `Credentials stored in “${selectedProfile.name}”` : undefined}
        actions={
          <>
            <select
              value={profileId ?? ""}
              onChange={(event) => setProfileId(event.target.value)}
              aria-label="Profile"
              style={{ width: "auto", minWidth: 160 }}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
            <Button
              variant="primary"
              icon="plus"
              onClick={() => setCreateOpen(true)}
            >
              New account
            </Button>
          </>
        }
      />
      <div className="page-body stack">
        {locked && (
          <Notice tone="info" icon="lock" title="Vault locked">
            Unlock the Vault to store passwords, import 2FA factors, or reveal secrets.{" "}
            <Button size="small" onClick={() => onNavigate("vault")}>
              Unlock Vault
            </Button>
          </Notice>
        )}

        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {accounts === null ? (
          <Loading label="Loading accounts…" />
        ) : accounts.length === 0 ? (
          <EmptyState
            icon="accounts"
            title="No accounts in this profile"
            description="An account record holds the service, username, email, encrypted password, 2FA factors, and recovery codes for one login."
            action={
              <Button variant="primary" icon="plus" onClick={() => setCreateOpen(true)}>
                Add an account
              </Button>
            }
          />
        ) : (
          <ul className="stack" style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {accounts.map((account) => (
              <li key={account.id}>
                <Card>
                  <AccountCard
                    account={account}
                    locked={locked}
                    onChanged={reload}
                    onEdit={() => setEditTarget(account)}
                    onDelete={() => setDeleteTarget(account)}
                    onError={setError}
                    onToast={onToast}
                  />
                </Card>
              </li>
            ))}
          </ul>
        )}
      </div>

      {(createOpen || editTarget) && (
        <AccountDialog
          account={editTarget}
          busy={false}
          onClose={() => {
            setCreateOpen(false);
            setEditTarget(null);
          }}
          onSubmit={async (input, account) => {
            try {
              if (account) {
                await api.accountUpdate({ ...account, ...input });
                onToast(`Saved “${input.displayName}”`);
              } else if (profileId) {
                await api.accountCreate(profileId, input);
                onToast(`Created “${input.displayName}”`);
              }
              await reload();
              setError(null);
              setCreateOpen(false);
              setEditTarget(null);
            } catch (thrown) {
              setError(toError(thrown));
            }
          }}
        />
      )}

      {deleteTarget && (
        <ConfirmDialog
          title={`Delete “${deleteTarget.displayName}”?`}
          message="This permanently deletes the account with its stored password, 2FA factors, and recovery codes. This cannot be undone."
          confirmLabel="Delete account"
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget;
            setDeleteTarget(null);
            void (async () => {
              try {
                await api.accountDelete(target.id);
                onToast(`Deleted “${target.displayName}”`);
                await reload();
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

/** Create/edit dialog. `account === null` means create. */
function AccountDialog({
  account,
  busy,
  onClose,
  onSubmit,
}: {
  account: Account | null;
  busy: boolean;
  onClose: () => void;
  onSubmit: (input: AccountInput, account: Account | null) => Promise<void>;
}) {
  const [form, setForm] = useState<AccountForm>(() =>
    account
      ? {
          displayName: account.displayName,
          service:
            account.serviceKind.kind === "custom" ? "custom" : account.serviceKind.kind,
          customLabel: account.serviceKind.kind === "custom" ? account.serviceKind.label : "",
          username: account.username ?? "",
          email: account.email ?? "",
          loginUrl: account.loginUrl ?? "",
          tags: account.tags.join(", "),
          notes: account.notes,
        }
      : EMPTY_ACCOUNT_FORM,
  );
  const [submitting, setSubmitting] = useState(false);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    void onSubmit(formToInput(form), account).finally(() => setSubmitting(false));
  };

  return (
    <Dialog
      wide
      title={account ? `Edit “${account.displayName}”` : "New account"}
      description="Notes and tags are not encrypted metadata; keep secrets in the password field or recovery codes."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={submitting || busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            type="submit"
            form="account-form"
            disabled={submitting || busy || form.displayName.trim().length === 0}
          >
            {account ? "Save changes" : "Create account"}
          </Button>
        </>
      }
    >
      <form id="account-form" className="stack" onSubmit={submit}>
        <div className="form-grid">
          <Field label="Display name">
            {(id) => (
              <input
                id={id}
                value={form.displayName}
                onChange={(event) => setForm({ ...form, displayName: event.target.value })}
                placeholder="Work GitHub"
                required
                autoFocus={account === null}
              />
            )}
          </Field>
          <Field label="Service">
            {(id) => (
              <select
                id={id}
                value={form.service}
                onChange={(event) => setForm({ ...form, service: event.target.value })}
              >
                {SERVICES.map((entry) => (
                  <option key={entry.id.kind} value={entry.id.kind}>
                    {entry.label}
                  </option>
                ))}
                <option value="custom">Custom…</option>
              </select>
            )}
          </Field>
          {form.service === "custom" && (
            <Field label="Custom service label">
              {(id) => (
                <input
                  id={id}
                  value={form.customLabel}
                  onChange={(event) => setForm({ ...form, customLabel: event.target.value })}
                  placeholder="Internal Wiki"
                />
              )}
            </Field>
          )}
        </div>
        <div className="form-grid">
          <Field label="Username">
            {(id) => (
              <input
                id={id}
                value={form.username}
                onChange={(event) => setForm({ ...form, username: event.target.value })}
                autoComplete="off"
              />
            )}
          </Field>
          <Field label="Email">
            {(id) => (
              <input
                id={id}
                type="email"
                value={form.email}
                onChange={(event) => setForm({ ...form, email: event.target.value })}
                autoComplete="off"
              />
            )}
          </Field>
          <Field label="Login URL">
            {(id) => (
              <input
                id={id}
                type="url"
                value={form.loginUrl}
                onChange={(event) => setForm({ ...form, loginUrl: event.target.value })}
                placeholder="https://github.com/login"
              />
            )}
          </Field>
        </div>
        <Field label="Tags" hint="Comma-separated">
          {(id) => (
            <input
              id={id}
              value={form.tags}
              onChange={(event) => setForm({ ...form, tags: event.target.value })}
              placeholder="work, 2fa"
            />
          )}
        </Field>
        <Field label="Notes (non-secret)">
          {(id) => (
            <textarea
              id={id}
              rows={3}
              value={form.notes}
              onChange={(event) => setForm({ ...form, notes: event.target.value })}
            />
          )}
        </Field>
      </form>
    </Dialog>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
