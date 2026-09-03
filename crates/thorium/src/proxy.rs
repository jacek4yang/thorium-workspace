//! Download-proxy support.
//!
//! The workspace can route its **own** downloads (Thorium release
//! discovery, install archives, and the connectivity probe) through a
//! user-configured proxy endpoint. This module deliberately contains no
//! other networking: browser profile traffic is never proxied here.
//!
//! Security posture:
//! - the proxy URL may embed credentials; it is therefore never included
//!   in error messages, logs, or diagnostics (see [`Client::new_with_proxy`]);
//! - the probe answer must look like an IP literal, so a proxy that returns
//!   an HTML error page or a captive-portal redirect fails loudly.

use std::time::Duration;

use crate::error::ThoriumError;
use crate::releases::Client;

/// Endpoint that answers with the caller's public IP as plain text.
pub const EXIT_IP_ENDPOINT: &str = "https://api.ip.sb/ip";

/// Timeout for the probe request; a proxy that cannot answer this fast is
/// not useful for a ~350 MB browser download anyway.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Descriptive UA; api.ip.sb asks clients to identify themselves.
const PROBE_USER_AGENT: &str = concat!(
    "thorium-workspace/",
    env!("CARGO_PKG_VERSION"),
    " proxy-probe"
);

impl Client {
    /// Builds a client that routes through `proxy_url` (`scheme://host:
    /// port`, optionally with credentials). Ambient proxy environment
    /// variables are disabled so the configured endpoint is used exactly.
    /// Errors never embed the proxy URL because it may contain credentials.
    pub fn new_with_proxy(proxy_url: &str) -> Result<Self, ThoriumError> {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|_| ThoriumError::ProxyConfig)?;
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "thorium-workspace/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/jacek4yang/thorium-workspace)"
            ))
            .connect_timeout(Duration::from_secs(20))
            .no_proxy()
            .proxy(proxy)
            .build()
            .map_err(|_| ThoriumError::ProxyConfig)?;
        Ok(Self { http })
    }

    /// Fetches the public exit IP as seen through this client's routing
    /// (direct, or through the proxy it was built with). Returns the
    /// trimmed IP literal.
    pub async fn fetch_exit_ip(&self) -> Result<String, ThoriumError> {
        let response = self
            .http
            .get(EXIT_IP_ENDPOINT)
            .timeout(PROBE_TIMEOUT)
            .header("User-Agent", PROBE_USER_AGENT)
            .send()
            .await
            .map_err(|error| ThoriumError::Probe(redact_url(&error.to_string())))?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            return Err(ThoriumError::Probe(format!(
                "probe endpoint returned {status}"
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|error| ThoriumError::Probe(redact_url(&error.to_string())))?;
        let ip = body.trim();
        if !looks_like_ip(ip) {
            return Err(ThoriumError::Probe(
                "probe answer was not an IP literal (proxy returned an error page?)".to_owned(),
            ));
        }
        Ok(ip.to_owned())
    }
}

/// Sanity check for the probe answer: IPv4 or IPv6 literals only.
fn looks_like_ip(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 45
        && text
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':')
}

/// Removes anything that looks like a URL from an error string so proxy
/// credentials can never reach a message the UI or logs would show.
fn redact_url(message: &str) -> String {
    if message.contains("://") {
        "request to the probe endpoint failed".to_owned()
    } else {
        message.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ThoriumError;

    #[test]
    fn proxy_client_rejects_invalid_endpoints() {
        assert!(matches!(
            Client::new_with_proxy("not a url at all"),
            Err(ThoriumError::ProxyConfig)
        ));
        // An unparseable host reaches the builder and is still ProxyConfig.
        assert!(matches!(
            Client::new_with_proxy("http://bad host name:port"),
            Err(ThoriumError::ProxyConfig)
        ));
    }

    /// Credentials embedded in the proxy URL are accepted for proxy
    /// authentication and never surface in the constructed client's
    /// rendering (reqwest strips them; we additionally never log clients).
    #[test]
    fn proxy_credentials_are_accepted_and_stripped() {
        let client = Client::new_with_proxy("http://user:hunter2@10.0.0.2:8080")
            .expect("credentials-bearing proxy URL parses");
        let rendered = format!("{client:?}");
        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("user:"));
    }

    #[test]
    fn ip_literal_check() {
        assert!(looks_like_ip("93.184.216.34"));
        assert!(looks_like_ip("2606:2800:220:1:248:1893:25c8:1946"));
        assert!(!looks_like_ip(""));
        assert!(!looks_like_ip("<html>blocked</html>"));
        assert!(!looks_like_ip("moved permanently"));
    }

    #[test]
    fn redaction_strips_urls_from_messages() {
        assert_eq!(
            redact_url("error parsing https://user:pass@api.ip.sb/ip"),
            "request to the probe endpoint failed"
        );
        assert_eq!(redact_url("connection refused"), "connection refused");
    }
}
