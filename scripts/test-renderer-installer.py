#!/usr/bin/env python3
"""Regression tests for safe renderer upgrades using private executable fixtures."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("installer", Path(__file__).with_name("install-renderer.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "renderer with spaces"
        self.runtime.mkdir()
        (self.runtime / "old-renderer").write_text("keep")
        (self.runtime / "renderer-path.txt").write_text("old-renderer\n")
        self.archive = self.root / "fixture.tar.gz"

    def fixture(self, help_text="--pause-control-file", unsafe=False):
        with tarfile.open(self.archive, "w:gz") as tar:
            content = f"#!/bin/sh\nprintf '%s\\n' '{help_text}'\n".encode()
            entry = tarfile.TarInfo("fixture/linux-wallpaperengine")
            entry.mode, entry.size = 0o755, len(content)
            tar.addfile(entry, io.BytesIO(content))
            if unsafe:
                link = tarfile.TarInfo("fixture/escape")
                link.type, link.linkname = tarfile.SYMTYPE, "/tmp"
                tar.addfile(link)
        installer.DIGEST = hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def kept_previous(self):
        self.assertEqual((self.runtime / "renderer-path.txt").read_text(), "old-renderer\n")
        self.assertEqual((self.runtime / "old-renderer").read_text(), "keep")
        self.assertFalse((self.runtime / installer.VERSION).exists())

    def test_upgrade_is_atomic_keeps_old_runtime_and_is_repeatable(self):
        self.fixture()
        path = installer.install(self.runtime, self.archive)
        self.assertTrue(path.is_file())
        self.assertEqual((self.runtime / "old-renderer").read_text(), "keep")
        self.assertEqual((self.runtime / "renderer-path.txt").read_text().strip(), str(path.relative_to(self.runtime)))
        self.assertEqual(installer.install(self.runtime, self.archive), path)

    def test_checksum_mismatch_cannot_change_selected_renderer(self):
        self.fixture()
        installer.DIGEST = "0" * 64
        with self.assertRaisesRegex(RuntimeError, "SHA-256"):
            installer.install(self.runtime, self.archive)
        self.kept_previous()

    def test_unsupported_renderer_cannot_replace_working_one(self):
        self.fixture("--screen-root")
        with self.assertRaisesRegex(RuntimeError, "cooperative pause"):
            installer.install(self.runtime, self.archive)
        self.kept_previous()

    def test_escaping_archive_cannot_change_selected_renderer(self):
        self.fixture(unsafe=True)
        with self.assertRaises(ValueError):
            installer.install(self.runtime, self.archive)
        self.kept_previous()


if __name__ == "__main__":
    unittest.main()
