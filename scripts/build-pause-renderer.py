#!/usr/bin/env python3
"""Build a pinned renderer with cooperative pause; never enables auto-pause."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = "https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde.git"
REVISION = "a239861367cb8d338ef8ba346f937067c3a266d8"
NAME = "v0.9.4-wallweave-pause1"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT / "runtime/pause-build/source")
    parser.add_argument("--build", type=Path, default=ROOT / "runtime/pause-build/build")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--v8-include-dir", type=Path)
    parser.add_argument("--v8-library", type=Path)
    parser.add_argument("--install", action="store_true", help="Install locally and select this renderer after successful build checks")
    args = parser.parse_args()
    source, build = args.source.resolve(), args.build.resolve()
    patch = ROOT / "patches/renderer-cooperative-pause.patch"
    if not source.exists():
        source.parent.mkdir(parents=True, exist_ok=True)
        run("git", "clone", "--depth=1", "--branch=v0.9.4", "--recurse-submodules", "--shallow-submodules", UPSTREAM, source)
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if revision != REVISION:
        raise SystemExit(f"Expected upstream {REVISION}; source is {revision}")
    applied = subprocess.run(["git", "-C", str(source), "apply", "--reverse", "--check", str(patch)], capture_output=True)
    if applied.returncode:
        run("git", "-C", source, "apply", "--check", patch)
        run("git", "-C", source, "apply", patch)
    options = []
    if args.v8_include_dir:
        options.append(f"-DV8_INCLUDE_DIR={args.v8_include_dir.resolve()}")
    if args.v8_library:
        options.append(f"-DV8_LIBRARY={args.v8_library.resolve()}")
    run("cmake", "-S", source, "-B", build, "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_POLICY_VERSION_MINIMUM=3.5", "-DBUILD_TESTING=OFF", *options)
    run("cmake", "--build", build, "--parallel", args.jobs)
    test = build / "wallweave-pause-test"
    run("c++", "-std=c++20", "-Wall", "-Wextra", "-Werror", "-I", source / "src", ROOT / "tests/native_pause_control.cpp", "-o", test)
    run(test)
    if not args.install:
        print(f"Built and tested in {build}. Pass --install to select it; auto-pause stays unchanged.")
        return
    runtime = ROOT / "runtime"
    target = runtime / NAME
    if target.exists():
        raise SystemExit(f"Refusing to overwrite {target}; choose the existing renderer or move it aside first.")
    runtime.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".pause-install-", dir=runtime) as temporary:
        staged = Path(temporary) / NAME
        run("cmake", "--install", build, "--prefix", staged)
        # A supplied libnode may come from the pinned portable runtime. Copy its
        # non-system dependencies beside it; do not replace system libraries.
        if args.v8_library:
            libraries = {args.v8_library.resolve()}
            listing = subprocess.check_output(["ldd", str(args.v8_library.resolve())], text=True)
            for line in listing.splitlines():
                if "=> /" in line:
                    path = Path(line.split("=>", 1)[1].split(" (", 1)[0].strip()).resolve()
                    if not str(path).startswith(("/usr/", "/lib/", "/lib64/")):
                        libraries.add(path)
            (staged / "lib").mkdir(exist_ok=True)
            for library in libraries:
                shutil.copy2(library, staged / "lib" / library.name)
        help_text = subprocess.check_output([str(staged / "linux-wallpaperengine"), "--help"], text=True)
        if "--pause-control-file" not in help_text:
            raise SystemExit("Renderer capability check failed; installed renderer is unchanged.")
        shutil.copy2(patch, staged / patch.name)
        for license_file in source.glob("LICENSE*"):
            if license_file.is_file():
                shutil.copy2(license_file, staged / license_file.name)
        # Keep the patched engine and submodule sources with this local build.
        run("tar", "-czf", staged / "renderer-source.tar.gz", "--exclude=.git", "-C", source, ".")
        (staged / "wallweave-install.json").write_text(json.dumps({
            "version": NAME, "source": UPSTREAM, "revision": REVISION,
            "patch_sha256": hashlib.sha256(patch.read_bytes()).hexdigest(),
            "launcher": "linux-wallpaperengine", "local_build": True,
        }, indent=2) + "\n")
        staged.rename(target)
    with tempfile.NamedTemporaryFile(mode="w", dir=runtime, delete=False) as pointer:
        pointer.write(f"{NAME}/linux-wallpaperengine\n")
    Path(pointer.name).replace(runtime / "renderer-path.txt")
    print(f"Installed {target}; existing wallpapers and auto-pause preference are unchanged.")


if __name__ == "__main__":
    main()
