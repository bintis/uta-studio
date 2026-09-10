import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("amd_summary", Path(__file__).with_name("summarize-amd-model-bench.py"))
summary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(summary)


class SummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def record(self, name="case", **changes):
        directory = self.root / "runs" / name
        directory.mkdir(parents=True)
        prepared = {"label": name, "time": "2026-09-11T00:00:00+00:00", "commit": "observed-commit", "boot_id": "boot"}
        report = {"event": "result", "status": "passed", "backend": "libtorch", "resource": "fcpe",
                  "device": "ROCm device 0", "precision": "strict", "build_profile": "release", "input_seconds": 30.0,
                  "input": "clip.wav", "model_path": "model.gguf", "scope": "complete_audio_model_pipeline",
                  "cold_execution_seconds": 2.0, "warm_execution_seconds": [1.2, 1.0, 1.1], "warm_median_seconds": 123.0}
        report.update(changes)
        (directory / "prepared.json").write_text(json.dumps(prepared))
        (directory / "result.json").write_text(json.dumps({"exit_code": 0, "boot_id": "boot"}))
        (directory / "stdout.txt").write_text(json.dumps(report) + "\n")
        return directory

    def test_warm_median_is_derived_from_actual_samples(self):
        report, exclusion = summary.read_record(self.record())
        self.assertIsNone(exclusion)
        self.assertEqual(report["warm_median_seconds"], 1.1)
        self.assertEqual(report["warm_sample_count"], 3)

    def test_single_pass_does_not_invent_a_warm_time(self):
        report, _ = summary.read_record(self.record(warm_execution_seconds=[]))
        self.assertIsNone(report["warm_median_seconds"])
        self.assertEqual(report["warm_sample_count"], 0)

    def test_failed_or_incomplete_process_never_becomes_a_timing(self):
        directory = self.record()
        (directory / "result.json").write_text(json.dumps({"exit_code": 124, "boot_id": "boot"}))
        report, exclusion = summary.read_record(directory)
        self.assertIsNone(report)
        self.assertEqual(exclusion["reason"], "process_failure")
        (directory / "result.json").unlink()
        report, exclusion = summary.read_record(directory)
        self.assertIsNone(report)
        self.assertEqual(exclusion["reason"], "operation_not_completed")

    def test_non_amd_or_debug_measurements_are_excluded(self):
        for name, fields, expected in [
            ("xpu", {"device": "XPU device 0"}, "not_the_explicit_amd_device"),
            ("debug", {"build_profile": "debug"}, "not_release_profile"),
            ("short", {"input_seconds": 12.0}, "not_thirty_second_input"),
        ]:
            report, exclusion = summary.read_record(self.record(name, **fields))
            self.assertIsNone(report)
            self.assertEqual(exclusion["reason"], expected)

    def test_boot_change_is_not_a_successful_operation(self):
        directory = self.record()
        (directory / "result.json").write_text(json.dumps({"exit_code": 0, "boot_id": "other-boot"}))
        report, exclusion = summary.read_record(directory)
        self.assertIsNone(report)
        self.assertEqual(exclusion["reason"], "boot_changed_during_operation")

    def test_precisions_are_not_cherry_picked_or_merged(self):
        self.record("strict", cold_execution_seconds=2.0)
        self.record("mixed", precision="mixed_attention", cold_execution_seconds=0.1)
        self.record("ggml", backend="ggml", device="Vulkan1: AMD Radeon 780M", precision="ggml_checkpoint", cold_execution_seconds=1.0)
        reports, exclusions = summary.collect(self.root)
        row = next(row for row in summary.pairs(reports) if row["resource"] == "fcpe")
        self.assertFalse(exclusions)
        self.assertEqual(row["first_inference_speedup"], 0.5)
        self.assertEqual(row["libtorch_cold_execution_seconds"], 2.0)
        self.assertEqual(row["pair_status"], "measured_same_workload")

    def test_different_input_or_scope_is_not_a_speedup_pair(self):
        left, _ = summary.read_record(self.record("ggml", backend="ggml", device="AMD Radeon 780M", precision="ggml_checkpoint"))
        right, _ = summary.read_record(self.record("lib", scope="encoder_only"))
        row = next(row for row in summary.pairs([left, right]) if row["resource"] == "fcpe")
        self.assertIsNone(row["first_inference_speedup"])
        self.assertEqual(row["pair_status"], "missing_or_not_comparable")


if __name__ == "__main__":
    unittest.main()
