#!/usr/bin/env python3
"""Observe one explicit command under record-operation.py; no GPU APIs or retries.

Usage: observe-roformer-run.py CASE_DIR -- COMMAND ...
Stdin is inherited untouched. This wrapper and its target use separate process
groups: the outer recorder signals this wrapper, which forwards once to the
target group and waits. Targets must retain their normal process-group membership.
Intent/PID records do not establish target initialization or host stability.
"""
import datetime
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time


_host_spec = importlib.util.spec_from_file_location(
    "uta_studio_host_load", Path(__file__).with_name("observe-host-load.py"))
host_load = importlib.util.module_from_spec(_host_spec)
_host_spec.loader.exec_module(host_load)


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sync_dir(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def save(path, value):
    with path.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
    sync_dir(path.parent)


def boot_id():
    try:
        return Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    except OSError:
        return None


def inspect_process(pid):
    root = Path("/proc") / str(pid)
    data = {"pid": pid, "children": [], "gpu_libraries": [], "drm": [], "read_errors": []}
    try:
        # Field 22 remains valid even when comm contains spaces or parentheses.
        stat = (root / "stat").read_text()
        fields = stat.rsplit(")", 1)[1].split()
        data["start_ticks"] = int(fields[19])
        data["main_thread_state"] = fields[0]
        data["process_user_ticks"] = int(fields[11])
        data["process_system_ticks"] = int(fields[12])
        for line in (root / "status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                data["rss_kib"] = int(line.split()[1])
        tasks = list((root / "task").iterdir())
    except (OSError, ValueError, IndexError) as error:
        data["read_errors"].append(str(error))
        return data
    # Passive host timing helps distinguish GPU activity from scheduler or
    # host waits. These optional reads never control target execution.
    for name in ("schedstat", "wchan", "io"):
        try:
            value = (root / name).read_text().strip()
            if name == "schedstat":
                runtime, runqueue, slices = (int(x) for x in value.split())
                data["main_thread_scheduler"] = {
                    "runtime_ns": runtime, "runqueue_ns": runqueue, "timeslices": slices,
                }
            elif name == "io":
                data["process_io"] = {
                    key: int(value) for key, value in
                    (line.split(":", 1) for line in value.splitlines())
                }
            else:
                data["main_thread_wchan"] = value
        except (OSError, ValueError) as error:
            data["read_errors"].append(f"{name}: {error}")
    for task in tasks:
        try:
            data["children"].extend(int(value) for value in (task / "children").read_text().split())
        except (OSError, ValueError) as error:
            data["read_errors"].append(str(error))
    try:
        for line in (root / "maps").read_text().splitlines():
            fields = line.split(maxsplit=5)
            if len(fields) == 6 and any(name in fields[5] for name in (
                "libvulkan", "libze_intel", "libigdrcl", "libamdhip", "libhsa-runtime",
            )):
                data["gpu_libraries"].append(fields[5])
    except OSError as error:
        data["read_errors"].append(str(error))
    try:
        fds = list((root / "fdinfo").iterdir())
    except OSError as error:
        data["read_errors"].append(str(error))
        fds = []
    for fd in fds:
        try:
            values = {}
            for line in fd.read_text().splitlines():
                if ":" in line:
                    key, value = line.split(":", 1)
                    if key.startswith("drm-"):
                        values[key] = value.strip()
            if values:
                data["drm"].append({"fd": fd.name, "fields": values})
        except OSError as error:
            data["read_errors"].append(str(error))
    data["children"] = sorted(set(data["children"]))
    data["gpu_libraries"] = sorted(set(data["gpu_libraries"]))
    return data


def sample_tree(pid):
    pending, seen, processes = [pid], set(), []
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        process = inspect_process(current)
        processes.append(process)
        pending.extend(process["children"])
    return {"time": now(), "processes": processes}


def main(args):
    if len(args) < 3 or args[1] != "--":
        raise SystemExit("usage: observe-roformer-run.py CASE_DIR -- COMMAND ...")
    case = Path(args[0]).absolute()
    command = args[2:]
    case.mkdir()  # Caller owns the parent and supplies a unique case path.
    sync_dir(case.parent)
    revision = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True)
    commit = revision.stdout.strip() if revision.returncode == 0 else None
    before = boot_id()
    intent = {
        "time": now(), "command": command, "cwd": os.getcwd(), "commit": commit,
        "git_revision_exit_code": revision.returncode, "git_revision_stderr": revision.stderr,
        "boot_id": before, "stdin": "inherited unchanged from outer recorder",
        "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
        "outer_operation_dir": os.environ.get("UTA_STUDIO_OPERATION_DIR"),
        "scope": "durable launch intent only; neither target execution nor setup is established",
    }
    save(case / "command.json", intent)
    process = None
    received, pending_signals, errors = [], [], []
    done = threading.Event()
    completion = {}
    libraries, devices = set(), set()
    memory_peak = {}
    rss_peak = 0
    samples_count = 0
    read_error_count = 0
    host_samples_count = 0
    last_host = None
    last_host_time = 0.0

    def record_host(stream, phase):
        nonlocal host_samples_count, last_host, last_host_time
        try:
            current = host_load.snapshot()
            record = {"phase": phase, "snapshot": current}
            if last_host is not None:
                record["interval_summary"] = host_load.summarize(
                    last_host, current, os.sysconf("SC_CLK_TCK"))
            stream.write(json.dumps(record, ensure_ascii=False) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
            last_host = current
            host_samples_count += 1
        except (OSError, ValueError, IndexError) as error:
            errors.append({"stage": "host-sample", "phase": phase, "error": str(error)})
        last_host_time = time.monotonic()

    def forward(signum, _frame):
        received.append(signum)
        if process is None:
            pending_signals.append(signum)
        else:
            deliver(signum)

    def deliver(signum):
        try:
            os.killpg(process.pid, signum)
        except ProcessLookupError:
            pass
        except OSError as error:
            errors.append({"stage": "signal", "error": str(error)})

    previous = {sig: signal.signal(sig, forward)
                for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    started = None
    waiter = None
    try:
        with (case / "stdout.txt").open("xb") as out, \
             (case / "stderr.txt").open("xb") as err, \
             (case / "samples.ndjson").open("x", encoding="utf-8") as samples, \
             (case / "host-samples.ndjson").open("x", encoding="utf-8") as host_samples:
            sync_dir(case)
            record_host(host_samples, "before-target")
            started = time.monotonic()
            try:
                process = subprocess.Popen(command, stdin=None, stdout=out, stderr=err,
                                           start_new_session=True)
            except OSError as error:
                save(case / "spawn-error.json", {"time": now(), "error": str(error)})
                return 127

            def wait_for_exit():
                completion["exit_code"] = process.wait()
                completion["monotonic"] = time.monotonic()
                done.set()

            waiter = threading.Thread(target=wait_for_exit, name="target-wait")
            waiter.start()
            # The target group is separate from the wrapper's group, so the
            # outer recorder cannot also deliver the same signal to the target.
            for signum in pending_signals:
                deliver(signum)
            try:
                save(case / "started.json", {
                    "time": now(), "pid": process.pid, "process_group": process.pid,
                    "commit": commit, "boot_id": boot_id(),
                    "scope": "Popen returned; does not prove target initialization completed",
                })
            except OSError as error:
                errors.append({"stage": "started-record", "error": str(error)})
            # Observation errors never launch retries or abandon a live target.
            while not done.is_set():
                # Child writes bypass the parent's Python buffers. Persist
                # completed writes during execution, not only after wait().
                # Hard power loss can still lose the interval since this sync
                # or data still buffered inside the target itself.
                for stream in (out, err):
                    try:
                        stream.flush()
                        os.fsync(stream.fileno())
                    except OSError as error:
                        errors.append({"stage": "live-output-sync", "error": str(error)})
                try:
                    snapshot = sample_tree(process.pid)
                    samples_count += 1
                    rss_peak = max(rss_peak, sum(p.get("rss_kib", 0) for p in snapshot["processes"]))
                    seen_clients, current_memory = set(), {}
                    for item in snapshot["processes"]:
                        read_error_count += len(item["read_errors"])
                        libraries.update(item["gpu_libraries"])
                        for drm in item["drm"]:
                            fields = drm["fields"]
                            driver, pdev = fields.get("drm-driver"), fields.get("drm-pdev")
                            devices.add((driver, pdev))
                            client = fields.get("drm-client-id")
                            key = ((driver, pdev, client) if client is not None
                                   else (item["pid"], item.get("start_ticks"), drm["fd"]))
                            if key in seen_clients:
                                continue
                            seen_clients.add(key)
                            for name, value in fields.items():
                                if name.startswith(("drm-total-", "drm-resident-", "drm-active-", "drm-shared-")):
                                    parts = value.split()
                                    if len(parts) == 2 and parts[0].isdigit() and parts[1] in ("KiB", "kB"):
                                        current_memory[name] = current_memory.get(name, 0) + int(parts[0])
                    for name, value in current_memory.items():
                        memory_peak[name] = max(memory_peak.get(name, 0), value)
                    samples.write(json.dumps(snapshot, ensure_ascii=False) + "\n")
                    samples.flush()
                    os.fsync(samples.fileno())
                    if time.monotonic() - last_host_time >= 1.0:
                        record_host(host_samples, "during-target")
                except (OSError, ValueError, IndexError) as error:
                    errors.append({"stage": "sample", "error": str(error)})
                done.wait(0.2)
            waiter.join()
            record_host(host_samples, "after-target")
            for stream in (out, err, samples, host_samples):
                try:
                    stream.flush()
                    os.fsync(stream.fileno())
                except OSError as error:
                    errors.append({"stage": "output-sync", "error": str(error)})
        result = {
            "time": now(), "command": command, "commit": commit, "pid": process.pid,
            "exit_code": completion["exit_code"],
            "wall_seconds": completion["monotonic"] - started,
            "wall_scope": "Popen start through wait-thread completion; excludes observation shutdown",
            "boot_before": before, "boot_after": boot_id(), "received_signals": received,
            "sample_count": samples_count, "sample_read_error_count": read_error_count,
            "host_sample_count": host_samples_count,
            "sampled_loaded_gpu_libraries": sorted(libraries),
            "sampled_drm_devices": [{"driver": driver, "pdev": pdev}
                                    for driver, pdev in sorted(devices, key=str)],
            "sampled_peak_drm_memory_kib": memory_peak,
            "sampled_peak_tree_sum_rss_kib": rss_peak,
            "observer_errors": errors,
            "limits": [
                "Sampling can miss short-lived descendants and allocations.",
                "stdout/stderr are synced each sampling cycle; target buffering and the unsynced tail can still be lost.",
                "Host sampling adds observer CPU work; compare runs with the same observer configuration.",
                "Host snapshots have partial DRM visibility and never establish isolation or control launch.",
                "Tree RSS sums may double-count shared pages; DRM fields are separate counters.",
                "Loaded libraries do not prove GPU work; missing records do not prove no launch.",
                "Process completion and unchanged boot IDs do not establish post-exit host stability.",
                "Signals reach descendants that retain the target process group; no timeouts or escalation.",
            ],
        }
        save(case / "result.json", result)
        print(json.dumps({"case_dir": str(case), **result}, ensure_ascii=False), flush=True)
        code = completion["exit_code"]
        return code if code >= 0 else 128 - code
    finally:
        # An unexpected observer failure must not silently leave its target
        # running. Cancellation uses the same target group and never retries.
        if process is not None and not done.is_set():
            deliver(signal.SIGTERM)
            if waiter is not None:
                waiter.join()
            else:
                process.wait()
        for signum, handler in previous.items():
            signal.signal(signum, handler)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
