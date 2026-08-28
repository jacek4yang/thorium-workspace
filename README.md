# Thorium Workspace

A portable Windows workspace for keeping several independent Thorium browser
profiles, the accounts that belong to them, and the secrets those accounts need,
in one folder you can move, copy or delete as a unit.

There is no installer. Put the executable in a folder you can write to and run
it; the workspace is created beside it.

> **Status**: v1.0.0 is built but not yet released. `STATUS.md` records exactly
> what has been verified and what has not.

---

## What it does

**Isolated browser profiles.** Each profile is one Thorium browser with its own
`User Data` directory: its own cookies, history and signed-in sessions. Two
profiles never share state, and two browsers can never run against one profile
directory.

**Accounts, with their secrets encrypted.** A generic account record — display
name, service, username, email, sign-in URL, tags, notes — with its password,
second factors and recovery codes held in an encrypted vault. The metadata
database stores only references to them.

**Standards-based two-factor authentication.** HOTP (RFC 4226) and TOTP
(RFC 6238) over SHA-1, SHA-256 and SHA-512, 6 or 8 digits, configurable period
or counter. Import a QR code from an image file, the clipboard, or by scanning
the screen — or paste the `otpauth://` link.

**Recovery codes.** Stored encrypted, with used/unused status and the time each
was marked used.

**Thorium version management.** Portable Thorium builds are downloaded from the
upstream project at your request, verified, and installed side by side. Updating
never deletes the version you had, so you can always roll back.

**Timezone and locale per profile.** Applied through the browser's own supported
mechanism, over a loopback debugging port that is opened only when a profile
overrides one of them and closed when the profile stops.

**Clipboard that cleans up after itself.** A copied password or code is cleared
on a timer — but only if the clipboard still contains exactly what this program
put there. If you copied something else in the meantime, your content is left
alone.

## What it deliberately does not do

No proxy, no Xray, no subscriptions, no network routing, no IP rotation, no WFP
kill-switch. No fingerprint randomization — canvas, WebGL, GPU and audio are left
exactly as Chromium reports them. No CAPTCHA handling, no automated account
registration, no automated posting. No telemetry.

A profile's timezone and locale are a convenience for using a site as intended,
not a disguise. `THREAT-MODEL.md` is explicit about what this protects and what
it does not.

---

## Requirements

- Windows 10 22H2 or Windows 11, x86-64.
- The Microsoft Edge WebView2 runtime, which ships with Windows 11 and with
  up-to-date Windows 10.
- A folder you can write to.

Nothing else. No Rust, no Node.js, no npm, no separate installer.

## Installing

1. Download `ThoriumWorkspace-v1.0.0-x86_64.exe` and its `.sha256` from the
   GitHub release.
2. Verify it:

   ```powershell
   Get-FileHash .\ThoriumWorkspace-v1.0.0-x86_64.exe -Algorithm SHA256
   ```

   and compare with the `.sha256` file.
3. Put it in a writable folder and run it.

To move the whole workspace, move the folder. To remove it, delete the folder.

## The workspace folder

```text
ThoriumWorkspace/
├── ThoriumWorkspace.exe
├── workspace.db                     metadata; contains no secret values
├── vault/workspace.twvault          the encrypted vault
├── browsers/thorium/                installed browser versions
├── profiles/<profile-id>/User Data  one directory per browser profile
├── runtime/                         transient; cleared at startup
├── backups/                         logical backups
└── logs/                            rolling log files
```

If the folder is not writable, the application says so and explains how to fix
it. It never quietly writes your data somewhere else.

---

## Security in one paragraph

The vault is encrypted with a key derived from your master password by Argon2id
and sealed with XChaCha20-Poly1305, with the file header authenticated so its
cost parameters cannot be rewritten. The master password is never stored and
cannot be recovered. Secrets are wrapped in types that render `[redacted]`
through every formatting path, so they cannot reach a log line or a diagnostic
report. The metadata database is **not** encrypted — account names, usernames and
notes are readable without the master password, which is why the notes field says
not to put secrets in it. `SECURITY.md` and `THREAT-MODEL.md` have the full
picture, including what this cannot defend against.

---

## Building from source

You do not need to build it to use it. If you want to:

```sh
# Prerequisites: the Rust toolchain (rust-toolchain.toml pins the version),
# Node.js 22, and on Windows the MSVC build tools.

npm --prefix app ci
npm --prefix app run build
cargo build --release --package thorium-workspace
# -> target/release/thorium-workspace.exe
```

The full gate set, which CI runs on `windows-latest`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm --prefix app run lint
npm --prefix app run typecheck
npm --prefix app test
```

## Documentation

| File | What it covers |
| --- | --- |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Layers, crates, on-disk layout, startup sequence |
| [`DECISIONS.md`](DECISIONS.md) | What was decided, what it was weighed against, when to revisit |
| [`SECURITY.md`](SECURITY.md) | Vault format, secret handling, untrusted input, reporting |
| [`THREAT-MODEL.md`](THREAT-MODEL.md) | Who this defends against, and who it does not |
| [`STATUS.md`](STATUS.md) | What is verified, what is not, known limitations |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Workflow and quality gates |
| [`CHANGELOG.md`](CHANGELOG.md) | Release notes |
| [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) | Dependencies and their licences |
| [`docs/V1.0.0.md`](docs/V1.0.0.md) | The v1.0.0 product contract |

## Thorium

Thorium is a separate project. This application downloads official portable
Thorium builds at your request and manages them; it does not bundle, fork or
modify Thorium or Chromium. Upstream: https://github.com/Alex313031/Thorium

## Licence

No licence has been chosen yet. Until one is, the usual defaults apply: the
source is published for review, not for redistribution. Do not copy GPL or other
restrictively licensed code into this repository without making the implications
explicit first.
