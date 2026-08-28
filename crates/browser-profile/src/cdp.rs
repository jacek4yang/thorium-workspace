//! The DevTools control channel used to apply timezone and locale.
//!
//! # Why a control channel at all
//!
//! `--lang` sets Chromium's UI language, and `TZ` is honoured by ICU on some
//! platforms and ignored on others. Neither reliably changes what a *page*
//! observes through `Intl.DateTimeFormat().resolvedOptions().timeZone` or
//! `navigator.language`. The DevTools `Emulation.setTimezoneOverride` and
//! `Emulation.setLocaleOverride` commands do, and they are the supported
//! mechanism.
//!
//! # Safety of the endpoint
//!
//! * The port is ephemeral (`--remote-debugging-port=0`) and chosen by
//!   Chromium.
//! * Chromium binds it to loopback; the bind address is never overridden.
//! * The endpoint is read from `DevToolsActivePort` inside the profile's own
//!   directory, so one profile cannot pick up another's channel.
//! * The connection is closed when the profile stops.
//!
//! Anyone who can already run code as this user can reach a loopback DevTools
//! port. That is inherent to using DevTools at all, and is stated plainly in
//! `THREAT-MODEL.md`.
//!
//! # Worker targets
//!
//! Both emulation commands have been observed to crash the renderer when sent
//! to a worker target, because they touch main-thread-only controllers. Only
//! `page` and `iframe` targets are ever addressed here.

use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::{ProfileError, ProfileResult};

/// The loopback DevTools endpoint for one browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpEndpoint {
    /// The ephemeral port Chromium chose.
    pub port: u16,
    /// The browser-level WebSocket path, for example
    /// `/devtools/browser/2f1c-...`.
    pub browser_path: String,
}

impl CdpEndpoint {
    /// The full browser WebSocket URL, always on loopback.
    #[must_use]
    pub fn websocket_url(&self) -> String {
        format!("ws://127.0.0.1:{}{}", self.port, self.browser_path)
    }
}

/// Parses the two-line `DevToolsActivePort` file Chromium writes.
///
/// Line one is the port; line two is the browser WebSocket path.
///
/// # Errors
///
/// Returns [`ProfileError::Cdp`] when the file is malformed.
pub fn parse_devtools_port_file(contents: &str) -> ProfileResult<CdpEndpoint> {
    let mut lines = contents.lines();
    let port: u16 = lines
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .ok_or_else(|| ProfileError::Cdp("the DevTools port file is empty".to_owned()))?
        .parse()
        .map_err(|_| ProfileError::Cdp("the DevTools port file does not start with a port".to_owned()))?;
    if port == 0 {
        return Err(ProfileError::Cdp(
            "the browser did not report a DevTools port".to_owned(),
        ));
    }
    let browser_path = lines.next().map(str::trim).unwrap_or_default().to_owned();
    if !browser_path.starts_with('/') {
        return Err(ProfileError::Cdp(
            "the DevTools port file has no browser endpoint".to_owned(),
        ));
    }
    Ok(CdpEndpoint { port, browser_path })
}

