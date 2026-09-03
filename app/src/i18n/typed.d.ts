// Strict translation-key typing: the en-US resource shape is the canonical
// key tree, so `t("...")` calls are checked at compile time. Dynamic keys
// (backend diagnostic codes) go through an explicit cast in lib/errors.ts.
import type { enUS } from "./locales/en-US";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: typeof enUS;
    };
  }
}
