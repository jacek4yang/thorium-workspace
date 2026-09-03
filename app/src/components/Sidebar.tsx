// The navigation sidebar: brand, section list with icons and Alt+ shortcuts,
// and the vault state chip. Pure presentation; state comes from the shell.

import { useTranslation } from "react-i18next";

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

const WORKSPACE_SECTIONS: SectionId[] = [
  "dashboard",
  "profiles",
  "accounts",
  "browser",
  "vault",
];

const SUPPORT_SECTIONS: SectionId[] = ["settings", "diagnostics"];

/** Alt+1..7 order across both groups. */
export const SECTIONS: SectionId[] = [...WORKSPACE_SECTIONS, ...SUPPORT_SECTIONS];

const SECTION_ICONS: Record<SectionId, IconName> = {
  dashboard: "dashboard",
  profiles: "profiles",
  accounts: "accounts",
  browser: "browser",
  vault: "vault",
  settings: "settings",
  diagnostics: "diagnostics",
};

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
  const { t } = useTranslation();

  return (
    <nav className="sidebar" aria-label={t("nav.workspace")}>
      <div className="brand">
        <BrandMark />
        <div>
          <div className="brand-name">{t("common.appName")}</div>
          <div className="brand-tagline">{t("common.tagline")}</div>
        </div>
      </div>

      <NavGroup ids={WORKSPACE_SECTIONS} section={section} onNavigate={onNavigate} startIndex={0} />
      <div className="nav-group-label" aria-hidden="true">
        {t("nav.support")}
      </div>
      <NavGroup ids={SUPPORT_SECTIONS} section={section} onNavigate={onNavigate} startIndex={5} />

      <div className="sidebar-footer">{vaultChip}</div>
    </nav>
  );
}

function NavGroup({
  ids,
  section,
  onNavigate,
  startIndex,
}: {
  ids: SectionId[];
  section: SectionId;
  onNavigate: (section: SectionId) => void;
  startIndex: number;
}) {
  const { t } = useTranslation();
  return (
    <div className="nav" role="presentation">
      {ids.map((id, index) => (
        <button
          key={id}
          type="button"
          className="nav-item"
          aria-current={section === id ? "page" : undefined}
          onClick={() => onNavigate(id)}
          title={`${t(`nav.${id}`)} (Alt+${startIndex + index + 1})`}
        >
          <Icon name={SECTION_ICONS[id]} className="nav-icon" size={16} />
          <span>{t(`nav.${id}`)}</span>
          <span className="nav-badge kbd" aria-hidden="true">
            Alt+{startIndex + index + 1}
          </span>
        </button>
      ))}
    </div>
  );
}
