# Decisions

Each entry records what was decided, what it was weighed against, and what would
justify revisiting it.

---

## 1. The vault format is this project's own, not KDBX4

**Decision.** The vault is a versioned format built on audited primitives —
Argon2id for key derivation, XChaCha20-Poly1305 for authenticated encryption —
rather than KeePass KDBX4.

**Why KDBX4 was considered.** Interoperability is real value: a user could open
their vault in KeePassXC, and a widely reviewed format is one less thing to get
wrong.

**What was measured.** The `keepass` crate (0.13.22, the current release) was
spiked against the exact operations this product needs. It handled all of them:

```text
SAVE ok bytes=924
REOPEN ok name=Some("thorium-workspace") version=KDB4(1)
KDF after reopen: Argon2id { iterations: 4, memory: 67108864, parallelism: 2, version: Version13 }
ENTRY title=Some("acct:0001") pw=Some("hunter2")
WRONG-PASSWORD rejected: Incorrect key
CORRUPTION rejected: Block hash mismatch for block 1
HEADER-TAMPER rejected: Invalid outer header entry: 139
```

**Why it was still rejected for v1.0.0.**

- The crate's own README describes KDBX4 *writing* as experimental. This file is
  the only copy of the user's passwords; "experimental" is not an acceptable
  qualifier on the write path.
- It is pre-1.0 (0.13.x) with an API that changed shape recently, and its public
  API leaks a transitive crate's type (`rust-argon2`'s `Version`), so a consumer
  must pin that crate too and track its semver.

**Consequences.** No interoperability with KeePass in v1.0.0. The format is
documented in `SECURITY.md` and in the `tw-vault` crate docs so it is auditable,
and the header carries a version field so it can evolve.

**Revisit when** the `keepass` crate reaches 1.0 with non-experimental write
support, or if users ask for interoperability more than for anything else. A
KDBX4 *export* is the natural first step and needs no format change.

---

## 2. Thorium assets are selected by rule, never by name

**Decision.** A channel is an upstream repository plus a set of scoring rules.
Asset selection evaluates whatever a release actually contains: `.zip` only,
reject `installer`, `symbols`, `chromedriver`, `shell` and the other artefacts
that ship in the same release, prefer the channel's CPU token, reject the other
variants' tokens, and bound the plausible size.

**Why.** Upstream has renamed and re-homed its Windows assets more than once —
AVX2 builds moved to their own repository, SSE3 and 32-bit builds to another. A
hard-coded file name is a time bomb: it fails silently at the worst moment, or
worse, matches the wrong artefact.

**Consequences.** A rename upstream keeps working. A genuinely unmatchable
release produces `TW-0502` naming the assets that *were* published, so the user
can report what changed. Tests cover installer/driver/symbol rejection, CPU
variant disambiguation, and a simulated rename.

---

## 3. `current` is a text file, not a junction or a symlink

**Decision.** The selected Thorium version is recorded in `current.txt`.

**Why not a directory junction.** Creating one can require elevation depending
on configuration, and a dangling junction is a far worse failure than a stale
line of text — it looks like a valid path right up until the browser fails to
start.

**Consequences.** Switching versions is an atomic write (temp file, then
rename), so a crash mid-switch leaves the previous version selected rather than
none. A marker naming a version that no longer exists reads as "nothing
selected" rather than as a broken path, and the value is sanitized so a hostile
marker cannot escape the versions directory.

---

## 4. Timezone and locale go through DevTools, not flags

**Decision.** `Emulation.setTimezoneOverride` and `Emulation.setLocaleOverride`
over a loopback DevTools connection, with `--lang` and `TZ` as best-effort
hints.

**Why.** `--lang` sets Chromium's *UI* language and `TZ` is honoured by ICU on
some platforms and ignored on others. Neither reliably changes what a page
observes through `Intl.DateTimeFormat().resolvedOptions().timeZone` or
`navigator.language`. The DevTools commands do, and they are the supported
mechanism.

**How the endpoint is kept safe.** The port is ephemeral
(`--remote-debugging-port=0`), Chromium binds it to loopback, and
`--remote-debugging-address` is deliberately never passed — overriding it is the
usual way DevTools ends up on the LAN. The port is read from the profile's own
`DevToolsActivePort` file, and the connection closes with the profile.

