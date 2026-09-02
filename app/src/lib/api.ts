// Typed wrappers around the Tauri command surface. Presentation code
// never calls raw `invoke` with stringly-typed commands.

import { invoke } from "@tauri-apps/api/core";
import {
  BrowserProfile,
  DiagnosticsSnapshot,
  ProfileInput,
  ThoriumSelection,
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
};

export type { BrowserProfile, DiagnosticsSnapshot, ThoriumSelection, VaultStatus, WorkspaceSettings };
