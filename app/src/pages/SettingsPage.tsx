// Settings read vertically and semantically: uppercase group labels
// (General / Security / Browser & Downloads), each followed by a raised
// section whose rows are title+description left, control right. It is a
// configuration form, not a dashboard of independent cards.
//
// Save stays explicit: the button tracks dirty state, a save shows a toast,
// and theme/language apply to the live document the moment they are saved
// (and are also owned by the shell on startup).

import { useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Badge,
  Button,
  ErrorNotice,
  Notice,
  PageHeader,
  SettingRow,
  SettingSection,
} from "../components/ui";
import { api } from "../lib/api";
import { localizedErrorMessage } from "../lib/errors";
import type { ToastFn } from "../lib/hooks";
import type { WorkspaceSettings } from "../lib/types";
import { WorkspaceError } from "../lib/types";

const VARIANTS = ["AVX2", "AVX", "AVX512", "SSE4", "SSE3", "WIN32_SSE2"];

export default function SettingsPage({
  settings,
  onSettingsChanged,
  onToast,
}: {
  settings: WorkspaceSettings;
  onSettingsChanged: (settings: WorkspaceSettings) => void;
  onToast: ToastFn;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<WorkspaceSettings>(settings);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);

  const update = (patch: Partial<WorkspaceSettings>) => {
    setSaved(false);
    setDraft({ ...draft, ...patch });
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.settingsSave(draft);
      onSettingsChanged(draft);
      setSaved(true);
      setError(null);
      onToast(t("settings.savedToast"));
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <PageHeader
        title={t("settings.title")}
        subtitle={t("settings.subtitle")}
        actions={
          <>
            <Button
              variant="primary"
              disabled={busy || !dirty}
              onClick={() => void save()}
            >
              {busy ? <span className="spinner" /> : null}
              {t("common.saveSettings")}
            </Button>
            {saved && !dirty && <span className="faint">{t("settings.saved")}</span>}
          </>
        }
      />
      <div className="page-body" style={{ maxWidth: 900, marginInline: "auto" }}>
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        <SettingSection
          label={t("settings.general.title")}
          title={t("settings.general.appearance")}
        >
          <SettingRow
            title={t("settings.general.theme")}
            description={t("settings.general.themeHint")}
            control={
              <select
                value={draft.theme}
                onChange={(event) =>
                  update({ theme: event.target.value as WorkspaceSettings["theme"] })
                }
                aria-label={t("settings.general.theme")}
                style={{ width: 170 }}
              >
                <option value="system">{t("settings.general.themeSystem")}</option>
                <option value="light">{t("settings.general.themeLight")}</option>
                <option value="dark">{t("settings.general.themeDark")}</option>
              </select>
            }
          />
          <SettingRow
            title={t("settings.general.language")}
            description={t("settings.general.languageHint")}
            control={
              <select
                value={draft.language}
                onChange={(event) =>
                  update({ language: event.target.value as WorkspaceSettings["language"] })
                }
                aria-label={t("settings.general.language")}
                style={{ width: 170 }}
              >
                <option value="system">{t("settings.general.languageSystem")}</option>
                <option value="en-US">English</option>
                <option value="zh-CN">简体中文</option>
              </select>
            }
          />
        </SettingSection>

        <SettingSection
          label={t("settings.security.title")}
          title={t("settings.security.vault")}
        >
          <SettingRow
            title={t("settings.security.autoLock")}
            description={t("settings.security.autoLockHint")}
            control={
              <input
                type="number"
                min={1}
                max={240}
                value={draft.vaultIdleLockMinutes ?? ""}
                onChange={(event) =>
                  update({
                    vaultIdleLockMinutes:
                      event.target.value === "" ? null : Number(event.target.value),
                  })
                }
                aria-label={t("settings.security.autoLock")}
                style={{ width: 110 }}
              />
            }
          />
          <SettingRow
            title={t("settings.security.lockOnMinimize")}
            description={t("settings.security.lockOnMinimizeDesc")}
            control={
              <input
                type="checkbox"
                checked={draft.vaultLockOnMinimize}
                onChange={(event) => update({ vaultLockOnMinimize: event.target.checked })}
                aria-label={t("settings.security.lockOnMinimize")}
              />
            }
          />
        </SettingSection>

        <SettingSection
          label={t("settings.security.title")}
          title={t("settings.security.clipboard")}
          subtitle={t("settings.security.clipboardSubtitle")}
        >
          <SettingRow
            title={t("settings.security.clearDelay")}
            description={t("settings.security.clearDelayHint")}
            control={
              <input
                type="number"
                min={5}
                max={120}
                value={draft.clipboardClearSeconds}
                onChange={(event) =>
                  update({ clipboardClearSeconds: Number(event.target.value) })
                }
                aria-label={t("settings.security.clearDelay")}
                style={{ width: 110 }}
              />
            }
          />
        </SettingSection>

        <SettingSection
          label={t("settings.browserDownloads.title")}
          title={t("settings.browserDownloads.thorium")}
          subtitle={t("settings.browserDownloads.thoriumSubtitle")}
        >
          <SettingRow
            title={t("settings.browserDownloads.preferredVariant")}
            description={t("settings.browserDownloads.preferredVariantHint")}
            control={
              <select
                value={draft.preferredThoriumVariant}
                onChange={(event) => update({ preferredThoriumVariant: event.target.value })}
                aria-label={t("settings.browserDownloads.preferredVariant")}
                style={{ width: 170 }}
              >
                {VARIANTS.map((variant) => (
                  <option key={variant} value={variant}>
                    {variant}
                  </option>
                ))}
              </select>
            }
          />
        </SettingSection>

        <SettingSection
          label={t("settings.browserDownloads.title")}
          title={t("settings.browserDownloads.downloads")}
          subtitle={t("settings.browserDownloads.downloadsSubtitle")}
        >
          <DownloadsRow
            draft={draft}
            onUpdate={(downloadProxy) => update({ downloadProxy })}
          />
        </SettingSection>
      </div>
    </>
  );
}

