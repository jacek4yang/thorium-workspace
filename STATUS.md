# Implementation Status — v1.0.0 branch

Last updated: 2026-09-03. Branch: `feature/v1.0.0-implementation`.
This file is the hand-off contract: a new agent should be able to resume
accurately from this state.

## Toolchain used (verified)

- Windows 11 (10.0.26200), rustc/cargo 1.98.0 stable MSVC, Node 26, pnpm 11.9.
- Quality gates at last commit: `cargo fmt --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  (161 tests, 0 failures), frontend lint/typecheck/test.
- Dev proxy used for all upstream access: `http://127.0.0.1:10808`
  (HTTP and SOCKS5H both verified). Never leak into committed code.

## Committed, tested subsystems

| Crate | State | Notes |
|---|---|---|
| `domain` | done | accounts, profiles, factors, recovery codes, settings, validation, diagnostic codes (34 tests) |
| `secrets` | done | `SecretText`/`SecretBytes`: redacting Debug, no Serialize, zeroized, constant-time compare (7 tests) |
| `otp` | done | RFC 4226/6238 HOTP/TOTP SHA-1/256/512, 6/8 digits; `otpauth://` parser that never leaks rejected URIs (17 tests; RFC vectors) |
| `storage` | done | SQLite (rusqlite bundled), schema v1, WAL, FK on; profiles/accounts/factors/recovery-codes/settings/installs/runtime-meta repos; atomic account writes; conflict mapping (28 tests) |
| `vault` | done | Argon2id (64 MiB, t=3, p=1) + ChaCha20-Poly1305; header authenticated as AAD; atomic save + `.bak`; create/unlock/lock/rotate; plaintext scrubbed (23 tests; plaintext-on-disk leak tests) |
| `windows-platform` | done | exe-relative portable bootstrap with writability probe; FNV-1a named mutexes (single-instance); Job Objects `KILL_ON_JOB_CLOSE`; `CREATE_NO_WINDOW` spawn + job assignment; clipboard copy + conditional clear (never erases foreign content). All Win32 FFI confined here with documented invariants (20 tests) |
| `browser-profile` | done | `LaunchSpec` (explicit absolute `--user-data-dir`, `--no-first-run`, `--lang`, argument allowlist); `ProfileLock` (in-process registry + named mutex; Win32 re-entrancy hole closed); `Session` supervision with shutdown+reap (16 tests with cmd.exe stand-ins) |
| `qr` | done | rqrr decode from PNG/JPEG; single/multiple/no-code semantics; payload never logged (5 tests with synthetic otpauth QR fixtures) |
| `thorium` | done | catalog verified against live GitHub API 2026-09-02 (portable zips `Thorium_<VARIANT>_<VERSION>.zip`; `gz83/thorium` carries current builds, `Alex313031/Thorium` M144+ tags are stubs); rustls discovery; bounded streaming download (`.part` + rename); staging extract with zip-slip guard; atomic promote; `current` marker; delete protection (11 tests + 1 ignored live test, verified passing through the dev proxy) |

`crates/controller` is implemented and tested (commit cd8a1fd): Workspace
bootstrap (portable root + layout + single-instance mutex/registry + stale
temp recovery), vault lifecycle, profile service (eager `profiles/<uuid>/User
Data` creation, launch planning, supervised launch/stop), account service
(deletes purge all secrets), passwords (set/get/copy with ClipboardScheduler
— superseded timers provably cannot erase newer secrets), factors (otpauth +
QR import, RFC-correct TOTP, counter-advancing HOTP), recovery codes, idle
auto-lock (explicit-instant, no sleeps), redaction-pinned diagnostics.

`src-tauri` is wired: 33 typed commands over the controller, `CmdError`
{code,message} boundary (no raw Debug), 1s housekeeping thread (clipboard
tick + idle lock), window-focus activity recording. Frontend vertical slice:
7-section navigation, Vault create/unlock/lock, Profiles create/list/launch/
stop/delete, Diagnostics; system dark/light theme.

Verified live on Windows: `pnpm tauri dev` launches; portable workspace
initializes beside the debug exe (workspace.db, vault/, profiles/, browsers/,
runtime/, backups/, logs/); first-run "Create your Vault" UI renders.

Stubs not yet implemented: `test-support`; Accounts/Browser/Settings/
Dashboard pages are placeholders in the UI.

## Upstream facts (verified 2026-09-02 through proxy)

- Latest tag `M152.0.7977.55`; Windows portable assets:
  `Thorium_AVX2_152.0.7977.55.zip` (~350 MB), AVX/AVX512/SSE4/SSE3/WIN32_SSE2.
- Portable zip extracts with `BIN/thorium.exe` (installer top-level dir).
- No upstream SHA-256 digests are published per asset; integrity currently
  relies on TLS + zip CRC + structure validation. Documented in SECURITY.md
  (TODO: digests if/when upstream publishes them).

## Remaining work for v1.0.0 (ordered)

1. Manual GUI checkpoint: profile persistence across restart (in progress).
2. Accounts page + password/OTP/recovery UI (backend commands ready).
3. CDP timezone/locale emulation (loopback-only, ephemeral port,
   DevToolsActivePort handshake) — the supported mechanism per contract;
   no deprecated CLI timezone flags.
4. Backup/recovery of metadata+vault.
5. Two-profile E2E with real Thorium on this machine (download 350 MB
   asset, run Test Profile A/B).
6. Release workflow (`v*` tag → portable EXE + sha256 artifact).

## Manual test checkpoint (when controller exists)

After wiring the Tauri shell, run: launch app beside a writable dir,
create profile `Test Profile A` (locale `en-US`, timezone
`America/Los_Angeles`), restart the app, confirm persistence and that
`profiles/<uuid>/User Data` exists.
