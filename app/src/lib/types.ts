// Types mirroring the serde DTOs exposed by the Rust commands.
// No secret values appear in any type here: passwords cross the boundary
// only as explicit call arguments (set/reveal) and are never stored in
// frontend state.

export interface VaultStatus {
  exists: boolean;
  lockState: "missing" | "locked" | "unlocked";
}

export type ThoriumSelection =
  | { selection: "current" }
  | { selection: "pinned"; version: string };

export interface BrowserProfile {
  id: string;
  name: string;
  thoriumVersion: ThoriumSelection;
  userDataRelPath: string;
  startupUrls: string[];
  locale: string | null;
  timezone: string | null;
  accountIds: string[];
  createdAt: string;
  updatedAt: string;
  lastLaunchedAt: string | null;
}

export interface ProfileInput {
  name: string;
  thoriumVersion: ThoriumSelection;
  startupUrls: string[];
  locale: string | null;
  timezone: string | null;
}

export interface WorkspaceSettings {
  clipboardClearSeconds: number;
  vaultIdleLockMinutes: number | null;
  vaultLockOnMinimize: boolean;
  theme: "system" | "light" | "dark";
  preferredThoriumVariant: string;
}

export interface DiagnosticsSnapshot {
  workspacePath: string;
  workspaceWritable: boolean;
  schemaVersion: number;
  vaultExists: boolean;
  vaultLockState: "missing" | "locked" | "unlocked";
  installedThoriumVersions: string[];
  currentThoriumVersion: string | null;
  runningProfiles: string[];
  idleLockMinutes: number | null;
  clipboardClearSeconds: number;
}

/** Frontend-safe error from the Rust layer. */
export class WorkspaceError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

export function toWorkspaceError(thrown: unknown): WorkspaceError {
  if (
    typeof thrown === "object" &&
    thrown !== null &&
    "code" in thrown &&
    "message" in thrown
  ) {
    const candidate = thrown as { code: unknown; message: unknown };
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return new WorkspaceError(candidate.code, candidate.message);
    }
  }
  return new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}

export type ServiceKind =
  | { kind: "github" }
  | { kind: "microsoft" }
  | { kind: "google" }
  | { kind: "gitlab" }
  | { kind: "custom"; label: string };

export interface Account {
  id: string;
  profileId: string;
  displayName: string;
  serviceKind: ServiceKind;
  username: string | null;
  email: string | null;
  loginUrl: string | null;
  tags: string[];
  notes: string;
  passwordRef: string | null;
  factors: SecondFactor[];
  recoveryCodes: RecoveryCode[];
  createdAt: string;
  updatedAt: string;
}

export interface AccountInput {
  displayName: string;
  serviceKind: ServiceKind;
  username: string | null;
  email: string | null;
  loginUrl: string | null;
  tags: string[];
  notes: string;
}

export type FactorKind = "totp" | "hotp" | "external";

export interface SecondFactor {
  id: string;
  accountId: string;
  kind: FactorKind;
  label: string | null;
  issuer: string | null;
  accountLabel: string | null;
  algorithm: "SHA1" | "SHA256" | "SHA512" | null;
  digits: number | null;
  periodSeconds: number | null;
  counter: number | null;
  secretRef: string | null;
  externalNote: string | null;
  createdAt: string;
}

export interface RecoveryCode {
  id: string;
  accountId: string;
  position: number;
  used: boolean;
  markedUsedAt: string | null;
  secretRef: string;
}

export interface OtpCode {
  code: string;
  secondsRemaining: number;
}

export interface ReleaseOption {
  repo: string;
  tag: string;
  version: string;
  variant: string;
  url: string;
  sizeBytes: number;
}

export interface ThoriumVersionInfo {
  version: string;
  variant: string | null;
  installedAt: string | null;
  isCurrent: boolean;
}

export interface DownloadProgress {
  downloaded: number;
  total: number;
}
