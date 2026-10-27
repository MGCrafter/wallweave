Wallweave v0.2.5 adds the redesigned German interface and fixes automatic wallpaper pause/resume on Hyprland, including special workspaces. Pausing now uses a cooperative renderer control instead of stopping renderer processes, so audio threads keep running.

### Downloads

| File | Use |
| --- | --- |
| `wallweave-0.2.5-1-x86_64.pkg.tar.zst` | Arch Linux / EndeavourOS / CachyOS package. |
| `Wallweave-0.2.5-x86_64.AppImage` | Make executable and run. |
| `wallweave-0.2.5-linux-x86_64.tar.gz` | Portable app with optional user installer. |
| `PKGBUILD` | Checksum-pinned Arch package recipe. |
| `install-renderer.py` | Installs and configures the new renderer without compiling. |
| `wallweave-renderer-v0.9.4-pause1-linux-x86_64.tar.gz` | Portable renderer with the pause fix; the installer downloads this automatically. |
| `wallweave-renderer-v0.9.4-pause1-sources.tar.gz` | Matching renderer and dependency sources, build recipes and licenses. |
| `SHA256SUMS` | Checksums for all release downloads. |

Linux x86_64, glibc 2.35 or newer. Wallpaper Engine assets and Workshop content are supplied by the user. The app and renderer are separate downloads.

### Update from v0.2.4

Close the GUI and run `wallweave shutdown` (or launch the old AppImage with `shutdown`). Then update your app:

- Arch/CachyOS: `sudo pacman -U ./wallweave-0.2.5-1-x86_64.pkg.tar.zst`
- AppImage: download the new file, then `chmod +x Wallweave-0.2.5-x86_64.AppImage`.
- User installation: extract the new portable archive and run `python3 install.py` again.

**Also update the renderer for the pause fix.** Download this release's `install-renderer.py`, then:

```sh
python3 install-renderer.py --directory "$HOME/.local/share/wallweave/renderer" --configure wallweave
```

For AppImage use `--configure ./Wallweave-0.2.5-x86_64.AppImage`. If the user launcher is not on `PATH`, use `--configure "$HOME/.local/bin/wallweave"`.

Reopen Wallweave. Settings, favorites and wallpaper assignments are retained. The existing auto-pause preference is preserved; choose **Einstellungen → Wiedergabe → Automatisch pausieren** to enable focus or fullscreen pause. New installations default to automatic pause off. Older renderer folders are kept.

[Deutsche Update-Anleitung](https://github.com/MGCrafter/wallweave/blob/v0.2.5/docs/INSTALL.de.md#aktualisieren-und-entfernen) · [English installation guide](https://github.com/MGCrafter/wallweave/blob/v0.2.5/docs/INSTALL.md)

### Changes and validation

- Redesigned library, Workshop, monitor selection and settings, with bundled IBM Plex and Lucide fonts.
- Automatic pause respects the visible normal/special workspace on each Hyprland output and resumes when the condition ends.
- Manual and automatic pauses remain independent. Expired renderer control releases pause after about three seconds.
- Renderer installer verifies its pinned checksum and cooperative-pause support before selecting a new runtime; failed upgrades retain the previous selection.
- Rust tests, formatting, Clippy, CLI-only build and package checks run before publication. Upgrade tests install over v0.2.4 and verify saved state is unchanged. The Arch package is built, installed and removed in a clean container.

The real renderer's pause/resume and independent audio were exercised in an isolated session. This is not a benchmark or a guarantee for every wallpaper, audio backend or game. Hyprland received desktop testing; KDE/GNOME behavior still depends on the renderer integration. See [validation](https://github.com/MGCrafter/wallweave/blob/v0.2.5/docs/VALIDATION.md).
