# Claude Code Instructions — Thorium Workspace v1.0.0

## Mission

Implement the first production-quality release of `thorium-workspace` for Windows.

The target version is **v1.0.0**.

This version deliberately contains **no proxy, Xray, subscription, routing, WFP proxy kill-switch, or anti-detect functionality**. Do not add those systems in this PR. The architecture should leave room for them later without introducing speculative complexity now.

The product is a portable Windows desktop workspace for persistent Thorium browser profiles, account metadata, encrypted credentials, and standards-based 2FA.

## Required stack

- Windows 10 22H2 / Windows 11 only.
- Current stable Rust.
- Tauri v2.
- TypeScript frontend; React is preferred unless a clearly better maintained small alternative is justified.
- Rust owns all security-sensitive and persistent behavior.
- Frontend is presentation/state orchestration only.
- Prefer rustls over OpenSSL runtime dependencies.
- Build the primary release as a portable `.exe` with frontend assets embedded.

## Product invariants

1. Persistent business data is rooted beside `std::env::current_exe()`.
2. Never silently fall back to `%APPDATA%` or `%LOCALAPPDATA%` for workspace business data.
3. If the executable directory is not writable, fail clearly and explain how to fix it.
4. One Browser Profile owns one distinct Thorium `User Data` directory.
5. Never run two conflicting instances against the same profile directory.
6. One Browser Profile may contain many accounts.
7. Passwords, OTP seeds, recovery codes, and other account secrets are never plaintext database fields.
8. Secrets must not appear in logs, panic text, diagnostics, URLs, or normal frontend state.
9. Browser binaries and browser profile data are separate.
10. Thorium updates are staged and atomically promoted; never destroy the last usable version during a failed update.
11. Browser child processes must be supervised using Windows Job Objects where practical.
12. No console windows should appear during normal GUI use.
13. No telemetry.
14. Do not implement account-registration automation, CAPTCHA handling, mass automation, or fingerprint randomization.

## Required v1.0.0 subsystems

### Portable bootstrap

On first start:

- resolve executable directory;
- verify writability;
- acquire a Windows named mutex derived from the normalized workspace path;
- create the portable directory layout;
- initialize versioned storage/schema;
- recover safely from stale temporary/runtime files;
- show first-run onboarding instead of crashing on absent data.

### Storage

Use SQLite for non-secret metadata if appropriate (`rusqlite` with bundled SQLite is preferred).

Persist at minimum:

- workspace settings;
- Browser Profiles;
- Accounts;
- factor metadata;
- Thorium installations;
- runtime/session metadata where useful;
- schema version.

Implement explicit schema migrations and tests.

### Vault

Prefer interoperable KDBX4 only if the current Rust ecosystem provides a mature, safe implementation for the exact operations needed. Evaluate this before committing to it.

If KDBX4 support is inadequate, use established audited primitives and a versioned encrypted vault format. Do not invent cryptography.

Requirements:

- master password not stored beside the app;
- memory-hard password derivation (Argon2id or KDBX4 equivalent);
- authenticated encryption;
- atomic save;
- backup before risky migration;
- redacting secret wrapper;
- zeroization where practical;
- idle auto-lock;
- manual lock;
- optional lock-on-minimize setting;
- frontend receives only explicitly requested secret material.

Document the final vault decision in `DECISIONS.md` and `SECURITY.md`.

### Account model

Generic account type with presets such as GitHub and Microsoft.

Support:

- display name;
- service kind;
- username;
- email;
- login URL;
- tags;
- notes;
- encrypted password reference;
- multiple second factors;
- recovery codes.

Do not hard-code the core around GitHub/Microsoft.

### 2FA

Implement standards-based:

- `otpauth://totp/`;
- `otpauth://hotp/`;
- SHA-1/SHA-256/SHA-512 where standards allow;
- 6/8 digits;
- configurable period/counter;
- issuer/account labels.

