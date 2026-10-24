#!/usr/bin/env python3
"""Install a pinned, hash-verified portable renderer inside this checkout only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import sys
import tarfile
import tempfile
import urllib.request

VERSION = "v0.9.4"
ASSET = "linux-wallpaperengine-kde-portable-x86_64.tar.gz"
DIGEST = "f6328cc6b106aa6ee66eb3f044eb09af47a9b154fe7c78c44209a4295afe07a3"
URL = f"https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde/releases/download/{VERSION}/{ASSET}"


def install(root: Path):
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "AMD64"):
        raise RuntimeError("This pinned portable renderer is for Linux x86_64. Configure a suitable local renderer on other architectures.")
    root.mkdir(parents=True, exist_ok=True)
    target = root / VERSION
    manifest = target / "wallweave-install.json"
    if target.exists():
        if manifest.is_file():
            data = json.loads(manifest.read_text())
            launcher = target / data["launcher"]
            if data.get("sha256") == DIGEST and launcher.is_file():
                (root / "renderer-path.txt").write_text(str(launcher.relative_to(root)) + "\n")
                print(f"Already installed: {launcher}")
                return
        raise RuntimeError(f"Destination already exists without a matching installation manifest: {target}")
    with tempfile.TemporaryDirectory(prefix="wallweave-renderer-", dir=root) as scratch:
        scratch = Path(scratch)
        archive = scratch / "renderer.tar.gz"
        print(f"Downloading {VERSION} portable renderer (~267 MB). No wallpaper will be started.", flush=True)
        request = urllib.request.Request(URL, headers={"User-Agent": "Wallweave/0.1"})
        digest = hashlib.sha256()
        size = 0
        with urllib.request.urlopen(request, timeout=90) as response, archive.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > 400 * 1024 * 1024:
                    raise RuntimeError("Renderer archive exceeded the expected size limit")
                digest.update(chunk)
                output.write(chunk)
                if size % (32 * 1024 * 1024) < 1024 * 1024:
                    print(f"  {size // (1024 * 1024)} MiB", flush=True)
        if digest.hexdigest() != DIGEST:
            raise RuntimeError("SHA-256 mismatch; downloaded renderer was not installed")
        extracted = scratch / "unpacked"
        extracted.mkdir()
        with tarfile.open(archive, "r:gz") as tar:
            # Python's data filter rejects absolute/traversal paths and escaping links.
            tar.extractall(extracted, filter="data")
        launchers = [p for p in extracted.rglob("linux-wallpaperengine") if p.is_file() and os.access(p, os.X_OK)]
        if len(launchers) != 1:
            raise RuntimeError(f"Expected exactly one renderer launcher, found {len(launchers)}")
        relative = launchers[0].relative_to(extracted)
        (extracted / "wallweave-install.json").write_text(json.dumps({"version": VERSION, "sha256": DIGEST, "url": URL, "launcher": str(relative)}, indent=2) + "\n")
        shutil.move(extracted, target)
        launcher = (target / relative).resolve()
        pointer = root / "renderer-path.txt.tmp"
        pointer.write_text(str(launcher.relative_to(root)) + "\n")
        pointer.replace(root / "renderer-path.txt")
        print(f"Installed: {launcher}")
        print("Wallweave will prefer this renderer unless an explicit executable is configured.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parents[1] / "runtime")
    args = parser.parse_args()
    try:
        install(args.directory.resolve())
    except Exception as error:
        print(f"Renderer installation failed: {error}", file=sys.stderr)
        sys.exit(1)
