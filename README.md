# Thorium Workspace

A portable Windows-only workspace for managing Thorium browser profiles, accounts, credentials, and two-factor authentication.

## v1.0.0 Scope

The first release intentionally focuses on the local browser/account workspace and contains **no proxy or Xray functionality**.

### Goals

- Windows 10/11 only.
- Portable application: persistent application data lives beside the executable.
- Tauri v2 desktop GUI with a Rust backend.
- A single portable application EXE as the primary GitHub Release artifact.
- Download, install, validate, update, and manage portable Thorium versions.
- Create multiple persistent Thorium browser profiles with isolated `User Data` directories.
- One browser profile may contain multiple accounts.
- Generic account records with useful GitHub and Microsoft presets.
- Encrypted secret storage for passwords, TOTP/HOTP seeds, and recovery codes.
- Standard `otpauth://` TOTP/HOTP support.
- Import 2FA QR codes from image files and clipboard images; screen-region capture is a v1.0.0 goal when a robust Windows-native implementation is practical.
- Recovery-code management.
- Secure clipboard handling and vault auto-lock.
- Browser-profile timezone and locale configuration using supported Chromium/DevTools mechanisms when possible.
- Crash-safe storage, migrations, backups, diagnostics, tests, and GitHub Actions.

### Explicitly out of scope for v1.0.0

- Xray-core.
- Proxy subscriptions or proxy pools.
- SOCKS/HTTP/VLESS/VMess/Trojan/Reality routing.
- WFP proxy leak protection.
- CAPTCHA solving.
- Automated account creation.
- Automated social-media operation.
- Anti-detect/randomized browser fingerprinting.

## Portable Layout

At runtime the application should initialize state relative to its executable, conceptually:

```text
ThoriumWorkspace/
├── ThoriumWorkspace.exe
├── workspace.db
├── workspace.json
├── vault/
├── browsers/
├── profiles/
├── runtime/
├── backups/
└── logs/
```

Business state must not silently move into `%APPDATA%` or `%LOCALAPPDATA%`. If the executable directory is not writable, the application should report that clearly.

## Development

The detailed v1.0.0 implementation contract is in [`CLAUDE.md`](CLAUDE.md) and [`docs/V1.0.0.md`](docs/V1.0.0.md).

Development should happen on a feature branch and be merged through a pull request.

## License

No project license has been selected yet. Do not copy code from GPL or other restrictively licensed projects into this repository without first making the licensing implications explicit.
