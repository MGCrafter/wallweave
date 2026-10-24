#!/usr/bin/env python3
"""Build Wallweave and atomically install its binary and Steam runtime in bin/."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def install(source: Path, destination: Path) -> None:
    fd, name = tempfile.mkstemp(prefix=".wallweave-build-", dir=destination.parent)
    os.close(fd)
    temporary = Path(name)
    try:
        shutil.copy2(source, temporary)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--debug", action="store_true", help="Use the development profile")
    args = parser.parse_args()
    project = Path(__file__).resolve().parent.parent
    command = ["cargo", "build", "--locked", "--bin", "wallweave", "--message-format=json"]
    if not args.debug:
        command.append("--release")
    binary = None
    runtime = None
    process = subprocess.Popen(command, cwd=project, stdout=subprocess.PIPE, text=True)
    assert process.stdout is not None
    for line in process.stdout:
        event = json.loads(line)
        if event.get("reason") == "compiler-message":
            print(event["message"].get("rendered", ""), file=sys.stderr, end="")
        if event.get("reason") == "build-script-executed" and event["package_id"].split("#")[-1].startswith("steamworks-sys@"):
            runtime = Path(event["out_dir"]) / "libsteam_api.so"
        if event.get("reason") == "compiler-artifact" and event.get("executable") and event["target"]["name"] == "wallweave":
            binary = Path(event["executable"])
    if process.wait() != 0:
        raise SystemExit("Build failed; the installed application was not changed.")
    if binary is None or runtime is None or not runtime.is_file():
        raise SystemExit("Build did not supply the executable and Steamworks runtime.")
    destination = project / "bin"
    destination.mkdir(exist_ok=True)
    install(runtime, destination / "libsteam_api.so")
    install(binary, destination / "wallweave")
    shutil.copy2(project / "docs" / "THIRD_PARTY.md", destination / "THIRD_PARTY.md")
    print(f"Installed Wallweave with its Steamworks runtime in {destination}")


if __name__ == "__main__":
    main()
