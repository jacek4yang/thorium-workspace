// The navigation sidebar: brand, section list with icons and Alt+ shortcuts,
// and the vault state chip. Pure presentation; state comes from the shell.

import { BrandMark, Icon } from "./Icon";
import type { IconName } from "./Icon";

export type SectionId =
  | "dashboard"
  | "profiles"
  | "accounts"
  | "browser"
  | "vault"
  | "settings"
  | "diagnostics";

export const SECTIONS: { id: SectionId; label: string; icon: IconName }[] = [
  { id: "dashboard", label: "Dashboard", icon: "dashboard" },
  { id: "profiles", label: "Profiles", icon: "profiles" },
  { id: "accounts", label: "Accounts", icon: "accounts" },
  { id: "browser", label: "Browser", icon: "browser" },
  { id: "vault", label: "Vault", icon: "vault" },
  { id: "settings", label: "Settings", icon: "settings" },
  { id: "diagnostics", label: "Diagnostics", icon: "diagnostics" },
];

export function Sidebar({
  section,
  onNavigate,
  vaultChip,
}: {
  section: SectionId;
  onNavigate: (section: SectionId) => void;
  /** Rendered at the bottom; shows vault state and jumps to the Vault page. */
  vaultChip: React.ReactNode;
}) {
  const workspace = SECTIONS.filter((entry) =>
    ["dashboard", "profiles", "accounts", "browser", "vault"].includes(entry.id),
  );
  const support = SECTIONS.filter((entry) =>
    ["settings", "diagnostics"].includes(entry.id),
  );

  return (
    <nav className="sidebar" aria-label="Sections">
      <div className="brand">
        <BrandMark />
        <div>
          <div className="brand-name">Thorium Workspace</div>
          <div className="brand-tagline">Portable profiles &amp; secrets</div>
        </div>
      </div>

      <NavGroup entries={workspace} section={section} onNavigate={onNavigate} startIndex={0} />
      <div className="nav-group-label" aria-hidden="true">
        Support
      </div>
      <NavGroup entries={support} section={section} onNavigate={onNavigate} startIndex={5} />

      <div className="sidebar-footer">{vaultChip}</div>
    </nav>
  );
}

function NavGroup({
  entries,
  section,
  onNavigate,
  startIndex,
}: {
  entries: typeof SECTIONS;
  section: SectionId;
  onNavigate: (section: SectionId) => void;
  startIndex: number;
}) {
  return (
    <div className="nav" role="presentation">
      {entries.map((entry, index) => (
        <button
          key={entry.id}
          type="button"
          className="nav-item"
          aria-current={section === entry.id ? "page" : undefined}
          onClick={() => onNavigate(entry.id)}
          title={`${entry.label} (Alt+${startIndex + index + 1})`}
        >
          <Icon name={entry.icon} className="nav-icon" size={16} />
          <span>{entry.label}</span>
          <span className="nav-badge kbd" aria-hidden="true">
            Alt+{startIndex + index + 1}
          </span>
        </button>
      ))}
    </div>
  );
}
