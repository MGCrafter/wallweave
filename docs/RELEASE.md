Wallweave's first public Linux release brings a native Rust desktop app to existing Wallpaper Engine libraries.

### Downloads

| File | Use |
| --- | --- |
| `wallweave-0.2.4-1-x86_64.pkg.tar.zst` | Native Arch Linux / EndeavourOS / CachyOS package (`pacman -U`). |
| `PKGBUILD` | Checksum-pinned recipe to build the native package with `makepkg -si`. |
| `Wallweave-0.2.4-x86_64.AppImage` | Single-file app. Make executable and run. |
| `wallweave-0.2.4-linux-x86_64.tar.gz` | Portable folder with optional user installer and application-menu entry. |
| `install-renderer.py` | Optional pinned, SHA-256-verified renderer installer. |
| `SHA256SUMS` | SHA-256 checksums for the release files. |

Built on Ubuntu 22.04 for Linux x86_64 with glibc 2.35 or newer. The Steamworks runtime and CJK font are included. Wallpaper Engine, Workshop content and the renderer are supplied separately.

### Install

On Arch Linux, EndeavourOS or CachyOS:

```sh
sudo pacman -U ./wallweave-0.2.4-1-x86_64.pkg.tar.zst
wallweave
```

For AppImage:

```sh
chmod +x Wallweave-0.2.4-x86_64.AppImage
./Wallweave-0.2.4-x86_64.AppImage
```

Or extract the archive and install for your user without sudo:

```sh
tar -xzf wallweave-0.2.4-linux-x86_64.tar.gz
cd wallweave-0.2.4-linux-x86_64
python3 install.py
```

See the [installation guide](https://github.com/MGCrafter/wallweave/blob/main/docs/INSTALL.md) for the renderer, Steam setup, updating, uninstalling and FUSE-free launch.

### Included

- Searchable thumbnail library, favorites and sorting by actual Steam subscription date.
- Native Steam Workshop browsing, subscriptions, download queue and recovery of missing downloads.
- Animated monitor picker, per-display wallpapers and double-click to apply.
- Wallpaper customization with sliders, colors, checkboxes and grouped settings.
- Saved assignments restored when opening the app; wallpapers keep running when the GUI closes.
- CLI diagnostics, per-output logs and pause/resume/stop controls.

Hyprland has received desktop playback testing. Other desktop integrations and individual wallpaper scripts depend on the renderer; see [validation and known limits](https://github.com/MGCrafter/wallweave/blob/main/docs/VALIDATION.md). This remains early software, with no guarantee that every Wallpaper Engine project works.

The release pipeline runs the Rust test suite, formatting, Clippy, a CLI-only build check and isolated package smoke tests before publishing. AppImage checks include keeping the supervisor alive after the launcher exits. No real wallpaper is applied by the automated checks.
