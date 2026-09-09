#!/usr/bin/env python3
"""Read-only host load sampling before an explicit performance experiment.

No GPU APIs, target execution, load thresholds, idle waits or retries. Run under
record-operation.py and inspect the result before deciding to launch anything.
CPU percentages use measured intervals; DRM client counters are partial visibility,
not a whole-device utilization claim. Raw snapshots remain in the JSON output.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import time


def read_text(path):
    return path.read_text().strip()


def process_stat(text):
    fields = text.rsplit(")", 1)[1].split()
    return {
        "name": text[text.index("(") + 1:text.rindex(")")],
        "start_ticks": int(fields[19]),
        "cpu_ticks": int(fields[11]) + int(fields[12]),
    }


def snapshot(proc=Path("/proc"), drm=Path("/sys/class/drm")):
    result = {
        "time": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "monotonic_ns": time.monotonic_ns(), "processes": [],
        "gpu_devices": [], "drm_clients": [], "read_errors": {},
    }

    def error(stage, detail):
        item = result["read_errors"].setdefault(stage, {"count": 0, "example": str(detail)})
        item["count"] += 1

    for name in ("stat", "loadavg", "meminfo", "sys/kernel/random/boot_id"):
        try:
            result[name] = read_text(proc / name)
        except OSError as exc:
            error(name, exc)
    for card in sorted(drm.glob("card*")):
        if not re.fullmatch(r"card\d+", card.name):
            continue
        device = card / "device"
        gpu = {"card": card.name, "device_path": str(device.resolve()), "fields": {}}
        for name in ("vendor", "device", "gpu_busy_percent", "mem_busy_percent"):
            try:
                gpu["fields"][name] = read_text(device / name)
            except OSError as exc:
                gpu["fields"][name] = None
                error("gpu:" + name, exc)
        result["gpu_devices"].append(gpu)

    clients = {}
    for path in proc.iterdir():
        if not path.name.isdigit():
            continue
        pid = int(path.name)
        try:
            process = {"pid": pid, **process_stat(read_text(path / "stat"))}
            process["sample_ns"] = time.monotonic_ns()
            result["processes"].append(process)
        except (OSError, ValueError, IndexError) as exc:
            error("process_stat", exc)
            continue
        try:
            fds = list((path / "fd").iterdir())
        except OSError as exc:
            error("process_fds", exc)
            continue
        for fd in fds:
            try:
                if not os.readlink(fd).startswith("/dev/dri/"):
                    continue
                fields = {}
                for line in read_text(path / "fdinfo" / fd.name).splitlines():
                    if line.startswith("drm-") and ":" in line:
                        key, value = line.split(":", 1)
                        fields[key] = value.strip()
                if not fields:
                    continue
                driver = fields.get("drm-driver")
                pdev = fields.get("drm-pdev")
                client_id = fields.get("drm-client-id")
                # The same open DRM client can appear in multiple FDs or PIDs.
                # Missing client/device identity cannot safely be deduplicated.
                key = ((driver, pdev, client_id) if client_id is not None and pdev is not None
                       else (pid, process["start_ticks"], fd.name))
                if key not in clients:
                    clients[key] = {"identity": list(key), "fields": fields,
                                    "owners": [], "sample_ns": time.monotonic_ns()}
                owner = {"pid": pid, "start_ticks": process["start_ticks"], "name": process["name"]}
                if owner not in clients[key]["owners"]:
                    clients[key]["owners"].append(owner)
            except OSError as exc:
                error("drm_fd", exc)
    result["drm_clients"] = list(clients.values())
    result["finished_monotonic_ns"] = time.monotonic_ns()
    return result


def cpu_counts(snapshot):
    for line in snapshot.get("stat", "").splitlines():
        if line.startswith("cpu "):
            # guest/guest_nice are already included in user/nice. iowait is
            # reported separately, not charged as CPU executing instructions.
            ticks = [int(value) for value in line.split()[1:9]]
            if len(ticks) != 8:
                return None
            return sum(ticks), ticks[3], ticks[4]
    return None


def summarize(before, after, clock_ticks):
    elapsed = (after["monotonic_ns"] - before["monotonic_ns"]) / 1e9
    result = {"snapshot_start_interval_seconds": elapsed, "cpu": None,
              "top_cpu_processes": [], "drm_engine_activity": [],
              "gpu_busy_snapshots": [before["gpu_devices"], after["gpu_devices"]]}
    if (before.get("sys/kernel/random/boot_id") != after.get("sys/kernel/random/boot_id")
            or elapsed <= 0):
        result["unavailable_reason"] = "boot changed or nonpositive sampling interval"
        return result
    a, b = cpu_counts(before), cpu_counts(after)
    if a is not None and b is not None:
        total, idle, wait = (y - x for x, y in zip(a, b))
        if total > 0 and min(idle, wait, total - idle - wait) >= 0:
            result["cpu"] = {"busy_percent_all_cpus": 100 * (total - idle - wait) / total,
                             "iowait_percent_all_cpus": 100 * wait / total}
    processes = {(p["pid"], p["start_ticks"]): p for p in before["processes"]}
    for process in after["processes"]:
        old = processes.get((process["pid"], process["start_ticks"]))
        if old is None:
            continue
        interval = (process["sample_ns"] - old["sample_ns"]) / 1e9
        ticks = process["cpu_ticks"] - old["cpu_ticks"]
        if ticks < 0 or interval <= 0:
            continue
        result["top_cpu_processes"].append({
            "pid": process["pid"], "name": process["name"],
            "percent_of_one_cpu": 100 * ticks / clock_ticks / interval,
        })
    result["top_cpu_processes"].sort(key=lambda p: p["percent_of_one_cpu"], reverse=True)
    result["top_cpu_processes"] = result["top_cpu_processes"][:15]
    clients = {tuple(c["identity"]): c for c in before["drm_clients"]}
    for client in after["drm_clients"]:
        old = clients.get(tuple(client["identity"]))
        if old is None:
            continue
        # Client IDs can be recycled: require at least one same PID/start-time
        # owner rather than joining unrelated processes that reused a client ID.
        old_owners = {(p["pid"], p["start_ticks"]) for p in old["owners"]}
        if not old_owners.intersection((p["pid"], p["start_ticks"]) for p in client["owners"]):
            continue
        interval_ns = client["sample_ns"] - old["sample_ns"]
        if interval_ns <= 0:
            continue
        for name, value in client["fields"].items():
            if not name.startswith("drm-engine-"):
                continue
            first = re.fullmatch(r"(\d+) ns", old["fields"].get(name, ""))
            last = re.fullmatch(r"(\d+) ns", value)
            if first is None or last is None:
                continue
            delta = int(last[1]) - int(first[1])
            if delta < 0:
                continue
            result["drm_engine_activity"].append({
                "device": client["fields"].get("drm-pdev"),
                "driver": client["fields"].get("drm-driver"), "engine": name,
                "counter_kind": "nanoseconds",
                "owners": client["owners"], "busy_ns_delta": delta,
                "percent_of_one_engine": 100 * delta / interval_ns,
            })
        # xe exposes busy and total engine cycles rather than ns. Normalize
        # against the matching total-cycle delta, not a guessed GPU frequency.
        for name, value in client["fields"].items():
            if not name.startswith("drm-cycles-"):
                continue
            total_name = "drm-total-cycles-" + name.removeprefix("drm-cycles-")
            values = [old["fields"].get(name, ""), value,
                      old["fields"].get(total_name, ""), client["fields"].get(total_name, "")]
            if not all(re.fullmatch(r"\d+", value) for value in values):
                continue
            busy_before, busy_after, total_before, total_after = map(int, values)
            delta, total = busy_after - busy_before, total_after - total_before
            if delta < 0 or total <= 0:
                continue
            result["drm_engine_activity"].append({
                "device": client["fields"].get("drm-pdev"),
                "driver": client["fields"].get("drm-driver"), "engine": name,
                "counter_kind": "cycles", "owners": client["owners"],
                "busy_cycles_delta": delta, "total_cycles_delta": total,
                "percent_of_one_engine": 100 * delta / total,
            })
    result["drm_engine_activity"].sort(key=lambda item: item["percent_of_one_engine"], reverse=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--interval", type=float, default=1.0,
                        help="seconds between two passive snapshots (default: 1)")
    options = parser.parse_args()
    before = snapshot()
    time.sleep(options.interval)
    after = snapshot()
    print(json.dumps({
        "summary": summarize(before, after, os.sysconf("SC_CLK_TCK")),
        "before": before, "after": after,
        "limits": [
            "No GPU context is created. No process is launched, stopped, or waited on for idleness.",
            "This is a pre-run observation, not a performance acceptance gate or proof of isolation.",
            "GPU sysfs percentages are driver-defined snapshots, not the full interval average.",
            "DRM rates cover only readable, surviving clients; no visible activity does not mean idle.",
            "Engine rates are separate counters; do not sum engines into device utilization.",
            "Cycle rates use matching total-cycle deltas, not an assumed GPU clock or ns conversion.",
            "Sampling is sequential; per-process/client timestamps account for scan time but not atomicity.",
            "Later background work can invalidate comparability even after a quiet pre-run sample.",
        ],
    }, indent=2))


if __name__ == "__main__":
    main()
