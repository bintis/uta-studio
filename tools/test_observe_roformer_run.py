#!/usr/bin/env python3
"""CPU-only command fixtures for host observation, timing and failure handling."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("run_observer", Path(__file__).with_name("observe-roformer-run.py"))
observer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observer)


def host_fixture():
    return {
        "monotonic_ns": time.monotonic_ns(), "gpu_devices": [],
        "processes": [], "drm_clients": [], "sys/kernel/random/boot_id": "fixture",
        "stat": "cpu  1 0 0 9 0 0 0 0\n", "read_errors": {},
    }


class RunObserverTests(unittest.TestCase):
    def test_stdout_and_stderr_are_synced_while_target_is_alive(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-observer-") as temp:
            case = Path(temp) / "case"
            alive = Path(temp) / "alive"
            synced = set()
            real_fsync = observer.os.fsync

            def watch_sync(fd):
                real_fsync(fd)
                path = Path(observer.os.readlink(f"/proc/self/fd/{fd}"))
                if alive.exists() and path.name in ("stdout.txt", "stderr.txt"):
                    if "live phase" in path.read_text():
                        synced.add(path.name)

            script = ("import pathlib,sys,time; p=pathlib.Path(sys.argv[1]); "
                      "p.touch(); print('live phase',flush=True); "
                      "print('live phase',file=sys.stderr,flush=True); "
                      "time.sleep(1.15); p.unlink()")
            with patch.object(observer.host_load, "snapshot", side_effect=host_fixture), \
                 patch.object(observer.os, "fsync", side_effect=watch_sync), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = observer.main([str(case), "--", sys.executable, "-c", script, str(alive)])
            self.assertEqual(code, 0)
            self.assertEqual(synced, {"stdout.txt", "stderr.txt"})
            self.assertEqual(json.loads((case / "result.json").read_text())["observer_errors"], [])

    def test_live_sync_failure_is_reported_without_relaunching_target(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-observer-") as temp:
            case = Path(temp) / "case"
            real_fsync = observer.os.fsync
            failed = False

            def fail_one_live_sync(fd):
                nonlocal failed
                path = Path(observer.os.readlink(f"/proc/self/fd/{fd}"))
                if path.name == "stdout.txt" and not failed:
                    failed = True
                    raise OSError("fixture live sync failure")
                return real_fsync(fd)

            with patch.object(observer.host_load, "snapshot", side_effect=host_fixture), \
                 patch.object(observer.os, "fsync", side_effect=fail_one_live_sync), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = observer.main([str(case), "--", sys.executable, "-c",
                                      "import time; print('once',flush=True); time.sleep(0.4)"])
            self.assertEqual(code, 0)
            self.assertEqual((case / "stdout.txt").read_text(), "once\n")
            errors = json.loads((case / "result.json").read_text())["observer_errors"]
            self.assertEqual([error["stage"] for error in errors], ["live-output-sync"])

    def test_records_before_during_after_without_including_prelaunch_in_wall_time(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-observer-") as temp:
            case = Path(temp) / "case"
            with patch.object(observer.host_load, "snapshot", side_effect=host_fixture), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = observer.main([str(case), "--", sys.executable, "-c",
                                      "import time; print('cpu fixture'); time.sleep(1.15)"])
            self.assertEqual(code, 0)
            self.assertEqual((case / "stdout.txt").read_text().strip(), "cpu fixture")
            records = [json.loads(line) for line in (case / "host-samples.ndjson").read_text().splitlines()]
            self.assertEqual(records[0]["phase"], "before-target")
            self.assertEqual(records[-1]["phase"], "after-target")
            self.assertTrue(any(record["phase"] == "during-target" for record in records))
            self.assertNotIn("interval_summary", records[0])
            self.assertIn("interval_summary", records[-1])
            result = json.loads((case / "result.json").read_text())
            self.assertEqual(result["host_sample_count"], len(records))
            self.assertEqual(result["observer_errors"], [])
            self.assertGreaterEqual(result["wall_seconds"], 1.15)
            self.assertIn("Popen start", result["wall_scope"])

    def test_host_read_failure_does_not_gate_or_retry_target(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-observer-") as temp:
            case = Path(temp) / "case"
            with patch.object(observer.host_load, "snapshot", side_effect=OSError("fixture unavailable")), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = observer.main([str(case), "--", sys.executable, "-c",
                                      "import sys; print('once'); sys.exit(7)"])
            self.assertEqual(code, 7)
            self.assertEqual((case / "stdout.txt").read_text(), "once\n")
            result = json.loads((case / "result.json").read_text())
            self.assertEqual(result["host_sample_count"], 0)
            self.assertEqual(result["exit_code"], 7)
            self.assertEqual([e["stage"] for e in result["observer_errors"]], ["host-sample", "host-sample"])


if __name__ == "__main__":
    unittest.main()
