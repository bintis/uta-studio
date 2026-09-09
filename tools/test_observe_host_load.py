#!/usr/bin/env python3
"""Isolated CPU fixtures for the passive host observer; no /proc or GPU access."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("host_load", Path(__file__).with_name("observe-host-load.py"))
observer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observer)


def stat_line(pid=123, ticks=25, start=100):
    fields = ["0"] * 22
    fields[0], fields[11], fields[12], fields[19] = "S", str(ticks - 5), "5", str(start)
    return f"{pid} (name with ) spaces) " + " ".join(fields)


def sample():
    return {
        "monotonic_ns": 1_000_000_000, "stat": "cpu  100 0 20 800 50 10 10 10 60 0\n",
        "sys/kernel/random/boot_id": "fixture-boot", "gpu_devices": [],
        "processes": [{"pid": 10, "start_ticks": 99, "cpu_ticks": 50,
                       "name": "worker", "sample_ns": 1_000_000_000}],
        "drm_clients": [{
            "identity": ["amdgpu", "0000:10:00.0", "7"], "sample_ns": 1_000_000_000,
            "fields": {"drm-driver": "amdgpu", "drm-pdev": "0000:10:00.0",
                       "drm-engine-compute": "100 ns", "drm-engine-capacity-compute": "2"},
            "owners": [{"pid": 10, "start_ticks": 99, "name": "worker"}],
        }],
    }


def later():
    result = copy.deepcopy(sample())
    result["monotonic_ns"] += 2_000_000_000
    # Total delta=200, idle=100, iowait=20, busy=80; guest is not counted twice.
    result["stat"] = "cpu  180 0 20 900 70 10 10 10 100 0\n"
    result["processes"][0].update(cpu_ticks=350, sample_ns=3_000_000_000)
    result["drm_clients"][0]["sample_ns"] = 3_000_000_000
    result["drm_clients"][0]["fields"]["drm-engine-compute"] = "1000000100 ns"
    return result


class HostLoadTests(unittest.TestCase):
    def test_stat_name_can_contain_spaces_and_parentheses(self):
        self.assertEqual(observer.process_stat(stat_line()), {
            "name": "name with ) spaces", "start_ticks": 100, "cpu_ticks": 25,
        })

    def test_rates_keep_cpu_and_engine_denominators_distinct(self):
        result = observer.summarize(sample(), later(), 100)
        self.assertEqual(result["cpu"], {"busy_percent_all_cpus": 40.0, "iowait_percent_all_cpus": 10.0})
        self.assertEqual(result["top_cpu_processes"][0]["percent_of_one_cpu"], 150.0)
        self.assertEqual(len(result["drm_engine_activity"]), 1)
        self.assertEqual(result["drm_engine_activity"][0]["percent_of_one_engine"], 50.0)

    def test_xe_cycles_use_matching_total_delta_and_preserve_units(self):
        before, after = sample(), later()
        before["drm_clients"][0]["fields"] = {
            "drm-driver": "xe", "drm-pdev": "0000:07:00.0",
            "drm-cycles-rcs": "20", "drm-total-cycles-rcs": "100",
        }
        after["drm_clients"][0]["fields"] = {
            "drm-driver": "xe", "drm-pdev": "0000:07:00.0",
            "drm-cycles-rcs": "70", "drm-total-cycles-rcs": "300",
        }
        activity = observer.summarize(before, after, 100)["drm_engine_activity"]
        self.assertEqual(len(activity), 1)
        self.assertEqual(activity[0]["counter_kind"], "cycles")
        self.assertEqual(activity[0]["busy_cycles_delta"], 50)
        self.assertEqual(activity[0]["total_cycles_delta"], 200)
        self.assertEqual(activity[0]["percent_of_one_engine"], 25.0)
        self.assertNotIn("busy_ns_delta", activity[0])
        for field, value in (("drm-total-cycles-rcs", "100"),
                             ("drm-total-cycles-rcs", "99"),
                             ("drm-total-cycles-rcs", ""),
                             ("drm-cycles-rcs", "19")):
            broken = copy.deepcopy(after)
            broken["drm_clients"][0]["fields"][field] = value
            self.assertEqual(observer.summarize(before, broken, 100)["drm_engine_activity"], [])

    def test_reused_pids_and_clients_are_not_joined(self):
        after = later()
        after["processes"][0]["start_ticks"] += 1
        after["drm_clients"][0]["owners"][0]["start_ticks"] += 1
        result = observer.summarize(sample(), after, 100)
        self.assertEqual(result["top_cpu_processes"], [])
        self.assertEqual(result["drm_engine_activity"], [])

    def test_resets_missing_counters_and_boot_changes_are_not_idle_claims(self):
        for value in ("50 ns", "100 cycles", "unavailable"):
            after = later()
            after["drm_clients"][0]["fields"]["drm-engine-compute"] = value
            self.assertEqual(observer.summarize(sample(), after, 100)["drm_engine_activity"], [])
        after = later()
        after["sys/kernel/random/boot_id"] = "other-boot"
        result = observer.summarize(sample(), after, 100)
        self.assertIsNone(result["cpu"])
        self.assertIn("unavailable_reason", result)
        after = later()
        after.pop("stat")
        self.assertIsNone(observer.summarize(sample(), after, 100)["cpu"])

    def test_fixture_scan_deduplicates_clients_and_reports_missing_visibility(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-host-load-") as temp:
            root = Path(temp)
            proc, drm = root / "proc", root / "drm"
            proc.mkdir()
            drm.mkdir()
            (proc / "stat").write_text("cpu 1 0 0 9 0 0 0 0\n")
            for pid in (1, 2):
                process = proc / str(pid)
                (process / "fd").mkdir(parents=True)
                (process / "fdinfo").mkdir()
                (process / "stat").write_text(stat_line(pid))
                for fd in (3, 4):
                    (process / "fd" / str(fd)).symlink_to("/dev/dri/renderD129")
                    (process / "fdinfo" / str(fd)).write_text(
                        "drm-driver: amdgpu\ndrm-pdev: 0000:10:00.0\n"
                        "drm-client-id: 7\ndrm-engine-compute: 123 ns\n")
            # Process exit/unreadable stat is missing visibility, not zero CPU.
            (proc / "3").mkdir()
            device = drm / "card1" / "device"
            device.mkdir(parents=True)
            (device / "vendor").write_text("0x1002")
            (device / "gpu_busy_percent").write_text("37")
            (drm / "card1-DP-1").mkdir()
            result = observer.snapshot(proc, drm)
            self.assertEqual(len(result["drm_clients"]), 1)
            self.assertEqual(len(result["drm_clients"][0]["owners"]), 2)
            self.assertEqual(len(result["gpu_devices"]), 1)
            self.assertEqual(result["gpu_devices"][0]["fields"]["gpu_busy_percent"], "37")
            self.assertIsNone(result["gpu_devices"][0]["fields"]["mem_busy_percent"])
            self.assertEqual(result["read_errors"]["process_stat"]["count"], 1)


if __name__ == "__main__":
    unittest.main()
