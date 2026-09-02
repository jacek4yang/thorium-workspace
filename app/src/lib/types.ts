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