Use RFC vectors in tests.

QR import:

- image file;
- clipboard image;
- Windows screen-region capture if a robust implementation can be completed for v1.0.0.

Never log raw QR payloads or `otpauth://` URIs.

Microsoft Authenticator push/number matching/passwordless registration is not ordinary TOTP. Do not emulate it. Represent it only as an external authenticator reference if useful.

Recovery codes:

- encrypted at rest;
- unused/used status;
- explicit mark-used action;
- timestamp when marked used.

### Clipboard security

For copied passwords/TOTP/recovery codes:

- automatically clear after a short configurable interval;
- only clear if the clipboard still contains the exact value written by this app;
- never erase newer clipboard content from another application.

### Thorium manager

Use official Windows portable Thorium releases. Do not fork Chromium/Thorium.

Implement:

- release discovery using current upstream information;
- portable asset download;
- bounded/time-limited download behavior;
- staging directory;
- integrity validation using trustworthy upstream digest/signature information when available;
- extraction validation (`thorium.exe` and expected structure);
- versioned installs under `browsers/thorium/versions/<version>/`;
- atomic `current` selection;
- retain previous known-good version;
- manual update check;
- install/delete unused versions safely;
- do not delete a version used by a running profile.

Do not hard-code stale Thorium asset names. Verify current upstream release structure during implementation.

### Browser Profiles

Each profile stores:

- ID/name;
- Thorium version selection (`Current` or pinned);
- unique User Data path;
- startup URLs;
- locale;
- timezone;
- account IDs;
- timestamps;
- safe advanced launch options only if necessary.

Launch with explicit `--user-data-dir=<absolute path>`.

Use a per-profile lock.

When already running, focus/show the existing session or clearly report the state; never create a conflicting second profile instance.

### Windows process supervision

Use Windows Job Objects for the Thorium process tree where practical.

Prefer `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` with carefully documented lifetime behavior.

No visible console windows for child processes.

Keep all necessary unsafe Win32 FFI inside a small Windows-only module/crate, with documented invariants and handle ownership.

### Timezone and locale

Allow a Browser Profile to configure an IANA timezone and locale.

Use current supported Chromium/DevTools mechanisms (for example CDP timezone/locale emulation if still valid). Verify against the current Thorium/Chromium version; do not assume old flags still work.

If CDP is used:

- bind debugging only to loopback;
- use an ephemeral/random port;
- do not expose it on LAN;
- handle new relevant targets;
- cleanly shut it down with the profile.

Clearly document any browser surfaces where overrides cannot be guaranteed.

Do not spoof random Canvas/WebGL/GPU/audio/hardware properties.

### Backup and recovery

Implement logical backup of:

- workspace metadata;
- vault;
- settings.

Browser User Data backup may be optional/explicit because it can be very large. Never blindly copy a running Chromium profile as if it were a consistent snapshot.

Startup recovery must clean only project-owned stale temporary/runtime files.

### Diagnostics

Provide a GUI diagnostics page showing safe information such as:

- workspace path/writability;
- schema version;
- vault locked/unlocked;
- installed Thorium versions;
- current Thorium path/version;
- profile running state;
- CDP state when applicable;
- timezone/locale application state.

Use stable diagnostic codes in Rust errors where useful.

A copied diagnostic report must redact secrets aggressively.

## Architecture

Use a Rust workspace rather than one giant backend file. A reasonable direction is:

```text
crates/
  domain/
  secrets/
  storage/
  vault/
  otp/
  qr/
  thorium/
  browser-profile/
  controller/
  windows-platform/
  test-support/

src-tauri/
app/
docs/
.github/workflows/
```

The exact split may change if justified, but preserve dependency direction:

```text
domain <- storage/vault/otp/thorium/browser-profile <- controller <- Tauri commands <- frontend
```

`domain` must not depend on Tauri, React, Win32, SQLite, or HTTP clients.

