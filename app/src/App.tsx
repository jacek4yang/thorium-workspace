import { useState } from "react";
import AccountsPage from "./pages/AccountsPage";
import BrowserPage from "./pages/BrowserPage";
import DashboardPage from "./pages/DashboardPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import ProfilesPage from "./pages/ProfilesPage";
import SettingsPage from "./pages/SettingsPage";
import VaultPage from "./pages/VaultPage";

type Section =
  | "dashboard"
  | "profiles"
  | "accounts"
  | "browser"
  | "vault"
  | "settings"
  | "diagnostics";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "dashboard", label: "Dashboard" },
  { id: "profiles", label: "Profiles" },
  { id: "accounts", label: "Accounts" },
  { id: "browser", label: "Browser" },
  { id: "vault", label: "Vault" },
  { id: "settings", label: "Settings" },
  { id: "diagnostics", label: "Diagnostics" },
];

export default function App() {
  const [section, setSection] = useState<Section>("vault");

  return (
    <div className="app">
      <nav aria-label="Main navigation" className="sidebar">
        <h1>Thorium Workspace</h1>
        <ul>
          {SECTIONS.map((entry) => (
            <li key={entry.id}>
              <button
                type="button"
                className={section === entry.id ? "active" : undefined}
                aria-current={section === entry.id ? "page" : undefined}
                onClick={() => setSection(entry.id)}
              >
                {entry.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <main className="content">
        {section === "dashboard" && <DashboardPage />}
        {section === "profiles" && <ProfilesPage />}
        {section === "accounts" && <AccountsPage />}
        {section === "browser" && <BrowserPage />}
        {section === "vault" && <VaultPage />}
        {section === "settings" && <SettingsPage />}
        {section === "diagnostics" && <DiagnosticsPage />}
      </main>
    </div>
  );
}
