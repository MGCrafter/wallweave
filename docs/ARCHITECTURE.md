# Architecture and scope

```text
Native egui GUI / CLI
        |
        +-- Steam VDF + project.json discovery -- persistent offline paths
        |
        +-- owned Steamworks helper (JSON lines over pipes)
        |        +-- current native Steam session: UGC queries / subscriptions
        |        +-- normal-priority Steam downloads / completion callbacks
        |
        +-- private Unix socket
                 |
             Rust supervisor
                 |
                 +-- one owned renderer process group per output
                 +-- separate windowed-preview process group
                           |
                  linux-wallpaperengine renderer
                  | scene runtime + SceneScript
                  | Chromium/CEF for web content
                  | video decoder
                           |
                  compositor integration supplied by renderer
                  | Hyprland / wlroots: layer-shell
                  | Plasma: Plasma integration
                  | GNOME: renderer's Shell extension
```

## Current boundary

The Workshop integration uses the Steamworks UGC API through `steamworks-rs`. Only its helper process receives `SteamAppId=431960` and `SteamGameId=431960`; the multithreaded GUI never mutates these environment variables. The SDK is initialized once per helper, callbacks are pumped there, and the parent terminates and reaps only its own helper when the connection is dropped. The frontend uses channels and nonblocking event polling, independent of the renderer supervisor.

Every download ID is queried first and must resolve to a non-banned community item for app 431960. Only then is the item subscribed and `DownloadItem(false)` issued. Native completion requires the matching app/item callback; byte counters reaching 100% alone mean installation is still pending. Install paths are read only once completion is confirmed (or an item is already fully installed before a transfer is requested). Existing independently validated staging discovery remains available through Installed while Steam finishes installation.

Searches have request IDs and timeouts. Item/link input and tag filters are validated before entering SDK functions that require C strings. Artwork requests have an HTTPS Steam-host allowlist, no redirects, a 12-second timeout and an 8 MiB compressed limit; image decoding retains the library UI's dimension/allocation limits. Four decoder/fetch workers and a bounded texture cache are shared across views; the on-disk Workshop artwork cache retains at most 256 images. A failed connection exposes a reconnect action and leaves local playback available.

The initial release deliberately integrates the existing engine through its documented CLI. It has no bundled reverse-engineered proprietary code, independent GPU renderer, independent SceneScript implementation or newly fixed SceneScript APIs. It does not pretend a CLI adapter solves rendering compatibility.

The interface is replaceable: `renderer::plan` produces an executable and argument vector, while `service` owns its lifecycle. Future direct C-ABI backends can be added without changing Steam discovery and the GUI. The public C ABI in `Aromatic05/wallpaper-engine-renderer` and the wrappers in `we-layerd` are useful references for such a step.

The supervisor retains real `Child` handles. It never loads a PID from disk and signals it later, and never uses `pkill`, process-name matching or global wallpaper shutdown. A failed replacement does not stop the previous process. GUI shutdown does not stop the supervisor; explicit Stop/Shutdown does.

Automatic app/game pause defaults to off; saved preferences are retained. On Hyprland, a bounded background IPC worker checks the active window and each monitor's active/special workspace every 250 ms while desktop wallpapers need detection. Workspace membership is mandatory even when hidden special-workspace clients retain mapped/focus/fullscreen state. The supervisor disables the renderer's competing window rules with `--no-fullscreen-pause` and writes a separate private pause file for each renderer advertising `--pause-control-file`. Both manual and automatic pause use the renderer's internal `RenderContext::setPause` path; no `SIGSTOP` is used. The renderer keeps its audio/event threads alive and polls the control file while paused. A 500 ms renewal and a three-second lease make a crashed supervisor fail open. The adapter patch is pinned to upstream commit `a239861367cb8d338ef8ba346f937067c3a266d8`; `scripts/build-pause-renderer.py` builds it. Older renderers cannot be manually suspended; on Hyprland their automatic pause is disabled. Manual and automatic pause reasons remain separate; missing, invalid or expired compositor data releases only automatic pauses. No IPC runs on the supervisor or UI thread. Other desktops retain the renderer's supported native pause flags.

A separate `auto-pause.json` preference keeps configuration readable by an older supervisor. `reload_playback` replaces only desktop renderers whose requested rule changed, preserving properties and manual pauses; previews stay independent. Status reports the requested cooperative pause state for supervisor-managed Hyprland renderers (applied on the renderer's next poll); native renderer-only detection reports the configured rule because it has no live status channel.

Filesystem and renderer operations run outside the UI thread. A disconnected or slow external mount should not prevent the GUI from painting. Linux filesystem calls can still wait on a stuck network/block mount; cancellation of a stuck scan is not implemented yet.

## Work after the Steam/iSCSI drive is available

1. Select 10–20 concrete failing wallpapers, separating web, SceneScript, video and unsupported application wallpapers.
2. Record Workshop ID, local project type, renderer build, compositor, GPU, expected behavior and renderer logs.
3. Compare the installed upstream engine with the pinned V8/Chromium fork. Keep differences attributable to the renderer, not to the UI.
4. For web wallpapers, test property delivery, audio callbacks, media callbacks, WebGL, local-file access, pause and resize.
5. For scene wallpapers, compare lifecycle ordering, vector/value conversion, layer APIs, animation and particles. A working JS parser is insufficient.
6. Fix reproducible gaps in a renderer backend, with small original fixtures and behavior comparison against the user's own Wallpaper Engine installation.
7. Run the same corpus on Hyprland, KDE and GNOME before claiming desktop coverage. Cover fractional scaling, multiple monitors, hotplug, suspend and fullscreen pause.

## Known first-release limitations

- No automatic renderer restarts: repeated crashes stay visible instead of creating a restart loop.
- Monitor hotplug does not automatically rewrite or reapply assignments.
- Hyprland and optional `wlr-randr` discovery are implemented; other desktops can enter connector names manually.
- GNOME extension installation remains an explicit desktop setup step.
- Project inspection does not unpack `.pkg` archives and cannot certify their scripts.
- The UI exposes textual property overrides; it does not yet resolve all localized labels or directory/file selector semantics.
- A complete Rust scene renderer is future work. Moving control logic to Rust does not move the upstream C++ rendering code to Rust.


## Completed staging protection and recovery (0.2.1)

Before SDK initialization and before each queued transfer, the helper preserves completed, revision-matching staged projects outside Steam's managed directory, under the same Steam library's `.wallweave/wallpapers/431960`. Copying publishes each folder with a rename; hidden temporary/previous folders are excluded from discovery. Traversal through external symlinks is rejected. Incomplete or stale-revision downloads are not promoted. A failed preservation step aborts the connection/transfer. Installed originals take precedence over preserved copies in the same library; old staging paths can resolve to their preserved copy.

This responds to a reproduced Steam behavior: a single-item SDK commit can remove the existing bulk staging tree while leaving its historical byte counters. Availability now requires project files, with separate reported and missing counts. Recovery serializes native transfers, saves waiting IDs atomically in the app state, and resumes them on the next GUI launch. A helper lock prevents simultaneous Wallweave SDK connections. Workshop NSFW/sort preferences are stored in a separate state JSON file so the existing renderer supervisor's configuration format remains compatible.
