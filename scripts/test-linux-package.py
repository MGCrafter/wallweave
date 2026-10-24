#!/usr/bin/env python3
"""Smoke-test release files with an isolated home and a fake wallpaper renderer."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], env: dict[str, str]) -> str:
    result = subprocess.run(command, env=env, check=True, capture_output=True, text=True, timeout=45)
    return result.stdout


def main() -> None:
    version = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)[1]
    output = ROOT / "dist"
    for line in (output / "SHA256SUMS").read_text().splitlines():
        expected, name = line.split("  ", 1)
        assert hashlib.sha256((output / name).read_bytes()).hexdigest() == expected, name
    with tempfile.TemporaryDirectory(prefix="wallweave-package-test-") as temporary:
        root = Path(temporary)
        home = root / "home with spaces"
        home.mkdir()
        env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"),
                   XDG_CACHE_HOME=str(home / ".cache"), XDG_CONFIG_HOME=str(home / ".config"),
                   XDG_STATE_HOME=str(home / ".local/state"), WALLWEAVE_HOME=str(home / "test-state"))
        state = Path(env["WALLWEAVE_HOME"])
        state.mkdir()
        (state / "config.toml").write_text("discover_steam = false\n")
        with tarfile.open(output / f"wallweave-{version}-linux-x86_64.tar.gz") as tar:
            tar.extractall(root, filter="data")
        portable = root / f"wallweave-{version}-linux-x86_64"
        launcher = portable / "run.sh"
        assert run([str(launcher), "--version"], env).strip() == f"wallweave {version}"
        json.loads(run([str(launcher), "doctor", "--json"], env))
        assert "Wallweave" in run([str(launcher), "inspect", str(portable / "examples/web-pulse")], env)
        run(["python3", str(portable / "install.py")], env)
        installed = home / ".local/bin/wallweave"
        assert run([str(installed), "--version"], env).strip() == f"wallweave {version}"
        desktop = home / ".local/share/applications/io.wallweave.Wallweave.desktop"
        run(["desktop-file-validate", str(desktop)], env)
        run(["python3", str(portable / "install.py"), "--uninstall"], env)
        assert not installed.exists() and not desktop.exists()
        assert (state / "config.toml").is_file()

        image = output / f"Wallweave-{version}-x86_64.AppImage"
        if image.exists():
            image.chmod(0o755)
            env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
            assert run([str(image), "--version"], env).strip() == f"wallweave {version}"
            assert run([str(image), "--version"], env).strip() == f"wallweave {version}"
            json.loads(run([str(image), "doctor", "--json"], env))
            renderer = root / "fake renderer"
            renderer.write_text('#!/bin/sh\nif [ "${1:-}" = "--help" ]; then\n  echo "--assets-dir --screen-root --window --fps --set-property --scaling"\n  exit 0\nfi\nexec sleep 120\n')
            renderer.chmod(0o755)
            assets = root / "fake assets"
            assets.mkdir()
            run([str(image), "config", "renderer", str(renderer)], env)
            run([str(image), "config", "assets", str(assets)], env)
            try:
                run([str(image), "apply", str(portable / "examples/web-pulse"), "--output", "PACKAGING-TEST"], env)
                # The first AppImage invocation has already exited/unmounted.
                status = json.loads(run([str(image), "status", "--json"], env))
                assert status["ok"] and len(status["states"]) == 1, status
                assert status["states"][0]["output"] == "PACKAGING-TEST", status
                cache = home / ".cache/wallweave/appimage"
                copies = list(cache.glob("*/usr/bin/wallweave"))
                assert len(copies) == 1, copies
                assert (copies[0].parent / "libsteam_api.so").is_file()
            finally:
                run([str(image), "shutdown"], env)
        print("Linux package smoke tests passed: checksums, launch, diagnostics, isolated user install/uninstall and desktop entry.")
        if image.exists():
            print("AppImage launch, cache reuse and supervisor lifetime checks passed.")


if __name__ == "__main__":
    main()
