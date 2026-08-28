/**
 * The typed command layer.
 *
 * Every backend call goes through here so the rest of the UI never touches
 * `invoke` directly, and so an error always arrives as a structured
 * {@link AppError} rather than a bare string.
 */
import { invoke } from "@tauri-apps/api/core";

import type {
  Account,
  AccountDraft,
  AppError,
  AvailableRelease,
  BackupEntry,
  BackupOutcome,
  BrowserProfile,
  BrowserProfileDraft,
  DiagnosticReport,
  ImportedFactor,
  InstalledVersion,
  LaunchOutcome,
  OtpCode,
  OtpParameters,
  ProfileRuntimeStatus,
  ProfileView,
  RecoveryCode,
  SecondFactor,
  SecondFactorDraft,
  ServicePreset,
  StartupStatus,
  ThoriumChannel,
  ThoriumInstallation,
  VaultState,
  WorkspaceSettings,
} from "./types";

/** Narrows an unknown rejection into an {@link AppError}. */
export function toAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    const candidate = error as Partial<AppError>;
    return {
      code: String(candidate.code ?? "TW-0903"),
      message: String(candidate.message ?? "Something went wrong."),
      remedy: candidate.remedy ?? null,
    };
  }
  return {
    code: "TW-0903",
    message: typeof error === "string" ? error : "Something went wrong.",
    remedy: null,
  };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

export const api = {
  startupStatus: () => call<StartupStatus>("startup_status"),

  getSettings: () => call<WorkspaceSettings>("get_settings"),
  setSettings: (settings: WorkspaceSettings) =>
    call<WorkspaceSettings>("set_settings", { settings }),
  listTimezones: () => call<string[]>("list_timezones"),
  listServicePresets: () => call<ServicePreset[]>("list_service_presets"),

  vaultState: () => call<VaultState>("vault_state"),
  createVault: (password: string) => call<VaultState>("create_vault", { password }),
  unlockVault: (password: string) => call<VaultState>("unlock_vault", { password }),
  lockVault: () => call<VaultState>("lock_vault"),
  changeMasterPassword: (current: string, next: string) =>
    call<void>("change_master_password", { current, next }),
  collectOrphanedSecrets: () => call<number>("collect_orphaned_secrets"),

  listAccounts: () => call<Account[]>("list_accounts"),
  listAccountsForProfile: (profileId: string) =>
    call<Account[]>("list_accounts_for_profile", { profileId }),
  createAccount: (draft: AccountDraft, password: string | null) =>
    call<Account>("create_account", { draft, password }),
  updateAccount: (id: string, draft: AccountDraft) =>
    call<Account>("update_account", { id, draft }),
  setAccountPassword: (id: string, password: string | null) =>
    call<Account>("set_account_password", { id, password }),
  deleteAccount: (id: string) => call<void>("delete_account", { id }),
  /** Returns plaintext. Called only when the user presses "show". */
  revealAccountPassword: (id: string) => call<string>("reveal_account_password", { id }),
  /** The password goes straight from the vault to the clipboard, never here. */
  copyAccountPassword: (id: string) => call<void>("copy_account_password", { id }),

  listFactors: (accountId: string) => call<SecondFactor[]>("list_factors", { accountId }),
  addOtpFactorFromUri: (accountId: string, uri: string, label: string | null) =>
    call<SecondFactor>("add_otp_factor_from_uri", { accountId, uri, label }),
  addOtpFactorManual: (
    accountId: string,
    label: string,
    parameters: OtpParameters,
    secret: string,
  ) => call<SecondFactor>("add_otp_factor_manual", { accountId, label, parameters, secret }),
  addExternalFactor: (accountId: string, draft: SecondFactorDraft) =>
    call<SecondFactor>("add_external_factor", { accountId, draft }),
  importOtpFromImageFile: (accountId: string, path: string) =>
    call<ImportedFactor>("import_otp_from_image_file", { accountId, path }),
  importOtpFromClipboard: (accountId: string) =>
    call<ImportedFactor>("import_otp_from_clipboard", { accountId }),
  importOtpFromScreen: (accountId: string) =>
    call<ImportedFactor>("import_otp_from_screen", { accountId }),
  generateCode: (factorId: string) => call<OtpCode>("generate_code", { factorId }),
  copyCode: (factorId: string) => call<OtpCode>("copy_code", { factorId }),
  deleteFactor: (factorId: string) => call<void>("delete_factor", { factorId }),

  listRecoveryCodes: (accountId: string) =>
    call<RecoveryCode[]>("list_recovery_codes", { accountId }),
  addRecoveryCodes: (accountId: string, pasted: string) =>
    call<RecoveryCode[]>("add_recovery_codes", { accountId, pasted }),
  setRecoveryCodeUsed: (id: string, used: boolean) =>
    call<RecoveryCode>("set_recovery_code_used", { id, used }),
  revealRecoveryCode: (id: string) => call<string>("reveal_recovery_code", { id }),
  copyRecoveryCode: (id: string) => call<void>("copy_recovery_code", { id }),
  deleteRecoveryCode: (id: string) => call<void>("delete_recovery_code", { id }),

  copyPlainValue: (value: string) => call<void>("copy_plain_value", { value }),
  clearClipboard: () => call<boolean>("clear_clipboard"),

  listProfiles: () => call<ProfileView[]>("list_profiles"),
  createProfile: (draft: BrowserProfileDraft) =>
    call<BrowserProfile>("create_profile", { draft }),
  updateProfile: (id: string, draft: BrowserProfileDraft) =>
    call<BrowserProfile>("update_profile", { id, draft }),
  deleteProfile: (id: string, deleteBrowserData: boolean) =>
    call<string | null>("delete_profile", { id, deleteBrowserData }),
  profileStatus: (id: string) => call<ProfileRuntimeStatus>("profile_status", { id }),
  launchProfile: (id: string) => call<LaunchOutcome>("launch_profile", { id }),
  stopProfile: (id: string) => call<void>("stop_profile", { id }),

  listThoriumVersions: () => call<InstalledVersion[]>("list_thorium_versions"),
  checkForThoriumUpdate: () => call<AvailableRelease>("check_for_thorium_update"),
  installThorium: (channel: ThoriumChannel | null) =>
    call<ThoriumInstallation>("install_thorium", { channel }),
  setCurrentThorium: (version: string) => call<void>("set_current_thorium", { version }),
  removeThorium: (version: string) => call<void>("remove_thorium", { version }),
  rollbackThorium: () => call<string>("rollback_thorium"),

  createBackup: () => call<BackupOutcome>("create_backup"),
  listBackups: () => call<BackupEntry[]>("list_backups"),

  diagnostics: () => call<DiagnosticReport>("diagnostics"),
  copyDiagnostics: () => call<string>("copy_diagnostics"),
};

/** Backend event names, mirrored from `events::names`. */
export const events = {
  vaultState: "vault:state",
  profilesChanged: "profiles:changed",
  thoriumChanged: "thorium:changed",
  thoriumInstallProgress: "thorium:install-progress",
} as const;
