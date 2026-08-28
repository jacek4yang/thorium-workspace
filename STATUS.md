# Status

**Version**: 1.0.0 · **Target**: Windows 10 22H2 / Windows 11, x86-64

This file records what is actually built and verified, and what is not. It is
maintained against observed evidence, not intent.

---

## Verified

Everything below was run and its output observed on the development host
(Ubuntu 24.04, Rust 1.94.1, Node 22) at the commit this file was last updated.

```text
cargo fmt --all -- --check                                 clean
cargo clippy --workspace --all-targets --locked -- -D warnings   clean
cargo test --workspace --locked                            413 passed, 0 failed
npm ci && npm run lint                                     clean
npm run typecheck                                          clean
npm test                                                   14 passed
npm run build                                              dist built, 273 KB JS / 14 KB CSS
```

Tests by crate:

| Crate | Tests | What they cover |
| --- | ---: | --- |
| `tw-domain` | 48 | Validation, IDs, Thorium asset selection, diagnostic-code uniqueness |
| `tw-secrets` | 11 | Redaction through `Debug`/`Display`/`Serialize`, constant-time compare |
| `tw-storage` | 69 | Migrations, every repository, cascade and uniqueness rules, corrupt-row tolerance |
| `tw-vault` | 47 | Round trip, wrong password, ciphertext and header tampering, truncation, re-key, orphan collection |
| `tw-otp` | 28 | RFC 4226 appendix D and RFC 6238 appendix B vectors, `otpauth://` parsing |
| `tw-qr` | 11 | Real QR images built from synthetic credentials, decode limits, payload discretion |
| `tw-thorium` | 51 | Release selection, bounded download, archive validation, install/rollback end to end |
| `tw-browser-profile` | 36 | Locking, launch plan, CDP parsing, profile isolation against a stand-in browser |
| `tw-windows-platform` | 19 | Instance naming and exclusion, process spawn and liveness |
| `tw-controller` | 90 | Bootstrap, clipboard rule, account lifecycle, backup, diagnostics redaction |
| `thorium-workspace` | 3 | Command-boundary state handling |

The Windows code paths (`tw-windows-platform`, `tw-browser-profile`) additionally
type-check and lint clean against `x86_64-pc-windows-msvc`:

```text
cargo clippy -p tw-windows-platform -p tw-browser-profile --all-targets \
  --target x86_64-pc-windows-msvc -- -D warnings          clean
```

---

## Verified on Windows CI

