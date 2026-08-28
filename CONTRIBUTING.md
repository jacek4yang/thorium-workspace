# Contributing

## Platform

The product is Windows-only. The platform-independent crates (`tw-domain`,
`tw-secrets`, `tw-storage`, `tw-vault`, `tw-otp`, `tw-qr`, `tw-thorium`) build and
test anywhere, which is what makes most of the logic reviewable without a Windows
machine. Anything touching Job Objects, the named mutex, screen capture or window
activation needs Windows.

## Workflow

1. Branch from `main`.
2. Small, reviewable commits, each with a message that says *why*, not just what.
3. Keep v1.0.0 inside the scope in `README.md` and `docs/V1.0.0.md`. Proxying,
   routing, fingerprint randomization and registration automation are out, and
   partial versions of them are also out.
4. Run every gate before opening a pull request.
5. Include real evidence in the description. "Tests pass" is not evidence;
   pasted output is.

## Quality gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

npm --prefix app ci
npm --prefix app run lint
npm --prefix app run typecheck
npm --prefix app test
npm --prefix app run build
```

The Windows-specific code can be type-checked from any host, and should be
before pushing:

```sh
rustup target add x86_64-pc-windows-msvc
cargo clippy -p tw-windows-platform -p tw-browser-profile --all-targets \
  --target x86_64-pc-windows-msvc -- -D warnings
```

Never weaken a test, skip one, or relax a lint to get a green run.

## House rules

### Errors

No `unwrap`, `expect` or `panic!` on any path that handles runtime input, user
input, network data or the filesystem. Return a typed error carrying a stable
diagnostic code. `expect` in tests is fine and preferred over `unwrap` because it
names what failed.

### Secrets

Anything secret is a `SecretString` or `SecretBytes`. Do not add a field, a log
line or a serialized struct that could carry plaintext. When you add a path that
handles one, add the test that proves it does not leak — the existing crates all
have one, and they are the reason the property holds.

### Unsafe

Only `tw-windows-platform` may use it. Every block states why it is necessary,
what it assumes about the pointers it passes, who owns each handle and when it is
released. A block without that comment does not get merged.

### Diagnostic codes

Add new codes to `tw_domain::DiagnosticCode`, keep them in the right range, and
add them to `all()` — a test checks the count, which is what stops a code from
silently escaping the uniqueness check. Never renumber an existing code: users
quote them.

### Tests

Prefer a test against a real file in a temporary directory over a mock. Where an
external system is unavoidable, build a fixture: the Thorium install pipeline is
tested against a local HTTP server serving a synthetic archive, and profile
isolation against a stand-in browser. Both catch real bugs; a mock of our own
code would not.

Name a test after the behaviour it pins, not the function it calls.
`a_wrong_password_is_rejected` tells a reader what broke; `test_open_2` does not.

### Test credentials

Synthetic only. Never a real password, a real OTP secret or a real recovery code
— not in a test, a fixture, a comment, a screenshot or a commit message. The
published RFC test vectors are fine; that is what they are for.

## Never commit

Runtime workspace data: `workspace.db`, `vault/`, `profiles/`, `browsers/`,
`runtime/`, `backups/`, `logs/`. They are in `.gitignore`; check `git status`
before committing anyway.

## Releasing

Releases are cut from a tag and built by CI. The workflow refuses to build unless
the tag, the `VERSION` file and the workspace version agree. Do not tag until the
change has been reviewed and merged.
