"""Isolated CPU/process tests; never create a GPU or read user media."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


RECORDER = Path(__file__).with_name("record-operation.py").resolve()


class OperationRecorderTests(unittest.TestCase):
    def run_recorded(self, directory, command, extra=()):
        root = Path(directory) / "records"
        result = subprocess.run(
            [sys.executable, str(RECORDER), "fixture", "--evidence-root", str(root),
             *extra, "--", *command], capture_output=True, text=True,
        )
        return result, next(root.iterdir())

    def test_commit_and_exec_intent_exist_before_target_and_stdin_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            request = Path(directory) / "request.ndjson"
            request.write_text('{"task":"isolated fixture"}\n')
            program = (
                "import os,json,pathlib,sys; p=pathlib.Path(os.environ['UTA_STUDIO_OPERATION_DIR']); "
                "a=json.loads((p/'prepared.json').read_text()); "
                "b=json.loads((p/'exec-intent.json').read_text()); "
                "assert a['commit'] and a['commit']==b['commit']; "
                "assert b['pid']==os.getpid(); "
                "assert sys.stdin.buffer.read()==(p/'stdin.bin').read_bytes(); "
                "print('target reached'); sys.exit(7)"
            )
            result, record = self.run_recorded(
                directory, [sys.executable, "-c", program],
                ["--stdin-file", str(request), "--input", str(request)],
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertEqual((record / "stdin.bin").read_bytes(), request.read_bytes())
            self.assertEqual((record / "stdout.txt").read_text(), "target reached\n")
            self.assertEqual(json.loads((record / "result.json").read_text())["exit_code"], 7)

    def test_exec_failure_keeps_intent_and_error_without_claiming_target_ran(self):
        with tempfile.TemporaryDirectory() as directory:
            result, record = self.run_recorded(directory, [str(Path(directory) / "missing-command")])
            self.assertEqual(result.returncode, 127)
            self.assertTrue((record / "exec-intent.json").is_file())
            self.assertTrue((record / "exec-error.json").is_file())

    def test_killed_target_preserves_signal_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            result, record = self.run_recorded(
                directory, [sys.executable, "-c", "import os,signal; os.kill(os.getpid(),signal.SIGKILL)"],
            )
            self.assertEqual(result.returncode, 137)
            self.assertEqual(json.loads((record / "result.json").read_text())["exit_code"], -9)

    def test_recorder_forwards_termination_and_waits_for_target(self):
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=signum), tempfile.TemporaryDirectory() as directory:
                root = Path(directory) / "records"
                program = (
                    "import os,pathlib,signal,sys\n"
                    "p=pathlib.Path(os.environ['UTA_STUDIO_OPERATION_DIR'])\n"
                    "def stop(sig, frame):\n"
                    "    (p/'signal.txt').write_text(str(sig))\n"
                    "    sys.exit(23)\n"
                    "for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):\n"
                    "    signal.signal(sig, stop)\n"
                    "(p/'ready.flag').write_text('ready')\n"
                    "signal.pause()\n"
                )
                recorder = subprocess.Popen(
                    [sys.executable, str(RECORDER), "signal-fixture", "--evidence-root", str(root),
                     "--", sys.executable, "-c", program], stdout=subprocess.PIPE, text=True,
                )
                record = None
                try:
                    record = Path(recorder.stdout.readline().strip())
                    deadline = time.monotonic() + 10
                    while not (record / "ready.flag").exists() and time.monotonic() < deadline:
                        time.sleep(0.02)
                    self.assertTrue((record / "ready.flag").exists())
                    recorder.send_signal(signum)
                    self.assertEqual(recorder.wait(timeout=10), 23)
                    result = json.loads((record / "result.json").read_text())
                    self.assertEqual(result["received_signals"], [signum])
                    self.assertEqual((record / "signal.txt").read_text(), str(signum))
                finally:
                    if recorder.poll() is None:
                        recorder.kill()
                        recorder.wait()
                    recorder.stdout.close()
                    if record and (record / "exec-intent.json").is_file():
                        pid = json.loads((record / "exec-intent.json").read_text())["pid"]
                        try:
                            os.killpg(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass


if __name__ == "__main__":
    unittest.main()
