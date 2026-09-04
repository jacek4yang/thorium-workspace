/**
 * Runtime internationalization.
 *
 * Language preference lives in WorkspaceSettings (persisted by the
 * backend). `system` resolves from the WebView locale at startup:
 * Simplified Chinese variants map to zh-CN, everything else to en-US.
 * zh-TW / zh-HK are deliberately NOT mapped to Simplified Chinese.
 */
import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import { enUS } from "./locales/en-US";
import { zhCN } from "./locales/zh-CN";

export type UiLanguage = "system" | "en-US" | "zh-CN";

export const FALLBACK_LANGUAGE = "en-US" as const;

export const resources = {
  "en-US": { translation: enUS },
  "zh-CN": { translation: zhCN },
} as const;

/** WebView languages treated as Simplified Chinese. */
const SIMPLIFIED_CHINESE_PREFIXES = ["zh-cn", "zh-sg", "zh", "zh-hans"];

/**
 * Resolves a `system` preference to a concrete language using the WebView
 * locale. The mapping is explicit: zh/zh-CN/zh-SG (and zh-Hans) → zh-CN;
 * everything else — including zh-TW/zh-HK — falls back to en-US.
 */
export function resolveSystemLanguage(detected: readonly string[]): "en-US" | "zh-CN" {
  for (const tag of detected) {
    const normalized = tag.toLowerCase();
    if (SIMPLIFIED_CHINESE_PREFIXES.includes(normalized)) {
      return "zh-CN";
    }
    const [language, region] = normalized.split("-");
    if (language === "zh") {
      if (!region) return "zh-CN";
      if (region === "cn" || region === "sg" || region === "hans") return "zh-CN";
      // zh-TW, zh-HK, zh-MO: Traditional Chinese is not provided; the
      // fallback stays en-US rather than shipping simplified text.
      return FALLBACK_LANGUAGE;
    }
    // The first non-zh preference is a definitive answer: every non-Chinese
    // locale resolves to the English fallback.
    return FALLBACK_LANGUAGE;
  }
  return FALLBACK_LANGUAGE;
}

export function isUiLanguage(value: unknown): value is UiLanguage {
  return value === "system" || value === "en-US" || value === "zh-CN";
}

let currentLanguage: "en-US" | "zh-CN" = FALLBACK_LANGUAGE;

/** The concrete language currently applied to the UI. */
export function activeLanguage(): "en-US" | "zh-CN" {
  return currentLanguage;
}

/** Applies the language to i18next and the document lang attribute. */
export function applyLanguage(preference: UiLanguage, detected?: readonly string[]): "en-US" | "zh-CN" {
  const resolved =
    preference === "system"
      ? resolveSystemLanguage(detected ?? (typeof navigator !== "undefined" ? navigator.languages : []))
      : preference;
  void i18next.changeLanguage(resolved);
  currentLanguage = resolved;
  if (typeof document !== "undefined") {
    document.documentElement.lang = resolved;
  }
  return resolved;
}

/** Initializes i18next; safe to call before React renders. */
export async function initI18n(preference: UiLanguage = "system"): Promise<void> {
  await i18next.use(initReactI18next).init({
    resources,
    lng: preference === "system" ? undefined : preference,
    fallbackLng: FALLBACK_LANGUAGE,
    // Missing keys render as the key path in development noise terms; we
    // prefer the English fallback silently so no raw key ever reaches the
    // UI while a translation is being added.
    returnEmptyString: false,
    interpolation: {
      // React already escapes output.
      escapeValue: false,
    },
  });
  applyLanguage(preference);
}

void i18next.on("languageChanged", (lng) => {
  if (lng === "en-US" || lng === "zh-CN") {
    currentLanguage = lng;
  }
});
