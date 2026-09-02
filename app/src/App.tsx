import { useState } from "react";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import ProfilesPage from "./pages/ProfilesPage";
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
        {section === "vault" && <VaultPage />}
        {section === "profiles" && <ProfilesPage />}
        {section === "diagnostics" && <DiagnosticsPage />}
        {section !== "vault" &&
          section !== "profiles" &&
          section !== "diagnostics" && (
            <p className="muted">
              The {SECTIONS.find((entry) => entry.id === section)?.label} section
              is part of the ongoing v1.0.0 build-out.
            </p>
          )}
      </main>
    </div>
  );
}
