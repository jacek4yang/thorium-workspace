/**
 * Settings.
 *
 * Every setting here has a real consequence, so each one says what it does
 * rather than relying on its label.
 */
import { Icon } from "../components/Icon";
import { ErrorNotice, Field, Notice } from "../components/ui";
import { api } from "../lib/api";
import { formatBytes, formatRelative } from "../lib/format";
import { useAsync } from "../lib/hooks";
import type { AppError, ThoriumChannel, WorkspaceSettings } from "../lib/types";
import type { ToastFn } from "../App";

const CHANNELS: { id: ThoriumChannel; label: string; note: string }[] = [
  {
    id: "windows_avx2",
    label: "Windows x64 (AVX2)",
    note: "The fastest build most processors made since about 2015 can run.",
  },
  {
    id: "windows_avx",
    label: "Windows x64 (AVX)",
    note: "The upstream baseline. Choose this if AVX2 builds will not start.",
  },
  {
    id: "windows_sse3",
    label: "Windows x64 (SSE3)",
    note: "For older processors without AVX.",
  },
  {
    id: "windows_arm64",
    label: "Windows on ARM (arm64)",
    note: "For ARM-based Windows devices.",
  },
];

export function SettingsPage({
  settings,
  onToast,
  onSettingsChanged,
}: {
  settings: WorkspaceSettings | null;
  onToast: ToastFn;
  onSettingsChanged: (settings: WorkspaceSettings) => void;
}) {
  const backups = useAsync(() => api.listBackups(), []);

  const update = async (next: WorkspaceSettings) => {
    try {
      onSettingsChanged(await api.setSettings(next));
    } catch (caught) {
      onToast((caught as AppError).message, "error");
    }
  };

  if (!settings) {
    return (
      <>
        <header className="page-header">
          <h1>Settings</h1>
        </header>
        <div className="page-body">
          <div className="row">
            <span className="spinner" />
            <span className="muted">Loading settings…</span>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Settings</h1>
          <div className="subtitle">Appearance, clipboard protection, updates and backups.</div>
        </div>
      </header>

      <div className="page-body stack">
        <div className="card stack">
          <div className="card-header">
            <h2>Appearance</h2>
          </div>
          <Field label="Theme" hint="Following the system matches the Windows app colour mode.">
            {(id) => (
              <select
                id={id}
                value={settings.theme}
                onChange={(event) =>
                  void update({
                    ...settings,
                    theme: event.target.value as WorkspaceSettings["theme"],
                  })
                }
              >
                <option value="system">Follow the system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            )}
          </Field>
        </div>

        <div className="card stack">
          <div className="card-header">
            <h2>Clipboard</h2>
          </div>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.clipboard.clear_enabled}
              onChange={(event) =>
                void update({
                  ...settings,
                  clipboard: { ...settings.clipboard, clear_enabled: event.target.checked },
                })
              }
            />
            <span className="checkbox-text">
              <strong>Clear copied secrets automatically</strong>
              <span className="faint">
                Only if the clipboard still contains exactly what this application put there. If you
                copy something else in the meantime, your content is left alone.
              </span>
            </span>
          </label>
          <Field label="Clear after" hint="Between 5 seconds and 5 minutes.">
            {(id) => (
              <select
                id={id}
                value={settings.clipboard.clear_after_seconds}
                disabled={!settings.clipboard.clear_enabled}
                onChange={(event) =>
                  void update({
                    ...settings,
                    clipboard: {
                      ...settings.clipboard,
                      clear_after_seconds: Number(event.target.value),
                    },
                  })
                }
              >
                <option value={10}>10 seconds</option>
                <option value={20}>20 seconds</option>
                <option value={30}>30 seconds</option>
                <option value={60}>1 minute</option>
                <option value={120}>2 minutes</option>
                <option value={300}>5 minutes</option>
              </select>
            )}
          </Field>
          <div>
            <button
              type="button"
              className="button"
              onClick={async () => {
                const cleared = await api.clearClipboard();
                onToast(
                  cleared
                    ? "Clipboard cleared"
                    : "The clipboard no longer holds anything this app copied",
                );
              }}
            >
              <Icon name="clipboard" />
              Clear the clipboard now
            </button>
          </div>
        </div>

        <div className="card stack">
          <div className="card-header">
            <h2>Browser updates</h2>
          </div>
          <Field label="Build" hint="Which upstream builds new installs come from.">
            {(id) => (
              <select
                id={id}
                value={settings.thorium_channel}
                onChange={(event) =>
                  void update({
                    ...settings,
                    thorium_channel: event.target.value as ThoriumChannel,
                  })
                }
              >
                {CHANNELS.map((channel) => (
                  <option key={channel.id} value={channel.id}>
                    {channel.label}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <p className="faint">
            {CHANNELS.find((channel) => channel.id === settings.thorium_channel)?.note}
          </p>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.check_thorium_updates_on_start}
              onChange={(event) =>
                void update({ ...settings, check_thorium_updates_on_start: event.target.checked })
              }
            />
            <span className="checkbox-text">
              <strong>Check for a newer Thorium at start</strong>
              <span className="faint">
                Off by default. With it off, this application makes no network request you did not
                ask for.
              </span>
            </span>
          </label>
        </div>

        <div className="card stack">
          <div className="card-header">
            <h2>Backups</h2>
            <span className="spacer" />
            <button
              type="button"
              className="button small"
              onClick={async () => {
                try {
                  const outcome = await api.createBackup();
                  onToast(`Backup written (${formatBytes(outcome.bytes)})`);
                  backups.reload();
                } catch (caught) {
                  onToast((caught as AppError).message, "error");
                }
              }}
            >
              <Icon name="download" size={13} />
              Back up now
            </button>
          </div>
          <p className="muted">
            A backup contains the workspace database, the still-encrypted vault and a manifest. It
            does not contain browser data: that is recreatable, very large, and cannot be copied
            safely while a browser is running.
          </p>
          {backups.error ? <ErrorNotice error={backups.error} /> : null}
          {(backups.data ?? []).length === 0 ? (
            <p className="faint">No backups yet.</p>
          ) : (
            <div className="scroll-x">
              <table>
                <thead>
                  <tr>
                    <th>File</th>
                    <th>Size</th>
                  </tr>
                </thead>
                <tbody>
                  {(backups.data ?? []).map((backup) => (
                    <tr key={backup.path}>
                      <td className="mono truncate selectable">{backup.name}</td>
                      <td className="muted">{formatBytes(backup.bytes)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <Notice tone="info" title="Restoring a backup">
            Restoring replaces the database and vault, so it is done deliberately rather than from a
            button that is easy to press by accident. Close the application, replace{" "}
            <span className="mono">workspace.db</span> and{" "}
            <span className="mono">vault/workspace.twvault</span> with the copies inside the archive,
            and start it again.
          </Notice>
        </div>

        <div className="card stack">
          <div className="card-header">
            <h2>Housekeeping</h2>
          </div>
          <p className="faint">
            Backups are kept until you delete them. The oldest listed above is{" "}
            {backups.data && backups.data.length > 0
              ? formatRelative(
                  Number(
                    backups.data[backups.data.length - 1]?.name.match(/(\d{10})/)?.[1] ?? "0",
                  ),
                )
              : "n/a"}
            .
          </p>
        </div>
      </div>
    </>
  );
}
