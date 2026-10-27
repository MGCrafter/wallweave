# External components and research references

Wallweave's code is original MIT-licensed Rust code. Its native UI uses egui/eframe. Dependency versions are recorded in Cargo.lock; dependencies retain their licenses.

Steam integration uses [steamworks-rs 0.13.1](https://github.com/Noxime/steamworks-rs), MIT/Apache-2.0, and its `steamworks-sys 0.13.0` dependency. The latter supplies Valve's Steamworks SDK 1.64 runtime. `scripts/build-app.py` copies the unmodified Linux `libsteam_api.so` from that dependency's build output beside the installed executable. Valve's SDK/runtime remains Valve's component under its upstream terms; Wallweave's MIT license does not relicense it. Workshop operations require the user's own running Steam session and Wallpaper Engine entitlement. No Steam credentials, Wallpaper Engine installation files or Workshop content are bundled.

Workshop preview HTTPS requests use `ureq` and Rustls. Source provenance and exact dependency versions are in Cargo.lock.

GUI builds embed the unmodified Noto Sans CJK Regular font collection (face 2, Noto Sans CJK SC), copyright 2014–2021 Adobe. It retains the SIL Open Font License 1.1. The original license, package provenance and SHA-256 are included in [assets/fonts](../assets/fonts/README.md). The font is part of the source archive so GUI builds do not depend on system font installation or runtime downloads.

The redesigned interface also embeds IBM Plex Sans Regular/SemiBold (copyright IBM Corp., SIL Open Font License 1.1) and the Lucide icon font (ISC, with the included MIT notice for Feather-derived icons). Their licenses and file digests are in [assets/fonts](../assets/fonts/README.md); application builds and Linux packages include the license notices.

The separately downloaded renderer is not part of Wallweave's source repository:

- Source: https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde
- Pinned release: https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde/releases/tag/v0.9.4
- Asset: `linux-wallpaperengine-kde-portable-x86_64.tar.gz`
- Archive SHA-256: `f6328cc6b106aa6ee66eb3f044eb09af47a9b154fe7c78c44209a4295afe07a3`
- Engine license: GPL-3.0; third-party components keep their upstream terms.
- Upstream basis: https://github.com/Almamu/linux-wallpaperengine

The installer records provenance in `runtime/v0.9.4/wallweave-install.json`. Do not upload `runtime/` as part of this app's source. The project's `.gitignore` excludes runtime and build artifacts.

The optional cooperative-pause build applies `patches/renderer-cooperative-pause.patch` to upstream commit `a239861367cb8d338ef8ba346f937067c3a266d8`. This renderer modification retains GPL-3.0; Wallweave's MIT license does not relicense the engine. `scripts/build-pause-renderer.py` records the patch digest and includes the modified engine/submodule sources, patch and engine license with the local runtime. CEF and optional copied Node/V8 runtime libraries retain their original component terms. The source archive for Wallweave includes the patch and build script, not these renderer binaries.

Other architectural references, not incorporated as code:

- https://github.com/Aromatic05/we-layerd
- https://github.com/Aromatic05/wallpaper-engine-renderer
- https://github.com/waywallen/waywallen
- https://github.com/waywallen/open-wallpaper-engine
- https://github.com/waywallen/waywallen-display

Wallpaper Engine API documentation:

- https://docs.wallpaperengine.io/en/web/overview.html
- https://docs.wallpaperengine.io/en/web/api/propertylistener.html
- https://docs.wallpaperengine.io/en/web/audio/visualizer.html
- https://docs.wallpaperengine.io/en/scene/scenescript/reference.html

Shared Wallpaper Engine assets and downloaded Workshop content are supplied by the user's own local installation. They are not included in Wallweave, its source archive or the local test fixture.
