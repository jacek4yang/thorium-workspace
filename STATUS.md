# Implementation Status — v1.0.0 branch

Last updated: 2026-09-03. Branch: `feature/v1.0.0-implementation`.
This file is the hand-off contract: a new agent should be able to resume
accurately from this state.

## Toolchain used (verified)

- Windows 11 (10.0.26200), rustc/cargo 1.98.0 stable MSVC, Node 26, pnpm 11.9.
- Quality gates at last commit: `cargo fmt --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  (197 tests, 0 failures), frontend lint/typecheck/test (12 tests)/build.
- Dev proxy used for all upstream access: `http://127.0.0.1:10808`
  (HTTP and SOCKS5H both verified). Never leak into committed code.

## UI architecture (this pass)

The frontend was rebuilt as a coherent desktop application (Fluent-inspired,
high-density, light/dark/system):

- `app/src/styles.css` — semantic design tokens only (surface/text/border/
  accent/status scales, radii, spacing); dark theme via `data-theme` attr +
  `prefers-color-scheme` fallback; custom thin scrollbars; no literal colors
  in page components.
- `app/src/components/Icon.tsx` — inline SVG icon set (no icon dependency).
- `app/src/components/ui.tsx` — Button (default/primary/ghost/danger),
  Card, Badge, Field, Notice/ErrorNotice, EmptyState, Dialog/ConfirmDialog,
  Progress, Stat, CodeRing, PageHeader, Disclosure, Loading.
- `app/src/components/Sidebar.tsx` — brand, grouped nav with icons,
  `aria-current`, visible Alt+1..7 hints; vault state chip at the bottom
  (one click = lock, or jump to unlock).
- `app/src/App.tsx` — the shell: owns navigation, polls vault lock state
  (3 s + window focus), owns settings/theme, toast queue, first-run routing
  to the Vault page. Pages receive only what they need.
- Pages redesigned: Dashboard (stat cards + real health checks + quick
  launch), Profiles (first-class cards, create/edit dialog incl. startup
  URLs and pinned-version picker fed by installed versions, delete
  confirmation), Accounts (structured cards, edit dialog using the
  previously-unused `account_update`, tags/login URL, collapsible secret
  sections, live TOTP with countdown ring — HOTP deliberately never
  auto-refreshes — external authenticator references, immediate removal of
  all revealed secret state on vault lock), Browser (current-version card,
  versions table, staged install display driven by real progress events),
  Vault (three distinct lifecycle states, change-master-password dialog),
  Settings (grouped cards: Appearance/Vault security/Clipboard/Browser/
  Downloads, responsive `grid-form` layout filling the window), Diagnostics
  (grouped cards, copy-as-report).
- Responsive: no page-level max-width; card grids reflow on resize; custom
  scrollbars; verified at ~1920 px and while narrowed.

## Download proxy (owner-requested, this pass)

`WorkspaceSettings.downloadProxy` (`scheme://host:port`, optional
credentials) routes **workspace downloads only** (Thorium discovery +
install + the ip.sb probe). Never browser traffic. Details in
`docs/ARCHITECTURE.md` ("Download proxy"). Settings page has a "Test
connection" action (`proxy_test` command) that fetches the public exit IP
from ip.sb through the candidate routing without saving.

- The ip.sb probe was verified live through the real dev proxy
  (`THORIUM_TEST_PROXY=http://127.0.0.1:10808`, both ignored live tests
  passing); discovery through the proxy verified in the same run.
- Proxy URLs are never embedded in errors (`THORIUM_PROXY_CONFIG`,
  `THORIUM_PROBE_FAILED`); `Client::new_with_proxy` disables ambient env
  proxies so the configured endpoint is used exactly.
- reqwest gained the `socks` feature (socks5/socks5h support).

## Committed, tested subsystems

