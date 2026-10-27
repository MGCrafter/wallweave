# Building and publishing Linux releases

The release pipeline builds on Ubuntu 22.04 (glibc 2.35) and packages a native Arch Linux package, an AppImage and a portable archive for x86_64.

## Automated release

1. Build/test the portable cooperative renderer as below. Pin its SHA-256 and target release URL in `scripts/install-renderer.py`.
   An unchanged runtime may be reused with its matching source archive after checking both against the previously published `SHA256SUMS`; do not rebuild or relabel its bytes.
2. Update `Cargo.toml`, `Cargo.lock`, versioned download examples and `docs/RELEASE.md`.
3. Run the normal Rust and installer checks, review and commit the changes.
4. Create a **draft** `v<package-version>` release with the renderer binary and matching source archives attached. Keep it unpublished until the app/package checks finish.
5. Push an annotated `v<package-version>` tag at the reviewed commit, for example `v0.2.6`.

`.github/workflows/release.yml` verifies the tag, runs the tests and builds the app with its Steamworks runtime. It downloads a pinned, hash-verified linuxdeploy, generates the AppImage, archive, renderer installer, `PKGBUILD` and checksums, and runs package smoke tests including an upgrade from the published v0.2.5 archive. A second job uses the official Arch Linux base-devel container to build a `.pkg.tar.zst`, install it with pacman, check the installed CLI and remove it again. The publishing job requires an existing draft, verifies the attached renderer against the installer's pinned digest and requires its matching source archive. Only after both jobs pass does it upload the app files, calculate combined checksums and publish the draft. An already published release is never overwritten by the workflow.

`workflow_dispatch` builds and tests artifacts without publishing a release. Rust CI runs for pushes and pull requests as well.

## Portable cooperative renderer

The Debian 12 build uses the pinned engine revision in `scripts/build-pause-renderer.py` and its upstream portable packaging tools. The latter bundle non-system libraries and enforce a glibc 2.35 floor. The packaging script also collects exact matching Debian dependency sources and copyright notices; `deb-src` repositories are enabled in the build image.

```sh
docker build -f packaging/renderer.Dockerfile -t wallweave-renderer-release:debian12 packaging
docker run --rm -v "$PWD:/work" wallweave-renderer-release:debian12 sh -c '
  python3 scripts/build-pause-renderer.py --source runtime/release-build/source --build runtime/release-build/build --jobs 4 &&
  python3 scripts/package-pause-renderer.py --source runtime/release-build/source --build runtime/release-build/build'
```

The outputs in `dist/renderer-release/` are the runtime and corresponding source archives. Check the runtime using upstream `tools/smoke_test_release.sh` in a clean Ubuntu 22.04 environment, then run `scripts/check-pause-renderer.py` in isolation for native pause/resume, lease expiry and independent audio. `packaging/renderer-smoke.Dockerfile` supplies that test environment. Do not ship a locally compiled Arch renderer as the generic Linux release.

For a rebuilt runtime, update the archive/version name rather than silently replacing an already published pinned download. Keep the binary and its corresponding sources together in the draft release.

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
