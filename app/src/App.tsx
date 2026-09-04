/**
 * The application shell.
 *
 * Owns navigation, the vault lock state every page depends on, the settings
 * that drive theme and language, and the toast queue. It deliberately holds
 * no domain state of its own: each page loads what it needs when it is
 * shown, so a stale list can never survive a navigation.
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { SECTIONS, Sidebar } from "./components/Sidebar";
import type { SectionId } from "./components/Sidebar";
import { Icon } from "./components/Icon";
import { Button, Loading } from "./components/ui";
import { api } from "./lib/api";
import { useToasts } from "./lib/hooks";
import { applyLanguage, isUiLanguage } from "./i18n";
import { localizedErrorMessage } from "./lib/errors";
import { VaultStatus, WorkspaceError, WorkspaceSettings } from "./lib/types";
import AccountsPage from "./pages/AccountsPage";
import BrowserPage from "./pages/BrowserPage";
import DashboardPage from "./pages/DashboardPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import ProfilesPage from "./pages/ProfilesPage";
import SettingsPage from "./pages/SettingsPage";
import VaultPage from "./pages/VaultPage";

/** How often the shell re-checks the vault lock state. The backend also
 * locks on its own schedule (idle timer, minimize), so the header has to
 * follow instead of trusting a value captured once. */
const VAULT_POLL_MS = 3000;

/** Applies the theme preference to the document root. */
function useTheme(theme: WorkspaceSettings["theme"] | undefined) {
  useEffect(() => {
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else if (theme) {
      document.documentElement.setAttribute("data-theme", theme);
    }
  }, [theme]);
}

export default function App() {
  const { t } = useTranslation();
  const [section, setSection] = useState<SectionId>("dashboard");
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [settings, setSettings] = useState<WorkspaceSettings | null>(null);
  const [startupError, setStartupError] = useState<WorkspaceError | null>(null);
  const { toasts, push, dismiss } = useToasts();

  useTheme(settings?.theme);

  // Language follows the persisted preference and re-renders immediately.
  useEffect(() => {
    if (settings && isUiLanguage(settings.language)) {
      applyLanguage(settings.language);
    }
  }, [settings?.language]);

  const refreshVault = useCallback(async () => {
    try {
      setVault(await api.vaultStatus());
      setStartupError(null);
    } catch (thrown) {
      setStartupError(toError(thrown));
    }
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const [status, loadedSettings] = await Promise.all([
          api.vaultStatus(),
          api.settingsGet(),
        ]);
        setVault(status);
        setSettings(loadedSettings);
        if (status.lockState === "missing") {
          // First run: onboarding lives on the Vault page.
          setSection("vault");
        }
      } catch (thrown) {
        setStartupError(toError(thrown));
      }
    })();
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => void refreshVault(), VAULT_POLL_MS);
    const onFocus = () => void refreshVault();
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshVault]);

  // Alt+1..7 moves between sections, which is what a keyboard-driven Windows
  // utility is expected to do.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey) return;
      const index = Number.parseInt(event.key, 10);
      if (Number.isNaN(index) || index < 1 || index > SECTIONS.length) return;
      const target = SECTIONS[index - 1];
      if (target) {
        event.preventDefault();
        setSection(target);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const locked = vault?.lockState !== "unlocked";

  const vaultChip = (
    <button
      type="button"
      className="vault-chip"
      title={locked ? t("shell.vaultChip.openToUnlock") : t("shell.vaultChip.lockNow")}
      onClick={async () => {
        if (vault === null || locked) {
          setSection("vault");
          return;
        }
        try {
          await api.vaultLock();
          setVault(await api.vaultStatus());
          push(t("shell.vaultChip.lockedToast"));
        } catch (thrown) {
          push(localizedErrorMessage(toError(thrown), t), "error");
        }
      }}
    >
      <span
        className={`vault-dot ${vault === null ? "" : locked ? "locked" : "unlocked"}`}
      />
      <span className="grow truncate">
        {vault === null
          ? t("shell.vaultChip.checking")
          : vault.lockState === "missing"
            ? t("shell.vaultChip.missing")
            : vault.lockState === "locked"
              ? t("shell.vaultChip.locked")
              : t("shell.vaultChip.unlocked")}
      </span>
      <Icon
        name={vault?.lockState === "unlocked" ? "unlock" : "lock"}
        size={14}
      />
    </button>
  );

  return (
    <div className="app">
      <Sidebar section={section} onNavigate={setSection} vaultChip={vaultChip} />

      <main className="main">
        {!vault ? (
          <div className="page-body">
            {startupError ? (
              <div className="notice error" role="alert">
                <Icon name="alert" />
                <div className="notice-body">
                  <div className="notice-title">{t("common.workspaceUnreachable")}</div>
                  <div className="muted">{localizedErrorMessage(startupError, t)}</div>
                  <code>{startupError.code}</code>
                </div>
                <Button variant="ghost" size="small" onClick={() => void refreshVault()}>
                  <Icon name="refresh" size={13} />
                  {t("common.retry")}
                </Button>
              </div>
            ) : (
              <Loading label={t("common.openingWorkspace")} />
            )}
          </div>
        ) : (
          <>
            {section === "dashboard" && (
              <DashboardPage onNavigate={setSection} onToast={push} />
            )}
            {section === "profiles" && <ProfilesPage onToast={push} />}
            {section === "accounts" && (
              <AccountsPage
                locked={locked}
                onNavigate={setSection}
                onToast={push}
              />
            )}
            {section === "browser" && <BrowserPage onToast={push} />}
            {section === "vault" && (
              <VaultPage
                vault={vault}
                settings={settings}
                onVaultChanged={setVault}
                onToast={push}
              />
            )}
            {section === "settings" &&
              (settings ? (
                <SettingsPage settings={settings} onSettingsChanged={setSettings} onToast={push} />
              ) : (
                <div className="page-body">
                  <Loading />
                </div>
              ))}

            {section === "diagnostics" && <DiagnosticsPage onToast={push} />}
          </>
        )}

        <div className="toasts" aria-live="polite">
          {toasts.map((toast) => (
            <div key={toast.id} className={`toast ${toast.tone === "error" ? "error" : ""}`}>
              <Icon name={toast.tone === "error" ? "alert" : "check"} />
              <span className="grow">{toast.message}</span>
              <Button
                variant="ghost"
                size="small"
                onClick={() => dismiss(toast.id)}
                aria-label={t("common.dismiss")}
              >
                <Icon name="x" size={12} />
              </Button>
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