**One subtlety that matters.** Both commands have been observed to crash the
renderer when sent to a *worker* target, because they touch main-thread-only
controllers. Only `page` and `iframe` targets are addressed; every other target
is resumed without being emulated. A test pins that allow-list.

**Known limit.** Auto-attach with `waitForDebuggerOnStart` means a new tab is
paused, configured, and only then released, so it never briefly observes the
host timezone. Surfaces outside a page's JavaScript environment are not covered;
`SECURITY.md` says which.

---

## 5. rustls with the `ring` provider, not aws-lc-rs

**Decision.** TLS is rustls (no OpenSSL runtime dependency, as required), with
the crypto provider pinned to `ring` rather than reqwest's `aws-lc-rs` default.

**Why.** `aws-lc-sys` compiles C and, on Windows, wants NASM for its assembly.
`ring` ships pre-generated object files for `x86_64-pc-windows-msvc`, so a
Windows build needs nothing beyond MSVC itself. For a product that is built only
on Windows CI, the less build-time toolchain surface the better.

**Consequences.** The provider is installed explicitly at startup rather than
picked up transitively, which is also clearer. Native certificate roots are
preferred so a corporate TLS-inspecting proxy keeps working, with the bundled
Mozilla roots as a fallback.

---

## 6. Screen QR capture scans the whole screen; there is no drag-to-select

**Decision.** "Scan the screen" captures the entire virtual desktop and looks
for a QR code anywhere in it.

**Why not a drag-to-select overlay.** A transparent, always-on-top, click-through
selection window is a large amount of fragile Win32 for a small gain: it has to
get DPI, multi-monitor layout, focus stealing and capture protection right, and
a mis-drag produces a confusing failure. Scanning everything has none of those
failure modes and is a smaller thing to get right.

**Consequences.** The user does not have to aim. The captured pixels never leave
the process, and `ScreenCapture`'s `Debug` prints only its dimensions, because a
screen capture can contain anything that was on screen.

---

## 7. The clipboard is cleared conditionally, never unconditionally

**Decision.** A copied secret is cleared only if the clipboard still contains
byte-for-byte what this application wrote.

**Why.** An unconditional timed clear is a data-loss bug wearing a security
feature's clothes: the user copies a paragraph they are working on, and thirty
seconds later it is gone. Three cases all mean "leave it alone": the content
changed, the clipboard cannot be read, or a newer copy superseded this one.

**Consequences.** A secret can outlive its timer if the clipboard becomes
unreadable. That is the right trade: the alternative destroys content this
program does not own.

---

## 8. Backups are logical and exclude browser data

**Decision.** A backup contains the metadata database, the still-encrypted vault
and a manifest. It does not contain `User Data`.

**Why.** Browser profile data is gigabytes of recreatable cache and history, and
copying a *running* Chromium profile produces a torn snapshot that looks valid
and is not. Including it would make backups too large to take routinely and
would give false confidence when they were taken at the wrong moment.

**Consequences.** Restoring a backup restores accounts, profiles and secrets;
browser sessions have to be signed into again. The database is snapshotted
through SQLite's online backup API rather than copied as a file, so it is
consistent even with the WAL active, and a restore always keeps a copy of what
it replaced.

---

## 9. Job Objects use `KILL_ON_JOB_CLOSE`

**Decision.** The browser process tree is held in a Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.

**Why.** Chromium is a process tree. Killing only the process we launched leaves
renderers, the GPU process and the network service behind, and a crashed manager
leaves the whole tree orphaned against a `User Data` directory it still holds
locks on.

**The consequence, stated plainly.** The browser cannot outlive the manager: if
this process dies, the kernel closes its handles and terminates the tree. That
is intended — an orphaned browser holding a profile directory is worse than a
browser that closes with its manager — and it is documented on the type rather
than left to be discovered.

---

## 10. The frontend gets no capability it does not need

**Decision.** The Tauri capability set allows exactly: a file picker, a message
dialog, revealing a file in Explorer, and opening an `https://` URL.

**Why.** The frontend is presentation. It has no reason to read or write files,
run a process, or make a network request, and every capability granted is one
that a cross-site scripting bug in a rendered account note could reach.

**Consequences.** Anything the UI needs goes through a typed Rust command that
validates its input. Adding a capability is a visible, reviewable change to one
file.
