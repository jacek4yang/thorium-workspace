import { describe, expect, it, vi } from "vitest";
import { applyLanguage, initI18n, resolveSystemLanguage } from "./index";
import { enUS } from "./locales/en-US";
import { zhCN } from "./locales/zh-CN";
import { localizedErrorMessage } from "../lib/errors";
import { WorkspaceError } from "../lib/types";

/** Collects leaf-key paths of a resource tree. */
function leafKeys(node: unknown, prefix = ""): string[] {
  if (typeof node !== "object" || node === null) return [prefix];
  return Object.entries(node as Record<string, unknown>).flatMap(([key, value]) =>
    leafKeys(value, prefix ? `${prefix}.${key}` : key),
  );
}

describe("translation resources", () => {
  it("zh-CN mirrors the en-US key tree exactly (both directions)", () => {
    const en = leafKeys(enUS);
    const zh = leafKeys(zhCN);
    const missingInZh = en.filter((key) => !zh.includes(key));
    const missingInEn = zh.filter((key) => !en.includes(key));
    // A missing translation must fail here, not ship as untranslated UI.
    expect(missingInZh, "keys missing in zh-CN").toEqual([]);
    expect(missingInEn, "keys missing in en-US").toEqual([]);
    expect(zh.sort()).toEqual(en.sort());
  });

  it("placeholders stay consistent between languages", () => {
    const placeholder = /\{\{\w+\}\}/g;
    const check = (enNode: unknown, zhNode: unknown) => {
      if (typeof enNode !== "object" || typeof zhNode !== "object") {
        const enVars = (String(enNode).match(placeholder) ?? []).sort();
        const zhVars = (String(zhNode).match(placeholder) ?? []).sort();
        expect(zhVars).toEqual(enVars);
        return;
      }
      for (const key of Object.keys(enNode as Record<string, unknown>)) {
        check(
          (enNode as Record<string, unknown>)[key],
          (zhNode as Record<string, unknown>)[key],
        );
      }
    };
    check(enUS, zhCN);
  });
});

describe("system language detection", () => {
  it("maps Simplified Chinese variants to zh-CN", () => {
    expect(resolveSystemLanguage(["zh-CN"])).toBe("zh-CN");
    expect(resolveSystemLanguage(["zh-SG"])).toBe("zh-CN");
    expect(resolveSystemLanguage(["zh"])).toBe("zh-CN");
    expect(resolveSystemLanguage(["zh-Hans-CN"])).toBe("zh-CN");
    // "zh-hans" alone is Simplified Chinese.
    expect(resolveSystemLanguage(["zh-hans"])).toBe("zh-CN");
  });

  it("does not map Traditional Chinese to Simplified", () => {
    expect(resolveSystemLanguage(["zh-TW"])).toBe("en-US");
    expect(resolveSystemLanguage(["zh-HK"])).toBe("en-US");
  });

  it("falls back to en-US for everything else", () => {
    expect(resolveSystemLanguage(["en-US"])).toBe("en-US");
    expect(resolveSystemLanguage(["en"])).toBe("en-US");
    expect(resolveSystemLanguage(["de-DE", "en"])).toBe("en-US");
    expect(resolveSystemLanguage([])).toBe("en-US");
  });

  it("prefers the first recognized language in order", () => {
    expect(resolveSystemLanguage(["en-US", "zh-CN"])).toBe("en-US");
    // First entry wins even when a later entry is also recognized.
    expect(resolveSystemLanguage(["ja-JP", "zh-CN"])).toBe("en-US");
    expect(resolveSystemLanguage(["zh-CN", "en-US"])).toBe("zh-CN");
  });
});

describe("runtime switching", () => {
  it("switches language and updates document lang without a restart", async () => {
    await initI18n("en-US");
    expect(applyLanguage("zh-CN", [])).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(applyLanguage("en-US", [])).toBe("en-US");
    expect(document.documentElement.lang).toBe("en-US");
  });

  it("resolves `system` through the detected WebView languages", async () => {
    await initI18n("en-US");
    expect(applyLanguage("system", ["zh-CN"])).toBe("zh-CN");
    expect(applyLanguage("system", ["en-GB"])).toBe("en-US");
  });
});

describe("error localization", () => {
  it("translates known diagnostic codes", async () => {
    await initI18n("en-US");
    const { t } = await import("i18next");
    const message = localizedErrorMessage(
      new WorkspaceError("VAULT_LOCKED", "backend message"),
      t,
    );
    expect(message).not.toBe("backend message");
    expect(message).toContain("Vault");

    applyLanguage("zh-CN");
    const zh = localizedErrorMessage(
      new WorkspaceError("VAULT_LOCKED", "backend message"),
      t,
    );
    expect(zh).toContain("密码库");
    applyLanguage("en-US");
  });

  it("falls back to the backend message for unknown codes", async () => {
    await initI18n("en-US");
    const { t } = await import("i18next");
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const message = localizedErrorMessage(
      new WorkspaceError("SOME_FUTURE_CODE", "backend detail"),
      t,
    );
    expect(message).toBe("backend detail");
    spy.mockRestore();
  });
});
