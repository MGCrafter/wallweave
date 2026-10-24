#!/usr/bin/env python3
"""Install or uninstall the portable Linux app for the current user (no sudo)."""
import argparse
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile


def desktop_quote(value: str) -> str:
    # Desktop Entry Exec quoting is different from shell quoting.
    value = value.replace("%", "%%")
    for character in ("\\", '"', "`", "$"):
        value = value.replace(character, "\\" + character)
    return '"' + value + '"'


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uninstall", action="store_true")
    args = parser.parse_args()
    home = Path.home()
    data = Path(os.environ.get("XDG_DATA_HOME", home / ".local/share"))
    if not data.is_absolute():
        parser.error("XDG_DATA_HOME must be an absolute path")
    app = data / "wallweave/app"
    launcher = home / ".local/bin/wallweave"
    desktop = data / "applications/io.wallweave.Wallweave.desktop"
    icon = data / "icons/hicolor/scalable/apps/io.wallweave.Wallweave.svg"
    marker = "# Installed by the Wallweave installer"

    if args.uninstall:
        # Only remove a launcher that belongs to this installer.
        if launcher.is_file() and marker in launcher.read_text():
            launcher.unlink()
        desktop.unlink(missing_ok=True)
        icon.unlink(missing_ok=True)
        if app.is_dir():
            shutil.rmtree(app)
        print("Removed the app and menu entry. Settings, logs and renderers were kept.")
        return

    source = Path(__file__).resolve().parent
    if source.name == "scripts":
        source = source.parent
    if not (source / "bin/wallweave").is_file():
        parser.error("Extract the complete Linux archive first, or run scripts/build-app.py")
    if app.resolve() == source:
        parser.error("Run the installer from the freshly extracted archive")
    if launcher.exists() and (not launcher.is_file() or marker not in launcher.read_text()):
        parser.error(f"An unrelated launcher already exists: {launcher}")

    app.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".wallweave-install-", dir=app.parent) as temporary:
        staged = Path(temporary) / "app"
        staged.mkdir()
        for name in ("bin", "run.sh", "scripts", "packaging", "docs", "licenses", "LICENSE", "README.md", "examples"):
            item = source / name
            if item.is_dir():
                shutil.copytree(item, staged / name, ignore=shutil.ignore_patterns("__pycache__"))
            elif item.is_file():
                shutil.copy2(item, staged / name)
        subprocess.run([str(staged / "bin/wallweave"), "--version"], check=True)
        previous = Path(temporary) / "previous"
        if app.exists():
            app.rename(previous)
        try:
            staged.rename(app)
        except OSError:
            if previous.exists():
                previous.rename(app)
            raise

    for parent in (launcher.parent, desktop.parent, icon.parent):
        parent.mkdir(parents=True, exist_ok=True)
    launcher.write_text(f"#!/bin/sh\n{marker}\nexec {shlex.quote(str(app / 'run.sh'))} \"$@\"\n")
    launcher.chmod(0o755)
    template = (app / "packaging/io.wallweave.Wallweave.desktop").read_text()
    desktop.write_text(template.replace("Exec=wallweave gui", f"Exec={desktop_quote(str(launcher))} gui"))
    shutil.copy2(app / "packaging/io.wallweave.Wallweave.svg", icon)
    if shutil.which("update-desktop-database"):
        subprocess.run(["update-desktop-database", str(desktop.parent)], check=False)
    print(f"Installed: {app}\nOpen Wallweave from your application menu or run {launcher}")
    if str(launcher.parent) not in os.get_exec_path():
        print('For terminal use, add this to your shell profile: export PATH="$HOME/.local/bin:$PATH"')


if __name__ == "__main__":
    main()
