// Localization of backend errors.
//
// The backend returns stable diagnostic codes plus an English message. The
// frontend recognizes known codes and shows a localized explanation; the
// original backend message remains the fallback for unknown codes so
// nothing becomes undiagnosable.

import type { TFunction } from "i18next";

import { WorkspaceError } from "./types";

/**
 * Localized explanation for a backend error. Unknown codes fall back to
 * the backend-provided message; diagnostic codes themselves are never
 * translated and are still shown alongside (see ErrorNotice).
 */
export function localizedErrorMessage(error: WorkspaceError, t: TFunction): string {
  const key = `common.errors.${error.code}`;
  // `as never` is the single sanctioned escape from strict key typing:
  // diagnostic codes arrive at runtime from the backend.
  const translated: string = t(key as never);
  // i18next returns the key itself when nothing matched.
  if (translated === key || translated.startsWith("common.errors.")) {
    return error.message;
  }
  return translated;
}
