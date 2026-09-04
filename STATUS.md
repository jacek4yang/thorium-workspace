# Implementation Status — v1.0.0 branch

Last updated: 2026-09-03. Branch: `feature/v1.0.0-implementation`.
This file is the hand-off contract: a new agent should be able to resume
accurately from this state.

## Toolchain used (verified)

- Windows 11 (10.0.26200), rustc/cargo 1.98.0 stable MSVC, Node 26, pnpm 11.9.
- Download engine: parallel segmented downloads (up to 8 ranges, retry +
  resume per segment) into a preallocated part file; no client-level
  total timeout (the 30s timeout was silently killing every >30s
  download); budgets unchanged; verified 2026-09-03 with local range
  server tests + live-through-proxy fallback test.
- Quality gates at last commit: `cargo fmt --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  (197 tests, 0 failures), frontend lint (0 errors)/typecheck/test
  (26 tests)/build.

## UI architecture (current)

Fluent-inspired, dense, Windows-desktop utility. Light/dark/system themes;
English / Simplified Chinese runtime languages.

Layout system:

- `.app` grid: 216px sidebar + main column. Sidebar: brand, grouped nav
  (Workspace / Support) with icons, accent-bar selected indicator,
  `aria-current`, tertiary Alt+1..7 badges, vault state chip at the bottom.
- Bounded content canvas: every page header (`.page-header-inner`) and every
  direct child of `.page-body` is capped at `--content-max` (1200px) and
  centered. Percentage-padding approaches were tried and rejected (they
  misresolve in some WebView/monitor-DPI configurations — see git history).
- Design tokens only (surface/text/border/accent/status scales, radii 5/8/10,
  spacing scale); custom thin scrollbars; no literal colors in pages.
- Components (`app/src/components/`): `Icon.tsx` (inline SVG set + brand
  mark), `ui.tsx` (Button variants, Card, Badge, Field, Notice/ErrorNotice
  with localized error codes, EmptyState, Dialog/ConfirmDialog, Progress,
  Stat, CodeRing, PageHeader, Disclosure, Loading, SettingSection/SettingRow),
  `Sidebar.tsx`.
- Settings reads vertically and semantically: uppercase group labels
  (General / Security / Browser & Downloads) above raised sections whose
  rows are title+description left, control right (`SettingRow` collapses to
  stacked under 860px). Explicit Save with dirty tracking; save toast;
  theme and language apply live through the shell.
- Dashboard: stat cards, real health checks, quick launch list, contextual
  action cards; Profiles/Accounts/Browser/Vault/Diagnostics all share the
  same layout system (details in git history; security semantics unchanged:
  secret reveal/copy disabled while vault locked, revealed material dropped
  immediately on lock).

## Internationalization

- `i18next` + `react-i18next` (the only new runtime dependencies).
- `app/src/i18n/`: `index.ts` (init, system detection, `applyLanguage`),
  `locales/en-US.ts` (canonical key tree, strictly typed via
  `typed.d.ts`), `locales/zh-CN.ts`, `i18n.test.ts`.
- Every user-facing string in shell + all seven pages + shared components
  is localized. Diagnostic codes, versions, paths, locale/timezone IDs,
  protocol names stay untranslated. `common.errors.*` maps known backend
  diagnostic codes to localized explanations; unknown codes fall back to
  the backend message (never raw keys in the UI).
- Language preference: `WorkspaceSettings.language` (`system`/`en-US`/
  `zh-CN`), added to the Rust domain model with `#[serde(default)]` (no
  migration; settings are a JSON row). `system` resolves from the WebView
  locale: zh/zh-CN/zh-SG/zh-Hans → zh-CN; zh-TW/zh-HK deliberately NOT
  (falls back to en-US); first recognized language wins.
- Switching happens in Settings → General → Language and applies
  immediately after the explicit Save (no restart); `document.lang` is
  updated; the shell re-renders through i18next.
- Completeness is enforced by tests: recursive key-tree parity between
  en-US and zh-CN (both directions) plus placeholder-consistency checks.

## Download proxy (owner-requested, previous pass)

`WorkspaceSettings.downloadProxy` (`scheme://host:port`) routes workspace
downloads only (Thorium discovery + install + the ip.sb probe); never
browser traffic. Settings → Browser & Downloads → Downloads has a "Test
connection" action probing ip.sb without saving. Verified live through the
real dev proxy (`THORIUM_TEST_PROXY=http://127.0.0.1:10808`, ignored live
tests passing). Proxy URLs never appear in errors or logs. reqwest gained
the `socks` feature.

## Committed, tested subsystems

