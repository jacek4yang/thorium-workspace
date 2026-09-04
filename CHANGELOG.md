# Changelog

All notable changes will be documented in this file.

## [1.0.0]

### Added

- Portable Windows workspace for persistent Thorium browser profiles:
  portable bootstrap with single-instance mutex, writability checks, and
  first-run onboarding; all business data stays beside the executable.
- Browser Profiles with isolated User Data directories, locale/timezone,
  startup URLs, pinned or following-current Thorium versions, launch/stop
  with Job Object supervision and double-launch protection.
- Accounts with GitHub/Microsoft/Google/GitLab presets and custom services;
  passwords, TOTP/HOTP factors, external authenticator references, and
  recovery codes encrypted in the Vault.
- Vault: Argon2id + ChaCha20-Poly1305 KDBX-grade container with atomic
  saves, backup-on-rotate, idle auto-lock, lock-on-minimize, and manual
  lock; secrets are redacted everywhere and clipboard copies auto-clear.
- Standards-based 2FA: RFC 4226/6238 HOTP/TOTP (SHA-1/256/512, 6/8 digits),
  otpauth:// and QR image import, live codes with countdown.
- Thorium manager: upstream release discovery, parallel segmented download
  (8 ranges, per-segment retry and resume, stall watchdog), staged extract,
  atomic promotion, previous-version retention, protected deletes.
- Download proxy setting (http/https/socks5/socks5h) used only for
  workspace downloads, with an ip.sb connectivity test; never applied to
  browser profile traffic.
- Diagnostics page with redaction-pinned facts and copy-as-report.
- UI: Fluent-inspired dense desktop shell (light/dark/system), Simplified
  Chinese and English runtime languages, keyboard shortcuts (Alt+1..7),
  and full keyboard accessibility.

### Distribution

- Tag-triggered GitHub Release with two artifacts per version: a portable
  `ThoriumWorkspace-v*-x86_64.exe` and an NSIS per-user installer
  (`*-x64-setup.exe`), each with a SHA-256 checksum file. The installer
  targets a user-writable directory so workspace data keeps living beside
  the executable; no admin rights and no external dependencies.

Proxy/Xray/subscription/routing functionality is intentionally deferred to
a later release.
