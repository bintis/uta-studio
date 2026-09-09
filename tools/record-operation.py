#!/usr/bin/env python3
"""Persist a development operation's commit and launch intent before execution.

This is a diagnostic wrapper, never a production worker or inference fallback.
Use: python3 tools/record-operation.py LABEL [options] -- COMMAND [ARG ...]
"""

import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def save(path, value):
    data = json.dumps(value, ensure_ascii=False, indent=2) + "\n"
    with path.open("x", encoding="utf-8") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    sync_directory(path.parent)


def boot_id():
    try:
        return Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    except OSError:
        return None


def git(*args):
    result = subprocess.run(["git", *args], capture_output=True, text=True)
    return {"exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def path_info(value):
    path = Path(value).absolute()
    result = {"path": str(path)}
    try:
        stat = path.stat()
        result.update(size=stat.st_size, mtime_ns=stat.st_mtime_ns)
    except OSError as error:
        result["stat_error"] = str(error)
    return result


def child(record_dir):
    prepared = json.loads((record_dir / "prepared.json").read_text())
    # Written by the child itself, before replacing this process with the
    # target. An intent marker is not proof that exec or target setup finished.
    save(record_dir / "exec-intent.json", {
        "time": now(), "pid": os.getpid(), "boot_id": boot_id(),
        "commit": prepared["commit"], "command": prepared["command"],
    })
    env = dict(os.environ, UTA_STUDIO_OPERATION_DIR=str(record_dir))
    try:
        os.execvpe(prepared["command"][0], prepared["command"], env)
    except OSError as error:
        save(record_dir / "exec-error.json", {"time": now(), "error": str(error)})
        return 127


def main(args):
    if args[:1] == ["--record-child"]:
        return child(Path(args[1]))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("label")
    parser.add_argument("--evidence-root", default="test-artifacts/operations")
    parser.add_argument("--stdin-file")
    parser.add_argument("--input", action="append", default=[])
    parser.add_argument("--output", action="append", default=[])
    if "--" not in args:
        parser.error("separate the command with --")
    split = args.index("--")
    options = parser.parse_args(args[:split])
    command = args[split + 1:]
    if not command:
        parser.error("a command is required after --")

    root = Path(options.evidence_root).absolute()
    root.mkdir(parents=True, exist_ok=True)
    sync_directory(root.parent)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S")
    directory = root / (stamp + "-" + uuid.uuid4().hex[:12])
    directory.mkdir()
    sync_directory(root)
    revision = git("rev-parse", "HEAD")
    status = git("status", "--porcelain=v1", "--untracked-files=normal")
    prepared = {
        "label": options.label, "time": now(), "cwd": os.getcwd(),
        "commit": revision["stdout"].strip() if revision["exit_code"] == 0 else None,
        "git_revision": revision, "git_status": status,
        "command": command, "boot_id": boot_id(),
        "inputs": [path_info(p) for p in options.input],
        "outputs": [path_info(p) for p in options.output],
        "stdin_source": path_info(options.stdin_file) if options.stdin_file else None,
        "identity_scope": "HEAD plus recorded dirty status; HEAD alone is not the whole working tree",
    }
    # Preserve exact stdin independently of its original pathname.
    if options.stdin_file:
        with Path(options.stdin_file).open("rb") as source, (directory / "stdin.bin").open("xb") as dest:
            while data := source.read(1024 * 1024):
                dest.write(data)
            dest.flush()
            os.fsync(dest.fileno())
    save(directory / "prepared.json", prepared)
    print(str(directory), flush=True)

    stdin = (directory / "stdin.bin").open("rb") if options.stdin_file else None
    started = time.monotonic()
    process = None
    received_signals = []
    pending_signals = []

    def deliver(signum):
        try:
            os.killpg(process.pid, signum)
        except ProcessLookupError:
            pass

    def forward(signum, _frame):
        received_signals.append(signum)
        if process is None:
            pending_signals.append(signum)
        else:
            deliver(signum)

    previous_handlers = {
        signum: signal.signal(signum, forward)
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
    }
    try:
        with (directory / "stdout.txt").open("xb") as out, (directory / "stderr.txt").open("xb") as err:
            sync_directory(directory)
            try:
                process = subprocess.Popen(
                    [sys.executable, str(Path(__file__).resolve()), "--record-child", str(directory)],
                    stdin=stdin if stdin else subprocess.DEVNULL, stdout=out, stderr=err,
                    start_new_session=True,
                )
            except OSError as error:
                save(directory / "spawn-error.json", {"time": now(), "error": str(error)})
                return 127
            # The target has its own process group: terminal Ctrl-C reaches
            # this recorder once, then is explicitly forwarded once. TERM/HUP
            # also reach descendants while the recorder waits and saves status.
            for signum in pending_signals:
                deliver(signum)
            code = process.wait()
            os.fsync(out.fileno())
            os.fsync(err.fileno())
    finally:
        for signum, previous in previous_handlers.items():
            signal.signal(signum, previous)
        if stdin:
            stdin.close()
    save(directory / "result.json", {
        "time": now(), "pid": process.pid, "commit": prepared["commit"],
        "boot_id": boot_id(), "exit_code": code, "received_signals": received_signals,
        "wall_seconds_including_child_logging": time.monotonic() - started,
        "scope": "process completion only; does not establish post-exit host stability",
    })
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
