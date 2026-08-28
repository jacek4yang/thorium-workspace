# Security

## Reporting a vulnerability

Open a private security advisory on the repository. Please do not open a public
issue for anything exploitable.

Include what you did, what happened, and what you expected. A diagnostic code
(`TW-nnnn`) helps if one was shown. Please do not include real credentials in a
report; a synthetic reproduction is always enough.

---

## The vault

### Format

A vault file is a fixed 64-byte cleartext header followed by one AEAD
ciphertext:

```text
offset size field
0      8    magic  b"TWVAULT1"
8      2    format version, u16 little-endian
10     1    KDF identifier      (1 = Argon2id, version 0x13)
11     1    cipher identifier   (1 = XChaCha20-Poly1305)
12     4    Argon2 memory cost in KiB,  u32 little-endian
16     4    Argon2 time cost (passes),  u32 little-endian
20     4    Argon2 parallelism,         u32 little-endian
24     16   Argon2 salt
40     24   XChaCha20 nonce
64     ...  ciphertext || 16-byte Poly1305 tag
```

The plaintext is a JSON document mapping each secret reference to its value and
kind. JSON is deliberate: the payload is small, and a format a person can read
after decrypting is worth more during a recovery than a few saved bytes.

### Why the header is authenticated

All 64 header bytes are passed as AEAD associated data. Editing the salt, the
nonce, the version or the declared KDF cost therefore fails authentication
rather than being honoured. Without that, an attacker holding the file could
rewrite the cost parameters down and hand it back.

Cost parameters are *also* range-checked before Argon2 is asked to allocate:
below 8 MiB or above 2 GiB, or outside 1–64 passes, the file is rejected. A
hostile header must not be able to make the application allocate until the
machine dies, and the check happens before the allocation, not after.

### Key derivation

Argon2id, version 0x13. Defaults: 64 MiB memory, 3 passes, 1 lane. The
parameters are stored per-vault, so they can be raised in a later release
without breaking existing files.

### Master password

Never stored, never written anywhere, never logged. A minimum of 12 characters
is enforced; no composition rules are imposed, because they push people toward
predictable substitutions. There is no recovery mechanism: losing the password
loses the vault, and the UI says so before the vault is created, behind an
explicit acknowledgement.

### Writes

Every mutation writes through immediately — full contents to a sibling temporary
file, flushed, fsynced, then renamed over the target. A crash leaves either the
previous complete vault or the new one, never a half-written file. Each save
uses a fresh nonce; reusing one under the same key would destroy the cipher's
confidentiality guarantee.

Changing the master password copies the current file aside first. That is the
one operation where a crash could otherwise leave a user with a file whose
password they do not know.

### Locking

The derived key and the decrypted document exist in exactly one place: the
`VaultSession`. Locking drops both, and the zeroizing wrappers scrub them.
Locking happens on: an explicit request, an idle timeout, minimising the window
when that setting is on, and shutdown.

---

## Secret handling

Every secret is wrapped in `SecretString` or `SecretBytes`, which render
`[redacted]` through `Debug`, `Display` and `Serialize`, zeroize on drop, and
compare in constant time. This is enforced by construction, not by filtering
output: there is no code path that formats an exposed secret into a log macro.

The metadata database stores only a `SecretRef` (a UUID) wherever a secret
belongs. Tests assert that the account, factor and recovery-code tables contain
no column that could hold one, and that a saved vault file contains no plaintext.

A secret reaches the user in two ways, both explicit:

- **Reveal** — a command returning plaintext, invoked when the user presses
  "show".
- **Copy** — the value goes from the vault to the clipboard inside the backend.
  It is never serialized across the Tauri boundary.

### What is deliberately not treated as a secret

A **live OTP code** is carried as plain text: it is valid for seconds, the user
is about to read it off the screen, and treating it as a long-lived secret would
be theatre. It is never logged or persisted.

**Issuer and account labels** from an `otpauth://` URI are not secrets — they are
the human-readable part the user needs to tell factors apart. The shared secret
in the same URI is.

---

## Zeroization, honestly

Zeroization is best effort and cannot be otherwise in a garbage-collected
webview process talking to a Rust backend:

- The operating system may have paged a copy to the swap file before the value
  was scrubbed. Nothing at this layer can prevent that.
- A `String` that grew while being built may have left a copy at an old
  allocation.
- A revealed secret rendered in the UI lives in the WebView's heap, where this
  program has no control over its lifetime.

What is guaranteed: the derived key and the decrypted vault document are
scrubbed when the session drops, and secret wrappers scrub on drop.

