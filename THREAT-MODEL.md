# Threat model

## What this program protects

A folder containing browser profiles, account records, and the passwords, OTP
secrets and recovery codes those accounts need. The folder is portable: it may
live on a USB stick, in a synced directory, or in a user profile.

## Who the program is for

One person managing several separate online identities on a Windows machine they
control. Not a shared administrative tool, not a team password manager, and not
an anonymity system.

---

## Attackers considered

### A1. Someone who obtains the folder but not the master password

**Examples**: a lost USB stick, a stolen laptop's disk, a backup archive that
ended up somewhere it should not have, a cloud sync provider.

**Defended.** The vault is encrypted with a key derived from the master password
by Argon2id (64 MiB, 3 passes) and sealed with XChaCha20-Poly1305. The header is
authenticated, so cost parameters cannot be rewritten downwards. Guessing rate is
bounded by the KDF cost.

**What they still learn.** The metadata database is not encrypted: account
display names, usernames, email addresses, sign-in URLs, tags, notes, profile
names, locales and timezones are all readable. This is a deliberate trade —
searchable, editable metadata while the vault is locked is what makes the program
usable — and it is stated so nobody is surprised by it. **Do not put a secret in
a notes field.** The UI says so at the field.

They also learn how many secrets exist, and can read browser profile data
(cookies, sessions, history) directly, exactly as they could for any Chromium
profile on that disk.

### A2. Another user account on the same machine

**Partly defended, by the operating system.** The workspace inherits the
directory's ACL. If it is placed inside the user's own profile, Windows protects
it. If it is placed somewhere world-readable, it is world-readable.

**Not defended by this program.** It does not set ACLs of its own. The remedy is
where the folder is put, and the documentation says so.

### A3. Malware running as the same user

**Not defended, and cannot be.** Code running with the user's privileges can read
process memory, the clipboard, keystrokes, and every file the user can read. No
user-space password manager defends against this, and claiming otherwise would be
dishonest.

What is done anyway, because it narrows the window:

- The vault is locked by default when idle, and can lock on minimise.
- Copied secrets are cleared from the clipboard on a timer.
- The derived key and decrypted secrets exist in one place and are scrubbed when
  the session drops.
- The DevTools port is opened only when a profile needs an override and is closed
  with the profile.

### A4. A compromised or hostile upstream Thorium release

**Partly defended.** The download pipeline treats everything upstream publishes
as hostile input: the asset is chosen by rule rather than by name, the transfer
is bounded in size, stall time and wall-clock time, the SHA-256 of what actually
arrived is computed from the stream and recorded, every archive entry is
validated against path traversal, absolute paths, symlinks and reserved device
names, and the extracted tree is checked to contain a plausible browser before
anything is promoted.

**Not defended.** Upstream does not publish signatures or digests this program
can verify against an independent trust root, so a *maliciously replaced* release
that is internally consistent would be installed. The recorded digest lets a user
compare against what upstream shows, and the release page is linked next to every
install action so they can check before downloading.

This is a supply-chain dependency the product accepts and names, rather than
papering over.

### A5. A network attacker between the user and GitHub

**Defended.** All requests are HTTPS with certificate validation through rustls.
Plain HTTP is refused. Native certificate roots are preferred so a corporate
TLS-inspecting proxy keeps working, which also means such a proxy can see the
request — that is a property of the user's own environment, not of this program.

### A6. A hostile web page in a launched browser

**Out of scope; the browser's problem.** This program launches Chromium with no
weakened security flags, and a test asserts none are ever added: no
`--disable-web-security`, no `--ignore-certificate-errors`, no `--no-sandbox`.
The workspace's own data is outside the browser's sandbox and is not reachable
from a page.

The DevTools port, when open, is not reachable from a web page (Chromium blocks
that), but is reachable by any local process running as the same user — see A3.

### A7. A second instance of this program, or a second browser on one profile

**Defended.** A named mutex derived from the workspace path admits one manager
per folder. A file lock per profile admits one browser per `User Data`
directory; launching an already-running profile brings its window to the front
instead of starting a conflicting second browser. A Job Object with
`KILL_ON_JOB_CLOSE` means a crashed manager cannot leave an orphaned browser tree
holding a profile.

### A8. Someone reading a support log or a screenshot

**Defended.** The diagnostic report contains versions, counts, booleans and paths
reduced to their last component. It contains no secret and no profile name, and
the redaction happens before the copy so what reaches the clipboard is what is on
screen. Log lines cannot contain secrets, because secrets are wrapped in types
that render `[redacted]`.

---

## Explicitly not threats this program addresses

- **Network-level identity.** No proxying, no routing, no IP rotation. Two
  profiles launched on one machine share one IP address, and the product does
  not pretend otherwise.
- **Browser fingerprinting.** No randomization of canvas, WebGL, GPU, audio or
  hardware properties. Two profiles present the same hardware fingerprint. A
  profile's timezone and locale are a convenience for using a site as intended,
  not a disguise.
- **Coercion.** There is no duress password and no hidden volume.
- **Physical attacks on a running machine.** Cold-boot attacks, DMA attacks and
  a hardware keylogger are all out of scope.
- **The account provider.** If a service is compromised, stored credentials for
  it are compromised.

---

## Assumptions

1. The operating system is not compromised.
2. The user's Windows account is not shared with someone untrusted.
3. The master password is not trivially guessable and is not stored beside the
   workspace.
4. The machine's clock is roughly correct. TOTP depends on it; a badly wrong
   clock produces codes a server rejects.
5. The folder the workspace lives in has appropriate permissions for where it is.

---

## Residual risks, in one list

| Risk | Status |
| --- | --- |
| Metadata readable without the master password | Accepted, documented, surfaced at the notes field |
| Malware as the same user | Not defensible; window narrowed by locking and clipboard clearing |
| No signature verification on upstream Thorium releases | Accepted; digest recorded, release page linked |
| Loopback DevTools reachable by local processes | Accepted; opened only when needed, closed with the profile |
| Zeroization defeated by swap or reallocation | Accepted; inherent to the platform |
| A revealed secret's lifetime in the WebView heap | Accepted; reveal is explicit and user-initiated |
| Browser fingerprint identical across profiles | By design; out of scope for v1.0.0 |
