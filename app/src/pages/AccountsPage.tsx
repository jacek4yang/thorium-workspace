import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import {
  Account,
  BrowserProfile,
  OtpCode,
  RecoveryCode,
  ServiceKind,
  WorkspaceError,
} from "../lib/types";

const SERVICES: { id: ServiceKind; label: string }[] = [
  { id: { kind: "github" }, label: "GitHub" },
  { id: { kind: "microsoft" }, label: "Microsoft" },
  { id: { kind: "google" }, label: "Google" },
  { id: { kind: "gitlab" }, label: "GitLab" },
  { id: { kind: "custom", label: "Custom" }, label: "Custom…" },
];

function serviceLabel(kind: ServiceKind): string {
  return kind.kind === "custom" ? kind.label : SERVICES.find((s) => s.id.kind === kind.kind)?.label ?? kind.kind;
}

export default function AccountsPage() {
  const [profiles, setProfiles] = useState<BrowserProfile[]>([]);
  const [profileId, setProfileId] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

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
    if (!profileId) {
      return;
    }
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

  const run = async (action: () => Promise<void>) => {
    try {
      await action();
      if (profileId) await loadAccounts(profileId);
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    }
  };

  return (
    <section aria-labelledby="accounts-heading">
      <h2 id="accounts-heading">Accounts</h2>
      {profiles.length === 0 ? (
        <p className="muted">Create a profile first; accounts belong to profiles.</p>
      ) : (
        <>
          <label>
            Profile
            <select
              value={profileId ?? ""}
              onChange={(event) => setProfileId(event.target.value)}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>
          {profileId && (
            <CreateAccountForm
              profileId={profileId}
              busy={false}
              onCreate={(input) =>
                run(async () => {
                  await api.accountCreate(profileId, input);
                })
              }
            />
          )}
          {notice && <p className="muted">{notice}</p>}
          {error && <p className="error" role="alert">{error.message}</p>}
          {accounts === null ? (
            <p className="muted">Loading accounts…</p>
          ) : accounts.length === 0 ? (
            <p className="muted">No accounts in this profile yet.</p>
          ) : (
            <ul className="profile-list">
              {accounts.map((account) => (
                <li key={account.id} className="card">
                  <AccountCard
                    account={account}
                    onError={setError}
                    onNotice={setNotice}
                    reload={() => run(async () => {})}
                  />
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}

function CreateAccountForm(props: {
  profileId: string;
  busy: boolean;
  onCreate: (input: {
    displayName: string;
    serviceKind: ServiceKind;
    username: string | null;
    email: string | null;
    loginUrl: string | null;
    tags: string[];
    notes: string;
  }) => Promise<void>;
}) {
  const [displayName, setDisplayName] = useState("");
  const [service, setService] = useState("github");
  const [customLabel, setCustomLabel] = useState("");
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [notes, setNotes] = useState("");

  return (
    <form
      className="card"
      onSubmit={(event) => {
        event.preventDefault();
        const kind: ServiceKind =
          service === "custom"
            ? { kind: "custom", label: customLabel || "Custom" }
            : ({ kind: service } as ServiceKind);
        void props.onCreate({
          displayName,
          serviceKind: kind,
          username: username || null,
          email: email || null,
          loginUrl: null,
          tags: [],
          notes,
        });
        setDisplayName("");
        setUsername("");
        setEmail("");
        setNotes("");
      }}
    >
      <h3>New account</h3>
      <label>
        Display name
        <input
          value={displayName}
          onChange={(event) => setDisplayName(event.target.value)}
          placeholder="Work GitHub"
          required
        />
      </label>
      <label>
        Service
        <select value={service} onChange={(event) => setService(event.target.value)}>
          {SERVICES.map((entry) => (
            <option key={entry.id.kind} value={entry.id.kind}>
              {entry.label}
            </option>
          ))}
        </select>
      </label>
      {service === "custom" && (
        <label>
          Custom service label
          <input
            value={customLabel}
            onChange={(event) => setCustomLabel(event.target.value)}
            placeholder="Internal Wiki"
          />
        </label>
      )}
      <label>
        Username (optional)
        <input value={username} onChange={(event) => setUsername(event.target.value)} />
      </label>
      <label>
        Email (optional)
        <input value={email} onChange={(event) => setEmail(event.target.value)} />
      </label>
      <label>
        Notes (non-secret)
        <textarea value={notes} onChange={(event) => setNotes(event.target.value)} rows={2} />
      </label>
      <button type="submit" disabled={props.busy}>
        Create account
      </button>
    </form>
  );
}

function AccountCard(props: {
  account: Account;
  reload: () => Promise<void>;
  onError: (error: WorkspaceError | null) => void;
  onNotice: (notice: string | null) => void;
}) {
  const { account } = props;
  const [password, setPassword] = useState("");
  const [revealed, setRevealed] = useState<string | null>(null);
  const [otpUri, setOtpUri] = useState("");
  const [otpCode, setOtpCode] = useState<OtpCode | null>(null);
  const [codes, setCodes] = useState<RecoveryCode[] | null>(null);
  const [newCodes, setNewCodes] = useState("");

  const run = async (action: () => Promise<void>) => {
    try {
      await action();
      await props.reload();
      props.onError(null);
    } catch (thrown) {
      props.onError(toError(thrown));
    }
  };

  return (
    <div>
      <div className="profile-title">
        <strong>{account.displayName}</strong>
        <span className="badge">{serviceLabel(account.serviceKind)}</span>
        {account.passwordRef && <span className="badge">password</span>}
      </div>
      <p className="muted">
        {account.username ?? "no username"}
        {account.factors.length > 0 && ` · ${account.factors.length} factor(s)`}
      </p>

      <div className="row">
        <input
          type="password"
          placeholder="new password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          style={{ maxWidth: 200 }}
        />
        <button
          type="button"
          onClick={() =>
            void run(async () => {
              if (password.length > 0) {
                await api.passwordSet(account.id, password);
                setPassword("");
              }
            })
          }
        >
          Set password
        </button>
        {account.passwordRef && (
          <>
            <button
              type="button"
              onClick={() =>
                void run(async () => {
                  const seconds = await api.passwordCopy(account.id);
                  props.onNotice(`Password copied; clipboard clears in ${seconds}s.`);
                })
              }
            >
              Copy
            </button>
            <button
              type="button"
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
            </button>
            <button
              type="button"
              className="danger"
              onClick={() => void run(async () => {
                await api.passwordDelete(account.id);
                setRevealed(null);
              })}
            >
              Delete password
            </button>
          </>
        )}
      </div>
      {revealed !== null && (
        <p className="mono">{revealed}</p>
      )}

      <h4>Second factors</h4>
      <ul className="plain">
        {account.factors.map((factor) => (
          <li key={factor.id}>
            <span className="mono">
              {factor.kind}
              {factor.issuer ? ` · ${factor.issuer}` : ""}
              {factor.algorithm ? ` · ${factor.algorithm}` : ""}
              {factor.digits ? ` · ${factor.digits}d` : ""}
            </span>{" "}
            {factor.kind !== "external" && (
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    setOtpCode(await api.factorGenerate(factor.id));
                  })
                }
              >
                Generate code
              </button>
            )}{" "}
            <button
              type="button"
              className="danger"
              onClick={() => void run(async () => {
                await api.factorDelete(factor.id);
              })}
            >
              Remove
            </button>
          </li>
        ))}
      </ul>
      {otpCode && (
        <p>
          Current code: <strong className="mono">{otpCode.code}</strong> (valid{" "}
          {otpCode.secondsRemaining}s)
        </p>
      )}
      <div className="row">
        <input
          placeholder="otpauth://…"
          value={otpUri}
          onChange={(event) => setOtpUri(event.target.value)}
          style={{ maxWidth: 280 }}
        />
        <button
          type="button"
          onClick={() =>
            void run(async () => {
              await api.factorImportOtpauth(account.id, otpUri);
              setOtpUri("");
            })
          }
        >
          Import otpauth URI
        </button>
        <button
          type="button"
          onClick={() =>
            void run(async () => {
              const selection = await open({
                filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "bmp"] }],
              });
              if (typeof selection === "string") {
                await api.factorImportQrFile(account.id, selection);
              }
            })
          }
        >
          Import QR image…
        </button>
      </div>

      <h4>Recovery codes</h4>
      <div className="row">
        <button type="button" onClick={() => void run(async () => {
          setCodes(await api.recoveryList(account.id));
        })}>
          List codes
        </button>
      </div>
      {codes && (
        <ul className="plain">
          {codes.map((code) => (
            <li key={code.id}>
              slot {code.position}: {code.used ? "used" : "unused"}{" "}
              {!code.used && (
                <button
                  type="button"
                  onClick={() => void run(async () => {
                    await api.recoveryMarkUsed(code.id);
                    setCodes(await api.recoveryList(account.id));
                  })}
                >
                  Mark used
                </button>
              )}{" "}
              <button
                type="button"
                className="danger"
                onClick={() => void run(async () => {
                  await api.recoveryDelete(code.id);
                  setCodes(await api.recoveryList(account.id));
                })}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="row">
        <textarea
          placeholder="one code per line"
          value={newCodes}
          onChange={(event) => setNewCodes(event.target.value)}
          rows={2}
          style={{ maxWidth: 280 }}
        />
        <button
          type="button"
          onClick={() =>
            void run(async () => {
              const values = newCodes
                .split("\n")
                .map((line) => line.trim())
                .filter((line) => line.length > 0);
              if (values.length > 0) {
                await api.recoveryAdd(account.id, values);
                setNewCodes("");
              }
            })
          }
        >
          Add codes
        </button>
      </div>
    </div>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
