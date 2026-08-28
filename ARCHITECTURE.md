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

## Future proxy extension point

A future release may add a `network_route_id`/equivalent reference to a Browser Profile. v1.0.0 does not implement network routes, Xray, subscriptions, or proxy UI. Do not let future proxy plans contaminate the current core with unused machinery.
