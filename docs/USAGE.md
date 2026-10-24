# Wallweave

A native Rust desktop app and session supervisor for existing Wallpaper Engine libraries on Linux.

**Status: working first-stage host, not a new complete scene engine.** Wallweave owns discovery, configuration, the UI, per-monitor process supervision and diagnostics. Rendering, Chromium hosting, SceneScript and compositor-specific wallpaper placement are delegated to a selected `linux-wallpaperengine` executable. A change of frontend alone does not repair unsupported scripts.

## What is implemented

- Native egui interface: library, setup, wallpaper properties and playback activity.
- Native Steam Workshop browser with title/ID/link search, type/resolution/category filters, pagination, subscriptions, artwork and download progress. Uses the running native Steam client and its existing login; no API key or password entry in Wallweave.
- Thumbnail library with type/source filters, search, sorting and adjustable tile size; a separate inspector selects the display and applies the background.
- Double-click an installed tile to apply it to the selected display. A current-wallpaper banner, per-display overview and tile labels show assignments and distinguish playing, paused and saved wallpapers. Selecting a display opens its assigned wallpaper and reveals its tile within the current library filters.
- Opening Wallweave automatically restores saved wallpapers and property overrides on connected displays after renderer/library discovery. Existing running or paused wallpapers keep their processes. Missing content does not erase assignments or prevent other displays from restoring.
- Per-wallpaper customization with checkboxes, sliders, color pickers, dropdowns and file/folder selectors. Author ordering, groups, localized English labels and common display conditions are respected; larger lists have search and a changed-only filter. Individual/all defaults can be restored, and Apply stays visible while scrolling.
- Installed wallpapers can be sorted by their Steam subscription date, newest or oldest first. Local favorites have a star on each tile and a Favorites filter; both favorites and the selected sort survive restarts.
- Animated startup display picker with the actual monitor arrangement, scale and rotation. Click a screen to choose it; the Apply button names that connector.
- Blue/anthracite desktop theme, with red labels and reasons for known incompatible content. Unmarked content is not a guarantee of full compatibility.
- Bundled Noto Sans CJK fallback for Chinese, Japanese and Korean titles, including machines without separately installed CJK fonts.
- Steam library discovery through modern and legacy `libraryfolders.vdf`, including external libraries.
- Built-in `defaultprojects` and local `myprojects` discovery, including custom installation names from Steam's app manifest.
- Workshop download/installation counts from Steam's manifest. Completed staging downloads appear as **Ready download** and can play directly while Steam downloads other items; the library refreshes every ten seconds while open.
- Packed scenes keep their logical JSON entry while validating the physical `scene.pkg`. Older projects without a declared type are classified from their entry filename.
- Explicit library paths remain configured when an iSCSI or removable drive is disconnected.
- Web, scene and video project discovery; packed scenes are labelled without claiming their scripts were inspected.
- Windowed preview and per-connector playback through the renderer.
- Pause, resume and stop for the app's own child process groups. Other wallpaper programs are never stopped.
- A private Unix socket and a session supervisor that keeps wallpapers alive after the GUI closes.
- Failed-startup replacement keeps the existing wallpaper running.
- CLI diagnostics, JSON library reports, literal argument passing, logs and explicit restore.
- An original web fixture for later browser/API checks.
- An optional pinned portable V8/Chromium renderer installer with SHA-256 verification.

## Run

If a prebuilt binary is present:

```sh
./run.sh
./run.sh doctor
```

Build from source with Rust 1.88 or later:

```sh
cargo run --release --locked
```

To update the prebuilt app used by `run.sh` and the desktop launcher:

```sh
python3 scripts/build-app.py
```

This atomically installs `bin/wallweave` and its required `bin/libsteam_api.so`. Keep these files together when moving a prebuilt app. Cargo supplies the runtime library path automatically for `cargo run` and tests.

For a CLI-only build:

```sh
cargo build --release --locked --no-default-features
```

The app uses native Wayland or X11 windows. The GUI does not require Chromium. Browser wallpapers use the separate renderer's browser process.

