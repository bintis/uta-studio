"""Regression tests for evidence summarization; no GPU or installed assets required."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "uta_studio_attention_study", Path(__file__).with_name("summarize-attention-study.py")
)
study = importlib.util.module_from_spec(spec)
spec.loader.exec_module(study)


class AttentionStudySummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def case(self, name="sample", exit_code=0):
        case = self.root / name
        case.mkdir()
        (case / "result.json").write_text(json.dumps({"pid": 10, "exit_code": exit_code}))
        record = {
            "queries": 1722, "heads": 8, "batches": 90,
            "warmup_calls": 2, "matmul_flops": 546561146880, "nmse": 1e-9,
        }
        lines = [
            "FLASH_ATTN_EXT dst(64,8,1722,90), q(64,1722,8,90): "
            f"1 x {duration} us = {duration} us"
            for duration in (100000, 90000, 43000, 45000)
        ]
        lines.append("ATTENTION_RESULT " + json.dumps(record))
        (case / "stderr.txt").write_text("\n".join(lines))
        return case

    def test_declared_warmups_excluded_without_selecting_fastest(self):
        result = study.summarize_case(self.case())
        self.assertEqual(result["numerical_cases"], 1)
        self.assertEqual(result["1722"]["measured_gpu_us"], [43000, 45000])
        self.assertEqual(result["1722"]["mean_ms"], 44)
        self.assertAlmostEqual(result["1722"]["tflops"], 546561146880 / 44000000000)

    def test_failed_run_is_retained_but_not_timing_eligible(self):
        result = study.summarize_case(self.case(exit_code=1))
        self.assertEqual(result["exit_code"], 1)
        self.assertFalse(result["timing_eligible"])
        self.assertIn("1722", result)

    def test_compiler_capture_is_not_a_performance_result(self):
        result = study.summarize_case(self.case(name="compiler-sample"))
        self.assertFalse(result["timing_eligible"])

    def test_competing_client_is_separated_from_target(self):
        case = self.case()
        records = []
        for percent in (3, 7):
            activity = [
                {"device": "gpu", "driver": "xe", "engine": "drm-cycles-ccs",
                 "owners": [{"pid": pid, "start_ticks": 100, "name": "fixture"}],
                 "percent_of_one_engine": rate}
                for pid, rate in ((10, 99), (11, percent))
            ]
            records.append(json.dumps({"interval_summary": {"drm_engine_activity": activity}}))
        (case / "host-samples.ndjson").write_text("\n".join(records))
        clients = study.summarize_case(case)["other_gpu_clients"]
        self.assertEqual(len(clients), 1)
        self.assertEqual(clients[0]["owners"][0]["pid"], 11)
        self.assertEqual(clients[0]["peak_percent_of_one_engine"], 7)

    def test_codec_loader_is_not_attributed_to_worker(self):
        report = self.root / "installation-verification.json"
        libraries = ["/active/libggml.so.0", "/codec/libggml.so.0"]
        report.write_text(json.dumps({"loaded_libraries": libraries}))
        worker = self.root / "worker-after"
        worker.mkdir()
        (worker / "result.json").write_text(json.dumps({"pid": 10}))
        (self.root / "loader.10").write_text("10: calling init: /active/libggml.so.0\n")
        study.correct_loader_scope(self.root)
        study.correct_loader_scope(self.root)
        result = json.loads(report.read_text())
        self.assertEqual(result["loaded_libraries"], libraries[:1])
        self.assertEqual(result["auxiliary_process_libraries"], libraries[1:])
        self.assertEqual(result["all_process_libraries"], libraries)


if __name__ == "__main__":
    unittest.main()
