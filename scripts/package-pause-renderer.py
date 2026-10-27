#!/usr/bin/env python3
"""Package the patched renderer on Debian 12, including licenses and matching dependency sources."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
NAME = "wallweave-renderer-v0.9.4-pause1-linux-x86_64"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist/renderer-release")
    args = parser.parse_args()
    source, build, output = args.source.resolve(), args.build.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    run(source / "tools/package_release.sh", "--portable", build, NAME, output)
    archive = output / f"{NAME}.tar.gz"
    with tempfile.TemporaryDirectory(prefix="renderer-package-", dir=output) as temporary:
        temporary = Path(temporary)
        # This archive was just produced by the pinned upstream packaging tool.
        run("tar", "-xzf", archive, "-C", temporary)
        package = temporary / NAME
        licenses = package / "licenses"
        licenses.mkdir()
        shutil.copy2(source / "LICENSE", licenses / "renderer-GPL-3.0.txt")
        cef = next((build / "cef").glob("cef_binary_*/LICENSE.txt")).parent
        for name in ("LICENSE.txt", "CREDITS.html", "README.txt"):
            shutil.copy2(cef / name, licenses / f"CEF-{name}")
        # Map every bundled Debian library back to its exact source package.
        providers = {}
        for lib in (package / "lib").rglob("*.so*"):
            if lib.name.startswith(("libkissfft", "liblwe-glibc")):
                continue  # Built from the included engine/submodule sources.
            match = subprocess.check_output(["dpkg-query", "-S", f"*/{lib.name}"], text=True).splitlines()[0]
            binary = match.split(": ", 1)[0]
            origin = subprocess.check_output(["dpkg-query", "-W", "-f=${source:Package} ${source:Version}", binary], text=True)
            providers[binary] = origin.split()
        sources = temporary / "sources"
        sources.mkdir()
        deps = sources / "debian-sources"
        deps.mkdir()
        for binary, origin in sorted(providers.items()):
            notice = Path("/usr/share/doc") / binary.split(":", 1)[0] / "copyright"
            shutil.copy2(notice, licenses / f"{binary.replace(':', '-')}-copyright")
        for name, version in sorted({tuple(value) for value in providers.values()}):
            run("apt-get", "source", "--download-only", f"{name}={version}", cwd=deps)
        (licenses / "debian-packages.json").write_text(json.dumps(providers, indent=2) + "\n")
        shutil.copytree(source, sources / "engine", ignore=shutil.ignore_patterns(".git"))
        shutil.copytree(ROOT / "patches", sources / "wallweave/patches")
        (sources / "wallweave/scripts").mkdir()
        for name in ("build-pause-renderer.py", "package-pause-renderer.py"):
            shutil.copy2(ROOT / "scripts" / name, sources / "wallweave/scripts" / name)
        shutil.copytree(ROOT / "tests", sources / "wallweave/tests", ignore=shutil.ignore_patterns("*.rs"))
        (sources / "wallweave/packaging").mkdir()
        shutil.copy2(ROOT / "packaging/renderer.Dockerfile", sources / "wallweave/packaging/renderer.Dockerfile")
        shutil.copytree(licenses, sources / "licenses")
        provenance = {
            "source": "https://github.com/WallpaperEngineLover/linux-wallpaperengine-kde",
            "revision": subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip(),
            "patch_sha256": hashlib.sha256((ROOT / "patches/renderer-cooperative-pause.patch").read_bytes()).hexdigest(),
            "build_os": Path("/etc/os-release").read_text(),
            "glibc_floor": (package / "lib/GLIBC_VERSION").read_text().strip(),
            "debian_packages": providers,
        }
        (package / "PROVENANCE.json").write_text(json.dumps(provenance, indent=2) + "\n")
        shutil.copy2(package / "PROVENANCE.json", sources / "PROVENANCE.json")
        (sources / "BUILD.txt").write_text(
            "Build packaging/renderer.Dockerfile in wallweave/ on Debian 12.\n"
            "The engine tree is already patched; git metadata was omitted from this archive.\n"
            "For the source archive, configure directly with cmake -S engine -B build -G Ninja "
            "-DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF; cmake --build build --parallel 4.\n"
            "engine/tools/package_release.sh --portable build PACKAGE OUTPUT creates the relocatable runtime.\n"
            "debian-sources/ contains exact matching source archives and Debian packaging for bundled libraries.\n"
            "Extract .dsc files with dpkg-source -x and follow their debian/rules to rebuild.\n"
        )
        (sources / "build-packages.txt").write_text(subprocess.check_output(
            ["dpkg-query", "-W", "-f=${binary:Package} ${Version}\n"], text=True))
        source_archive = output / "wallweave-renderer-v0.9.4-pause1-sources.tar.gz"
        with tarfile.open(source_archive, "w:gz") as tar:
            tar.add(sources, arcname="wallweave-renderer-sources")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(package, arcname=NAME)
    for path in (archive, source_archive):
        print(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}", flush=True)


if __name__ == "__main__":
    main()
