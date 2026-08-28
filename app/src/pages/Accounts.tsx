/**
 * The Accounts page.
 *
 * The densest screen in the product: a list of accounts on the left, and
 * everything about the selected one on the right — password, second factors with
 * live codes, and recovery codes.
 *
 * Secrets are only ever fetched for display when the user presses "show", and
 * copying goes through the backend so a copied password never enters this
 * process at all.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Icon } from "../components/Icon";
import {
  CodeRing,
  ConfirmDialog,
  Dialog,
  EmptyState,
  ErrorNotice,
  Field,
  Notice,
} from "../components/ui";
import { api } from "../lib/api";
import { formatDate, groupCode, serviceLabel } from "../lib/format";
import { useAsync, useTicker } from "../lib/hooks";
import type {
  Account,
  AccountDraft,
  AppError,
  OtpCode,
  RecoveryCode,
  SecondFactor,
  ServiceKind,
  ServicePreset,
} from "../lib/types";
import type { PageId, ToastFn } from "../App";

function emptyDraft(): AccountDraft {
  return {
    display_name: "",
    service: { kind: "other", label: "Other" },
    username: null,
    email: null,
    login_url: null,
    tags: [],
    notes: "",
  };
}

function toDraft(account: Account): AccountDraft {
  return {
    display_name: account.display_name,
    service: account.service,
    username: account.username,
    email: account.email,
    login_url: account.login_url,
    tags: account.tags,
    notes: account.notes,
  };
}

export function AccountsPage({
  onToast,
  locked,
  onNavigate,
}: {
  onToast: ToastFn;
  locked: boolean;
  onNavigate: (page: PageId) => void;
}) {
  const accounts = useAsync(() => api.listAccounts(), []);
  const presets = useAsync(() => api.listServicePresets(), []);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<{ id: string | null; draft: AccountDraft } | null>(null);
  const [deleting, setDeleting] = useState<Account | null>(null);

  const list = useMemo(() => accounts.data ?? [], [accounts.data]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return list;
    return list.filter((account) =>
      [account.display_name, account.username, account.email, serviceLabel(account.service), ...account.tags]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(needle)),
    );
  }, [list, query]);

  // The selection is derived rather than synchronised: an account that was
  // deleted, or filtered out, falls back to the first visible one without an
  // effect having to notice and correct it.
  const effectiveId =
    selectedId !== null && filtered.some((account) => account.id === selectedId)
      ? selectedId
      : (filtered[0]?.id ?? null);
  const selected = list.find((account) => account.id === effectiveId) ?? null;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Accounts</h1>
          <div className="subtitle">
            Credentials and second factors, stored encrypted and grouped by profile.
          </div>
        </div>
        <div className="page-header-actions">
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search accounts…"
            aria-label="Search accounts"
            style={{ width: 220 }}
          />
          <button
            type="button"
            className="button primary"
            onClick={() => setEditing({ id: null, draft: emptyDraft() })}
          >
            <Icon name="plus" />
            New account
          </button>
        </div>
      </header>

      <div className="page-body">
        {accounts.error ? <ErrorNotice error={accounts.error} /> : null}

        {list.length === 0 && !accounts.loading ? (
          <EmptyState
            icon="accounts"
            title="No accounts yet"
            description="An account holds a username, an optional password, its second factors and its recovery codes. Passwords and one-time-password secrets go into the encrypted vault, never into the database."
            action={
              <button
                type="button"
                className="button primary"
                onClick={() => setEditing({ id: null, draft: emptyDraft() })}
              >
                <Icon name="plus" />
                Add an account
              </button>
            }
          />
        ) : (
          <div className="split">
            <div className="list">
              {filtered.length === 0 ? (
                <p className="faint">Nothing matches “{query}”.</p>
              ) : (
                filtered.map((account) => (
                  <button
                    type="button"
                    key={account.id}
                    className="list-item"
                    aria-selected={account.id === effectiveId}
                    onClick={() => setSelectedId(account.id)}
                  >
                    <Icon name="accounts" />
                    <span className="grow" style={{ minWidth: 0 }}>
                      <span className="truncate" style={{ display: "block" }}>
                        {account.display_name}
                      </span>
                      <span className="faint truncate" style={{ display: "block" }}>
                        {serviceLabel(account.service)}
                        {account.username ? ` · ${account.username}` : ""}
                      </span>
                    </span>
                    {account.password_ref ? <Icon name="key" size={13} /> : null}
                  </button>
                ))
              )}
            </div>

            {selected ? (
              <AccountDetail
                // Keyed on the lock state as well as the account: locking the
                // vault remounts this subtree, which discards every revealed
                // password and recovery code. Without it a value revealed
                // before an idle auto-lock would reappear on unlock without
                // the user asking for it again.
                key={`${selected.id}:${locked ? "locked" : "open"}`}
                account={selected}
                locked={locked}
                onToast={onToast}
                onNavigate={onNavigate}
                onEdit={() => setEditing({ id: selected.id, draft: toDraft(selected) })}
                onDelete={() => setDeleting(selected)}
                onChanged={accounts.reload}
              />
            ) : null}
          </div>
        )}
      </div>

      {editing ? (
        <AccountDialog
          draft={editing.draft}
          isNew={editing.id === null}
          presets={presets.data ?? []}
          locked={locked}
          onClose={() => setEditing(null)}
          onSave={async (draft, password) => {
            if (editing.id === null) {
              const created = await api.createAccount(draft, password);
              setSelectedId(created.id);
              onToast(`Added ${created.display_name}`);
            } else {
              await api.updateAccount(editing.id, draft);
              if (password !== null) await api.setAccountPassword(editing.id, password);
              onToast(`Saved ${draft.display_name}`);
            }
            setEditing(null);
            accounts.reload();
          }}
        />
      ) : null}

      {deleting ? (
        <ConfirmDialog
          title={`Delete ${deleting.display_name}?`}
          message="The account, its second factors and its recovery codes are removed, and their stored secrets are deleted from the vault. This cannot be undone."
          confirmLabel="Delete account"
          destructive
          onCancel={() => setDeleting(null)}
          onConfirm={async () => {
            try {
              await api.deleteAccount(deleting.id);
              onToast(`Deleted ${deleting.display_name}`);
              setSelectedId(null);
              accounts.reload();
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

function AccountDetail({
  account,
  locked,
  onToast,
  onNavigate,
  onEdit,
  onDelete,
  onChanged,
}: {
  account: Account;
  locked: boolean;
  onToast: ToastFn;
  onNavigate: (page: PageId) => void;
  onEdit: () => void;
  onDelete: () => void;
  onChanged: () => void;
}) {
  const factors = useAsync(() => api.listFactors(account.id), [account.id]);
  const codes = useAsync(() => api.listRecoveryCodes(account.id), [account.id]);
  const [revealed, setRevealed] = useState<string | null>(null);
  const [addingFactor, setAddingFactor] = useState(false);
  const [addingCodes, setAddingCodes] = useState(false);

  return (
    <div className="stack">
      <div className="card stack">
        <div className="card-header">
          <h2>{account.display_name}</h2>
          <span className="spacer" />
          <button type="button" className="button small" onClick={onEdit}>
            Edit
          </button>
          <button type="button" className="button danger small" onClick={onDelete}>
            <Icon name="trash" size={13} />
          </button>
        </div>

        <div className="row" style={{ gap: 6 }}>
          <span className="badge accent">{serviceLabel(account.service)}</span>
          {account.tags.map((tag) => (
            <span className="tag" key={tag}>
              {tag}
            </span>
          ))}
        </div>

        <table>
          <tbody>
            {account.username ? (
              <DetailRow label="Username" value={account.username} onToast={onToast} />
            ) : null}
            {account.email ? (
              <DetailRow label="Email" value={account.email} onToast={onToast} />
            ) : null}
            {account.login_url ? (
              <DetailRow label="Sign-in page" value={account.login_url} onToast={onToast} />
            ) : null}
            <tr>
              <th style={{ paddingTop: 8 }}>Password</th>
              <td>
                {!account.password_ref ? (
                  <span className="faint">Not stored</span>
                ) : locked ? (
                  <button type="button" className="button small" onClick={() => onNavigate("vault")}>
                    <Icon name="lock" size={13} />
                    Unlock the vault
                  </button>
                ) : (
                  <div className="row" style={{ gap: 6 }}>
                    <span className="mono selectable">
                      {revealed ?? "••••••••••••"}
                    </span>
                    <button
                      type="button"
                      className="button ghost small"
                      aria-label={revealed ? "Hide password" : "Show password"}
                      onClick={async () => {
                        if (revealed) {
                          setRevealed(null);
                          return;
                        }
                        try {
                          setRevealed(await api.revealAccountPassword(account.id));
                        } catch (caught) {
                          onToast((caught as AppError).message, "error");
                        }
                      }}
                    >
                      <Icon name={revealed ? "eye-off" : "eye"} size={14} />
                    </button>
                    <button
                      type="button"
                      className="button ghost small"
                      aria-label="Copy password"
                      onClick={async () => {
                        try {
                          await api.copyAccountPassword(account.id);
                          onToast("Password copied — it will be cleared automatically");
                        } catch (caught) {
                          onToast((caught as AppError).message, "error");
                        }
                      }}
                    >
                      <Icon name="copy" size={14} />
                    </button>
                  </div>
                )}
              </td>
            </tr>
          </tbody>
        </table>

        {account.notes ? <p className="muted selectable">{account.notes}</p> : null}
      </div>

      <div className="card stack">
        <div className="card-header">
          <h2>Second factors</h2>
          <span className="spacer" />
          <button
            type="button"
            className="button small"
            onClick={() => setAddingFactor(true)}
            disabled={locked}
            title={locked ? "Unlock the vault first" : undefined}
          >
            <Icon name="plus" size={13} />
            Add
          </button>
        </div>

        {factors.error ? <ErrorNotice error={factors.error} /> : null}

        {(factors.data ?? []).length === 0 ? (
          <p className="faint">
            No second factors yet. Import one from a QR code, or type the secret in by hand.
          </p>
        ) : (
          <div className="list">
            {(factors.data ?? []).map((factor) => (
              <FactorRow
                key={factor.id}
                factor={factor}
                locked={locked}
                onToast={onToast}
                onDeleted={() => {
                  factors.reload();
                  onChanged();
                }}
              />
            ))}
          </div>
        )}
      </div>

      <div className="card stack">
        <div className="card-header">
          <h2>Recovery codes</h2>
          <span className="spacer" />
          <button
            type="button"
            className="button small"
            onClick={() => setAddingCodes(true)}
            disabled={locked}
            title={locked ? "Unlock the vault first" : undefined}
          >
            <Icon name="plus" size={13} />
            Add codes
          </button>
        </div>

        {(codes.data ?? []).length === 0 ? (
          <p className="faint">
            No recovery codes stored. These are the one-time codes an issuer gives you when you turn
            on two-factor authentication.
          </p>
        ) : (
          <RecoveryCodeList
            codes={codes.data ?? []}
            locked={locked}
            onToast={onToast}
            onChanged={codes.reload}
          />
        )}
      </div>

      {addingFactor ? (
        <AddFactorDialog
          accountId={account.id}
          onClose={() => setAddingFactor(false)}
          onAdded={(label) => {
            setAddingFactor(false);
            onToast(`Added ${label}`);
            factors.reload();
          }}
        />
      ) : null}

      {addingCodes ? (
        <AddRecoveryCodesDialog
          accountId={account.id}
          onClose={() => setAddingCodes(false)}
          onAdded={(count) => {
            setAddingCodes(false);
            onToast(`Stored ${count} recovery code${count === 1 ? "" : "s"}`);
            codes.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function DetailRow({
  label,
  value,
  onToast,
}: {
  label: string;
  value: string;
  onToast: ToastFn;
}) {
  return (
    <tr>
      <th style={{ paddingTop: 8 }}>{label}</th>
      <td>
        <div className="row" style={{ gap: 6 }}>
          <span className="truncate selectable">{value}</span>
          <button
            type="button"
            className="button ghost small"
            aria-label={`Copy ${label.toLowerCase()}`}
            onClick={async () => {
              await api.copyPlainValue(value);
              onToast(`${label} copied`);
            }}
          >
            <Icon name="copy" size={14} />
          </button>
        </div>
      </td>
    </tr>
  );
}

function FactorRow({
  factor,
  locked,
  onToast,
  onDeleted,
}: {
  factor: SecondFactor;
  locked: boolean;
  onToast: ToastFn;
  onDeleted: () => void;
}) {
  const [code, setCode] = useState<OtpCode | null>(null);
  const [confirming, setConfirming] = useState(false);
  const isTotp = factor.kind === "otp" && factor.otp?.kind === "totp";
  const period = factor.otp?.period_seconds ?? 30;
  const tick = useTicker(isTotp && code !== null && !locked);

  const refresh = useCallback(async () => {
    if (locked || factor.kind !== "otp") return;
    try {
      setCode(await api.generateCode(factor.id));
    } catch (caught) {
      onToast((caught as AppError).message, "error");
      setCode(null);
    }
  }, [factor.id, factor.kind, locked, onToast]);

  // A TOTP code is only valid for its step, so the next fetch is scheduled from
  // the expiry the backend reported rather than polled for.
  useEffect(() => {
    if (!isTotp || locked || code === null) return;
    const delay = Math.max(500, (code.valid_for_seconds ?? 0) * 1000 + 250);
    const timer = setTimeout(() => void refresh(), delay);
    return () => clearTimeout(timer);
  }, [code, isTotp, locked, refresh]);

  const remaining = Math.max(0, (code?.valid_for_seconds ?? 0) - tick);

  if (factor.kind === "external_authenticator") {
    return (
      <div className="list-item">
        <Icon name="alert" />
        <div className="grow">
          <div>{factor.label}</div>
          <div className="faint">
            Handled by another app or device. No code is generated here.
          </div>
        </div>
        <button
          type="button"
          className="button danger small"
          onClick={() => setConfirming(true)}
          aria-label="Delete factor"
        >
          <Icon name="trash" size={13} />
        </button>
        {confirming ? (
          <ConfirmDialog
            title={`Delete ${factor.label}?`}
            message="This only removes the record here. The factor stays active on the service."
            confirmLabel="Delete"
            destructive
            onCancel={() => setConfirming(false)}
            onConfirm={async () => {
              await api.deleteFactor(factor.id);
              setConfirming(false);
              onDeleted();
            }}
          />
        ) : null}
      </div>
    );
  }

  return (
    <div className="list-item">
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="truncate">{factor.label}</div>
        <div className="faint">
          {factor.otp?.kind.toUpperCase()} · {factor.otp?.algorithm} · {factor.otp?.digits} digits
          {factor.otp?.kind === "totp" ? ` · ${period}s` : ` · counter ${factor.otp?.counter}`}
        </div>
      </div>

      {locked ? (
        <span className="badge warning">
          <Icon name="lock" size={11} />
          locked
        </span>
      ) : code ? (
        <div className="code-display">
          {isTotp ? <CodeRing remaining={remaining} period={period} /> : null}
          <span className="code-value selectable">{groupCode(code.code)}</span>
          <button
            type="button"
            className="button ghost small"
            aria-label="Copy code"
            onClick={async () => {
              try {
                const copied = await api.copyCode(factor.id);
                setCode(copied);
                onToast("Code copied — it will be cleared automatically");
              } catch (caught) {
                onToast((caught as AppError).message, "error");
              }
            }}
          >
            <Icon name="copy" size={14} />
          </button>
        </div>
      ) : (
        <button type="button" className="button small" onClick={() => void refresh()}>
          Show code
        </button>
      )}

      <button
        type="button"
        className="button danger small"
        onClick={() => setConfirming(true)}
        aria-label="Delete factor"
      >
        <Icon name="trash" size={13} />
      </button>

      {confirming ? (
        <ConfirmDialog
          title={`Delete ${factor.label}?`}
          message="The stored secret is removed from the vault. If this is your only second factor for the account, make sure you have recovery codes first."
          confirmLabel="Delete factor"
          destructive
          onCancel={() => setConfirming(false)}
          onConfirm={async () => {
            await api.deleteFactor(factor.id);
            setConfirming(false);
            onDeleted();
          }}
        />
      ) : null}
    </div>
  );
}

function RecoveryCodeList({
  codes,
  locked,
  onToast,
  onChanged,
}: {
  codes: RecoveryCode[];
  locked: boolean;
  onToast: ToastFn;
  onChanged: () => void;
}) {
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const unused = codes.filter((code) => !code.used).length;

  return (
    <>
      {unused <= 2 ? (
        <Notice tone="warning" title={unused === 0 ? "All codes used" : "Running low"}>
          {unused === 0
            ? "Generate a fresh set at the service and replace these."
            : `Only ${unused} unused code${unused === 1 ? "" : "s"} left.`}
        </Notice>
      ) : null}
      <div className="scroll-x">
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th>Code</th>
              <th>Status</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {codes.map((code) => (
              <tr key={code.id}>
                <td className="faint mono">{code.position + 1}</td>
                <td>
                  <span className="mono selectable">{revealed[code.id] ?? "••••-••••"}</span>
                </td>
                <td>
                  {code.used ? (
                    <span className="badge">used {formatDate(code.used_at)}</span>
                  ) : (
                    <span className="badge success">unused</span>
                  )}
                </td>
                <td>
                  <div className="row" style={{ justifyContent: "flex-end", gap: 4 }}>
                    <button
                      type="button"
                      className="button ghost small"
                      disabled={locked}
                      aria-label={revealed[code.id] ? "Hide code" : "Show code"}
                      onClick={async () => {
                        if (revealed[code.id]) {
                          setRevealed(({ [code.id]: _removed, ...rest }) => rest);
                          return;
                        }
                        try {
                          const value = await api.revealRecoveryCode(code.id);
                          setRevealed((current) => ({ ...current, [code.id]: value }));
                        } catch (caught) {
                          onToast((caught as AppError).message, "error");
                        }
                      }}
                    >
                      <Icon name={revealed[code.id] ? "eye-off" : "eye"} size={14} />
                    </button>
                    <button
                      type="button"
                      className="button ghost small"
                      disabled={locked}
                      aria-label="Copy code"
                      onClick={async () => {
                        try {
                          await api.copyRecoveryCode(code.id);
                          onToast("Recovery code copied — it will be cleared automatically");
                        } catch (caught) {
                          onToast((caught as AppError).message, "error");
                        }
                      }}
                    >
                      <Icon name="copy" size={14} />
                    </button>
                    <button
                      type="button"
                      className="button small"
                      onClick={async () => {
                        await api.setRecoveryCodeUsed(code.id, !code.used);
                        onChanged();
                      }}
                    >
                      {code.used ? "Mark unused" : "Mark used"}
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function AccountDialog({
  draft: initial,
  isNew,
  presets,
  locked,
  onClose,
  onSave,
}: {
  draft: AccountDraft;
  isNew: boolean;
  presets: ServicePreset[];
  locked: boolean;
  onClose: () => void;
  onSave: (draft: AccountDraft, password: string | null) => Promise<void>;
}) {
  const [draft, setDraft] = useState(initial);
  const [password, setPassword] = useState("");
  const [tags, setTags] = useState(initial.tags.join(", "));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const applyPreset = (preset: ServicePreset) => {
    setDraft((current) => ({
      ...current,
      service: preset.kind.kind === "other" ? { kind: "other", label: "" } : preset.kind,
      login_url: preset.login_url || current.login_url,
    }));
  };

  const note = presets.find((preset) => {
    const kind: ServiceKind = draft.service ?? { kind: "other", label: "" };
    return preset.kind.kind === kind.kind;
  })?.note;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSave(
        {
          ...draft,
          tags: tags
            .split(",")
            .map((tag) => tag.trim())
            .filter(Boolean),
        },
        password.length > 0 ? password : null,
      );
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
      setPassword("");
    }
  };

  return (
    <Dialog
      title={isNew ? "New account" : "Edit account"}
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
            disabled={busy || !draft.display_name.trim()}
          >
            {busy ? <span className="spinner" /> : null}
            {isNew ? "Add account" : "Save changes"}
          </button>
        </>
      }
    >
      <Field label="Service">
        {(id) => (
          <div className="row" id={id}>
            {presets.map((preset) => (
              <button
                key={preset.id}
                type="button"
                className={
                  (draft.service?.kind ?? "other") === preset.kind.kind
                    ? "button primary small"
                    : "button small"
                }
                onClick={() => applyPreset(preset)}
              >
                {preset.name}
              </button>
            ))}
          </div>
        )}
      </Field>

      {draft.service?.kind === "other" ? (
        <Field label="Service name">
          {(id) => (
            <input
              id={id}
              type="text"
              value={draft.service?.kind === "other" ? draft.service.label : ""}
              onChange={(event) =>
                setDraft({ ...draft, service: { kind: "other", label: event.target.value } })
              }
              placeholder="Fastmail, Cloudflare, an internal system…"
            />
          )}
        </Field>
      ) : null}

      {note ? <p className="faint">{note}</p> : null}

      <Field label="Display name">
        {(id) => (
          <input
            id={id}
            type="text"
            value={draft.display_name}
            onChange={(event) => setDraft({ ...draft, display_name: event.target.value })}
            placeholder="What you call this account"
          />
        )}
      </Field>

      <div className="row" style={{ gap: 16, alignItems: "flex-start" }}>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Field label="Username">
            {(id) => (
              <input
                id={id}
                type="text"
                value={draft.username ?? ""}
                onChange={(event) => setDraft({ ...draft, username: event.target.value || null })}
              />
            )}
          </Field>
        </div>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Field label="Email">
            {(id) => (
              <input
                id={id}
                type="email"
                value={draft.email ?? ""}
                onChange={(event) => setDraft({ ...draft, email: event.target.value || null })}
              />
            )}
          </Field>
        </div>
      </div>

      <Field label="Sign-in page">
        {(id) => (
          <input
            id={id}
            type="url"
            value={draft.login_url ?? ""}
            onChange={(event) => setDraft({ ...draft, login_url: event.target.value || null })}
            placeholder="https://…"
          />
        )}
      </Field>

      <Field
        label={isNew ? "Password" : "New password"}
        hint={
          locked
            ? "Unlock the vault to store a password."
            : isNew
              ? "Optional. Stored encrypted in the vault, never in the database."
              : "Leave blank to keep the current password."
        }
      >
        {(id) => (
          <input
            id={id}
            type="password"
            value={password}
            autoComplete="new-password"
            disabled={locked}
            onChange={(event) => setPassword(event.target.value)}
          />
        )}
      </Field>

      <Field label="Tags" hint="Comma separated.">
        {(id) => (
          <input
            id={id}
            type="text"
            value={tags}
            onChange={(event) => setTags(event.target.value)}
            placeholder="work, ci"
          />
        )}
      </Field>

      <Field label="Notes" hint="Not a place for passwords or recovery codes.">
        {(id) => (
          <textarea
            id={id}
            value={draft.notes}
            onChange={(event) => setDraft({ ...draft, notes: event.target.value })}
            style={{ minHeight: 60 }}
          />
        )}
      </Field>

      {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
    </Dialog>
  );
}

function AddFactorDialog({
  accountId,
  onClose,
  onAdded,
}: {
  accountId: string;
  onClose: () => void;
  onAdded: (label: string) => void;
}) {
  const [mode, setMode] = useState<"qr" | "manual" | "external">("qr");
  const [uri, setUri] = useState("");
  const [label, setLabel] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const run = async (action: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    try {
      onAdded(await action());
    } catch (caught) {
      setError(caught as AppError);
    } finally {
      setBusy(false);
      setUri("");
      setSecret("");
    }
  };

  return (
    <Dialog
      title="Add a second factor"
      description="Standards-based one-time passwords only. Vendor push approvals and number matching are not one-time passwords and are recorded, not emulated."
      onClose={onClose}
      wide
      footer={
        <button type="button" className="button" onClick={onClose} disabled={busy}>
          Close
        </button>
      }
    >
      <div className="row">
        {(
          [
            ["qr", "From a QR code"],
            ["manual", "Type the secret"],
            ["external", "Another app or device"],
          ] as const
        ).map(([id, text]) => (
          <button
            key={id}
            type="button"
            className={mode === id ? "button primary small" : "button small"}
            onClick={() => setMode(id)}
          >
            {text}
          </button>
        ))}
      </div>

      {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}

      {mode === "qr" ? (
        <div className="stack">
          <p className="muted">
            The QR code an issuer shows when you turn on two-factor authentication contains the
            shared secret. It is read here and stored encrypted; it is never logged or written to
            disk in the clear.
          </p>
          <div className="row">
            <button
              type="button"
              className="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const path = await open({
                    multiple: false,
                    directory: false,
                    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "bmp", "gif", "webp"] }],
                  });
                  if (typeof path !== "string") throw { code: "TW-0404", message: "No file chosen", remedy: null };
                  return (await api.importOtpFromImageFile(accountId, path)).label;
                })
              }
            >
              <Icon name="image" />
              From an image file
            </button>
            <button
              type="button"
              className="button"
              disabled={busy}
              onClick={() =>
                void run(async () => (await api.importOtpFromClipboard(accountId)).label)
              }
            >
              <Icon name="clipboard" />
              From the clipboard
            </button>
            <button
              type="button"
              className="button"
              disabled={busy}
              onClick={() => void run(async () => (await api.importOtpFromScreen(accountId)).label)}
            >
              <Icon name="screen" />
              Scan the screen
            </button>
          </div>
          <Field label="Or paste the otpauth:// link" hint="Some issuers offer this instead of a QR code.">
            {(id) => (
              <input
                id={id}
                type="text"
                value={uri}
                onChange={(event) => setUri(event.target.value)}
                placeholder="otpauth://totp/…"
                spellCheck={false}
              />
            )}
          </Field>
          <div>
            <button
              type="button"
              className="button primary"
              disabled={busy || !uri.trim()}
              onClick={() =>
                void run(async () => {
                  const factor = await api.addOtpFactorFromUri(accountId, uri.trim(), null);
                  return factor.label;
                })
              }
            >
              {busy ? <span className="spinner" /> : null}
              Add from link
            </button>
          </div>
        </div>
      ) : null}

      {mode === "manual" ? (
        <div className="stack">
          <Field label="Label">
            {(id) => (
              <input
                id={id}
                type="text"
                value={label}
                onChange={(event) => setLabel(event.target.value)}
                placeholder="Authenticator app"
              />
            )}
          </Field>
          <Field label="Secret" hint="The Base32 secret the issuer shows next to the QR code.">
            {(id) => (
              <input
                id={id}
                type="text"
                value={secret}
                onChange={(event) => setSecret(event.target.value)}
                spellCheck={false}
                autoComplete="off"
                className="mono"
              />
            )}
          </Field>
          <div>
            <button
              type="button"
              className="button primary"
              disabled={busy || !label.trim() || !secret.trim()}
              onClick={() =>
                void run(async () => {
                  const factor = await api.addOtpFactorManual(
                    accountId,
                    label.trim(),
                    {
                      kind: "totp",
                      algorithm: "SHA1",
                      digits: 6,
                      period_seconds: 30,
                      counter: 0,
                      issuer: null,
                      account_label: null,
                    },
                    secret.trim(),
                  );
                  return factor.label;
                })
              }
            >
              {busy ? <span className="spinner" /> : null}
              Add factor
            </button>
          </div>
        </div>
      ) : null}

      {mode === "external" ? (
        <div className="stack">
          <Notice tone="info" title="Recorded, not emulated">
            A vendor push approval, a number-matching prompt or a hardware security key is not a
            one-time password. This records that the factor exists so you know what protects the
            account; no code is produced here.
          </Notice>
          <Field label="Label">
            {(id) => (
              <input
                id={id}
                type="text"
                value={label}
                onChange={(event) => setLabel(event.target.value)}
                placeholder="Microsoft Authenticator push"
              />
            )}
          </Field>
          <div>
            <button
              type="button"
              className="button primary"
              disabled={busy || !label.trim()}
              onClick={() =>
                void run(async () => {
                  const factor = await api.addExternalFactor(accountId, {
                    label: label.trim(),
                    kind: "external_authenticator",
                    otp: null,
                  });
                  return factor.label;
                })
              }
            >
              {busy ? <span className="spinner" /> : null}
              Record factor
            </button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}

function AddRecoveryCodesDialog({
  accountId,
  onClose,
  onAdded,
}: {
  accountId: string;
  onClose: () => void;
  onAdded: (count: number) => void;
}) {
  const [pasted, setPasted] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  return (
    <Dialog
      title="Add recovery codes"
      description="Paste the whole list. Numbering and blank lines are ignored."
      onClose={onClose}
      footer={
        <>
          <button type="button" className="button" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="button primary"
            disabled={busy || !pasted.trim()}
            onClick={async () => {
              setBusy(true);
              setError(null);
              try {
                const stored = await api.addRecoveryCodes(accountId, pasted);
                onAdded(stored.length);
              } catch (caught) {
                setError(caught as AppError);
              } finally {
                setBusy(false);
                setPasted("");
              }
            }}
          >
            {busy ? <span className="spinner" /> : null}
            Store codes
          </button>
        </>
      }
    >
      <Field label="Codes" hint="One per line. Each is stored encrypted in the vault.">
        {(id) => (
          <textarea
            id={id}
            value={pasted}
            onChange={(event) => setPasted(event.target.value)}
            spellCheck={false}
            autoComplete="off"
            style={{ minHeight: 160 }}
          />
        )}
      </Field>
      {error ? <ErrorNotice error={error} onDismiss={() => setError(null)} /> : null}
    </Dialog>
  );
}
