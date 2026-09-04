import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./lib/api";
import { applyLanguage, initI18n } from "./i18n";

// vitest runs with globals:false, so @testing-library cannot auto-register
// cleanup; do it explicitly or DOM from earlier tests leaks into queries.
afterEach(cleanup);

// The shell talks to the Tauri backend through lib/api; tests replace that
// boundary entirely so no real IPC is attempted under jsdom.
vi.mock("./lib/api", () => ({
  api: {
    vaultStatus: vi.fn(),
    settingsGet: vi.fn(),
    settingsSave: vi.fn(),
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

function baseSettings(language: "system" | "en-US" | "zh-CN" = "system") {
  return {
    clipboardClearSeconds: 30,
    vaultIdleLockMinutes: 10,
    vaultLockOnMinimize: true,
    theme: "system" as const,
    preferredThoriumVariant: "AVX2",
    downloadProxy: null,
    language,
  };
}

function mockSteadyState(language: "system" | "en-US" | "zh-CN" = "system") {
  vi.mocked(api.vaultStatus).mockResolvedValue(unlocked);
  vi.mocked(api.settingsGet).mockResolvedValue(baseSettings(language));
  vi.mocked(api.settingsSave).mockResolvedValue(undefined);
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

beforeEach(async () => {
  mockSteadyState();
  await initI18n("en-US");
  document.documentElement.removeAttribute("data-theme");
});

describe("App shell", () => {
  it("renders the brand and all seven sections in the default language", async () => {
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
      expect(screen.getByRole("button", { name: label })).toBeDefined();
    }
  });

  it("marks the active section with aria-current", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    expect(screen.getByRole("button", { name: "Dashboard" }).getAttribute("aria-current")).toBe(
      "page",
    );
  });

  it("navigates via Alt+number shortcuts", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    fireEvent.keyDown(window, { altKey: true, key: "2" });
    expect(await screen.findByRole("heading", { name: "Profiles" })).toBeDefined();
    fireEvent.keyDown(window, { altKey: true, key: "7" });
    expect(await screen.findByRole("heading", { name: "Diagnostics" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Diagnostics" }).getAttribute("aria-current")).toBe(
      "page",
    );
  });

  it("renders Simplified Chinese when the persisted language is zh-CN", async () => {
    mockSteadyState("zh-CN");
    render(<App />);
    expect(await screen.findByRole("button", { name: "概览" })).toBeDefined();
    expect(screen.getByRole("button", { name: "配置档案" })).toBeDefined();
    expect(screen.getByRole("button", { name: "设置" })).toBeDefined();
    // The document lang attribute follows the language (accessibility).
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(screen.getByRole("heading", { name: "概览" })).toBeDefined();
  });

  it("switches language to Chinese from Settings after saving, and back", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });

    fireEvent.keyDown(window, { altKey: true, key: "6" });
    const languageSelect = (await screen.findByLabelText("Language")) as HTMLSelectElement;
    fireEvent.change(languageSelect, { target: { value: "zh-CN" } });

    // Explicit-save semantics: the UI applies on Save, which also persists.
    fireEvent.click(await screen.findByRole("button", { name: "Save settings" }));

    // Applies live: navigation re-renders in Chinese without a restart.
    await waitFor(() => expect(screen.getByRole("button", { name: "概览" })).toBeDefined());
    expect(document.documentElement.lang).toBe("zh-CN");
    await waitFor(() => expect(api.settingsSave).toHaveBeenCalled());
    expect(vi.mocked(api.settingsSave).mock.calls[0][0].language).toBe("zh-CN");

    // Back to English works the same way.
    fireEvent.change(languageSelect, { target: { value: "en-US" } });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Dashboard" })).toBeDefined());
    expect(document.documentElement.lang).toBe("en-US");
  });

  it("persists the chosen language through settings_save", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    fireEvent.keyDown(window, { altKey: true, key: "6" });
    const languageSelect = await screen.findByLabelText("Language");
    fireEvent.change(languageSelect, { target: { value: "zh-CN" } });

    const save = await screen.findByRole("button", { name: "Save settings" });
    expect(save.hasAttribute("disabled")).toBe(false);
    fireEvent.click(save);
    await waitFor(() => expect(api.settingsSave).toHaveBeenCalled());
    const saved = vi.mocked(api.settingsSave).mock.calls[0][0];
    expect(saved.language).toBe("zh-CN");
  });

  it("disables Save settings until something changed", async () => {
    render(<App />);
    fireEvent.keyDown(window, { altKey: true, key: "6" });
    const save = await screen.findByRole("button", { name: "Save settings" });
    expect(save.hasAttribute("disabled")).toBe(true);
    const theme = await screen.findByLabelText("Theme");
    fireEvent.change(theme, { target: { value: "dark" } });
    expect(save.hasAttribute("disabled")).toBe(false);
  });

  it("applies the persisted theme to the document root", async () => {
    vi.mocked(api.settingsGet).mockResolvedValue({
      ...baseSettings(),
      theme: "dark",
    });
    render(<App />);
    await screen.findByRole("heading", { name: "Dashboard" });
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
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

  it("restores the English UI when the preference switches back", async () => {
    // applyLanguage is the runtime mechanism used by the shell.
    applyLanguage("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    applyLanguage("en-US");
    expect(document.documentElement.lang).toBe("en-US");
  });
});
