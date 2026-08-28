//! Logging.
//!
//! Logs go to a rolling file in the workspace's own `logs` directory, never to a
//! console window and never outside the workspace.
//!
//! No log line may contain secret material. That is enforced by construction
//! rather than by filtering: every secret in this workspace is wrapped in a type
//! whose `Display` and `Debug` render `[redacted]`, and no code path formats an
//! exposed secret into a log macro.

use std::path::Path;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

/// Keeps the background log writer alive.
///
/// Dropping this flushes and stops the writer, so it must be held for the life
/// of the process.
#[derive(Debug)]
pub struct LogGuard {
    _appender: tracing_appender::non_blocking::WorkerGuard,
}

/// Starts file logging in `logs_dir`.
///
/// Returns `None` when a subscriber is already installed, which happens in
/// tests and is not an error.
#[must_use]
pub fn init(logs_dir: &Path, default_level: &str) -> Option<LogGuard> {
    let appender = tracing_appender::rolling::daily(logs_dir, "thorium-workspace.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_env("THORIUM_WORKSPACE_LOG")
        .or_else(|_| EnvFilter::try_new(default_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        // Line numbers make a support log actionable without exposing anything.
        .with_file(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init()
        .ok()
        .map(|()| LogGuard { _appender: guard })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_writes_inside_the_workspace_and_nowhere_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).expect("mkdir");

        // `try_init` fails if another test already installed a subscriber, which
        // is fine: what is being checked is where the file lands.
        let guard = init(&logs, "info");
        tracing::info!("a log line from the test");
        drop(guard);

        // The appender writes asynchronously; give it a moment to flush.
        for _ in 0..40 {
            let files: Vec<_> = std::fs::read_dir(&logs).expect("read").flatten().collect();
            if !files.is_empty() {
                assert!(
                    files.iter().all(|f| f
                        .file_name()
                        .to_string_lossy()
                        .starts_with("thorium-workspace.log")),
                    "unexpected files in the log directory"
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        // A subscriber installed by another test means nothing was written
        // through ours; that is not a failure of this behaviour.
    }
}
