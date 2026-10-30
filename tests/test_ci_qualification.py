"""Offline CI provenance/package regressions; no processes, windows or network."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import check_release
import ci_qualification as ci

SHA = "a" * 40


def run(**changes):
    value = dict(id=42, head_sha=SHA, head_branch="main", event="push",
                 path=".github/workflows/ci.yml", conclusion="success", status="completed")
    value.update(changes)
    return value


class Fake:
    def __init__(self, runs=None, jobs=None, expired=False):
        self.runs = [run()] if runs is None else runs
        self.jobs = jobs if jobs is not None else [dict(name=n, conclusion="success") for n in ci.REQUIRED]
        self.expired = expired

    def get(self, path):
        if path.startswith("workflows/"):
            return {"workflow_runs": self.runs}
        if "/jobs?" in path:
            return {"jobs": self.jobs}
        if "/artifacts?" in path:
            return {"artifacts": [dict(name=ci.ARTIFACT, expired=self.expired)]}
        if path == "runs/42":
            return self.runs[0]
        raise AssertionError(path)


class Clock:
    def __init__(self):
        self.now = 0

    def read(self):
        return self.now

    def pause(self, duration):
        self.now += duration


class Qualification(unittest.TestCase):
    def resolve(self, api, seconds=30):
        clock = Clock()
        value = ci.resolve(api, SHA, seconds, clock.read, clock.pause)
        return value, clock.now

    def test_reuses_only_exact_successful_main_commit(self):
        self.assertEqual(self.resolve(Fake()), (42, 0))
        for changes in [dict(head_sha="b" * 40), dict(event="pull_request"),
                        dict(head_branch="v0.1.52"), dict(path="other.yml"),
                        dict(conclusion="failure")]:
            self.assertFalse(ci.qualified(Fake(), run(**changes), SHA), changes)

    def test_missing_failed_matrix_or_expired_artifact_cannot_qualify(self):
        self.assertFalse(ci.qualified(Fake(jobs=[]), run(), SHA))
        jobs = [dict(name=n, conclusion="failure" if "windows" in n else "success") for n in ci.REQUIRED]
        self.assertFalse(ci.qualified(Fake(jobs=jobs), run(), SHA))
        self.assertFalse(ci.qualified(Fake(expired=True), run(), SHA))

    def test_pending_main_waits_then_reuses_without_real_sleep(self):
        api = Fake([run(status="in_progress", conclusion=None)])
        clock = Clock()
        def complete(duration):
            clock.pause(duration)
            api.runs = [run()]
        self.assertEqual(ci.resolve(api, SHA, 30, clock.read, complete), 42)
        self.assertEqual(clock.now, 10)

    def test_discovery_grace_covers_main_tag_push_race(self):
        api, clock = Fake([]), Clock()
        def appear(duration):
            clock.pause(duration)
            api.runs = [run()]
        self.assertEqual(ci.resolve(api, SHA, 30, clock.read, appear), 42)

    def test_missing_or_stuck_main_falls_back_at_deadline(self):
        for api in [Fake([]), Fake([run(status="queued", conclusion=None)])]:
            self.assertEqual(self.resolve(api), (None, 30))
        self.assertEqual(self.resolve(Fake([run(conclusion="failure")])), (None, 0))

    def test_reuse_rechecks_provenance_before_reading_archive(self):
        with patch.object(check_release, "inspect_package") as inspect:
            with self.assertRaises(AssertionError):
                ci.verify(Fake([run(head_sha="b" * 40)]), 42, SHA, Path("unused"))
            inspect.assert_not_called()


class Packages(unittest.TestCase):
    def test_reused_package_integrity_and_independent_build_guard(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            for source in ["LICENSE", "docs/USER-GUIDE.md", "docs/PORTABLE-MANIFESTO.md"]:
                (root / source).write_bytes(b"source\r\n")
            archive = root / "Constellation-1.0-windows-x64.zip"
            executable = root / "qualified.exe"
            executable.write_bytes(b"qualified executable")
            def package(extra=False):
                with zipfile.ZipFile(archive, "w") as z:
                    for name in check_release.FILES:
                        data = b"qualified executable" if name == "constellation.exe" else b"source\n"
                        z.writestr("Constellation/" + name, data)
                    if extra:
                        z.writestr("Constellation/data/config.json", "{}")
                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n", encoding="ascii")
                return digest
            with patch.object(check_release, "ROOT", root):
                digest = package()
                self.assertEqual(check_release.inspect_package(archive, "1.0", executable), digest)
                executable.write_bytes(b"different executable")
                with self.assertRaisesRegex(AssertionError, "qualified build"):
                    check_release.inspect_package(archive, "1.0", executable)
                self.assertEqual(check_release.inspect_package(archive, "1.0"), digest)
                package(extra=True)
                with self.assertRaisesRegex(AssertionError, "Unexpected package"):
                    check_release.inspect_package(archive, "1.0")
                package()
                archive.with_suffix(".zip.sha256").write_text("wrong checksum", encoding="ascii")
                with self.assertRaisesRegex(AssertionError, "Checksum"):
                    check_release.inspect_package(archive, "1.0")


if __name__ == "__main__":
    unittest.main()
