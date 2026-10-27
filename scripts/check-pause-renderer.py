#!/usr/bin/env python3
"""Exercise real video pause with isolated X11, PipeWire, Pulse and D-Bus servers.

Requires Xvfb, ffmpeg, pipewire, pipewire-pulse, wireplumber and PulseAudio tools.
The real desktop, audio server and user's wallpaper settings are never used.
"""
import argparse
import array
import json
import math
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import threading
import time
import wave


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("renderer", type=Path)
    parser.add_argument("assets", type=Path)
    parser.add_argument("--xvfb", default="Xvfb")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--session", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if not args.session:
        with tempfile.TemporaryDirectory(prefix="wallweave-audio-lab-") as temporary:
            runtime = Path(temporary)
            env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), PIPEWIRE_RUNTIME_DIR=str(runtime),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
                       XDG_DATA_HOME=str(root / "data"), PULSE_SERVER=f"unix:{runtime}/pulse/native",
                       PULSE_RUNTIME_PATH=str(runtime / "pulse"), PIPEWIRE_REMOTE="pipewire-0",
                       XDG_SESSION_TYPE="x11", XDG_CURRENT_DESKTOP="", WAYLAND_DISPLAY="",
                       LIBGL_ALWAYS_SOFTWARE="1", SDL_AUDIODRIVER="pulseaudio")
            for key in ("HYPRLAND_INSTANCE_SIGNATURE", "DBUS_SESSION_BUS_ADDRESS", "PIPEWIRE_CONFIG_DIR", "WIREPLUMBER_CONFIG_DIR", "DISPLAY"):
                env.pop(key, None)
            # No desktop service activation: in particular no document-portal
            # FUSE mount should outlive this disposable bus.
            bus = runtime / "bus.conf"
            bus.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>'
                           '<auth>EXTERNAL</auth><policy context="default">'
                           '<allow send_destination="*" eavesdrop="true"/>'
                           '<allow eavesdrop="true"/><allow own="*"/></policy></busconfig>')
            result = subprocess.run(["dbus-run-session", f"--config-file={bus}", "--", sys.executable, __file__, *sys.argv[1:], "--session"], env=env)
            raise SystemExit(result.returncode)
    assert os.environ["XDG_RUNTIME_DIR"].startswith("/tmp/wallweave-audio-lab-")
    children, files = [], []
    renew = threading.Event()
    closing = threading.Event()
    control = root / "pause-state"

    def command(*cmd):
        return subprocess.check_output([str(c) for c in cmd], text=True, stderr=subprocess.STDOUT, timeout=10)

    def spawn(name, *cmd, **kwargs):
        log = open(root / f"{name}.log", "wb")
        files.append(log)
        child = subprocess.Popen([str(c) for c in cmd], stdout=log, stderr=log, start_new_session=True, **kwargs)
        children.append(child)
        return child

    def wait_for(check, description, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            try:
                if check():
                    return
            except (subprocess.SubprocessError, FileNotFoundError):
                pass
            time.sleep(.05)
        raise AssertionError(description)

    def set_control(paused):
        temporary = control.with_suffix(".tmp")
        temporary.write_text("pause\n" if paused else "play\n")
        temporary.replace(control)

    def heartbeat():
        while not closing.wait(.4):
            if renew.is_set():
                set_control(True)

    try:
        config = root / "config/wireplumber/wireplumber.conf.d"
        config.mkdir(parents=True, exist_ok=True)
        (config / "10-lab.conf").write_text("wireplumber.profiles = { main = { hardware.audio = disabled hardware.bluetooth = disabled hardware.video-capture = disabled } }\n")
        read_fd, write_fd = os.pipe()
        spawn("xvfb", args.xvfb, "-displayfd", write_fd, "-screen", "0", "640x360x24", "-nolisten", "tcp", "-ac", pass_fds=(write_fd,))
        os.close(write_fd)
        assert select.select([read_fd], [], [], 10)[0], "Xvfb did not start"
        os.environ["DISPLAY"] = ":" + os.read(read_fd, 128).decode().strip()
        os.close(read_fd)
        spawn("pipewire", "pipewire")
        spawn("pulse", "pipewire-pulse")
        spawn("wireplumber", "wireplumber")
        wait_for(lambda: "Server Name" in command("pactl", "info"), "Isolated audio server unavailable")
        command("pactl", "load-module", "module-null-sink", "sink_name=wallweave_test", "rate=48000", "channels=2")
        command("pactl", "set-default-sink", "wallweave_test")
        folder = root / "video"
        folder.mkdir(exist_ok=True)
        (folder / "project.json").write_text(json.dumps({"title": "Pause fixture", "type": "video", "file": "test.mp4"}))
        command("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=15", "-f", "lavfi", "-i", "sine=frequency=220:sample_rate=48000", "-t", "4", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", folder / "test.mp4")
        with wave.open(str(root / "tone.wav"), "wb") as wav:
            wav.setparams((1, 2, 48000, 0, "NONE", "none"))
            second = array.array("h", (int(10000 * math.sin(2 * math.pi * 440 * i / 48000)) for i in range(48000)))
            for _ in range(90):
                wav.writeframes(second.tobytes())
        capture = open(root / "capture.f32", "wb")
        files.append(capture)
        recorder = subprocess.Popen(["parec", "--device=wallweave_test.monitor", "--format=float32le", "--rate=48000", "--channels=1"], stdout=capture, stderr=subprocess.DEVNULL, start_new_session=True)
        children.append(recorder)
        spawn("tone", "paplay", "--device=wallweave_test", root / "tone.wav")
        set_control(False)
        renderer = spawn("renderer", args.renderer.resolve(), "--window", "0x0x320x180", "--assets-dir", args.assets.resolve(), "--no-fullscreen-pause", "--pause-control-file", control, "--fps", "15", "--volume", "30", folder)
        log = root / "renderer.log"
        worker = threading.Thread(target=heartbeat)
        worker.start()
        checks = []

        def frame():
            return subprocess.check_output([
                "ffmpeg", "-v", "error", "-f", "x11grab", "-video_size", "640x360",
                "-i", os.environ["DISPLAY"], "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
            ], timeout=10)

        def frames_check(paused):
            first = frame()
            time.sleep(.35)
            second = frame()
            assert (first == second) == paused, "Video must freeze only while paused"

        def audio_check(label):
            start = (root / "capture.f32").stat().st_size
            time.sleep(1.0)
            data = (root / "capture.f32").read_bytes()[start:]
            samples = array.array("f", data[:len(data) // 4 * 4])
            assert len(samples) >= 24000, f"Audio stopped delivering samples during {label}"
            amplitudes = []
            for offset in range(0, len(samples) - 9600 + 1, 9600):
                chunk = samples[offset:offset + 9600]
                real = sum(value * math.cos(2 * math.pi * 440 * i / 48000) for i, value in enumerate(chunk))
                imag = sum(value * math.sin(2 * math.pi * 440 * i / 48000) for i, value in enumerate(chunk))
                amplitudes.append(2 * math.hypot(real, imag) / len(chunk))
            assert amplitudes and min(amplitudes) > .05, (label, amplitudes)
            assert renderer.poll() is None, "Renderer exited"
            assert "T (stopped)" not in Path(f"/proc/{renderer.pid}/status").read_text()
            checks.append({"state": label, "minimum_440hz_amplitude": min(amplitudes), "samples": len(samples)})

        time.sleep(2)
        audio_check("playing")
        for cycle in range(3):
            before = log.read_text(errors="replace").count("Paused: external")
            renew.set()
            wait_for(lambda: log.read_text(errors="replace").count("Paused: external") > before, "Renderer did not acknowledge pause")
            audio_check(f"paused-{cycle}")
            if cycle == 0:
                frames_check(True)
            renew.clear()
            # Avoid a concurrent renewal racing this explicit resume.
            time.sleep(.45)
            before = log.read_text(errors="replace").count("Resumed")
            set_control(False)
            wait_for(lambda: log.read_text(errors="replace").count("Resumed") > before, "Renderer did not resume")
            audio_check(f"resumed-{cycle}")
            if cycle == 0:
                frames_check(False)
        before = log.read_text(errors="replace").count("Paused: external")
        renew.set()
        wait_for(lambda: log.read_text(errors="replace").count("Paused: external") > before, "Lease test pause missing")
        renew.clear()
        before = log.read_text(errors="replace").count("Resumed")
        wait_for(lambda: log.read_text(errors="replace").count("Resumed") > before, "Abandoned pause did not expire", timeout=5)
        audio_check("expired-lease")
        (root / "results.json").write_text(json.dumps(checks, indent=2) + "\n")
        print("PASS: three native pause/resume cycles, lease expiry and uninterrupted independent audio", flush=True)
    finally:
        closing.set()
        if "worker" in locals():
            worker.join()
        for child in reversed(children):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
        for file in files:
            file.close()


if __name__ == "__main__":
    main()
