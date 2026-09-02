import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { WorkspaceError, WorkspaceSettings } from "../lib/types";

export default function SettingsPage() {
  const [settings, setSettings] = useState<WorkspaceSettings | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const loaded = await api.settingsGet();
        if (active) {
          setSettings(loaded);
          applyTheme(loaded.theme);
        }
      } catch (thrown) {
        if (active) setError(toError(thrown));
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  if (!settings) {
    return (
      <section aria-labelledby="settings-heading">
        <h2 id="settings-heading">Settings</h2>
        {error && <p className="error" role="alert">{error.message}</p>}
        {!error && <p className="muted">Loading settings…</p>}
      </section>
    );
  }

  const update = (patch: Partial<WorkspaceSettings>) => {
    setSaved(false);
    setSettings({ ...settings, ...patch });
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.settingsSave(settings);
      applyTheme(settings.theme);
      setSaved(true);
      setError(null);
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="settings-heading">
      <h2 id="settings-heading">Settings</h2>
      <form
        className="card"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <label>
          Clipboard clear delay (seconds, 5–120)
          <input
            type="number"
            min={5}
            max={120}
            value={settings.clipboardClearSeconds}
            onChange={(event) =>
              update({ clipboardClearSeconds: Number(event.target.value) })
            }
          />
        </label>
        <label>
          Vault idle auto-lock (minutes, empty disables)
          <input
            type="number"
            min={1}
            max={240}
            value={settings.vaultIdleLockMinutes ?? ""}
            onChange={(event) =>
              update({
                vaultIdleLockMinutes:
                  event.target.value === "" ? null : Number(event.target.value),
              })
            }
          />
        </label>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={settings.vaultLockOnMinimize}
            onChange={(event) =>
              update({ vaultLockOnMinimize: event.target.checked })
            }
          />
          Lock the Vault when the window is minimized
        </label>
        <label>
          Theme
          <select
            value={settings.theme}
            onChange={(event) =>
              update({ theme: event.target.value as WorkspaceSettings["theme"] })
            }
          >
            <option value="system">Follow system</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
        <label>
          Preferred Thorium variant for new installs
          <select
            value={settings.preferredThoriumVariant}
            onChange={(event) =>
              update({ preferredThoriumVariant: event.target.value })
            }
          >
            <option value="AVX2">AVX2</option>
            <option value="AVX">AVX</option>
            <option value="AVX512">AVX512</option>
            <option value="SSE4">SSE4</option>
            <option value="SSE3">SSE3</option>
            <option value="WIN32_SSE2">WIN32_SSE2</option>
          </select>
        </label>
        <button type="submit" disabled={busy}>
          Save settings
        </button>
        {saved && <p className="muted">Saved.</p>}
        {error && <p className="error" role="alert">{error.message}</p>}
      </form>
    </section>
  );
}

/** Applies the theme preference; "system" defers to the OS. */
function applyTheme(theme: WorkspaceSettings["theme"]): void {
  document.documentElement.style.colorScheme =
    theme === "system" ? "" : theme;
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
