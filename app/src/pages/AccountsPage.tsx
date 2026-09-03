// Accounts belong to Browser Profiles. The page is a two-level list: pick a
// profile, then manage its account records. Secret operations route through
// the Vault; when it is locked the page says so, disables every reveal/copy
// path, and any revealed material is dropped (see AccountCard).

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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

const SERVICE_IDS = ["github", "microsoft", "google", "gitlab"] as const;

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
  const { t } = useTranslation();
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
        <PageHeader title={t("accounts.title")} subtitle={t("accounts.subtitle")} />
        <div className="page-body">
          <Loading label={t("common.loading")} />
        </div>
      </>
    );
  }

  if (profiles.length === 0) {
    return (
      <>
        <PageHeader title={t("accounts.title")} subtitle={t("accounts.subtitle")} />
        <div className="page-body">
          <EmptyState
            icon="accounts"
            title={t("accounts.emptyNoProfiles.title")}
            description={t("accounts.emptyNoProfiles.description")}
            action={
              <Button variant="primary" icon="plus" onClick={() => onNavigate("profiles")}>
                {t("accounts.emptyNoProfiles.action")}
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
        title={t("accounts.title")}
        subtitle={
          selectedProfile
            ? t("accounts.subtitleFor", { name: selectedProfile.name })
            : t("accounts.subtitle")
        }
        actions={
          <>
            <select
              value={profileId ?? ""}
              onChange={(event) => setProfileId(event.target.value)}
              aria-label={t("accounts.profileLabel")}
              style={{ width: "auto", minWidth: 160 }}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
            <Button variant="primary" icon="plus" onClick={() => setCreateOpen(true)}>
              {t("accounts.newAccount")}
            </Button>
          </>
        }
      />
      <div className="page-body stack">
        {locked && (
          <Notice tone="info" icon="lock" title={t("shell.vaultChip.locked")}>
            {t("accounts.lockedNotice")}{" "}
            <Button size="small" onClick={() => onNavigate("vault")}>
              {t("accounts.unlockVault")}
            </Button>
          </Notice>
        )}

        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        {accounts === null ? (
          <Loading label={t("common.loading")} />
        ) : accounts.length === 0 ? (
          <EmptyState
            icon="accounts"
            title={t("accounts.emptyNoAccounts.title")}
            description={t("accounts.emptyNoAccounts.description")}
            action={
              <Button variant="primary" icon="plus" onClick={() => setCreateOpen(true)}>
                {t("accounts.emptyNoAccounts.action")}
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
          onClose={() => {
            setCreateOpen(false);
            setEditTarget(null);
          }}
          onSubmit={async (input, account) => {
            try {
              if (account) {
                await api.accountUpdate({ ...account, ...input });
                onToast(t("accounts.toasts.saved", { name: input.displayName }));
              } else if (profileId) {
                await api.accountCreate(profileId, input);
                onToast(t("accounts.toasts.created", { name: input.displayName }));
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
          title={t("accounts.deleteDialog.title", { name: deleteTarget.displayName })}
          message={t("accounts.deleteDialog.message")}
          confirmLabel={t("accounts.deleteDialog.confirm")}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            const target = deleteTarget;
            setDeleteTarget(null);
            void (async () => {
              try {
                await api.accountDelete(target.id);
                onToast(t("accounts.toasts.deleted", { name: target.displayName }));
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
  onClose,
  onSubmit,
}: {
  account: Account | null;
  onClose: () => void;
  onSubmit: (input: AccountInput, account: Account | null) => Promise<void>;
}) {
  const { t } = useTranslation();
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
      title={
        account
          ? t("accounts.dialog.editTitle", { name: account.displayName })
          : t("accounts.dialog.createTitle")
      }
      description={t("accounts.dialog.description")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            type="submit"
            form="account-form"
            disabled={submitting || form.displayName.trim().length === 0}
          >
            {account ? t("accounts.dialog.save") : t("accounts.dialog.create")}
          </Button>
        </>
      }
    >
      <form id="account-form" className="stack" onSubmit={submit}>
        <div className="form-grid">
          <Field label={t("accounts.dialog.displayName")}>
            {(id) => (
              <input
                id={id}
                value={form.displayName}
                onChange={(event) => setForm({ ...form, displayName: event.target.value })}
                placeholder={t("accounts.dialog.displayNamePlaceholder")}
                required
                autoFocus={account === null}
              />
            )}
          </Field>
          <Field label={t("accounts.dialog.service")}>
            {(id) => (
              <select
                id={id}
                value={form.service}
                onChange={(event) => setForm({ ...form, service: event.target.value })}
              >
                {SERVICE_IDS.map((service) => (
                  <option key={service} value={service}>
                    {t(`accounts.services.${service}`)}
                  </option>
                ))}
                <option value="custom">{t("accounts.services.custom")}</option>
              </select>
            )}
          </Field>
          {form.service === "custom" && (
            <Field label={t("accounts.dialog.customServiceLabel")}>
              {(id) => (
                <input
                  id={id}
                  value={form.customLabel}
                  onChange={(event) => setForm({ ...form, customLabel: event.target.value })}
                  placeholder={t("accounts.dialog.customServicePlaceholder")}
                />
              )}
            </Field>
          )}
        </div>
        <div className="form-grid">
          <Field label={t("accounts.dialog.username")}>
            {(id) => (
              <input
                id={id}
                value={form.username}
                onChange={(event) => setForm({ ...form, username: event.target.value })}
                autoComplete="off"
              />
            )}
          </Field>
          <Field label={t("accounts.dialog.email")}>
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
          <Field label={t("accounts.dialog.loginUrl")}>
            {(id) => (
              <input
                id={id}
                type="url"
                value={form.loginUrl}
                onChange={(event) => setForm({ ...form, loginUrl: event.target.value })}
                placeholder={t("accounts.dialog.loginUrlPlaceholder")}
              />
            )}
          </Field>
        </div>
        <Field label={t("accounts.dialog.tags")} hint={t("accounts.dialog.tagsHint")}>
          {(id) => (
            <input
              id={id}
              value={form.tags}
              onChange={(event) => setForm({ ...form, tags: event.target.value })}
              placeholder={t("accounts.dialog.tagsPlaceholder")}
            />
          )}
        </Field>
        <Field label={t("accounts.dialog.notes")}>
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
