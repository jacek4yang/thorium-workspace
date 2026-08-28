# Architecture

## What this program is

A portable Windows desktop utility that keeps several independent Thorium
browser profiles, the accounts that belong to them, and the secrets those
accounts need, in one folder that the user can move, copy or delete as a unit.

Everything follows from two decisions:

1. **Data lives beside the executable.** There is no installer, no registry key
   and no `%APPDATA%` fallback. Moving the folder moves the whole workspace.
2. **One browser profile owns one `User Data` directory.** That is the isolation
   boundary. Chromium does not defend it, so this program does.

## Layers

```text
              React frontend (app/)
                       │  presentation and state orchestration only
                       ▼
        Typed Tauri commands (src-tauri/)
                       │  one Workspace behind an async mutex
                       ▼
              tw-controller
        ┌──────────┬───┴────┬───────────┬──────────────┐
        ▼          ▼        ▼           ▼              ▼
   tw-storage  tw-vault  tw-otp    tw-thorium  tw-browser-profile
        │          │      tw-qr        │              │
        └──────────┴────────┬──────────┴──────────────┘
                            ▼
                        tw-domain
                            │
                            ▼
                   tw-windows-platform
                (the only crate with unsafe code)
```

Dependencies point one way. `tw-domain` depends on nothing in the workspace and
on no Tauri, React, Win32, SQLite or HTTP crate, so the rules it encodes can be
tested on any machine.

## The crates

| Crate | Owns |
| --- | --- |
| `tw-domain` | Entities, value objects, validation, 50 stable diagnostic codes. No I/O. |
| `tw-secrets` | `SecretString` / `SecretBytes`: redact on format, zeroize on drop, compare in constant time. |
| `tw-storage` | SQLite metadata and versioned migrations. Holds no secret, only references to them. |
| `tw-vault` | The encrypted vault: format, key derivation, atomic save, session and locking. |
| `tw-otp` | RFC 4226 / RFC 6238 code generation and `otpauth://` URIs. |
| `tw-qr` | QR decoding from files, encoded bytes and raw pixels, with decode limits. |
| `tw-thorium` | Upstream release discovery, bounded download, validated extraction, atomic promotion. |
| `tw-browser-profile` | Per-profile locking, launch, DevTools timezone/locale, session lifetime. |
| `tw-windows-platform` | Named mutex, Job Objects, console-free spawn, window activation, screen capture. |
| `tw-controller` | Bootstrap, application services, clipboard guard, backup, diagnostics. |
| `thorium-workspace` | The Tauri boundary: commands, events, background tasks. |

A `test-support` crate was sketched in the original plan and is deliberately not
here: the shared fixtures each crate actually needed turned out to be local to
it, and an empty crate carried in the workspace is scaffolding, not structure.

## Where the data is

```text
ThoriumWorkspace/
├── ThoriumWorkspace.exe
├── workspace.db                     metadata; contains no secret values
├── vault/workspace.twvault          the encrypted vault
├── browsers/thorium/
│   ├── versions/<version>/          one directory per installed build
│   ├── staging/                     in-progress installs; cleared at startup
│   └── current.txt                  which version is selected
├── profiles/<profile-id>/
│   ├── thorium-workspace.lock       held while the profile is running
│   └── User Data/                   Chromium's own directory for this profile
├── runtime/                         transient; cleared at startup
├── backups/                         logical backups
└── logs/                            rolling log files
```

The `<profile-id>` directory name is the profile's immutable UUID, never its
name, so renaming a profile can never move, merge or orphan browser state.

## Startup

1. Resolve the executable's directory.
2. Prove it is writable by writing a probe file. A read-only *attribute* check
   misses ACLs, read-only media and virtualised install locations.
3. Take a named mutex derived from a hash of the normalized path. Two managers
   sharing one database, vault and profile set is the failure this prevents.
4. Create the directory layout.
5. Open the database and run migrations.
6. Clear the runtime directory and the Thorium staging directory. Nothing else
   is ever cleaned implicitly.
7. Reconcile observed runtime state: every row in the sessions table describes a
   process supervised by a manager that is no longer running, so all are
   cleared, and the installed-version table is reconciled against the
   filesystem.

If any of the first three steps fails the window still opens and shows the error
with its remedy, because "this folder is read-only" and "another copy is already
running" are things a user has to be told how to fix.

## Secrets

The metadata database stores a `SecretRef` — a UUID — wherever a password, an
OTP seed or a recovery code belongs. The value itself only exists inside the
encrypted vault and, while unlocked, inside one `VaultSession` in memory.

A secret reaches the user in exactly two ways, both explicit:

- **Reveal**: a command that returns plaintext, called when the user presses
  "show".
- **Copy**: the value goes from the vault to the clipboard inside the backend.
  It is never serialized to the frontend at all.

Everything else — `Debug`, `Display`, `Serialize`, log lines, error messages,
diagnostics — renders `[redacted]` by construction rather than by filtering.

## Runtime state is observed, never authoritative

A browser session's status, process id and DevTools port are a cache of what was
true when the manager last looked. They are reconstructed at startup from the
real process table and the on-disk locks. No user configuration is ever stored
only in that cache.

## The extension point that is deliberately empty

`browser_profiles.network_route_id` exists in the schema and in the domain type,
and v1.0.0 always writes `NULL`. It is the one concession to a future release
that adds network routing: adding that will not need a migration. There is no
other speculative machinery for it, and a test asserts the column stays empty.

## Concurrency

- One process per workspace folder (the named mutex).
- One browser process per profile (the per-profile file lock).
- One `Workspace` per process, behind an async mutex, because a SQLite
  connection is `Send` but not `Sync`.
- Background ticks use `try_lock` and skip a turn rather than queueing behind a
  long-running install.

## Unsafe code

`tw-windows-platform` is the only crate permitted to use `unsafe`, and every
other crate that can is `#![forbid(unsafe_code)]`. Each unsafe block states why
it is necessary, what it assumes about the pointers it passes, who owns each
handle and when it is released. Raw handles are never exposed: each is owned by
a wrapper whose `Drop` releases it exactly once.

## Testing

- Pure rules (`tw-domain`, `tw-otp`, `tw-secrets`) are tested directly, including
  the published RFC test vectors.
- The vault, storage, Thorium and browser-profile crates are tested against real
  files in temporary directories rather than mocks.
- Thorium installation is driven end to end against a local fixture HTTP server
  serving a synthetic archive: no external network, no real browser.
- Profile isolation is driven against a stand-in browser that behaves like the
  parts of Chromium the code depends on.
- Windows Job Object cleanup is covered by tests that run on Windows CI.