Version 0.1.1 disables blocking EGL VSync in Wayland sessions: a hidden window must still answer compositor pings. Redraws remain paced by Wayland frame callbacks. This follows [glutin's Wayland guidance](https://docs.rs/glutin/0.32.3/glutin/surface/enum.SwapInterval.html).

GUI launches through `run.sh` keep startup output and panic backtraces in `$XDG_STATE_HOME/wallweave/gui.log` (normally `~/.local/state/wallweave/gui.log`), with the previous launch in `gui-previous.log`. An isolated `WALLWEAVE_HOME` uses its own `state` folder instead. CLI output is unchanged.

## Renderer

The executable is selected in this order:

1. An explicit renderer configured in Settings or through `config renderer`.
2. The portable renderer recorded in `runtime/renderer-path.txt` beside this project/binary.
3. `linux-wallpaperengine` on `PATH`.

The installer downloads **WallpaperEngineLover/linux-wallpaperengine-kde v0.9.4**, KDE portable x86_64 build. That build disables KDE-specific parts outside Plasma and includes the fork's V8 and CEF dependencies. Its current upstream requires glibc 2.35 or newer. GNOME needs the renderer's matching Shell extension in addition to the executable.

```sh
python3 scripts/install-renderer.py
./run.sh doctor
```

The download is about 267 MB. It stays inside `runtime/`, is ignored by Git, does not replace system packages, does not enable autostart, and does not start a wallpaper. The original renderer's source and licenses remain separate; see [third-party components](THIRD_PARTY.md).

Renderer help is probed for supported flags. Detecting extended flags is **not** a claim that a particular wallpaper, script or GPU works. Renderer crashes are surfaced in Activity and in per-output logs.

## Connect your Steam library

Once the external drive is mounted, add the Linux mount path containing `steamapps`. A `steamapps` directory or `workshop/content/431960` directory is also accepted.

```sh
./run.sh config add-library "/mnt/your-drive/SteamLibrary"
./run.sh scan
./run.sh doctor
```

If shared assets are not discovered automatically:

```sh
./run.sh config assets "/mnt/your-drive/SteamLibrary/steamapps/common/wallpaper_engine/assets"
```

Paths may be saved before the disk is mounted. Unavailable paths are reported and retained. Windows drive-letter paths in an old Steam manifest are not interpreted as Linux mount points; add the actual mounted library explicitly.

Steam may finish downloading individual Workshop items into `steamapps/workshop/downloads/431960` before installing them into `steamapps/workshop/content/431960`. Since 0.1.3, items with positive, complete byte counters and a matching requested manifest revision are available directly as **Ready download**. Presence of a directory or preallocated files alone is insufficient. Eligibility is rechecked at launch time. Wallweave leaves Steam's files in place and also lists built-in Wallpaper Engine projects immediately.

Installed copies take precedence over staged copies. Saved staging paths still resolve after Steam installs the item. When a playing staging directory disappears and the installed copy appears, the supervisor restarts its renderer from the installed location and preserves property overrides and pause state. This also applies to window previews; preview assignments are not persisted.

Preset projects referencing a separate base wallpaper are not supported yet and are reported with their dependency ID.

## Steam Workshop

Since 0.2.4, **Installed → Sort** also offers **Subscribed: newest first** and **Subscribed: oldest first**. Selecting either reads every page of the signed-in account's subscriptions through the existing Steam connection and caches the dates for offline use. **Sync Steam dates** refreshes them. The source is Steam's `m_rtimeAddedToUserList` on subscription-list queries ([Steamworks documentation](https://partner.steamgames.com/doc/api/isteamugc#SteamUGCDetails_t)); file modification, installation and Workshop publication times are not used. Local projects and items without a known subscription date appear last in both directions, with title ordering for ties. A failed sync leaves the previous cache available.

Click a tile's star or **Add favorite** in the inspector to save a local Wallweave favorite. The **Favorites** filter combines with search, type, source and compatibility filters. Steam items retain their favorite when moved from staging into the installed library or a preserved copy; local projects use their full path. These preferences live in `library-preferences.json` in Wallweave's state directory and do not change Steam favorites.

Open **Workshop** while the native Linux Steam client is running and signed in with the account that owns Wallpaper Engine. Wallweave connects when the tab opens. Search by words, a numeric Workshop ID, or a `https://steamcommunity.com/sharedfiles/filedetails/?id=…` link. **My subscriptions** lists the signed-in account's subscriptions. Type, resolution and category filters use Steam's Workshop tags.

Select a wallpaper and choose **Download & subscribe**. This adds the item to your Steam subscriptions and queues a normal-priority transfer. Wallweave starts one native transfer at a time; Steam chooses its library location and manages updates. Failed requests can be retried. Completed wallpapers appear in Installed, where applying to a display remains explicit. The waiting queue is saved and resumes when Wallweave reopens.

**Sort by** includes Popular Today, Popular Week, Popular Month, Popular Year, Popular All Time, Newest and My subscriptions. The windows use Steam's 1/7/30/365-day trend ranking; All Time uses lifetime vote popularity. **Show NSFW** is off by default. Enabling it includes Mature/Questionable content subject to Steam's account preferences. Sort and NSFW preferences are remembered; Installed is independent of this switch.

Since 0.2.1, verified completed staging projects are preserved in `<SteamLibrary>/.wallweave/wallpapers/431960` before opening the SDK connection or starting another native transfer. Steam can clear its bulk staging tree when committing a single requested item. Preserved copies remain available and are hidden when their original is present. Copies share filesystem extents where supported, including the tested Btrfs library; other filesystems can need additional disk space. Steam's files are left in place. A failed preservation step prevents the SDK transfer from starting.

Download counts now require actual files. If Steam's old counters refer to missing projects, Installed shows **Restore missing downloads**. This queues those IDs for download again, with a saved queue that resumes after an app restart. A historical byte counter is not a content backup.

Wallweave never asks for a Steam password, reads login cookies or stores account credentials. A separate owned helper uses the SDK, with a lock preventing competing Wallweave connections. Steam may show Wallpaper Engine as running while connected; its downloads-while-gaming setting can pause other downloads during that context. Leaving Workshop with no active downloads releases the connection. Closing Wallweave releases its helper; subscriptions remain in Steam and waiting requests resume when Wallweave opens again.

Steam must be online for browsing and new downloads. The initial integration is tested with native Steam; a Flatpak Steam client may require using the native client for SDK connectivity. Local wallpaper playback remains independent of Workshop connectivity. Download availability does not guarantee renderer compatibility.

The same backend is available from the CLI:

```sh
./run.sh workshop status
./run.sh workshop search "forest"
./run.sh workshop search "forest" --sort popular-all-time
./run.sh workshop search "forest" --sort popular-today --nsfw
./run.sh workshop search --sort subscriptions --json
./run.sh workshop download 1234567890
```

Workshop thumbnails use a bounded Steam-hosted image cache in the app's state directory. The connection's native diagnostics are saved to `workshop.log` there. This repository and distributable contain no Workshop wallpapers.

## Playback

```sh
# Inspect without rendering; also works for the bundled fixture without Steam.
./run.sh inspect examples/web-pulse

# Replace 1234567890 with a downloaded Workshop ID.
./run.sh preview 1234567890

# Hyprland uses native connector names from hyprctl, e.g. DP-1.
./run.sh apply 1234567890 --output DP-1
./run.sh apply 1234567890 --output HDMI-A-1 --property speed=1.5

./run.sh status --json
./run.sh pause --output DP-1
./run.sh resume --output DP-1
./run.sh stop --output DP-1
./run.sh shutdown
```

`plan` prints the exact argument list without launching the renderer:

```sh
./run.sh plan 1234567890 --output DP-1
```

No shell is used to start renderers. Preview is a separate managed output named `preview`. Pausing suspends that renderer's process group; resuming continues it. Automatic fullscreen pause remains the renderer's responsibility.

Assignments are saved only after the renderer survives its initial startup interval. A running process is not a visual compatibility check. Opening the GUI automatically restores saved wallpapers and property overrides on connected displays once renderer, assets and monitor discovery are ready. It keeps existing running or paused wallpapers. Unavailable content remains saved and errors for one display do not stop the other displays from restoring. The CLI `restore` command explicitly reapplies saved assignments. Starting Wallweave at login remains a separate desktop-session preference.

## Desktop coverage

| Desktop | Route used by the selected renderer | Current Wallweave validation |
| --- | --- | --- |
| Hyprland | Native layer-shell | Video, scene and web backgrounds tested on DP-2, including pause/resume and stop; desktop layer placement and representative rendered output verified. See [validation and known limits](VALIDATION.md). |
| Sway, niri and similar | Native layer-shell | Routing model only; not tested in a desktop session |
| KDE Plasma | Renderer with Plasma integration | Requires the extended renderer; no local desktop test |
| GNOME | Renderer plus matching GNOME Shell extension | Extension setup and visual tests pending |
| X11 desktops | Renderer-dependent desktop output | Not tested |

Wallweave does not claim universal Wayland compatibility. Its frontend is independent of KDE, but input, output placement, automatic pause and compositor behavior still need testing with the selected renderer on each desktop.

## Data and development

- Configuration: `$XDG_CONFIG_HOME/wallweave/config.toml`.
- Logs: `$XDG_STATE_HOME/wallweave/logs/`.
- Control socket: `$XDG_RUNTIME_DIR/wallweave/control.sock`.
- Without a runtime directory, a private user-owned temporary directory is used.
- Set `WALLWEAVE_HOME` to an absolute temporary directory to isolate config, logs and the supervisor during development.

```sh
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Tests cover external-library discovery, missing mounts, malformed projects, path containment, argument handling, failed replacement and per-output process lifecycle. They use a fake renderer and do not change the desktop wallpaper. The egui views also receive headless layout checks.

See [the next development stages](ARCHITECTURE.md) and [the German test guide](TESTEN.md). This repository contains no Wallpaper Engine installation files or third-party Workshop wallpapers.

## License

Wallweave's original code is MIT licensed. External renderers, browser runtimes and Wallpaper Engine assets retain their own licenses.
