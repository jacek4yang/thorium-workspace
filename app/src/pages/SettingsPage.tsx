// Settings are grouped by concern rather than presented as one form. The
// Save button stays explicit: settings persist through the backend, and the
// theme applies live the moment it is saved (and is also owned by the shell
// on startup).

import { useState } from "react";

import {
  Button,
  Card,
  ErrorNotice,
  Field,
  PageHeader,
} from "../components/ui";
import { api } from "../lib/api";
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
      onToast("Settings saved");
    } catch (thrown) {
      setError(toError(thrown));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <PageHeader
        title="Settings"
        subtitle="Workspace-wide behaviour; saved settings apply immediately"
        actions={
          <>
            <Button variant="primary" disabled={busy || !dirty} onClick={() => void save()}>
              Save settings
            </Button>
            {saved && !dirty && <span className="faint">Saved</span>}
          </>
        }
      />
      <div className="page-body stack" style={{ maxWidth: 760 }}>
        {error && <ErrorNotice error={error} onDismiss={() => setError(null)} />}

        <Card title="Appearance">
          <Field label="Theme" hint="Follows the Windows light/dark setting by default">
            {(id) => (
              <select
                id={id}
                value={draft.theme}
                onChange={(event) =>
                  update({ theme: event.target.value as WorkspaceSettings["theme"] })
                }
              >
                <option value="system">Follow system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            )}
          </Field>
        </Card>

        <Card title="Vault security">
          <div className="stack">
            <Field
              label="Vault idle auto-lock (minutes)"
              hint="Empty disables auto-lock. Locking happens even when this window is in the background."
            >
              {(id) => (
                <input
                  id={id}
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
                />
              )}
            </Field>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={draft.vaultLockOnMinimize}
                onChange={(event) => update({ vaultLockOnMinimize: event.target.checked })}
              />
              <span className="checkbox-text">
                <strong>Lock the Vault when the window is minimized</strong>
                <span className="faint">
                  Recommended. Minimizing is treated as leaving the workspace.
                </span>
              </span>
            </label>
          </div>
        </Card>

        <Card title="Clipboard" subtitle="How long copied secrets survive in the clipboard">
          <Field
            label="Clipboard clear delay (seconds, 5–120)"
            hint="Only clears if the clipboard still holds the exact value this app wrote — newer content from other applications is never erased."
          >
            {(id) => (
              <input
                id={id}
                type="number"
                min={5}
                max={120}
                value={draft.clipboardClearSeconds}
                onChange={(event) =>
                  update({ clipboardClearSeconds: Number(event.target.value) })
                }
              />
            )}
          </Field>
        </Card>

        <Card
          title="Browser"
          subtitle="Default used when installing new Thorium versions"
        >
          <Field
            label="Preferred Thorium variant"
            hint="Pick the newest instruction set your CPU supports; AVX2 fits most modern machines."
          >
            {(id) => (
              <select
                id={id}
                value={draft.preferredThoriumVariant}
                onChange={(event) => update({ preferredThoriumVariant: event.target.value })}
              >
                {VARIANTS.map((variant) => (
                  <option key={variant} value={variant}>
                    {variant}
                  </option>
                ))}
              </select>
            )}
          </Field>
        </Card>
      </div>
    </>
  );
}

function toError(thrown: unknown): WorkspaceError {
  return thrown instanceof WorkspaceError
    ? thrown
    : new WorkspaceError("FRONTEND_UNKNOWN", String(thrown));
}
