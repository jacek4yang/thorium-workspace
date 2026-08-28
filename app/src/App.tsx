/**
 * The application shell.
 *
 * Owns navigation, the vault state every page depends on, and the toast queue.
 * It deliberately holds no domain state of its own: each page loads what it
 * needs when it is shown, so a stale list can never survive a navigation.
 */
import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { BrandMark, Icon } from "./components/Icon";
import type { IconName } from "./components/Icon";
import { ErrorNotice } from "./components/ui";
import { api, events } from "./lib/api";
import { vaultSummary } from "./lib/format";
import { useToasts } from "./lib/hooks";
import type { AppError, StartupStatus, VaultState, WorkspaceSettings } from "./lib/types";
import { AccountsPage } from "./pages/Accounts";
import { BrowserPage } from "./pages/Browser";
import { DashboardPage } from "./pages/Dashboard";
import { DiagnosticsPage } from "./pages/Diagnostics";
import { Onboarding } from "./pages/Onboarding";
import { ProfilesPage } from "./pages/Profiles";
import { SettingsPage } from "./pages/Settings";
import { VaultPage } from "./pages/Vault";

export type PageId =
  | "dashboard"
  | "profiles"
  | "accounts"
  | "browser"
  | "vault"
  | "settings"
  | "diagnostics";

const PAGES: { id: PageId; label: string; icon: IconName }[] = [
  { id: "dashboard", label: "Dashboard", icon: "dashboard" },
  { id: "profiles", label: "Profiles", icon: "profiles" },
  { id: "accounts", label: "Accounts", icon: "accounts" },
  { id: "browser", label: "Browser", icon: "browser" },
  { id: "vault", label: "Vault", icon: "vault" },
  { id: "settings", label: "Settings", icon: "settings" },
  { id: "diagnostics", label: "Diagnostics", icon: "diagnostics" },
];

/** Applies the theme preference to the document root. */
function useTheme(settings: WorkspaceSettings | null) {
  useEffect(() => {
    const theme = settings?.theme ?? "system";
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
    }
  }, [settings?.theme]);
}

export function App() {
  const [startup, setStartup] = useState<StartupStatus | null>(null);
  const [startupError, setStartupError] = useState<AppError | null>(null);
  const [vault, setVault] = useState<VaultState | null>(null);
  const [settings, setSettings] = useState<WorkspaceSettings | null>(null);
  const [page, setPage] = useState<PageId>("dashboard");
  const { toasts, push, dismiss } = useToasts();

  useTheme(settings);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const status = await api.startupStatus();
        if (cancelled) return;
        setStartup(status);
        if (status.error) {
          setStartupError(status.error);
          return;
        }
        const [state, loaded] = await Promise.all([api.vaultState(), api.getSettings()]);
        if (cancelled) return;
        setVault(state);
        setSettings(loaded);
      } catch (error) {
        if (!cancelled) setStartupError(error as AppError);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // The backend locks the vault on its own schedule; the header must follow.
  useEffect(() => {
    const unlisten = listen<VaultState>(events.vaultState, (event) => {
      setVault(event.payload);
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  const locked = vault?.state !== "unlocked";

  // Alt+1..7 moves between sections, which is what a keyboard-driven Windows
  // utility is expected to do.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey) return;
      const index = Number.parseInt(event.key, 10);
      if (Number.isNaN(index) || index < 1 || index > PAGES.length) return;
      const target = PAGES[index - 1];
      if (target) {
        event.preventDefault();
        setPage(target.id);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const content = useMemo(() => {
    switch (page) {
      case "profiles":
        return <ProfilesPage onToast={push} locked={locked} onNavigate={setPage} />;
      case "accounts":
        return <AccountsPage onToast={push} locked={locked} onNavigate={setPage} />;
      case "browser":
        return <BrowserPage onToast={push} />;
      case "vault":
        return (
          <VaultPage
            vault={vault}
            settings={settings}
            onToast={push}
            onVaultChanged={setVault}
            onSettingsChanged={setSettings}
          />
        );
      case "settings":
        return <SettingsPage settings={settings} onToast={push} onSettingsChanged={setSettings} />;
      case "diagnostics":
        return <DiagnosticsPage onToast={push} />;
      default:
        return <DashboardPage vault={vault} onToast={push} onNavigate={setPage} />;
    }
  }, [page, vault, settings, push, locked]);

  if (startupError) {
    return (
      <div className="onboarding">
        <div className="card onboarding-card">
          <BrandMark className="onboarding-mark" size={52} />
          <h1>Thorium Workspace cannot start here</h1>
          <ErrorNotice error={startupError} />
          <p className="muted">
            The workspace keeps everything beside the application file, and never falls back to a
            hidden folder somewhere else. Fixing the folder is the only step needed; nothing has
            been written anywhere.
          </p>
        </div>
      </div>
    );
  }

  if (!startup || !vault) {
    return (
      <div className="onboarding">
        <div className="row">
          <span className="spinner" />
          <span className="muted">Opening the workspace…</span>
        </div>
      </div>
    );
  }

  if (vault.state === "uninitialized") {
    return (
      <Onboarding
        startup={startup}
        onCreated={(state) => {
          setVault(state);
          void api.getSettings().then(setSettings);
        }}
      />
    );
  }

  return (
    <div className="app">
      <nav className="sidebar" aria-label="Sections">
        <div className="brand">
          <BrandMark className="brand-mark" />
          <div>
            <div className="brand-name">Thorium Workspace</div>
            <div className="brand-version">v{startup.appVersion}</div>
          </div>
        </div>
        <div className="nav">
          {PAGES.map((entry, index) => (
            <button
              key={entry.id}
              type="button"
              className="nav-item"
              aria-current={page === entry.id ? "page" : undefined}
              onClick={() => setPage(entry.id)}
            >
              <Icon name={entry.icon} className="nav-icon" />
              <span>{entry.label}</span>
              <span className="nav-badge kbd">Alt+{index + 1}</span>
            </button>
          ))}
        </div>
        <div className="sidebar-footer">
          <button
            type="button"
            className="vault-chip"
            onClick={async () => {
              if (locked) {
                setPage("vault");
                return;
              }
              try {
                setVault(await api.lockVault());
                push("Vault locked");
              } catch (error) {
                push((error as AppError).message, "error");
              }
            }}
            title={locked ? "Go to the Vault page to unlock" : "Lock the vault now"}
          >
            <span className={`vault-dot ${locked ? "locked" : "unlocked"}`} />
            <span className="grow truncate">{vaultSummary(vault)}</span>
            <Icon name={locked ? "lock" : "unlock"} size={14} />
          </button>
        </div>
      </nav>

      <main className="main">{content}</main>

      <div className="toasts" aria-live="polite">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast ${toast.tone}`}>
            <Icon name={toast.tone === "error" ? "alert" : "check"} />
            <span className="grow">{toast.message}</span>
            <button
              type="button"
              className="button ghost small"
              onClick={() => dismiss(toast.id)}
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

export type ToastFn = ReturnType<typeof useToasts>["push"];

/** Re-exported so pages can refresh the header without importing the shell. */
export { PAGES };
