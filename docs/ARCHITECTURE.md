# Architecture Baseline

This file defines the intended v1.0.0 boundaries. Implementation details may evolve through reviewed architectural decisions.

## Layers

```text
Frontend (Tauri WebView)
        |
        v
Typed Tauri command/event boundary
        |
        v
Controller / application services
   |        |        |        |
   v        v        v        v
Storage    Vault    Thorium   Browser Profile Runtime
   |        |                   |
   +--------+-------------------+
                |
                v
          Windows platform layer
```

## Rules

- Domain types are platform/UI independent.
- Frontend never owns cryptographic or persistence policy.
- Secrets cross the frontend boundary only for explicit reveal/copy operations.
- Windows-specific FFI is isolated.
- Browser binaries are separate from Browser Profile data.
- Desired configuration is persisted; process/runtime state is observed and reconstructible.
- Runtime state must not become the sole copy of user configuration.
- Portable workspace data is relative to the application executable.

## Runtime ownership

A launched Browser Profile owns a runtime session containing the Thorium process tree and any local CDP control endpoint required for timezone/locale behavior.

Windows Job Objects should make child lifetime explicit and prevent orphaned browser process trees after manager failure where practical.

## Download proxy (owner-requested, v1.0.0)

At the owner's explicit request, v1.0.0 includes a single **download-scoped**
proxy setting (`WorkspaceSettings.download_proxy`, `scheme://host:port`):

- It routes **workspace downloads only**: Thorium release discovery, install
  archives, and the ip.sb connectivity probe (`proxy_test` command).
- It **never** routes browser profile traffic, CDP, or vault operations; the
  browser-profile crate has no proxy input at all.
- The proxy URL may embed credentials, so it is never logged, never echoed
  into error messages, and never rendered in diagnostic dumps (the Settings
  input shows it because the user typed it).
- Validation is dependency-free (`domain::validate_proxy_url`), applied before
  any network attempt, with the stable `DOMAIN_INVALID_PROXY_URL` code.

## Future proxy extension point

A future release may still add a `network_route_id`/equivalent reference to a
Browser Profile. v1.0.0 does not implement network routes, Xray, subscriptions,
or any browser-traffic proxying. Do not let future proxy plans contaminate the
current core with unused machinery.