Run [33155370138](https://github.com/jacek4yang/thorium-workspace/actions/runs/33155370138)
on `windows-latest` at commit `58b2e2d` completed **successfully**, every step:

```text
cargo fmt --all -- --check                                 pass
cargo clippy --workspace --all-targets --locked -D warnings pass
npm run lint / typecheck / test                            pass
cargo test --workspace --locked                            pass
npm run build                                              pass
cargo build --release --locked -p thorium-workspace        pass (9m 55s)
Built target/release/thorium-workspace.exe (18.5 MB)       PE smoke test pass
```

That run was the first time the release build and the smoke test ever executed.
The Job Object tests, the named-mutex guard, the GDI capture and the window
activation paths all built and ran on Windows as part of it.

The Ubuntu "Platform-independent crates" job passed in the same run.

Two fixes landed after that run and are covered by the next one: the vault
plaintext-buffer scrub (`107cdb0`) and clearing revealed secrets on vault lock
(`3a0f68c`).

---

## Windows behaviour still needing a desktop

CI proves these compile, link and pass their tests on Windows. It cannot prove
they behave correctly in front of a user, because the runner has no desktop
session and no real browser:

| Item | What CI proved | What it could not |
| --- | --- | --- |
| Job Object process-tree cleanup | `tests/job_cleanup.rs` passed: a grandchild process is terminated with the job | Nothing further — this one is genuinely covered |
| Named-mutex single-instance guard | Builds and its unit tests pass | Two real GUI instances racing for the same workspace |
| GDI screen capture for QR scanning | Builds and links | Capturing an actual screen; the runner has no desktop session |
| Window activation for an already-running profile | Builds and links | Focusing a real browser window |
| The portable `.exe` | Links, is a valid 18.5 MB PE, frontend embedded | Opening a window; a Tauri binary cannot run headlessly |

CI is the first place the Windows paths ran — and the first run found a real
Windows-only bug, recorded below. The second run, after the fix, was green.

### What the first Windows CI run found

Four `ProfileLock` tests failed on Windows and passed on Linux. Windows file
locks are *mandatory*: while a range is locked, no other handle can read it, not
even one in the same process. The holder record was stored inside the locked
file, so it was unreadable exactly when it mattered — while something held the
lock — and the UI could never have told a user which process was using a
profile.

The record now lives in a separate file beside the lock, which stays empty.
Three tests were added to pin the property, including one that asserts the lock
file is empty and the record is readable while the lock is held.

## Not yet verified anywhere

| Item | Why |
| --- | --- |
| A real Thorium download and launch | Requires network access to the upstream release and a Windows desktop. The pipeline is covered end to end against a local fixture server with a synthetic archive, which exercises every step except the real bytes. |
| DevTools timezone/locale against a real Chromium | Requires a real browser. The command shapes, parameter names and target allow-list are unit tested; the wire interaction is not. |
| Clipboard against the real Windows clipboard | The conditional-clear rule is tested exhaustively against an in-memory backend; `arboard` is the untested edge. |

---

## Definition of Done: where each step stands

| # | Step | Status |
| --: | --- | --- |
| 1 | Download one portable EXE from a GitHub Release | Workflow written; runs on tag |
| 2 | Put it in a writable folder and run it | 18.5 MB EXE built and smoke-tested in CI; first run needs a Windows desktop |
| 3 | Workspace initializes beside the EXE | Implemented; 11 bootstrap tests |
| 4 | Create/unlock the encrypted Vault | Implemented; 47 vault tests |
| 5 | Install portable Thorium from the GUI | Implemented; 4 end-to-end tests against a fixture server. Not yet run against the real upstream |
| 6 | Create two independent Browser Profiles | Implemented; isolation proven by test |
| 7 | Add several account records to each Profile | Implemented |
| 8 | Store and retrieve account passwords securely | Implemented; tests assert no plaintext in the database or the vault file |
| 9 | Import a standard TOTP QR and produce RFC-correct codes | Implemented; RFC vectors and real QR images tested |
| 10 | Store and mark recovery codes used | Implemented |
| 11 | Launch both profiles with independent User Data | Implemented; proven against a stand-in browser. Not yet with real Thorium |
| 12 | Confirm timezone/locale behaviour | Implemented; not yet observed against a real Chromium |
| 13 | Stop/restart without losing state | Implemented; covered by a restart test |
| 14 | Browser processes clean up correctly | **Observed**; Job Object tests passed on Windows CI |
| 15 | Diagnostics expose no secrets | Implemented; tested with secret canaries |
| 16 | CI green | **Observed green** on `windows-latest`, run 33155370138 |
| 17 | Tag build produces the EXE and SHA-256 | Workflow written; runs on tag |

**v1.0.0 is not complete** until 5, 11 and 12 have been observed on a Windows
desktop. Step 16 is now observed.
Steps 1, 2 and 17 depend on a tag, which is deliberately not created before
review.

---

## Known limitations

### Functional

- **Restore is manual.** Backups are created from the UI and their contents are
  restorable, but restoring is done by replacing two files with the application
  closed, and the UI explains how. An in-app restore would have to close the
  database and vault mid-session; that is worth doing carefully rather than
  quickly.
- **Screen QR capture scans the whole screen** rather than offering a
  drag-to-select region. See `DECISIONS.md` §6.
- **No KDBX interoperability.** See `DECISIONS.md` §1.
- **No HOTP resynchronisation UI.** The counter advances on each generation and
  the look-ahead verification exists in `tw-otp`, but nothing in the UI exposes
  "this code was rejected, try the next few".
- **Profile account associations are informational.** Attaching accounts to a
  profile groups them; it does not auto-fill anything in the browser.

### Platform

- **Metadata is not encrypted.** Deliberate, documented in `THREAT-MODEL.md`.
- **No signature verification on Thorium downloads.** Upstream publishes none
  that can be verified against an independent root. The digest of what was
  received is recorded and the release page is linked. See `THREAT-MODEL.md` A4.
- **The browser cannot outlive the manager.** A consequence of
  `KILL_ON_JOB_CLOSE`, chosen deliberately. See `DECISIONS.md` §9.

### Scope, by design

No proxy, no Xray, no subscriptions, no routing, no WFP kill-switch, no
fingerprint randomization, no CAPTCHA handling, no account-registration
automation. None of these are partially implemented; there is no dormant code
for them. The single exception is one nullable database column
(`browser_profiles.network_route_id`) that exists so adding routing later needs
no migration, and a test asserts v1.0.0 never writes to it.

---

## Size

| | |
| --- | ---: |
| Rust | 22,192 lines across 11 crates |
| TypeScript / TSX | 4,883 lines |
| `unsafe` blocks | 28 blocks + 4 `unsafe impl` + 1 `unsafe extern fn`, all in `tw-windows-platform`, each documented |
| Diagnostic codes | 50 |
| Tests | 413 Rust, 14 frontend |
