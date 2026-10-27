#!/usr/bin/env python3
"""Install/update Wallweave's pinned portable renderer with cooperative pause."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

VERSION = "v0.9.4-wallweave-pause1"
ASSET = "wallweave-renderer-v0.9.4-pause1-linux-x86_64.tar.gz"
DIGEST = "ed19a3f83cdbca5a28de47783b358c25fc6d331930d89cd808a4662450cd8140"
URL = f"https://github.com/MGCrafter/wallweave/releases/download/v0.2.6/{ASSET}"


def check_renderer(launcher: Path):
    result = subprocess.run([str(launcher), "--help"], capture_output=True, text=True, timeout=15, check=True)
    if "--pause-control-file" not in result.stdout:
        raise RuntimeError("Renderer lacks cooperative pause support; the current renderer was kept")


def select(root: Path, launcher: Path):
    pointer = root / "renderer-path.txt.tmp"
    pointer.write_text(str(launcher.relative_to(root)) + "\n")
    pointer.replace(root / "renderer-path.txt")


def extract_data(tar: tarfile.TarFile, destination: Path):
    """Data-only extraction, including Python 3.10/3.11 without tarfile filters."""
    destination = destination.resolve()
    for member in tar:
        path = Path(member.name)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError(f"Unsafe archive path: {member.name}")
        target = (destination / path).resolve()
        if not target.is_relative_to(destination):
            raise ValueError(f"Archive path leaves destination: {member.name}")
        if member.issym():
            link = Path(member.linkname)
            parent = (destination / path).parent.resolve()
            if link.is_absolute() or not (parent / link).resolve().is_relative_to(destination):
                raise ValueError(f"Unsafe archive link: {member.name}")
        elif not (member.isfile() or member.isdir()):
            raise ValueError(f"Unsupported archive entry: {member.name}")
        member.mode &= 0o755  # No setuid/setgid or group/other write bits.
        member.uid, member.gid = os.getuid(), os.getgid()
        member.uname = member.gname = None
        if hasattr(tarfile, "data_filter"):
            tar.extract(member, destination, filter="data", numeric_owner=True)
        else:
            tar.extract(member, destination, numeric_owner=True)


def install(root: Path, archive_path: Path | None = None) -> Path:
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
                check_renderer(launcher)
                select(root, launcher)
                print(f"Already installed: {launcher}")
                return launcher
        raise RuntimeError(f"Destination already exists without a matching installation manifest: {target}")
    with tempfile.TemporaryDirectory(prefix="wallweave-renderer-", dir=root) as scratch:
        scratch = Path(scratch)
        archive = scratch / "renderer.tar.gz"
        print(f"Installing {VERSION} portable renderer. No wallpaper will be started.", flush=True)
        request = urllib.request.Request(URL, headers={"User-Agent": "Wallweave/0.1"})
        digest = hashlib.sha256()
        size = 0
        with (archive_path.open("rb") if archive_path else urllib.request.urlopen(request, timeout=90)) as response, archive.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > 750 * 1024 * 1024:
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
            extract_data(tar, extracted)
        launchers = [p for p in extracted.rglob("linux-wallpaperengine") if p.is_file() and os.access(p, os.X_OK)]
        if len(launchers) != 1:
            raise RuntimeError(f"Expected exactly one renderer launcher, found {len(launchers)}")
        check_renderer(launchers[0])
        relative = launchers[0].relative_to(extracted)
        (extracted / "wallweave-install.json").write_text(json.dumps({"version": VERSION, "sha256": DIGEST, "url": URL, "launcher": str(relative)}, indent=2) + "\n")
        shutil.move(extracted, target)
        launcher = (target / relative).resolve()
        select(root, launcher)
        print(f"Installed: {launcher}")
        print("Select this executable in Wallweave, or use --configure with your Wallweave executable.")
        return launcher


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parents[1] / "runtime")
    parser.add_argument("--archive", type=Path, help="Use an already downloaded archive (the pinned checksum is still required)")
    parser.add_argument("--configure", help="Set the renderer in this Wallweave executable, e.g. wallweave or ./Wallweave-0.2.6-x86_64.AppImage")
    args = parser.parse_args()
    try:
        launcher = install(args.directory.resolve(), args.archive)
        if args.configure:
            subprocess.run([args.configure, "config", "renderer", str(launcher)], check=True)
            print("Renderer configured. Reopen Wallweave to restore your saved wallpapers.")
    except Exception as error:
        print(f"Renderer installation failed: {error}", file=sys.stderr)
        sys.exit(1)
