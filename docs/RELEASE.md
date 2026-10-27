Wallweave v0.2.6 adds an optional gaming mode that releases desktop wallpaper memory while a game is running. All desktop renderer processes are stopped and restarted after the last detected game exits, with their monitor assignments, wallpaper properties and manual pause state preserved.

### Enable the new option

Open **Einstellungen → Wiedergabe → Beim Spielen**, enable **Grafikspeicher beim Spielen freigeben** and save. It is off by default, including after an update. The supervisor continues monitoring when the Wallweave window is closed.

- Detects Steam game processes, including native games, Proton and Steam shortcuts. On Hyprland, Gamescope, `steam_app_<id>` windows and windows tagged `game` are also recognized.
- Keeps wallpapers unloaded through Alt-Tab, minimized windows and workspace changes. With several games open, restoration waits until all have exited.
- Restarts wallpapers after three seconds without a detected game. Manually paused wallpapers start paused; stopping an unloaded wallpaper cancels its return. Newly selected wallpapers wait until the game exits.
- Keeps missing wallpapers and disconnected Hyprland outputs queued for retry. Window previews remain independent.
- Shows **Spiel erkannt · Wallpaper entladen** in settings and an unloaded status per display. CLI users can select `wallweave config gaming-mode unload` or `off`.

Detection is limited to the categories above. Other Steam-launched software carrying a game ID can also trigger this mode. Wallpaper Engine itself is excluded. [Detailed behavior and detection coverage](https://github.com/MGCrafter/wallweave/blob/v0.2.6/docs/USAGE.md#release-graphics-memory-while-gaming).

### Downloads

| File | Use |
| --- | --- |
| `wallweave-0.2.6-1-x86_64.pkg.tar.zst` | Arch Linux / EndeavourOS / CachyOS package. |
| `Wallweave-0.2.6-x86_64.AppImage` | Make executable and run. |
| `wallweave-0.2.6-linux-x86_64.tar.gz` | Portable app with optional user installer. |
| `PKGBUILD` | Checksum-pinned Arch package recipe. |
| `install-renderer.py` | Installs and configures the pinned renderer without compiling. |
| `wallweave-renderer-v0.9.4-pause1-linux-x86_64.tar.gz` | The same cooperative-pause renderer supplied with v0.2.5. |
| `wallweave-renderer-v0.9.4-pause1-sources.tar.gz` | Matching renderer and dependency sources, build recipes and licenses. |
| `SHA256SUMS` | Checksums for all release downloads. |

Linux x86_64, glibc 2.35 or newer. Wallpaper Engine assets and Workshop content are supplied by the user. The app and renderer are separate downloads.

### Update from v0.2.5

Close the GUI and run `wallweave shutdown` with the old installation (or pass `shutdown` to the old AppImage). This is required to replace the running supervisor with the version that understands gaming mode. Then update the app:

- Arch/CachyOS: `sudo pacman -U ./wallweave-0.2.6-1-x86_64.pkg.tar.zst`
- AppImage: download the new file, then `chmod +x Wallweave-0.2.6-x86_64.AppImage`.
- User installation: extract the new portable archive and run `python3 install.py` again.

Reopen Wallweave and enable the new option if desired. Settings, favorites, wallpaper assignments and the existing auto-pause preference are retained. **The renderer from v0.2.5 remains compatible; no renderer update is needed.** Users upgrading from v0.2.4 or older should also install the cooperative renderer for ordinary pause/resume support, as described in the installation guide.

[Deutsche Update-Anleitung](https://github.com/MGCrafter/wallweave/blob/v0.2.6/docs/INSTALL.de.md#aktualisieren-und-entfernen) · [English installation guide](https://github.com/MGCrafter/wallweave/blob/v0.2.6/docs/INSTALL.md#update)

### Validation

- 80 Rust unit/workflow tests cover detection, actual child-process termination, restoration, properties, manual pause, stop cancellation and failed-restore retries. Formatting, Clippy and the CLI-only build check passed locally; CI repeats them before publication.
- A live Hyprland check used three actual desktop wallpapers and two simulated Steam game processes. All three renderers exited, GPU memory usage fell by approximately 2.5 GiB in that session, and the wallpapers returned after both test processes ended. Saved configuration remained unchanged. This is a lifecycle/memory check, not a game FPS benchmark.
- Release packages are built on Ubuntu 22.04. Packaging checks verify an upgrade from the published v0.2.5 archive, saved preferences, AppImage startup and supervisor lifetime. The Arch package is installed and removed in a clean container before publication.

See [validation details](https://github.com/MGCrafter/wallweave/blob/v0.2.6/docs/VALIDATION.md).
