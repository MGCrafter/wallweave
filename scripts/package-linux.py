#!/usr/bin/env python3
"""Package bin/wallweave and its runtime as a Linux archive and optional AppImage."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def copy(source: Path, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        shutil.copytree(source, target, ignore=shutil.ignore_patterns("__pycache__"))
    else:
        shutil.copy2(source, target)


def licenses(destination: Path) -> None:
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version=1", "--filter-platform=x86_64-unknown-linux-gnu"],
        cwd=ROOT, text=True,
    ))
    lines = ["# Dependency licenses", "", "Versions and upstream source are recorded in Cargo.lock.", ""]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["name"] == "wallweave":
            continue
        identifier = f"{package['name']}-{package['version']}"
        lines.append(f"- {identifier}: {package.get('license') or 'See upstream terms'}; {package.get('repository') or package.get('source') or ''}")
        source = Path(package["manifest_path"]).parent
        files = set()
        for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "COPYRIGHT*"):
            files.update(source.glob(pattern))
        if package.get("license_file"):
            files.add(source / package["license_file"])
        for file in sorted(files):
            if file.is_file():
                copy(file, destination / identifier / file.name)
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "README.md").write_text("\n".join(lines) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--linuxdeploy", type=Path, help="Also build an AppImage using this linuxdeploy executable")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("This release packaging targets Linux x86_64")
    version = re.search(r'^version = "([^\"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)[1]
    binary = ROOT / "bin/wallweave"
    runtime = ROOT / "bin/libsteam_api.so"
    actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if actual != f"wallweave {version}" or not runtime.is_file():
        parser.error("Rebuild the app and runtime with python3 scripts/build-app.py")
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    stem = f"wallweave-{version}-linux-x86_64"
    assets = []
    with tempfile.TemporaryDirectory(prefix="wallweave-package-") as temporary:
        temporary = Path(temporary)
        portable = temporary / stem
        portable.mkdir()
        for name in ("bin/wallweave", "bin/libsteam_api.so", "run.sh", "LICENSE", "README.md", "docs", "examples", "packaging"):
            copy(ROOT / name, portable / name)
        for name in ("install-renderer.py", "install-app.py", "build-pause-renderer.py", "check-pause-renderer.py"):
            copy(ROOT / "scripts" / name, portable / "scripts" / name)
        copy(ROOT / "scripts/install-app.py", portable / "install.py")
        copy(ROOT / "patches/renderer-cooperative-pause.patch", portable / "patches/renderer-cooperative-pause.patch")
        copy(ROOT / "tests/native_pause_control.cpp", portable / "tests/native_pause_control.cpp")
        for name in ("OFL.txt", "IBM-Plex-OFL.txt", "Lucide-LICENSE.txt"):
            copy(ROOT / "assets/fonts" / name, portable / "licenses/fonts" / name)
        licenses(portable / "licenses/dependencies")
        archive = output / f"{stem}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(portable, arcname=stem)
        assets.append(archive)

        if args.linuxdeploy:
            appdir = temporary / "Wallweave.AppDir"
            for name in ("bin", "run.sh", "scripts", "patches", "tests", "docs", "licenses", "LICENSE", "README.md", "examples"):
                copy(portable / name, appdir / "usr" / name)
            copy(ROOT / "packaging/io.wallweave.Wallweave.desktop", appdir / "usr/share/applications/io.wallweave.Wallweave.desktop")
            copy(ROOT / "packaging/io.wallweave.Wallweave.svg", appdir / "usr/share/icons/hicolor/scalable/apps/io.wallweave.Wallweave.svg")
            # Include the full bundle digest: dependencies can change without a
            # source-version bump, and must never reuse a stale extracted cache.
            digest = hashlib.sha256()
            for file in sorted((appdir / "usr").rglob("*")):
                if file.is_file():
                    digest.update(str(file.relative_to(appdir)).encode())
                    digest.update(file.read_bytes())
            (appdir / "wallweave-build-id").write_text(f"{version}-{digest.hexdigest()[:16]}\n")
            image = output / f"Wallweave-{version}-x86_64.AppImage"
            env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="1", ARCH="x86_64", VERSION=version, OUTPUT=str(image), LDAI_OUTPUT=str(image), LDAI_NO_APPSTREAM="1")
            subprocess.run([
                str(args.linuxdeploy.resolve()), "--appdir", str(appdir),
                "--executable", str(appdir / "usr/bin/wallweave"),
                "--library", str(appdir / "usr/bin/libsteam_api.so"),
                "--custom-apprun", str(ROOT / "packaging/AppRun"), "--output", "appimage",
            ], env=env, check=True, cwd=output)
            assets.append(image)

    installer = output / "install-renderer.py"
    shutil.copy2(ROOT / "scripts/install-renderer.py", installer)
    assets.append(installer)
    pkgbuild = output / "PKGBUILD"
    template = (ROOT / "packaging/arch/PKGBUILD.in").read_text()
    pkgbuild.write_text(template.replace("@VERSION@", version).replace("@ARCHIVE_SHA256@", hashlib.sha256(archive.read_bytes()).hexdigest()))
    assets.append(pkgbuild)
    checksums = output / "SHA256SUMS"
    checksums.write_text("".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in assets))
    print("Release assets:\n" + "\n".join(str(path) for path in [*assets, checksums]))


if __name__ == "__main__":
    main()