/**
 * Download proxy configuration. The proxy routes workspace downloads only
 * (Thorium discovery and install archives); browser profile traffic never
 * touches it. The Test action probes ip.sb through the candidate routing
 * without saving, so the endpoint can be verified before committing.
 */
function DownloadsRow({
  draft,
  onUpdate,
}: {
  draft: WorkspaceSettings;
  onUpdate: (downloadProxy: string | null) => void;
}) {
  const { t } = useTranslation();
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [failure, setFailure] = useState<WorkspaceError | null>(null);

  const test = async () => {
    const candidate = draft.downloadProxy?.trim() || null;
    setTesting(true);
    setResult(null);
    setFailure(null);
    try {
      const probed = await api.proxyTest(candidate);
      setResult(probed.exitIp);
    } catch (thrown) {
      setFailure(toError(thrown));
    } finally {
      setTesting(false);
    }
  };

  const proxied = Boolean(draft.downloadProxy?.trim());

  return (
    <div className="setting-row stacked" style={{ borderTop: "none" }}>
      <div className="setting-row-text">
        <div className="setting-row-title">{t("settings.browserDownloads.proxy")}</div>
        <div className="setting-row-description">
          {t("settings.browserDownloads.proxyHint")}
        </div>
      </div>
      <div className="stack-tight" style={{ marginTop: 8 }}>
        <div className="row">
          <input
            type="text"
            value={draft.downloadProxy ?? ""}
            onChange={(event) =>
              onUpdate(event.target.value.trim() === "" ? null : event.target.value)
            }
            placeholder={t("settings.browserDownloads.proxyPlaceholder")}
            spellCheck={false}
            style={{ maxWidth: 340 }}
            aria-label={t("settings.browserDownloads.proxy")}
          />
          <Button icon="external" disabled={testing} onClick={() => void test()}>
            {testing ? (
              <>
                <span className="spinner" />
                {t("common.testing")}
              </>
            ) : (
              t("settings.browserDownloads.test")
            )}
          </Button>
          {result !== null && !testing && (
            <Badge tone="success" icon="check">
              {t("settings.browserDownloads.exitIp", { ip: result })}
            </Badge>
          )}
          {proxied && result === null && !failure && !testing && (
            <Badge>{t("settings.browserDownloads.routedBadge")}</Badge>
          )}
        </div>
        {failure && (
          <Notice tone="error" title={t("settings.browserDownloads.testFailed")}>
            {localizedErrorMessage(failure, t)} <code>{failure.code}</code>
          </Notice>
        )}
        <p className="setting-row-description" style={{ maxWidth: "none" }}>
          {t("settings.browserDownloads.testExplanation")}
        </p>
      </div>
    </div>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
