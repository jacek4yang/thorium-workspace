//! Value objects and the validation rules that guard them.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::diagnostics::DiagnosticCode;
use crate::error::{DomainError, DomainResult};

/// Maximum length of any user-supplied display name.
pub const MAX_DISPLAY_NAME: usize = 120;
/// Maximum length of a free-text notes field.
pub const MAX_NOTES: usize = 8_000;
/// Maximum number of tags on one account.
pub const MAX_TAGS: usize = 32;
/// Maximum length of a single tag.
pub const MAX_TAG: usize = 48;
/// Maximum number of startup URLs on one profile.
pub const MAX_STARTUP_URLS: usize = 16;

/// Validates and normalizes a user-visible display name.
///
/// Trims surrounding whitespace, rejects empty or over-long names and rejects
/// control characters (which would corrupt log lines and list rendering).
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] when the name is empty, too long or
/// contains control characters.
pub fn validate_display_name(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::invalid("name must not be empty"));
    }
    if trimmed.chars().count() > MAX_DISPLAY_NAME {
        return Err(DomainError::invalid(format!(
            "name must be at most {MAX_DISPLAY_NAME} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(DomainError::invalid("name must not contain control characters"));
    }
    Ok(trimmed.to_owned())
}

/// Normalizes a tag list: trims, lowercases, drops empties, de-duplicates and
/// sorts so two accounts tagged the same way compare and render identically.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] when a tag is too long or there are
/// too many tags.
pub fn normalize_tag_list<S: AsRef<str>>(raw: &[S]) -> DomainResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for tag in raw {
        let t = tag.as_ref().trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > MAX_TAG {
            return Err(DomainError::invalid(format!(
                "tag must be at most {MAX_TAG} characters"
            )));
        }
        if t.chars().any(char::is_control) {
            return Err(DomainError::invalid("tag must not contain control characters"));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(DomainError::invalid(format!(
            "at most {MAX_TAGS} tags are allowed"
        )));
    }
    out.sort();
    Ok(out)
}

/// Validates a URL a browser profile opens at startup.
///
/// Only `http`, `https` and `about` are accepted. `file://` is rejected because a
/// profile's startup list is persisted configuration that would otherwise be a
/// convenient way to make the browser open arbitrary local content.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] for unparseable URLs and unsupported
/// schemes.
pub fn validate_startup_url(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::invalid("startup URL must not be empty"));
    }
    let parsed = url::Url::parse(trimmed).map_err(|_| {
        DomainError::invalid("startup URL must be an absolute URL, for example https://example.com")
    })?;
    match parsed.scheme() {
        "http" | "https" | "about" => Ok(parsed.to_string()),
        other => Err(DomainError::invalid(format!(
            "startup URL scheme '{other}' is not supported; use http, https or about"
        ))),
    }
}

/// Validates an account's login URL. Accepts the same schemes as a startup URL.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] for unparseable URLs and unsupported
/// schemes.
pub fn validate_login_url(raw: &str) -> DomainResult<String> {
    validate_startup_url(raw)
}

/// A BCP 47 language tag such as `en-US`.
///
/// Validation is structural rather than a registry lookup: Chromium accepts any
/// syntactically valid tag and a registry snapshot would go stale.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocaleTag(String);

impl LocaleTag {
    /// The locale used when a profile does not override it.
    #[must_use]
    pub fn default_tag() -> Self {
        Self("en-US".to_owned())
    }

    /// Parses and normalizes a BCP 47 language tag.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::InvalidInput`] when the tag is structurally
    /// invalid.
    pub fn parse(raw: &str) -> DomainResult<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(DomainError::invalid("locale must not be empty"));
        }
        if trimmed.len() > 35 {
            return Err(DomainError::invalid("locale is not a valid BCP 47 language tag"));
        }
        let mut subtags = trimmed.split('-');
        let primary = subtags.next().unwrap_or_default();
        let primary_ok = (2..=8).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_alphabetic());
        if !primary_ok {
            return Err(DomainError::invalid(
                "locale must start with a 2-8 letter language subtag, for example en or en-US",
            ));
        }
        for sub in subtags {
            let ok = (1..=8).contains(&sub.len()) && sub.chars().all(|c| c.is_ascii_alphanumeric());
            if !ok {
                return Err(DomainError::invalid(
                    "locale subtags must be 1-8 alphanumeric characters",
                ));
            }
        }
        // Canonical BCP 47 casing: language lowercase, script Titlecase,
        // 2-letter region uppercase, everything else lowercase.
        let mut parts = Vec::new();
        for (index, sub) in trimmed.split('-').enumerate() {
            let canonical = if index == 0 {
                sub.to_lowercase()
            } else if sub.len() == 4 && sub.chars().all(|c| c.is_ascii_alphabetic()) {
                let mut chars = sub.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + &chars.as_str().to_lowercase(),
                    None => String::new(),
                }
            } else if sub.len() == 2 && sub.chars().all(|c| c.is_ascii_alphabetic()) {
                sub.to_uppercase()
            } else {
                sub.to_lowercase()
            };
            parts.push(canonical);
        }
        Ok(Self(parts.join("-")))
    }

    /// Returns the canonical tag.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LocaleTag {
    fn default() -> Self {
        Self::default_tag()
    }
}

