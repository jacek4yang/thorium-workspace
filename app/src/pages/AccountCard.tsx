// One account record, presented with deliberate hierarchy: identity first,
// secrets behind explicit actions, technical detail in collapsible sections.
// Secret values never live here longer than they are visibly needed: when the
// vault locks, all revealed material is dropped immediately.

import { useEffect, useState } from "react";

import { Icon } from "../components/Icon";
import {
  Badge,
  Button,
  CodeRing,
  Disclosure,
  Notice,
} from "../components/ui";
import { api } from "../lib/api";
import type { ToastFn } from "../lib/hooks";
import type {
  Account,
  OtpCode,
  RecoveryCode,
  SecondFactor,
  ServiceKind,
} from "../lib/types";
import { WorkspaceError } from "../lib/types";

const SERVICES: { id: ServiceKind; label: string }[] = [
  { id: { kind: "github" }, label: "GitHub" },
  { id: { kind: "microsoft" }, label: "Microsoft" },
  { id: { kind: "google" }, label: "Google" },
  { id: { kind: "gitlab" }, label: "GitLab" },
];

export function serviceLabel(kind: ServiceKind): string {
  return kind.kind === "custom"
    ? kind.label
    : (SERVICES.find((entry) => entry.id.kind === kind.kind)?.label ?? kind.kind);
}

