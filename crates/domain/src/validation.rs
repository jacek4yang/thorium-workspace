//! Shared validation helpers for user-entered metadata.
//!
//! None of these validators accept secret material; they only constrain
//! non-secret metadata fields.

use crate::error::DomainError;

/// Maximum length for human-facing names (profiles, accounts, factors).
pub const NAME_MAX: usize = 100;

/// Maximum length for free-text notes.
pub const NOTES_MAX: usize = 10_000;

/// Maximum number of tags per entity.
pub const TAGS_MAX: usize = 20;

/// A required, human-facing name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name(String);

impl Name {
    /// Validates and wraps a human-facing name.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(DomainError::EmptyName);
        }
        if trimmed.chars().count() > NAME_MAX {
            return Err(DomainError::NameTooLong { max: NAME_MAX });
        }
        if trimmed.chars().any(|c| c.is_control()) {
            return Err(DomainError::ControlCharacters);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the validated name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A URL restricted to http/https schemes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl(String);

impl HttpUrl {
    /// Validates and wraps an http(s) URL.
    ///
    /// Parsing is intentionally conservative: a valid absolute URL with an
    /// `http`/`https` scheme and a non-empty host.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let has_scheme = lowered.starts_with("http://") || lowered.starts_with("https://");
        if !has_scheme {
            return Err(DomainError::InvalidUrl);
        }
        let authority_start = trimmed.find("://").map(|i| i + 3).unwrap_or(0);
        let authority_end = trimmed[authority_start..]
            .find(['/', '?', '#'])
            .map(|i| authority_start + i)
            .unwrap_or(trimmed.len());
        let host = &trimmed[authority_start..authority_end];
        let host = host.rsplit('@').next().unwrap_or(host);
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err(DomainError::InvalidUrl);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the validated URL.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An IANA timezone identifier such as `America/Los_Angeles`.
///
/// Full tz-database membership is not validated here (that would require
/// embedding tzdata); structural validation catches the common mistakes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IanaTimeZone(String);

impl IanaTimeZone {
    /// Validates and wraps an IANA timezone identifier.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        let looks_like_iana = trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-' || c == '+')
            && trimmed.contains('/')
            && !trimmed.starts_with('/')
            && !trimmed.ends_with('/')
            && !trimmed.contains("//")
            && !trimmed.contains("..")
            && trimmed.len() <= 64;
        let legacy_exception = trimmed == "UTC" || trimmed == "Etc/UTC";
        if !looks_like_iana && !legacy_exception {
            return Err(DomainError::InvalidTimezone);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the validated timezone identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A BCP-47 style language tag such as `en-US` or `zh-CN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleTag(String);

impl LocaleTag {
    /// Validates and wraps a BCP-47 style language tag.
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        let valid = !trimmed.is_empty()
            && trimmed.len() <= 35
            && trimmed.split('-').all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.chars().all(|c| c.is_ascii_alphanumeric())
            })
            && trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
        if !valid {
            return Err(DomainError::InvalidLocale);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the validated locale tag.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Normalizes and validates a tag list.
///
/// Tags are deduplicated case-insensitively, keeping the casing of the
/// first occurrence.
pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, DomainError> {
    let mut normalized: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.chars().any(|c| c.is_control()) || trimmed.len() > 50 {
            return Err(DomainError::InvalidTag);
        }
        let already_present = normalized
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed));
        if !already_present {
            normalized.push(trimmed.to_owned());
        }
        if normalized.len() > TAGS_MAX {
            return Err(DomainError::OutOfRange { field: "tags" });
        }
    }
    Ok(normalized)
}

/// Validates free-text notes.
pub fn validate_notes(notes: &str) -> Result<(), DomainError> {
    if notes.chars().count() > NOTES_MAX {
        return Err(DomainError::OutOfRange { field: "notes" });
    }
    if notes
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(DomainError::ControlCharacters);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_accepts_normal_text() {
        let name = Name::new("  Test Profile A  ").expect("valid");
        assert_eq!(name.as_str(), "Test Profile A");
    }

    #[test]
    fn name_rejects_empty_and_long() {
        assert!(Name::new("   ").is_err());
        assert!(Name::new(&"x".repeat(NAME_MAX + 1)).is_err());
    }

    #[test]
    fn url_requires_http_scheme_and_host() {
        assert!(HttpUrl::new("https://github.com/login").is_ok());
        assert!(HttpUrl::new("http://example.com").is_ok());
        assert!(HttpUrl::new("ftp://example.com").is_err());
        assert!(HttpUrl::new("https://").is_err());
        assert!(HttpUrl::new("file:///C:/x").is_err());
        assert!(HttpUrl::new("javascript:alert(1)").is_err());
    }

    #[test]
    fn timezone_accepts_iana_and_rejects_garbage() {
        assert!(IanaTimeZone::new("America/Los_Angeles").is_ok());
        assert!(IanaTimeZone::new("Europe/Warsaw").is_ok());
        assert!(IanaTimeZone::new("UTC").is_ok());
        assert!(IanaTimeZone::new("Etc/UTC").is_ok());
        assert!(IanaTimeZone::new("not a timezone").is_err());
        assert!(IanaTimeZone::new("/leading").is_err());
        assert!(IanaTimeZone::new("a//b").is_err());
        assert!(IanaTimeZone::new("..").is_err());
    }

    #[test]
    fn locale_accepts_bcp47_and_rejects_garbage() {
        assert!(LocaleTag::new("en-US").is_ok());
        assert!(LocaleTag::new("zh-CN").is_ok());
        assert!(LocaleTag::new("pl").is_ok());
        assert!(LocaleTag::new("en_US").is_err());
        assert!(LocaleTag::new("123").is_err());
        assert!(LocaleTag::new("").is_err());
    }

    #[test]
    fn tags_dedupe_and_trim() {
        let tags = normalize_tags(&[" work ".into(), "WORK".into(), "".into(), "github".into()])
            .expect("valid");
        assert_eq!(tags, vec!["work", "github"]);
    }

    #[test]
    fn notes_allow_newlines_but_not_controls() {
        assert!(validate_notes("line1\nline2\ttab").is_ok());
        assert!(validate_notes(&"x".repeat(NOTES_MAX + 1)).is_err());
        assert!(validate_notes("bad\u{0007}bell").is_err());
    }
}
