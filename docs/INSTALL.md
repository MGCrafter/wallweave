# Install Wallweave on Linux

[Deutsch →](INSTALL.de.md) · [Back to README](../README.md)

## Requirements

- Linux **x86_64**, glibc **2.35+** (Ubuntu 22.04 or newer, or a compatible distribution).
- A graphical Wayland or X11 session and working EGL/OpenGL drivers.
- Your own Wallpaper Engine installation and shared `assets` directory.
- A compatible `linux-wallpaperengine` renderer, installed separately.
- Native Linux Steam, running and signed in with an account that owns Wallpaper Engine, for Workshop browsing/downloads. Local playback does not need an active Workshop connection.
- Python **3.10+** for the optional app and renderer installers.

Hyprland has received desktop playback testing. Other compositors and GNOME's required renderer extension need their own setup and testing. Alpine/musl and ARM are not covered by these prebuilt files. The app includes `libsteam_api.so` and its CJK font.

## Arch Linux, EndeavourOS and CachyOS

Download `wallweave-0.2.5-1-x86_64.pkg.tar.zst` from [Releases](https://github.com/MGCrafter/wallweave/releases/latest), then:

```sh
sudo pacman -U ./wallweave-0.2.5-1-x86_64.pkg.tar.zst
wallweave
```

Pacman handles dependencies, the application-menu entry and icon. Install the renderer separately:

```sh
python3 /usr/share/wallweave/install-renderer.py --directory "$HOME/.local/share/wallweave/renderer"
```

Then select the printed executable in Settings. Python is an optional package dependency; install it with `sudo pacman -S python` if you use this helper. Native Steam can be installed with `sudo pacman -S steam` after enabling Arch's multilib repository if necessary. Wallpaper Engine ownership and its assets are still required.

To rebuild the native package from the matching release archive, download the release's `PKGBUILD` into an empty folder, review it and run:

```sh
sudo pacman -S --needed base-devel
makepkg -si
```

The PKGBUILD downloads the matching portable archive and verifies its pinned SHA-256. This builds the package around the precompiled application; it does not compile Rust locally. It is provided on GitHub and is not an AUR listing.

To update, quit the GUI, run `wallweave shutdown`, then install the newly downloaded package with `sudo pacman -U ./wallweave-NEW-VERSION-1-x86_64.pkg.tar.zst` (replace `NEW-VERSION`). To uninstall:

```sh
wallweave shutdown
sudo pacman -R wallweave
```

Personal settings, logs, Steam content and separately installed renderers remain in place.

## Option A: AppImage

1. Download `Wallweave-0.2.5-x86_64.AppImage` from [the latest release](https://github.com/MGCrafter/wallweave/releases/latest).
2. Keep it in a stable location, such as `~/Applications`.
3. Make it executable and open it:

```sh
chmod +x Wallweave-0.2.5-x86_64.AppImage
./Wallweave-0.2.5-x86_64.AppImage
```

The same file accepts CLI commands, for example:

```sh
./Wallweave-0.2.5-x86_64.AppImage doctor
```

If your distribution does not provide FUSE 2, use the runtime's built-in extraction mode:

```sh
./Wallweave-0.2.5-x86_64.AppImage --appimage-extract-and-run
# With a CLI command:
./Wallweave-0.2.5-x86_64.AppImage --appimage-extract-and-run doctor
```

On first launch the app keeps a versioned copy under `${XDG_CACHE_HOME:-~/.cache}/wallweave/appimage/`. The supervisor uses this copy so wallpapers can continue after the AppImage launcher and GUI exit. Keep that cache while wallpapers are running. AppImage does not install a menu entry automatically; use Option B if you want the included menu installer.

## Option B: Portable archive / user installation

Download `wallweave-0.2.5-linux-x86_64.tar.gz` from [Releases](https://github.com/MGCrafter/wallweave/releases/latest), then:

```sh
tar -xzf wallweave-0.2.5-linux-x86_64.tar.gz
cd wallweave-0.2.5-linux-x86_64
./run.sh
```

You can keep and run this folder anywhere. Keep `bin/wallweave` beside its `bin/libsteam_api.so`; moving only the executable breaks Steam runtime loading.

For a permanent user installation with an icon and menu entry:

```sh
python3 install.py
```

No `sudo` is needed. The installer writes:

| Path | Purpose |
| --- | --- |
| `~/.local/share/wallweave/app/` | Application files |
| `~/.local/bin/wallweave` | Terminal launcher |
| `~/.local/share/applications/io.wallweave.Wallweave.desktop` | Application-menu entry |
| `~/.local/share/icons/hicolor/scalable/apps/io.wallweave.Wallweave.svg` | App icon |

The data, menu and icon paths honor `XDG_DATA_HOME`. If your terminal cannot find `wallweave`, use `~/.local/bin/wallweave` or add `export PATH="$HOME/.local/bin:$PATH"` to your shell profile and open a new terminal.

## Install the renderer

Wallweave's UI is native; wallpaper playback uses a separate renderer. If you already have a suitable `linux-wallpaperengine`, select its executable in **Settings** or leave it on `PATH`.

For the pinned portable renderer, download `install-renderer.py` from the Wallweave release. It is also included as `scripts/install-renderer.py` in the portable archive and installed app.

```sh
python3 install-renderer.py --directory "$HOME/.local/share/wallweave/renderer"
```

The installer downloads **linux-wallpaperengine-kde v0.9.4 with Wallweave's cooperative-pause patch**, checks its pinned SHA-256 and prints the launcher path. It does not apply a wallpaper or enable autostart. Select the printed launcher in **Einstellungen → Dateien & Renderer → Renderer**, or configure it from the installed CLI:

```sh
wallweave config renderer "$HOME/.local/share/wallweave/renderer/v0.9.4-wallweave-pause1/wallweave-renderer-v0.9.4-pause1-linux-x86_64/linux-wallpaperengine"
wallweave doctor
```

For AppImage, replace `wallweave` with the AppImage path. This explicit configuration also lets AppImage and archive installations share the same renderer. GNOME needs the matching renderer Shell extension in addition to these files; consult [the renderer project](https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde).

## Connect Steam and choose a wallpaper

1. Launch Steam and install Wallpaper Engine in one of your Steam libraries. Shared assets normally live in `steamapps/common/wallpaper_engine/assets`.
2. Open Wallweave. If your library is not detected, choose **Settings → Add Steam library** and select the directory containing `steamapps`.
3. If needed, configure the **Assets** directory in Settings.
4. Select a monitor in the display picker. Choose an installed wallpaper and use **Apply to …**, or double-click its tile to apply to that display.
5. Open **Workshop** to browse and choose **Download & subscribe**. Steam handles subscriptions, download storage and updates.

External library paths remain saved when the drive is disconnected. Reconnect it and rescan. Windows drive letters must be replaced with the actual Linux mount paths.

```sh
wallweave config add-library "/mnt/your-drive/SteamLibrary"
wallweave config assets "/mnt/your-drive/SteamLibrary/steamapps/common/wallpaper_engine/assets"
wallweave doctor
```

Closing the GUI keeps wallpapers running. To stop Wallweave's wallpapers and supervisor:

```sh
wallweave shutdown
```

Saved assignments restore on the next normal GUI launch. Login autostart is a separate desktop-session preference; the installer does not enable it.

## Verify downloads

Download `SHA256SUMS` beside your chosen assets:

```sh
sha256sum --ignore-missing -c SHA256SUMS
```

Every downloaded release asset listed in the file should report `OK`. Checksums help detect damaged downloads; they are not a code-signing signature.

## Update

Quit the GUI and run `wallweave shutdown` using the old installation first. This stops the old supervisor so the next launch uses the updated version.

- **AppImage:** download the new AppImage, make it executable and open it. Keep old version caches until their supervisor has stopped.
- **User installation:** extract the new archive and run its `python3 install.py` again.
- **Portable folder:** use the freshly extracted folder, keeping its binary and runtime together.

Settings, favorites, download queue and wallpaper assignments are retained. Do not delete your renderer directory during an app update.

**When updating from v0.2.4 to v0.2.5**, also install the cooperative renderer for the pause fix. Download the new release's `install-renderer.py`, then run while Wallweave is closed and its supervisor is stopped:

```sh
python3 install-renderer.py --directory "$HOME/.local/share/wallweave/renderer" --configure wallweave
```

Use `--configure "$HOME/.local/bin/wallweave"` if the user launcher is not on `PATH`, or `--configure ./Wallweave-0.2.5-x86_64.AppImage` for AppImage. The installer verifies the pinned digest and pause capability, preserves the older renderer directory and selects the new executable in Wallweave. No compilation is required. Reopen Wallweave to restore assignments; existing auto-pause preferences are retained. Enable the desired rule under **Einstellungen → Wiedergabe**. There is no automatic app updater yet.

## Uninstall

Quit the GUI and run `wallweave shutdown` first (or the equivalent AppImage command).

For a user installation, run the installer included in the installed app:

```sh
python3 "$HOME/.local/share/wallweave/app/scripts/install-app.py" --uninstall
```

Adjust the path if you set `XDG_DATA_HOME`. This removes the app, its launcher, icon and menu entry while retaining settings, logs and separately installed renderers. You can also run `python3 install.py --uninstall` from an extracted release archive.

For AppImage, remove the AppImage file and, once its supervisor is stopped, `~/.cache/wallweave/appimage/` (or the corresponding `XDG_CACHE_HOME` path). For a portable archive, remove its extracted folder.

Optional cleanup of your personal configuration and logs is separate: `~/.config/wallweave/` and `~/.local/state/wallweave/`, respecting the relevant XDG variables. The optional renderer is in the directory you passed to its installer. Steam subscriptions, Steam-managed wallpapers and Wallweave's preserved copies inside Steam libraries are not removed by the app uninstaller.

## Troubleshooting

| Problem | Check |
| --- | --- |
| AppImage mentions FUSE | Use `--appimage-extract-and-run`. |
| `GLIBC_… not found` | Check `ldd --version`; the prebuilt app needs glibc 2.35+. |
| `libsteam_api.so` missing | Keep the complete archive together or reinstall it. |
| GUI cannot open | Check the graphical session, GPU/EGL drivers and `~/.local/state/wallweave/gui.log`. |
| Missing `libxkbcommon` / EGL with the archive | Install the distribution's XKB/EGL/Wayland/X11 runtime packages, or use AppImage. |
| No renderer / Apply disabled | Install and select a renderer, configure assets, then run `doctor`. |
| Workshop cannot connect | Use native Linux Steam, signed in and online with Wallpaper Engine ownership. Flatpak Steam SDK connectivity is not validated. |
| External library missing | Mount the drive, add its real Linux path and rescan. |
| Black or broken wallpaper | Try window preview and inspect the per-output log in Activity. Individual SceneScript/web features depend on the renderer. |

For a report, include `wallweave doctor --json`, your desktop and app version, and the affected Workshop ID/link. Review diagnostic output for private paths before posting to [Issues](https://github.com/MGCrafter/wallweave/issues).
