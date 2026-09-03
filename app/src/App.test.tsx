import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./lib/api";

// vitest runs with globals:false, so @testing-library cannot auto-register
// cleanup; do it explicitly or DOM from earlier tests leaks into queries.
afterEach(cleanup);

// The shell talks to the Tauri backend through lib/api; tests replace that
// boundary entirely so no real IPC is attempted under jsdom.
vi.mock("./lib/api", () => ({
  api: {
    vaultStatus: vi.fn(),
    settingsGet: vi.fn(),
    diagnostics: vi.fn(),
    profilesList: vi.fn(),
    runningProfiles: vi.fn(),
    thoriumInstalled: vi.fn(),
    vaultLock: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => async () => undefined),
}));

const unlocked = { exists: true, lockState: "unlocked" as const };

function mockSteadyState() {
  vi.mocked(api.vaultStatus).mockResolvedValue(unlocked);
  vi.mocked(api.settingsGet).mockResolvedValue({
    clipboardClearSeconds: 30,
    vaultIdleLockMinutes: 10,
    vaultLockOnMinimize: true,
    theme: "system",
    preferredThoriumVariant: "AVX2",
    downloadProxy: null,
  });
  vi.mocked(api.diagnostics).mockResolvedValue({
    workspacePath: "C:\\workspace",
    workspaceWritable: true,
    schemaVersion: 1,
    vaultExists: true,
    vaultLockState: "unlocked",
    installedThoriumVersions: [],
    currentThoriumVersion: null,
    runningProfiles: [],
    idleLockMinutes: 10,
    clipboardClearSeconds: 30,
  });
  vi.mocked(api.profilesList).mockResolvedValue([]);
  vi.mocked(api.runningProfiles).mockResolvedValue([]);
  vi.mocked(api.thoriumInstalled).mockResolvedValue([]);
}

beforeEach(() => {
  mockSteadyState();
});

describe("App shell", () => {
  it("renders the brand and all seven sections", async () => {
    render(<App />);
    expect(await screen.findByText("Thorium Workspace")).toBeDefined();
    for (const label of [
      "Dashboard",
      "Profiles",
      "Accounts",
      "Browser",
      "Vault",
      "Settings",
      "Diagnostics",
    ]) {
      // The Alt+<n> hint span is aria-hidden, so the accessible name is the
      // bare label — which also keeps the vault chip ("Vault unlocked") from
      // colliding with the nav entry.
      expect(screen.getByRole("button", { name: label })).toBeDefined();
    }
  });

  it("marks the active section with aria-current", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    const dashboardButton = screen.getByRole("button", { name: /Dashboard/ });
    expect(dashboardButton.getAttribute("aria-current")).toBe("page");
  });

  it("navigates via Alt+number shortcuts", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    fireEvent.keyDown(window, { altKey: true, key: "2" });
    expect(await screen.findByRole("heading", { name: "Profiles" })).toBeDefined();
    fireEvent.keyDown(window, { altKey: true, key: "7" });
    expect(await screen.findByRole("heading", { name: "Diagnostics" })).toBeDefined();
    // aria-current followed the navigation
    expect(screen.getByRole("button", { name: /Diagnostics/ }).getAttribute("aria-current")).toBe(
      "page",
    );
  });

  it("ignores Alt shortcuts outside 1..7", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    fireEvent.keyDown(window, { altKey: true, key: "8" });
    expect(screen.getByRole("heading", { name: "Dashboard" })).toBeDefined();
    fireEvent.keyDown(window, { altKey: true, key: "0" });
    expect(screen.getByRole("heading", { name: "Dashboard" })).toBeDefined();
  });

  it("shows the vault chip state for an unlocked vault", async () => {
    render(<App />);
    await waitFor(() =>
      expect(screen.getByText("Vault unlocked")).toBeDefined(),
    );
  });

  it("lands on the Vault page on first run (no vault yet)", async () => {
    vi.mocked(api.vaultStatus).mockResolvedValue({ exists: false, lockState: "missing" });
    render(<App />);
    expect(await screen.findByText("Create your Vault")).toBeDefined();
  });

  it("locks the vault from the sidebar chip", async () => {
    vi.mocked(api.vaultLock).mockResolvedValue(undefined);
    render(<App />);
    const chip = await screen.findByText("Vault unlocked");
    fireEvent.click(chip);
    await waitFor(() => expect(api.vaultLock).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByText("Vault locked")).toBeDefined());
  });

  it("shows a dashboard empty state when there are no profiles", async () => {
    render(<App />);
    expect(await screen.findByText("No profiles yet")).toBeDefined();
  });
});
