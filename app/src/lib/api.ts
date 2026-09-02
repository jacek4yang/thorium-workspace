// Typed wrappers around the Tauri command surface. Presentation code
// never calls raw `invoke` with stringly-typed commands.

import { invoke } from "@tauri-apps/api/core";
import {
  Account,
  AccountInput,
  BrowserProfile,
  DiagnosticsSnapshot,
  OtpCode,
  ProfileInput,
  RecoveryCode,
  ReleaseOption,
  SecondFactor,
  ThoriumSelection,
  ThoriumVersionInfo,
  VaultStatus,
  WorkspaceSettings,
  toWorkspaceError,
} from "./types";

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (thrown) {
    throw toWorkspaceError(thrown);
  }
}

export const api = {
  // vault
  vaultStatus: () => call<VaultStatus>("vault_status"),
  vaultCreate: (masterPassword: string) =>
    call<void>("vault_create", { masterPassword }),
  vaultUnlock: (masterPassword: string) =>
    call<void>("vault_unlock", { masterPassword }),
  vaultLock: () => call<void>("vault_lock"),
  vaultChangePassword: (current: string, newPassword: string) =>
    call<void>("vault_change_password", { current, newPassword }),

  // settings
  settingsGet: () => call<WorkspaceSettings>("settings_get"),
  settingsSave: (settings: WorkspaceSettings) =>
    call<void>("settings_save", { settings }),

  // profiles
  profilesList: () => call<BrowserProfile[]>("profiles_list"),
  profileCreate: (input: ProfileInput) =>
    call<BrowserProfile>("profile_create", { input }),
  profileUpdate: (profile: BrowserProfile) =>
    call<BrowserProfile>("profile_update", { profile }),
  profileDelete: (profileId: string) =>
    call<void>("profile_delete", { profileId }),
  profileLaunch: (profileId: string) =>
    call<{ executable: string; version: string }>("profile_launch", { profileId }),
  profileStop: (profileId: string) =>
    call<void>("profile_stop", { profileId }),
  runningProfiles: () => call<string[]>("running_profiles"),

  // diagnostics
  diagnostics: () => call<DiagnosticsSnapshot>("diagnostics"),

  // accounts
  accountsList: (profileId: string) =>
    call<Account[]>("accounts_list", { profileId }),
  accountCreate: (profileId: string, input: AccountInput) =>
    call<Account>("account_create", { profileId, input }),
  accountUpdate: (account: Account) => call<void>("account_update", { account }),
  accountDelete: (accountId: string) =>
    call<void>("account_delete", { accountId }),

  // passwords
  passwordSet: (accountId: string, password: string) =>
    call<void>("password_set", { accountId, password }),
  passwordDelete: (accountId: string) =>
    call<void>("password_delete", { accountId }),
  passwordCopy: (accountId: string) =>
    call<number>("password_copy", { accountId }),
  passwordReveal: (accountId: string) =>
    call<string>("password_reveal", { accountId }),

  // factors / otp / qr
  factorImportOtpauth: (accountId: string, uri: string) =>
    call<SecondFactor>("factor_import_otpauth", { accountId, uri }),
  factorImportQrFile: (accountId: string, imagePath: string) =>
    call<SecondFactor>("factor_import_qr_file", { accountId, imagePath }),
  factorAddExternal: (accountId: string, label: string | null, note: string | null) =>
    call<SecondFactor>("factor_add_external", { accountId, label, note }),
  factorGenerate: (factorId: string) =>
    call<OtpCode>("factor_generate", { factorId }),
  factorDelete: (factorId: string) => call<void>("factor_delete", { factorId }),

  // recovery codes
  recoveryAdd: (accountId: string, values: string[]) =>
    call<RecoveryCode[]>("recovery_add", { accountId, values }),
  recoveryList: (accountId: string) =>
    call<RecoveryCode[]>("recovery_list", { accountId }),
  recoveryMarkUsed: (codeId: string) =>
    call<void>("recovery_mark_used", { codeId }),
  recoveryDelete: (codeId: string) => call<void>("recovery_delete", { codeId }),

  // thorium
  thoriumInstalled: () => call<ThoriumVersionInfo[]>("thorium_installed"),
  thoriumDiscover: () => call<ReleaseOption[]>("thorium_discover"),
  thoriumInstall: (release: { url: string; version: string; variant: string; sizeBytes: number }) =>
    call<void>("thorium_install", release),
  thoriumSetCurrent: (version: string) =>
    call<void>("thorium_set_current", { version }),
  thoriumDelete: (version: string) => call<void>("thorium_delete", { version }),
};

export type { BrowserProfile, DiagnosticsSnapshot, ThoriumSelection, VaultStatus, WorkspaceSettings };