impl fmt::Display for LocaleTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An IANA timezone identifier such as `Europe/Warsaw`.
///
/// Validated against the compiled-in IANA database so the UI can only ever
/// persist a name Chromium's `Emulation.setTimezoneOverride` will accept.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TimeZoneId(String);

impl TimeZoneId {
    /// The timezone used when a profile does not override it.
    #[must_use]
    pub fn utc() -> Self {
        Self("UTC".to_owned())
    }

    /// Parses an IANA timezone identifier.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::InvalidInput`] when the name is not in the IANA
    /// database.
    pub fn parse(raw: &str) -> DomainResult<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(DomainError::invalid("timezone must not be empty"));
        }
        let found = chrono_tz::TZ_VARIANTS.iter().find(|tz| tz.name() == trimmed);
        match found {
            Some(tz) => Ok(Self(tz.name().to_owned())),
            None => Err(DomainError::new(
                DiagnosticCode::InvalidInput,
                format!("'{trimmed}' is not an IANA timezone identifier"),
            )
            .with_remedy("Pick a timezone from the list, for example Europe/Warsaw or America/New_York.")),
        }
    }

    /// Returns the canonical identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns every IANA identifier this build knows about, sorted.
    #[must_use]
    pub fn available() -> Vec<&'static str> {
        let mut names: Vec<&'static str> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

impl Default for TimeZoneId {
    fn default() -> Self {
        Self::utc()
    }
}

impl fmt::Display for TimeZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_are_trimmed_and_bounded() {
        assert_eq!(validate_display_name("  Work  ").expect("valid"), "Work");
        assert!(validate_display_name("   ").is_err());
        assert!(validate_display_name(&"x".repeat(MAX_DISPLAY_NAME + 1)).is_err());
        assert!(validate_display_name("bad\u{0}name").is_err());
    }

    #[test]
    fn tags_are_normalized_deduplicated_and_sorted() {
        let tags = normalize_tag_list(&["Work", " work ", "Personal", ""]).expect("valid");
        assert_eq!(tags, vec!["personal".to_owned(), "work".to_owned()]);
    }

    #[test]
    fn tag_limits_are_enforced() {
        assert!(normalize_tag_list(&["x".repeat(MAX_TAG + 1)]).is_err());
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("tag{i}")).collect();
        assert!(normalize_tag_list(&many).is_err());
    }

    #[test]
    fn startup_urls_accept_web_schemes_only() {
        assert_eq!(
            validate_startup_url("https://example.com").expect("valid"),
            "https://example.com/"
        );
        assert!(validate_startup_url("about:blank").is_ok());
        assert!(validate_startup_url("file:///C:/secret.txt").is_err());
        assert!(validate_startup_url("javascript:alert(1)").is_err());
        assert!(validate_startup_url("example.com").is_err());
        assert!(validate_startup_url("").is_err());
    }

    #[test]
    fn locale_tags_are_canonicalized() {
        assert_eq!(LocaleTag::parse("EN-us").expect("valid").as_str(), "en-US");
        assert_eq!(
            LocaleTag::parse("zh-hans-cn").expect("valid").as_str(),
            "zh-Hans-CN"
        );
        assert_eq!(LocaleTag::parse("pl").expect("valid").as_str(), "pl");
        assert!(LocaleTag::parse("e").is_err());
        assert!(LocaleTag::parse("en_US").is_err());
        assert!(LocaleTag::parse("").is_err());
    }

    #[test]
    fn timezones_are_validated_against_the_iana_database() {
        assert_eq!(
            TimeZoneId::parse("Europe/Warsaw").expect("valid").as_str(),
            "Europe/Warsaw"
        );
        assert_eq!(TimeZoneId::parse(" UTC ").expect("valid").as_str(), "UTC");
        assert!(TimeZoneId::parse("Mars/Olympus_Mons").is_err());
        assert!(
            TimeZoneId::parse("europe/warsaw").is_err(),
            "IANA names are case sensitive"
        );
    }

    #[test]
    fn available_timezones_are_sorted_and_non_empty() {
        let names = TimeZoneId::available();
        assert!(
            names.len() > 300,
            "expected a full IANA database, got {}",
            names.len()
        );
        assert!(names.windows(2).all(|w| w[0] < w[1]));
        assert!(names.contains(&"America/New_York"));
    }
}