---

## Clipboard

A copied secret is cleared only if the clipboard still contains byte-for-byte
what this application wrote. If the content changed, the clipboard cannot be
read, or a newer copy superseded this one, it is left alone.

This is a deliberate trade: a secret can outlive its timer if the clipboard
becomes unreadable, and that is better than deleting a paragraph the user was
working on. Nine tests cover the rule.

Anything on the Windows clipboard is readable by any process running as the same
user. The timeout bounds exposure; it does not eliminate it.

---

## Untrusted input

Everything from outside is treated as hostile:

| Input | Handling |
| --- | --- |
| Upstream release metadata | Asset chosen by rule, sizes bounded, tag sanitized before it becomes a path component. |
| Downloaded archive | Total size cap, stall timeout, wall-clock limit; SHA-256 computed from the stream; partial files removed. |
| ZIP entries | Absolute paths, drive letters, `..`, symlinks and reserved Windows device names all rejected; entry count and uncompressed total bounded. |
| QR images | Decode limits on dimensions and allocation; file size capped before decoding. |
| `otpauth://` URIs | Fully parsed and validated; no error quotes the input, because the input contains the secret. |
| DevTools messages | Only `Target.attachedToTarget` is acted on; only `page` and `iframe` targets are emulated. |
| Stored database rows | A corrupt startup-URL list, locale or timezone falls back to a default rather than making a profile unopenable. |
| Vault header | Magic, version, algorithm identifiers and cost parameters all validated before use. |

### QR payloads specifically

A two-factor QR code *is* the shared secret. Nothing logs, prints or returns a
decoded payload except as the parsed credential. A payload that turns out not to
be an `otpauth://` URI is discarded without being reported back, because the
user may have scanned something else sensitive by accident.

---

## The DevTools control channel

Used only to apply a profile's timezone and locale.

- Requested only when a profile actually overrides one of them.
- The port is ephemeral (`--remote-debugging-port=0`) and chosen by Chromium.
- Chromium binds it to loopback. `--remote-debugging-address` is never passed —
  overriding it is the usual way DevTools ends up reachable on the LAN.
- The port is read from the profile's own `DevToolsActivePort` file, so one
  profile cannot pick up another's channel. A file left by a previous run is
  removed before launch.
- The connection closes when the profile stops.

**The residual risk, stated plainly**: while a profile with overrides is running,
any process on the machine running as the same user can connect to that loopback
port and drive the browser. That is inherent to using DevTools at all. It is why
the channel is opened only when needed and closed with the profile.

### Where overrides cannot be guaranteed

The DevTools overrides cover a page's JavaScript environment: `Date`,
`Intl.DateTimeFormat`, `navigator.language` and `navigator.languages`, and the
`Accept-Language` header. They do **not** cover:

- Chromium's own UI chrome beyond what `--lang` sets;
- times rendered by the browser itself, such as in the download shelf or
  `chrome://` pages;
- extensions, which run in their own contexts;
- anything a site infers from IP address, which this release does not touch at
  all.

A profile's timezone and locale are a configuration convenience, not an anonymity
mechanism, and the product does not claim otherwise.

---

## What this release deliberately does not do

- No proxying, no network routing, no IP rotation.
- No fingerprint randomization. Canvas, WebGL, GPU, audio and hardware
  properties are left exactly as Chromium reports them. A test asserts no
  user-agent, proxy or fingerprint flag is ever added to the command line.
- No CAPTCHA handling, no automated account registration, no automated posting.
- No emulation of vendor push approval, number matching or passwordless sign-in.
  Those are not one-time passwords; they are recorded as external factors so the
  user knows what protects an account, and no code is produced for them.
- No telemetry. The only network requests are to the GitHub Releases API and the
  release asset URL, and only when the user asks for them.

---

## Diagnostics

The diagnostic report contains versions, counts, booleans, and paths reduced to
their last component. It contains no password, OTP secret, recovery code or
profile name. The redaction happens before the report is copied, so what reaches
the clipboard is exactly what is shown on screen.

---

## Logs

Logs are written to `logs/` inside the workspace, never to a console window and
never outside the workspace folder. No log line contains secret material,
enforced by the wrapper types rather than by filtering.

---

## Unsafe code

`tw-windows-platform` is the only crate that may use `unsafe`; the crates that
handle secrets are `#![forbid(unsafe_code)]`. Every unsafe block documents why it
is necessary, its pointer assumptions, handle ownership and cleanup invariant.
`unsafe_op_in_unsafe_fn` is denied.