## Unsafe policy

Use `#![forbid(unsafe_code)]` in platform-independent/security-sensitive crates where practical.

Only the smallest Windows FFI layer may use unsafe code when unavoidable.

Every unsafe block must explain:

- why unsafe is necessary;
- pointer/lifetime assumptions;
- handle ownership;
- cleanup invariant.

## Production error policy

Do not use `unwrap`, `expect`, or `panic!` for runtime/user/network/untrusted input paths.

Return typed errors and surface useful diagnostics.

## UI

Main sections:

- Dashboard
- Profiles
- Accounts
- Browser
- Vault
- Settings
- Diagnostics

Requirements:

- polished Windows desktop utility;
- responsive on common DPI scales;
- dark/light theme following system where practical;
- keyboard-accessible common actions;
- useful empty states;
- no developer-console UX;
- no modal spam.

## CI

Create Windows-only CI on `windows-latest`.

At minimum run:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
frontend lint/test
Tauri build smoke test
```

Use locked dependency files.

## Release

Create a tag-triggered release workflow for `v*`.

Target v1.0.0 release artifacts:

```text
ThoriumWorkspace-v1.0.0-x86_64.exe
ThoriumWorkspace-v1.0.0-x86_64.exe.sha256
```

The end user must not need Rust, Node.js, npm, or a separate installer.

Thorium itself is runtime-managed/downloaded and is not embedded into the main EXE.

## Required documentation

Create and maintain:

- `ARCHITECTURE.md`
- `DECISIONS.md`
- `SECURITY.md`
- `THREAT-MODEL.md`
- `STATUS.md`
- `CONTRIBUTING.md`
- `CHANGELOG.md`
- `THIRD_PARTY_NOTICES.md`

Do not claim a feature is complete without executable evidence.

## Required tests

At minimum cover:

- portable path initialization;
- unwritable directory behavior where testable;
- workspace/profile locks;
- database migrations;
- vault create/unlock/wrong-password/read/write/reopen/corruption paths;
- RFC HOTP/TOTP vectors;
- QR import with synthetic credentials;
- clipboard conditional-clear behavior;
- Thorium install staging/rollback using fixtures/mocks;
- browser-profile isolation;
- Job Object child cleanup on Windows integration tests;
- redaction tests proving secrets do not appear in `Debug`/diagnostics/log-safe errors.

Never use personal credentials in tests.

## Definition of Done for v1.0.0

Do not mark v1.0.0 complete until this workflow works on Windows:

1. Download one portable app EXE from GitHub Release.
2. Put it in a writable folder and run it.
3. The workspace initializes beside the EXE.
4. Create/unlock the encrypted Vault.
5. Install portable Thorium from the GUI.
6. Create two independent Browser Profiles.
7. Add several account records to each Profile.
8. Store and retrieve account passwords securely.
9. Import a standard TOTP QR and produce RFC-correct live codes.
10. Store and mark recovery codes used.
11. Launch both profiles with independent Thorium User Data directories.
12. Confirm configured timezone/locale behavior where supported.
13. Stop/restart the manager without losing persistent state.
14. Browser profile processes clean up correctly.
15. Diagnostics expose no secrets.
16. CI is green.
17. Tag build produces the portable v1.0.0 EXE and SHA-256 artifact.

## Work style

Start by inspecting this repository and current upstream documentation/source for Tauri v2 and Thorium Windows releases.

Create an implementation plan, then begin implementing immediately. Do not spend the entire session writing plans.

Make small, meaningful commits.

Run quality gates continuously.

Do not rewrite `main`, force-push, or weaken tests merely to get green CI.

When finished, push a feature branch and open a PR against `main`.

In the PR description include:

- architecture summary;
- implemented features;
- security decisions;
- test evidence;
- Windows build evidence;
- known limitations;
- exact remaining work, if any.

Never claim CI/build/tests passed unless you actually observed them.