| Crate | State | Notes |
|---|---|---|
| `domain` | done | accounts, profiles, factors, recovery codes, settings (incl. `download_proxy` + `validate_proxy_url`), validation, diagnostic codes |
| `secrets` | done | `SecretText`/`SecretBytes`: redacting Debug, no Serialize, zeroized, constant-time compare |
| `otp` | done | RFC 4226/6238 HOTP/TOTP SHA-1/256/512, 6/8 digits; `otpauth://` parser that never leaks rejected URIs |
| `storage` | done | SQLite (rusqlite bundled), schema v1, WAL, FK on; settings stored as a JSON row — new settings fields need only `#[serde(default)]`, no migration; profiles/accounts/factors/recovery-codes/settings/installs/runtime-meta repos; atomic account writes |
| `vault` | done | Argon2id (64 MiB, t=3, p=1) + ChaCha20-Poly1305; header as AAD; atomic save + `.bak`; create/unlock/lock/rotate |
| `windows-platform` | done | portable bootstrap, single-instance mutex, Job Objects, `CREATE_NO_WINDOW` spawn, conditional clipboard clear |
| `browser-profile` | done | `LaunchSpec` (explicit `--user-data-dir`, allowlist), `ProfileLock`, supervised sessions with shutdown+reap |
| `qr` | done | rqrr decode from PNG/JPEG; payloads never logged |
| `thorium` | done | live catalog verified 2026-09-02; rustls; bounded download; zip-slip-guarded staged extract; atomic promote; `current` marker; delete protection; **download proxy support** (`proxy.rs`: `new_with_proxy`, `fetch_exit_ip` via ip.sb) |
| `controller` | done | workspace bootstrap, vault lifecycle, profile/account services, clipboard scheduler, idle lock, diagnostics; `release_client` routes downloads through `download_proxy`; `test_download_proxy` |

All seven UI sections are functional and share the design system. Tauri
command surface: 34 typed commands (`proxy_test` added), `CmdError`
{code,message} boundary, 1 s housekeeping thread, window-focus activity
recording.

## Manual verification actually performed (2026-09-03, `pnpm tauri dev`)

- App launches against the existing portable workspace beside the debug
  exe; process stable, no runtime errors in the dev log.
- Shell renders: brand, icon sidebar with Alt+ hints, vault chip; Alt+6
  navigates to Settings (verified on screen); Settings groups render in
  the responsive two-column grid with the Downloads proxy card and Test
  button visible (screenshot-verified); custom scrollbar renders at the
  window edge (screenshot-verified).
- Live backend proxy path verified at crate level through the real proxy
  (exit IP from ip.sb; discovery of real upstream releases).
- NOT yet performed by hand: clicking "Test connection" in the running GUI
  (UI automation lost window focus repeatedly); full two-profile launch
  with real Thorium.

## Upstream facts (verified 2026-09-02 through proxy)

- Latest tag `M152.0.7977.55`; Windows portable assets:
  `Thorium_AVX2_152.0.7977.55.zip` (~350 MB), AVX/AVX512/SSE4/SSE3/WIN32_SSE2.
- Portable zip extracts with `BIN/thorium.exe` (installer top-level dir).
- No upstream SHA-256 digests published per asset; integrity relies on
  TLS + zip CRC + structure validation (documented in SECURITY notes).

## Remaining work for v1.0.0 (ordered)

1. Manual GUI checkpoint: profile persistence across restart; "Test
   connection" click-through in the running GUI.
2. CDP timezone/locale emulation (loopback-only, ephemeral port,
   DevToolsActivePort handshake) — the supported mechanism per contract.
3. Backup/recovery of metadata+vault.
4. Two-profile E2E with real Thorium (~350 MB asset download).
5. `DECISIONS.md` / `SECURITY.md` / `THREAT-MODEL.md` as standalone docs
   (currently only `docs/ARCHITECTURE.md` exists).
6. Release workflow already exists (`ci-release.yml`, tag-triggered);
   first real `v1.0.0` tag build still to be observed.
