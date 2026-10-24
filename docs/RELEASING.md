# Building and publishing Linux releases

The release pipeline builds on Ubuntu 22.04 (glibc 2.35) and packages a native Arch Linux package, an AppImage and a portable archive for x86_64.

## Automated release

1. Update `Cargo.toml`, `Cargo.lock`, versioned download examples and `docs/RELEASE.md`.
2. Run the normal Rust checks and review the changes.
3. Push an annotated `v<package-version>` tag, for example `v0.2.4`.

`.github/workflows/release.yml` verifies the tag, runs the tests and builds the app with its Steamworks runtime. It downloads a pinned, hash-verified linuxdeploy, generates the AppImage, archive, renderer installer, `PKGBUILD` and checksums, and runs package smoke tests. A second job uses the official Arch Linux base-devel container to build a `.pkg.tar.zst`, install it with pacman, check the installed CLI and remove it again. Only after both jobs pass is a public GitHub Release published with all assets and combined checksums.

`workflow_dispatch` builds and tests artifacts without publishing a release. Rust CI runs for pushes and pull requests as well.

## Build the portable archive locally

```sh
python3 scripts/build-app.py
python3 scripts/package-linux.py
python3 scripts/test-linux-package.py
```

The resulting binary inherits your build machine's glibc requirements. Use the Ubuntu 22.04 workflow for public compatibility-baseline builds.

To include AppImage, supply the pinned linuxdeploy executable from the workflow:

```sh
python3 scripts/package-linux.py --linuxdeploy dist/tools/linuxdeploy-x86_64.AppImage
python3 scripts/test-linux-package.py
```

On Arch with `base-devel` installed, `cd dist && makepkg` uses the generated `PKGBUILD` and already present, checksum-verified portable archive. Users can also download the release `PKGBUILD` alone and run `makepkg -si`; it fetches the matching archive and verifies its pinned digest. No AUR listing is implied by providing a PKGBUILD.

## Package behavior

- The portable archive includes `run.sh`, a user installer, the renderer installer, documentation, the original demo and third-party license notices. It excludes local configuration, downloaded runtimes and Workshop files.
- The Arch package installs the app in `/usr/lib/wallweave`, its launcher in `/usr/bin`, the renderer installer in `/usr/share/wallweave`, and the standard desktop entry/icon. The renderer remains separate.
- AppImage bundles shared-library dependencies discovered by linuxdeploy. Host graphics drivers remain system-provided. Its custom AppRun caches the executable and libraries on persistent storage so background helpers survive the AppImage mount disappearing.
- Smoke tests use an isolated HOME with spaces in its path, a private supervisor and a fake renderer. They verify checksums, startup, diagnostics, desktop-entry validity, user installation/removal and background playback after AppImage exit.
- Checksums detect file changes; the packages are not cryptographically signed.