export default function AccountCard({
  account,
  locked,
  onChanged,
  onEdit,
  onDelete,
  onError,
  onToast,
}: {
  account: Account;
  locked: boolean;
  /** Called after any mutation so the page can reload the list. */
  onChanged: () => Promise<void>;
  onEdit: () => void;
  onDelete: () => void;
  onError: (error: WorkspaceError | null) => void;
  onToast: ToastFn;
}) {
  const [revealed, setRevealed] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  const [openPassword, setOpenPassword] = useState(false);
  const [openFactors, setOpenFactors] = useState(false);
  const [openRecovery, setOpenRecovery] = useState(false);

  // Vault lock (idle timer, minimize, manual) must instantly remove any
  // secret material from the UI. React's documented render-time adjustment
  // pattern is used instead of an effect so the reset happens in the same
  // commit that notices the lock.
  const [prevLocked, setPrevLocked] = useState(locked);
  if (locked !== prevLocked) {
    setPrevLocked(locked);
    if (locked) {
      setRevealed(null);
      setNewPassword("");
      setOpenPassword(false);
      setOpenFactors(false);
      setOpenRecovery(false);
    }
  }

  const run = async (action: () => Promise<void>, okMessage?: string) => {
    try {
      await action();
      if (okMessage) onToast(okMessage);
      await onChanged();
      onError(null);
    } catch (thrown) {
      onError(toError(thrown));
    }
  };

  return (
    <div className="stack-tight">
      <div className="row-wide">
        <div className="grow stack-tight" style={{ minWidth: 0 }}>
          <div className="row" style={{ flexWrap: "nowrap" }}>
            <strong style={{ fontSize: 15 }}>{account.displayName}</strong>
            <Badge tone="accent">{serviceLabel(account.serviceKind)}</Badge>
            {account.passwordRef ? (
              <Badge icon="key">Password</Badge>
            ) : (
              <Badge tone="warning">No password</Badge>
            )}
            {account.factors.length > 0 && <Badge>2FA · {account.factors.length}</Badge>}
          </div>
          <div className="muted truncate">
            {account.username ?? "no username"}
            {account.username && account.email ? " · " : ""}
            {account.email ?? ""}
          </div>
          {account.loginUrl && (
            <div className="faint mono truncate">{account.loginUrl}</div>
          )}
          {account.tags.length > 0 && (
            <div className="row">
              {account.tags.map((tag) => (
                <span key={tag} className="tag">
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="row" style={{ flexWrap: "nowrap", alignItems: "flex-start" }}>
          {account.passwordRef && (
            <>
              <Button
                size="small"
                icon="clipboard"
                disabled={locked}
                title="Copy password (clipboard clears automatically)"
                onClick={() =>
                  void run(async () => {
                    const seconds = await api.passwordCopy(account.id);
                    onToast(`Password copied — clipboard clears in ${seconds}s`);
                  })
                }
              >
                Copy
              </Button>
              <Button
                size="small"
                icon={revealed === null ? "eye" : "eye-off"}
                disabled={locked}
                onClick={() =>
                  void run(async () => {
                    if (revealed === null) {
                      setRevealed(await api.passwordReveal(account.id));
                    } else {
                      setRevealed(null);
                    }
                  })
                }
              >
                {revealed === null ? "Reveal" : "Hide"}
              </Button>
            </>
          )}
          <Button size="small" icon="edit" onClick={onEdit}>
            Edit
          </Button>
          <Button size="small" variant="danger" icon="trash" onClick={onDelete}>
            Delete
          </Button>
        </div>
      </div>

      {revealed !== null && (
        <div className="list-row selectable mono" style={{ fontSize: 14 }}>
          {revealed}
        </div>
      )}

      {locked && account.passwordRef && (
        <Notice tone="info" icon="lock" title="Vault locked">
          Unlock the Vault to copy or reveal this password.
        </Notice>
      )}

      <div>
        <Disclosure
          label="Password"
          meta={account.passwordRef ? "stored" : "not set"}
          open={openPassword}
          onToggle={() => setOpenPassword(!openPassword)}
        >
          <div className="stack-tight">
            <div className="row">
              <input
                type="password"
                placeholder="New password"
                value={newPassword}
                onChange={(event) => setNewPassword(event.target.value)}
                style={{ maxWidth: 260 }}
                autoComplete="new-password"
              />
              <Button
                disabled={newPassword.length === 0}
                onClick={() =>
                  void run(
                    async () => {
                      await api.passwordSet(account.id, newPassword);
                      setNewPassword("");
                    },
                    "Password stored",
                  )
                }
              >
                Set password
              </Button>
              {account.passwordRef && (
                <Button
                  variant="danger"
                  onClick={() =>
                    void run(() => api.passwordDelete(account.id), "Password removed")
                  }
                >
                  Remove password
                </Button>
              )}
            </div>
            <p className="faint">
              Passwords are encrypted in the Vault and never written to the profile or logs.
            </p>
          </div>
        </Disclosure>

        <Disclosure
          label="Second factors"
          meta={
            account.factors.length > 0
              ? `${account.factors.length} configured`
              : "none"
          }
          open={openFactors}
          onToggle={() => setOpenFactors(!openFactors)}
        >
          <FactorsSection
            account={account}
            locked={locked}
            onChanged={onChanged}
            onError={onError}
            onToast={onToast}
          />
        </Disclosure>

        <Disclosure
          label="Recovery codes"
          meta={`${account.recoveryCodes.filter((code) => !code.used).length} unused`}
          open={openRecovery}
          onToggle={() => setOpenRecovery(!openRecovery)}
        >
          <RecoverySection
            accountId={account.id}
            locked={locked}
            onChanged={onChanged}
            onError={onError}
            onToast={onToast}
          />
        </Disclosure>
      </div>
    </div>
  );
}

/* ---- Second factors ------------------------------------------------------ */

function FactorsSection({
  account,
  locked,
  onChanged,
  onError,
  onToast,
}: {
  account: Account;
  locked: boolean;
  onChanged: () => Promise<void>;
  onError: (error: WorkspaceError | null) => void;
  onToast: ToastFn;
}) {
  const [otpUri, setOtpUri] = useState("");
  const [externalLabel, setExternalLabel] = useState("");
  const [liveCodeId, setLiveCodeId] = useState<string | null>(null);

  const run = async (action: () => Promise<void>, okMessage?: string) => {
    try {
      await action();
      if (okMessage) onToast(okMessage);
      await onChanged();
      onError(null);
    } catch (thrown) {
      onError(toError(thrown));
    }
  };

  return (
    <div className="stack-tight">
      {account.factors.length === 0 && (
        <p className="faint">
          No second factors. Import a standard otpauth:// URI or a QR image exported by another
          authenticator.
        </p>
      )}
      <ul className="stack-tight" style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {account.factors.map((factor) => (
          <li key={factor.id} className="list-row">
            <FactorRow
              factor={factor}
              expanded={liveCodeId === factor.id}
              locked={locked}
              onToggle={() => setLiveCodeId(liveCodeId === factor.id ? null : factor.id)}
              onRemoved={() =>
                void run(() => api.factorDelete(factor.id), "Factor removed")
              }
            />
          </li>
        ))}
      </ul>

      <div className="row">
        <input
          placeholder="otpauth://totp/…"
          value={otpUri}
          onChange={(event) => setOtpUri(event.target.value)}
          style={{ maxWidth: 280 }}
          aria-label="otpauth URI"
        />
        <Button
          size="small"
          disabled={otpUri.trim().length === 0}
          onClick={() =>
            void run(async () => {
              await api.factorImportOtpauth(account.id, otpUri);
              setOtpUri("");
            }, "Factor imported")
          }
        >
          Import URI
        </Button>
        <Button
          size="small"
          icon="image"
          onClick={() =>
            void (async () => {
              const { open } = await import("@tauri-apps/plugin-dialog");
              const selection = await open({
                filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "bmp"] }],
              });
              if (typeof selection === "string") {
                await run(
                  async () => {
                    await api.factorImportQrFile(account.id, selection);
                  },
                  "Factor imported from QR",
                );
              }
            })()
          }
        >
          Import QR image…
        </Button>
      </div>

      <div className="row">
        <input
          placeholder="External authenticator label (e.g. Microsoft Authenticator push)"
          value={externalLabel}
          onChange={(event) => setExternalLabel(event.target.value)}
          style={{ maxWidth: 280 }}
          aria-label="External authenticator label"
        />
        <Button
          size="small"
          disabled={externalLabel.trim().length === 0}
          onClick={() =>
            void run(async () => {
              await api.factorAddExternal(account.id, externalLabel.trim(), null);
              setExternalLabel("");
            }, "External authenticator reference added")
          }
        >
          Add external reference
        </Button>
      </div>
      <p className="faint">
        External authenticators (push, number matching, passkeys) are represented as references
        only — their secrets never enter this workspace.
      </p>
    </div>
  );
}

function FactorRow({
  factor,
  expanded,
  locked,
  onToggle,
  onRemoved,
}: {
  factor: SecondFactor;
  expanded: boolean;
  locked: boolean;
  onToggle: () => void;
  onRemoved: () => void;
}) {
  const kindLabel =
    factor.kind === "external"
      ? "External"
      : factor.kind.toUpperCase();
  const detail = [
    factor.issuer,
    factor.accountLabel,
    factor.algorithm,
    factor.digits ? `${factor.digits} digits` : null,
    factor.kind === "totp" && factor.periodSeconds ? `${factor.periodSeconds}s` : null,
    factor.kind === "hotp" && factor.counter !== null ? `counter ${factor.counter}` : null,
    factor.externalNote ?? undefined,
  ]
    .filter(Boolean)
    .join(" · ");

  if (factor.kind === "external") {
    return (
      <>
        <Icon name="shield" size={15} />
        <div className="grow">
          <div>{factor.label ?? "External authenticator"}</div>
          {detail && <div className="faint">{detail}</div>}
        </div>
        <Button size="small" variant="danger" icon="trash" onClick={onRemoved} aria-label="Remove factor">
          Remove
        </Button>
      </>
    );
  }

  return (
    <>
      <Icon name="clock" size={15} />
      <div className="grow">
        <div className="row" style={{ flexWrap: "nowrap" }}>
          <span>{factor.label ?? factor.issuer ?? kindLabel}</span>
          <span className="faint mono">{detail}</span>
        </div>
        {expanded && (
          <LiveCode
            factorId={factor.id}
            isTotp={factor.kind === "totp"}
            period={factor.periodSeconds ?? 30}
            locked={locked}
          />
        )}
      </div>
      <Button size="small" onClick={onToggle} aria-expanded={expanded}>
        {expanded ? "Hide code" : "Generate code"}
      </Button>
      <Button
        size="small"
        variant="danger"
        icon="trash"
        onClick={onRemoved}
        aria-label="Remove factor"
      >
        Remove
      </Button>
    </>
  );
}

/**
 * A live OTP code. TOTP codes refresh automatically at the period boundary;
 * HOTP codes never do, because every generation advances the server-side
 * counter and silently burning counters would desynchronise the factor.
 */
function LiveCode({
  factorId,
  isTotp,
  period,
  locked,
}: {
  factorId: string;
  isTotp: boolean;
  period: number;
  locked: boolean;
}) {
  const [code, setCode] = useState<OtpCode | null>(null);
  const [remaining, setRemaining] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (locked) return;
    let active = true;
    const generate = async () => {
      try {
        const generated = await api.factorGenerate(factorId);
        if (active) {
          setCode(generated);
          setRemaining(generated.secondsRemaining);
          setError(null);
        }
      } catch {
        if (active) setError("Code generation failed — is the Vault unlocked?");
      }
    };
    void generate();
    if (!isTotp) return () => { active = false; };
    const timer = window.setInterval(() => {
      setRemaining((current) => {
        if (current <= 1) {
          void generate();
          return period;
        }
        return current - 1;
      });
    }, 1000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [factorId, isTotp, period, locked]);

  // A locked vault stops the timer (see the effect cleanup) and hides any
  // previously generated code from the UI immediately.
  if (locked) {
    return null;
  }
  if (error) {
    return <p className="faint">{error}</p>;
  }
  if (!code) {
    return <p className="faint">Generating…</p>;
  }
  return (
    <div className="code-display" style={{ marginTop: 8 }}>
      <CodeRing remaining={remaining} period={period} />
      <span className="code-value selectable">{code.code}</span>
      <span className="faint">{remaining}s</span>
    </div>
  );
}

/* ---- Recovery codes ------------------------------------------------------- */

function RecoverySection({
  accountId,
  locked,
  onChanged,
  onError,
  onToast,
}: {
  accountId: string;
  locked: boolean;
  onChanged: () => Promise<void>;
  onError: (error: WorkspaceError | null) => void;
  onToast: ToastFn;
}) {
  const [codes, setCodes] = useState<RecoveryCode[] | null>(null);
  const [newCodes, setNewCodes] = useState("");

  // Locked vault means codes cannot be listed or marked; drop any loaded list
  // in the same render that notices the lock.
  const [prevLocked, setPrevLocked] = useState(locked);
  if (locked !== prevLocked) {
    setPrevLocked(locked);
    if (locked) setCodes(null);
  }

  const run = async (action: () => Promise<void>, okMessage?: string) => {
    try {
      await action();
      if (okMessage) onToast(okMessage);
      await onChanged();
      onError(null);
    } catch (thrown) {
      onError(toError(thrown));
    }
  };

  const reload = async () => setCodes(await api.recoveryList(accountId));

  return (
    <div className="stack-tight">
      <div className="row">
        <Button
          size="small"
          disabled={locked}
          onClick={() =>
            void run(async () => {
              await reload();
            })
          }
        >
          List codes
        </Button>
      </div>
      {codes && (
        <ul className="stack-tight" style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {codes.length === 0 && <li className="faint">No recovery codes stored.</li>}
          {codes.map((code) => (
            <li key={code.id} className="list-row">
              <span className="mono">#{code.position}</span>
              {code.used ? (
                <Badge tone="danger">Used{code.markedUsedAt ? ` · ${formatWhen(code.markedUsedAt)}` : ""}</Badge>
              ) : (
                <Badge tone="success">Unused</Badge>
              )}
              <div className="grow" />
              {!code.used && (
                <Button
                  size="small"
                  disabled={locked}
                  onClick={() =>
                    void run(async () => {
                      await api.recoveryMarkUsed(code.id);
                      await reload();
                    }, "Marked as used")
                  }
                >
                  Mark used
                </Button>
              )}
              <Button
                size="small"
                variant="danger"
                icon="trash"
                disabled={locked}
                onClick={() =>
                  void run(async () => {
                    await api.recoveryDelete(code.id);
                    await reload();
                  }, "Recovery code removed")
                }
                aria-label={`Remove recovery code ${code.position}`}
              >
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="row">
        <textarea
          placeholder="Paste recovery codes, one per line"
          value={newCodes}
          onChange={(event) => setNewCodes(event.target.value)}
          rows={2}
          style={{ maxWidth: 320 }}
          aria-label="New recovery codes"
        />
        <Button
          disabled={locked || newCodes.trim().length === 0}
          onClick={() =>
            void run(async () => {
              const values = newCodes
                .split("\n")
                .map((line) => line.trim())
                .filter((line) => line.length > 0);
              if (values.length === 0) return;
              await api.recoveryAdd(accountId, values);
              setNewCodes("");
              await reload();
            }, "Recovery codes stored")
          }
        >
          Add codes
        </Button>
      </div>
      <p className="faint">
        Recovery codes are encrypted in the Vault; only their used/unused status is stored in the
        database.
      </p>
    </div>
  );
}

function formatWhen(iso: string): string {
  const parsed = new Date(iso);
  return Number.isNaN(parsed.getTime()) ? iso : parsed.toLocaleString();
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
