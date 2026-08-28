# Changelog

All notable changes are documented here. This project uses semantic versioning.

## [1.0.0] — unreleased

The first release: a portable Windows workspace for Thorium browser profiles,
accounts, encrypted credentials and standards-based two-factor authentication.

### Portable workspace

- All data lives beside the executable. There is no installer, no registry key,
  and no fallback to `%APPDATA%` or `%LOCALAPPDATA%`.
- Startup resolves the executable's own directory, proves it is writable by
  writing to it, takes a named mutex so a second copy cannot open the same
  folder, creates the layout, migrates storage and clears stale runtime files.
- An unusable folder opens the window and explains the problem and its remedy
  rather than exiting silently.

### Vault

- Versioned encrypted format: Argon2id key derivation, XChaCha20-Poly1305
  authenticated encryption, with the header authenticated so cost parameters
  cannot be rewritten downwards.
- Atomic writes: a crash leaves either the previous complete vault or the new
  one. Changing the master password backs up first.
- Idle auto-lock, manual lock, and an optional lock-on-minimize.
- Orphan collection for secrets nothing references any more.

### Accounts and two-factor authentication

- Generic account records with GitHub and Microsoft presets, tags and notes.
- HOTP (RFC 4226) and TOTP (RFC 6238) over SHA-1, SHA-256 and SHA-512, with 6 or
  8 digits and a configurable period or counter.
- QR import from an image file, the clipboard, or a screen scan; `otpauth://`
  links can also be pasted directly.
- Recovery codes stored encrypted, with used/unused status and the time each was
  marked used.
- Factors handled by another application or device are recorded as such. Vendor
  push approval, number matching and passwordless sign-in are not one-time
  passwords and are not emulated.

### Browser profiles

- One profile owns one Thorium `User Data` directory, named after the profile's
  immutable id so renaming can never move or merge browser state.
- A per-profile lock makes two conflicting browsers against one directory
  impossible; launching a running profile brings its window to the front.
- Timezone and locale applied through the DevTools emulation commands over an
  ephemeral loopback port, opened only when a profile overrides one of them and
  closed with the profile.
- The process tree is supervised by a Windows Job Object, so a crashed manager
  cannot orphan a browser.

### Thorium management

- Release discovery by rule rather than by file name, so an upstream rename does
  not break installs or select the wrong artefact.
- Bounded downloads, digest recorded from the stream, archive entries validated
  against traversal, symlinks and reserved device names, and the extracted tree
  checked before promotion.
- Atomic version selection; an update never deletes the previous version;
  rollback and safe removal.

### Clipboard, backup and diagnostics

- Copied secrets are cleared on a timer, but only if the clipboard still holds
  exactly what this application wrote.
- Logical backups of the database, the still-encrypted vault and a manifest.
- A diagnostics page and a copyable report that redacts paths and omits profile
  names.

### Not in this release, by design

No proxy, Xray, subscription, routing or WFP kill-switch. No fingerprint
randomization, CAPTCHA handling or account-registration automation. No telemetry.