| Crate | State | Notes |
|---|---|---|
| `domain` | done | accounts, profiles, factors, recovery codes, settings (`download_proxy`, `language`), `validate_proxy_url`, diagnostic codes |
| `secrets` | done | `SecretText`/`SecretBytes`: redacting Debug, no Serialize, zeroized, constant-time compare |
| `otp` | done | RFC 4226/6238 HOTP/TOTP SHA-1/256/512; `otpauth://` parser that never leaks rejected URIs |
| `storage` | done | SQLite (bundled), schema v1, WAL; settings as JSON row (new fields need only `#[serde(default)]`) |
| `vault` | done | Argon2id + ChaCha20-Poly1305; atomic save + `.bak`; create/unlock/lock/rotate |
| `windows-platform` | done | portable bootstrap, single-instance mutex, Job Objects, `CREATE_NO_WINDOW`, conditional clipboard clear |
| `browser-profile` | done | `LaunchSpec` allowlist, `ProfileLock`, supervised sessions |
| `qr` | done | rqrr decode; payloads never logged |
| `thorium` | done | rustls; bounded download; zip-slip-guarded staged install; atomic promote; delete protection; download-proxy support + ip.sb probe |
| `controller` | done | workspace bootstrap, vault lifecycle, services, clipboard scheduler, idle lock, diagnostics; proxy-aware release client |

Tauri command surface: 34 typed commands (`proxy_test` included),
`CmdError {code, message}` boundary, 1 s housekeeping thread.

## Manual Windows GUI verification actually performed (2026-09-03)

- App launched (`pnpm tauri dev`) against the dev workspace beside the
  debug exe; stable process, no runtime errors.
- Chinese UI verified on screen (persisted `language: "zh-CN"` written to
  the dev workspace DB): sidebar (概览/配置档案/账户/浏览器/密码库/设置/诊断,
  support group, accent selected state), dashboard (stat cards, health
  checks with ✓/⚠ icons, contextual cards) all render in Chinese.
- Settings page verified on screen in Chinese: 设置 header + subtitle,
  常规 group with 外观 (主题/语言 rows), 安全 group (密码库/剪贴板), semantic
  rows with title+description left / control right.
- Dark mode verified on screen in Chinese: neutral dark palette, dark
  sidebar/raised cards, accent selected item, bounded centered canvas.
- Layout-bug found and fixed during verification: the earlier
  percentage-padding content-bounding approach collapsed content into a
  narrow column in some window/monitor configurations; replaced with the
  bounded-children canvas (see git history, `fix(ui)`).
- Capture caveat: screenshots on this multi-monitor mixed-DPI machine have
  a PrintWindow offset artifact (scrollbar position proves the shift);
  window-position artifacts when the window straddles two DPI-different
  monitors are WebView2/OS-level and not reproducible in normal single-
  monitor use.
- NOT yet performed by hand: clicking "Test connection" in the running GUI
  (crate-level live test passes); a full two-profile launch with real
  Thorium; English-mode screenshots (English is covered by 26 frontend
  tests incl. nav/save/theme flows).

## Upstream facts (verified 2026-09-02 through proxy)

- Latest tag `M152.0.7977.55`; Windows portable assets:
  `Thorium_AVX2_152.0.7977.55.zip` (~350 MB), AVX/AVX512/SSE4/SSE3/WIN32_SSE2.
- Portable zip extracts with `BIN/thorium.exe`; no per-asset SHA-256
  digests upstream; integrity relies on TLS + zip CRC + structure checks.

## Release v1.0.0 (published 2026-09-04)

- GitHub Release: https://github.com/jacek4yang/thorium-workspace/releases/tag/v1.0.0
- Artifacts (all verified present): portable `ThoriumWorkspace-v1.0.0-x86_64.exe`,
  NSIS installer `ThoriumWorkspace-v1.0.0-x64-setup.exe`, each with a
  `.sha256`. CI build observed green (`Portable release build` 8m26s);
  PR #2 quality gates observed green (17m30s) before the tag.
- Installer verified locally end to end: silent install to
  `%LOCALAPPDATA%/ThoriumWorkspace` (no admin), launch, workspace
  bootstrap beside the installed exe, Start Menu shortcut, Chinese
  first-run onboarding, uninstall keeps user data.
- The published setup.exe was downloaded back and its SHA-256 matched the
  published checksum file (`faab30c1...`).
- Release notes are bilingual (zh-CN/en-US) with per-artifact guidance.
- Binaries are unsigned; SmartScreen warning is expected and documented.

## Remaining work for v1.1 (ordered)

1. Manual GUI checkpoint: "Test connection" click-through; profile
   persistence across restart; English-mode visual pass.
2. CDP timezone/locale emulation (loopback-only, ephemeral port).
3. Backup/recovery of metadata+vault.
4. Two-profile E2E with real Thorium (~350 MB asset download).
5. `DECISIONS.md` / `SECURITY.md` / `THREAT-MODEL.md` as standalone docs.
6. Release workflow exists (`ci-release.yml`, tag-triggered); first real
   `v1.0.0` tag build still to be observed.