/// Waits for Chromium to publish its DevTools endpoint.
///
/// Chromium writes the file a moment after start, so this polls rather than
/// reading once. The file from a previous run is removed by the caller before
/// launch, so a stale port can never be adopted.
///
/// # Errors
///
/// Returns [`ProfileError::Cdp`] when the endpoint does not appear in time.
pub async fn read_devtools_endpoint(port_file: &Path, timeout: Duration) -> ProfileResult<CdpEndpoint> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(contents) = tokio::fs::read_to_string(port_file).await
            && let Ok(endpoint) = parse_devtools_port_file(&contents)
        {
            return Ok(endpoint);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ProfileError::Cdp(
                "the browser did not publish a DevTools endpoint in time".to_owned(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What to apply to every page in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulationSettings {
    /// IANA timezone identifier.
    pub timezone: String,
    /// BCP 47 locale tag.
    pub locale: String,
}

/// Target types the emulation commands may safely be sent to.
///
/// Deliberately excludes every worker type: sending these commands to a worker
/// has been observed to crash the renderer.
const EMULATABLE_TARGETS: [&str; 2] = ["page", "iframe"];

/// Connects to the browser endpoint and keeps every page's timezone and locale
/// overridden until `shutdown` resolves.
///
/// Auto-attach is enabled with `waitForDebuggerOnStart`, so a page created after
/// the browser started is paused before it runs any script, has the overrides
/// applied, and is only then released. Without that, a newly opened tab would
/// briefly observe the host timezone.
///
/// # Errors
///
/// Returns [`ProfileError::Cdp`] when the connection cannot be established, and
/// [`ProfileError::Emulation`] when the first override round fails.
pub async fn apply_emulation(
    endpoint: &CdpEndpoint,
    settings: &EmulationSettings,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> ProfileResult<()> {
    let url = endpoint.websocket_url();
    let (stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| ProfileError::Cdp(format!("the DevTools endpoint refused the connection: {e}")))?;
    let (mut writer, mut reader) = stream.split();

    // Every command needs a monotonically increasing id; the browser echoes it
    // back on the reply, which this client does not need to correlate.
    let mut next_id = 1i64;

    // Attach to existing and future targets, pausing new ones until the
    // overrides are in place. `flatten` puts every session on this one socket.
    send_command(
        &mut writer,
        &mut next_id,
        json!({
            "method": "Target.setAutoAttach",
            "params": { "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }
        }),
    )
    .await
    .map_err(|e| ProfileError::Emulation(format!("auto-attach could not be enabled: {e}")))?;

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                let _ = writer.send(Message::Close(None)).await;
                return Ok(());
            }
            incoming = reader.next() => {
                let Some(message) = incoming else {
                    // The browser closed the connection: it is shutting down.
                    return Ok(());
                };
                let Ok(Message::Text(text)) = message else {
                    continue;
                };
                let Ok(event) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if event.get("method").and_then(Value::as_str) != Some("Target.attachedToTarget") {
                    continue;
                }
                let Some(session_id) = event
                    .pointer("/params/sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                let target_type = event
                    .pointer("/params/targetInfo/type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();

                if EMULATABLE_TARGETS.contains(&target_type.as_str()) {
                    for payload in emulation_commands(settings, &session_id) {
                        if let Err(e) = send_command(&mut writer, &mut next_id, payload).await {
                            tracing::warn!(error = %e, "an emulation command could not be sent");
                        }
                    }
                }

                // Release the target whether or not it was emulated: a paused
                // worker would otherwise hang the browser.
                let resume = json!({
                    "sessionId": session_id,
                    "method": "Runtime.runIfWaitingForDebugger",
                    "params": {}
                });
                if let Err(e) = send_command(&mut writer, &mut next_id, resume).await {
                    tracing::warn!(error = %e, "a target could not be resumed");
                }
            }
        }
    }
}

/// Sends one CDP command, stamping it with the next request id.
async fn send_command<S>(
    writer: &mut S,
    next_id: &mut i64,
    mut payload: Value,
) -> Result<(), tokio_tungstenite::tungstenite::Error>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    if let Some(object) = payload.as_object_mut() {
        object.insert("id".to_owned(), json!(*next_id));
    }
    *next_id += 1;
    writer.send(Message::Text(payload.to_string().into())).await
}

/// The commands applied to one page session.
fn emulation_commands(settings: &EmulationSettings, session_id: &str) -> Vec<Value> {
    vec![
        json!({
            "sessionId": session_id,
            "method": "Emulation.setTimezoneOverride",
            "params": { "timezoneId": settings.timezone }
        }),
        json!({
            "sessionId": session_id,
            "method": "Emulation.setLocaleOverride",
            "params": { "locale": settings.locale }
        }),
        // Keeps the Accept-Language header and navigator.languages consistent
        // with the locale override, which otherwise only affects formatting.
        json!({
            "sessionId": session_id,
            "method": "Emulation.setUserAgentOverride",
            "params": { "userAgent": "", "acceptLanguage": settings.locale }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_port_file_parses() {
        let endpoint =
            parse_devtools_port_file("51234\n/devtools/browser/2f1c9a2e-0000-4000-8000-000000000000\n")
                .expect("parse");
        assert_eq!(endpoint.port, 51234);
        assert_eq!(
            endpoint.browser_path,
            "/devtools/browser/2f1c9a2e-0000-4000-8000-000000000000"
        );
        assert_eq!(
            endpoint.websocket_url(),
            "ws://127.0.0.1:51234/devtools/browser/2f1c9a2e-0000-4000-8000-000000000000"
        );
    }

    #[test]
    fn the_websocket_url_is_always_loopback() {
        let endpoint = CdpEndpoint {
            port: 1,
            browser_path: "/devtools/browser/x".to_owned(),
        };
        assert!(endpoint.websocket_url().starts_with("ws://127.0.0.1:"));
    }

    #[test]
    fn a_malformed_port_file_is_rejected() {
        for bad in [
            "",
            "\n",
            "not-a-port\n/devtools/browser/x",
            "51234",
            "51234\nno-leading-slash",
            "0\n/x",
        ] {
            assert!(parse_devtools_port_file(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[tokio::test]
    async fn waiting_for_the_endpoint_gives_up_rather_than_hanging() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = read_devtools_endpoint(&dir.path().join("DevToolsActivePort"), Duration::from_millis(120))
            .await
            .expect_err("must time out");
        assert!(matches!(err, ProfileError::Cdp(_)), "{err:?}");
    }

    #[tokio::test]
    async fn the_endpoint_is_picked_up_once_the_browser_writes_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("DevToolsActivePort");
        let writer = {
            let path = path.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(60)).await;
                tokio::fs::write(&path, "51234\n/devtools/browser/abc\n")
                    .await
                    .expect("write");
            })
        };
        let endpoint = read_devtools_endpoint(&path, Duration::from_secs(5))
            .await
            .expect("read");
        assert_eq!(endpoint.port, 51234);
        writer.await.expect("writer");
    }

    #[test]
    fn the_emulation_commands_carry_the_documented_parameter_names() {
        let settings = EmulationSettings {
            timezone: "Europe/Warsaw".to_owned(),
            locale: "pl-PL".to_owned(),
        };
        let commands = emulation_commands(&settings, "SESSION-1");
        assert_eq!(commands.len(), 3);

        assert_eq!(commands[0]["method"], "Emulation.setTimezoneOverride");
        assert_eq!(commands[0]["params"]["timezoneId"], "Europe/Warsaw");
        assert_eq!(commands[0]["sessionId"], "SESSION-1");

        assert_eq!(commands[1]["method"], "Emulation.setLocaleOverride");
        assert_eq!(commands[1]["params"]["locale"], "pl-PL");

        assert_eq!(commands[2]["method"], "Emulation.setUserAgentOverride");
        assert_eq!(commands[2]["params"]["acceptLanguage"], "pl-PL");
        assert_eq!(
            commands[2]["params"]["userAgent"], "",
            "an empty user agent keeps Chromium's real one; this must never spoof it"
        );
    }

    #[test]
    fn worker_targets_are_never_emulated() {
        // Sending these commands to a worker has been observed to crash the
        // renderer, so the allow-list must stay restricted to page-like targets.
        assert_eq!(EMULATABLE_TARGETS, ["page", "iframe"]);
        for worker in ["worker", "service_worker", "shared_worker", "browser", "other"] {
            assert!(
                !EMULATABLE_TARGETS.contains(&worker),
                "{worker} must not be emulated"
            );
        }
    }
}
