/**
 * Types mirroring the Rust command boundary.
 *
 * These are hand-written rather than generated: the surface is small, and a
 * hand-written mirror makes it obvious when a field carrying secret material
 * would be added. There is exactly one place a secret value appears in this
 * file — the explicit `reveal*` return types — and that is deliberate.
 */

export type DiagnosticCode = string;

export interface AppError {
  code: DiagnosticCode;
  message: string;
  remedy: string | null;
}

export interface StartupStatus {
  appVersion: string;
  error: AppError | null;
  workspaceRoot: string | null;
  firstRun: boolean;
  vaultExists: boolean;
  windowsSupervision: boolean;
}

export type ThemePreference = "system" | "light" | "dark";

export type ThoriumChannel =
  | "windows_avx2"
  | "windows_avx"
  | "windows_sse3"
  | "windows_arm64";

export interface ClipboardSettings {
  clear_enabled: boolean;
  clear_after_seconds: number;
}

export interface VaultSettings {
  idle_lock_enabled: boolean;
  idle_lock_seconds: number;
  lock_on_minimize: boolean;
}

export interface WorkspaceSettings {
  theme: ThemePreference;
  clipboard: ClipboardSettings;
  vault: VaultSettings;
  thorium_channel: ThoriumChannel;
  check_thorium_updates_on_start: boolean;
}

export type LockReason =
  | "never_unlocked"
  | "manual"
  | "idle"
  | "minimized"
  | "shutdown";

export type VaultState =
  | { state: "uninitialized" }
  | { state: "locked"; reason: LockReason }
  | {
      state: "unlocked";
      secret_count: number;
      unlocked_at: number;
      idle_lock_seconds: number | null;
    };

export type ServiceKind =
  | { kind: "git_hub" }
  | { kind: "microsoft" }
  | { kind: "other"; label: string };

export interface ServicePreset {
  id: string;
  name: string;
  kind: ServiceKind;
  login_url: string;
  two_factor_url: string;
  note: string;
}

export interface Account {
  id: string;
  display_name: string;
  service: ServiceKind;
  username: string | null;
  email: string | null;
  login_url: string | null;
  tags: string[];
  notes: string;
  password_ref: string | null;
  created_at: number;
  updated_at: number;
}

export interface AccountDraft {
  display_name: string;
  service: ServiceKind | null;
  username: string | null;
  email: string | null;
  login_url: string | null;
  tags: string[];
  notes: string;
}

export type OtpKind = "totp" | "hotp";
export type OtpAlgorithm = "SHA1" | "SHA256" | "SHA512";

export interface OtpParameters {
  kind: OtpKind;
  algorithm: OtpAlgorithm;
  digits: 6 | 8;
  period_seconds: number;
  counter: number;
  issuer: string | null;
  account_label: string | null;
}

export type SecondFactorKind = "otp" | "external_authenticator";

export interface SecondFactor {
  id: string;
  account_id: string;
  label: string;
  kind: SecondFactorKind;
  otp: OtpParameters | null;
  seed_ref: string | null;
  created_at: number;
  updated_at: number;
}

export interface SecondFactorDraft {
  label: string;
  kind: SecondFactorKind;
  otp: OtpParameters | null;
}

/** A live one-time code. Short-lived by nature; never persisted by the UI. */
export interface OtpCode {
  code: string;
  valid_for_seconds: number | null;
  counter: number;
}

export interface ImportedFactor {
  factor: SecondFactor;
  label: string;
}

export interface RecoveryCode {
  id: string;
  account_id: string;
  code_ref: string;
  position: number;
  used: boolean;
  used_at: number | null;
  created_at: number;
}

export type ThoriumSelection =
  | { mode: "current" }
  | { mode: "pinned"; version: string };

export type ProfileRuntimeStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "failed";

export interface BrowserProfile {
  id: string;
  name: string;
  thorium: ThoriumSelection;
  startup_urls: string[];
  locale: string;
  timezone: string;
  account_ids: string[];
  notes: string;
  network_route_id: string | null;
  created_at: number;
  updated_at: number;
}

export interface BrowserProfileDraft {
  name: string;
  thorium: ThoriumSelection;
  startup_urls: string[];
  locale: string | null;
  timezone: string | null;
  account_ids: string[];
  notes: string;
}

export interface ProfileView {
  profile: BrowserProfile;
  status: ProfileRuntimeStatus;
  accountCount: number;
}

export interface LaunchOutcome {
  started: boolean;
  focused: boolean;
  profileId: string;
}

export interface InstalledVersion {
  version: string;
  channel: ThoriumChannel;
  executablePath: string;
  installedAt: number;
  archiveSha256: string;
  isCurrent: boolean;
  pinnedByProfiles: number;
  inUse: boolean;
  presentOnDisk: boolean;
}

export interface AvailableRelease {
  tag: string;
  name: string;
  htmlUrl: string;
  assetName: string;
  assetSizeBytes: number;
  alreadyInstalled: boolean;
}

export interface ThoriumInstallation {
  version: string;
  channel: ThoriumChannel;
  install_dir: string;
  executable_path: string;
  installed_at: number;
  source_url: string;
  archive_sha256: string;
  is_current: boolean;
}

export type InstallProgress =
  | { stage: "resolving" }
  | { stage: "downloading"; received: number; total: number | null }
  | { stage: "verifying" }
  | { stage: "extracting"; done: number; total: number }
  | { stage: "activating" }
  | { stage: "done"; version: string };

export interface InstallProgressEvent {
  progress: InstallProgress;
}

export interface BackupManifest {
  formatVersion: number;
  appVersion: string;
  schemaVersion: number;
  createdAt: number;
  includesVault: boolean;
}

export interface BackupOutcome {
  path: string;
  bytes: number;
  manifest: BackupManifest;
}

export interface BackupEntry {
  path: string;
  name: string;
  bytes: number;
}

export interface ProfileDiagnostic {
  id: string;
  name: string;
  status: ProfileRuntimeStatus;
  thoriumSelection: string;
  locale: string;
  timezone: string;
  userDataPresent: boolean;
  cdpActive: boolean;
  emulationActive: boolean;
}

export interface DiagnosticReport {
  appVersion: string;
  platform: string;
  windowsSupervision: boolean;
  workspaceRoot: string;
  workspaceWritable: boolean;
  instanceName: string;
  schemaVersion: number;
  databaseIntegrity: string;
  vaultState: string;
  vaultFormatVersion: number | null;
  vaultKdfMemoryKib: number | null;
  vaultSecretCount: number | null;
  thoriumVersions: string[];
  thoriumCurrent: string | null;
  thoriumExecutable: string | null;
  thoriumChannel: string;
  profiles: ProfileDiagnostic[];
  accountCount: number;
  factorCount: number;
  theme: string;
  clipboardClearEnabled: boolean;
  clipboardClearSeconds: number;
  vaultIdleLockEnabled: boolean;
  vaultIdleLockSeconds: number;
  staleFilesRemoved: number;
  staleStagingRemoved: number;
}
