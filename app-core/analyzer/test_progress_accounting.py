"""Dependency-light contract tests for DAG progress and run logs."""

from __future__ import annotations

import importlib.util
import pathlib
import sys
import tempfile
import types
import unittest


def _load_server_module():
    stubs = {
        "gpu": types.SimpleNamespace(
            end_of_song_cleanup=lambda: None,
            hard_free_gpu=lambda *_args: None,
            log_vram=lambda *_args: None,
            reset_peak_stats=lambda: None,
            vram_snapshot=lambda: None,
        ),
        "whisper_compat": types.SimpleNamespace(
            detect_device=lambda: "cpu",
            is_oom=lambda _message: False,
            set_align_backend=lambda _name: None,
            set_progress_sink=lambda _sink: None,
        ),
        "audio": types.SimpleNamespace(set_vocal_threshold_pct=lambda _value: None),
        "pipeline": types.SimpleNamespace(run_pipeline=lambda *_args, **_kwargs: None),
    }
    previous = {name: sys.modules.get(name) for name in stubs}
    sys.modules.update(stubs)
    try:
        path = pathlib.Path(__file__).with_name("server.py")
        spec = importlib.util.spec_from_file_location("uta_progress_server", path)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        return module
    finally:
        for name, value in previous.items():
            if value is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = value


server = _load_server_module()


class ProgressAccountingTests(unittest.TestCase):
    def command(self):
        return {
            "audio_processing": {
                "steps": [
                    {"step_id": "extract_vocals"},
                    {"step_id": "denoise_vocals"},
                ]
            },
            "engine": "whisper",
        }

    def test_structured_node_id_overrides_the_legacy_four_percent_bucket(self):
        payload = server._progress_payload(
            self.command(),
            "cpu",
            4,
            "Loading vocal extraction model",
            {"node_id": "stems.vocals", "event": "started", "node_progress_pct": 0},
            {},
        )
        self.assertEqual(payload["stage"], "separation")
        self.assertEqual(payload["event"], "started")
        self.assertEqual(payload["stage_progress"], 0)
        self.assertLess(payload["pct"], 100)

    def test_plain_backend_updates_attach_to_the_active_real_node(self):
        state = {}
        server._progress_payload(
            self.command(), "cpu", 4, "Starting extraction",
            {"node_id": "stems.vocals", "event": "started", "node_progress_pct": 0},
            state,
        )
        payload = server._progress_payload(
            self.command(), "cpu", 20, "Processing model chunks", {}, state
        )
        self.assertEqual(payload["node_id"], "stems.vocals")
        self.assertEqual(payload["event"], "progress")

    def test_overall_progress_is_monotonic_and_capped_until_done(self):
        state = {}
        values = []
        for node_id in (
            "preflight",
            "music.key",
            "music.rhythm",
            "music.descriptors",
            "stems.vocals",
            "vocals.denoise",
            "stems.bind_analysis_outputs",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "chart.build_candidate",
        ):
            payload = server._progress_payload(
                self.command(), "cpu", 100, f"{node_id} complete",
                {"node_id": node_id, "event": "completed", "node_progress_pct": 100},
                state,
            )
            values.append(payload["pct"])
        self.assertEqual(values, sorted(values))
        self.assertEqual(values[-1], 99)

    def test_terminal_optional_failure_counts_as_processed_work(self):
        command = {
            "skip_separation": True,
            "skip_transcription": True,
        }
        state = {}
        for node_id, event in (
            ("preflight", "completed"),
            ("music.key", "completed"),
            ("music.rhythm", "completed"),
            ("music.descriptors", "skipped"),
            ("pitch.extract", "failed"),
            ("chart.build_candidate", "completed"),
        ):
            payload = server._progress_payload(
                command,
                "cpu",
                100,
                f"{node_id} terminal",
                {"node_id": node_id, "event": event, "node_progress_pct": 100},
                state,
            )
        self.assertEqual(payload["pct"], 99)

    def test_route_metadata_does_not_bleed_between_sibling_nodes(self):
        state = {}
        server._progress_payload(
            self.command(),
            "cpu",
            4,
            "Extracting vocals",
            {
                "node_id": "stems.vocals",
                "event": "started",
                "implementation": "Vocal extractor",
                "model": "vocal-model",
                "node_progress_pct": 0,
            },
            state,
        )
        payload = server._progress_payload(
            self.command(),
            "cpu",
            20,
            "Denoising vocals",
            {
                "node_id": "vocals.denoise",
                "event": "started",
                "node_progress_pct": 0,
            },
            state,
        )
        denoise = next(
            route
            for route in payload["stage_routes"]
            if route["node_id"] == "vocals.denoise"
        )
        self.assertNotEqual(denoise["implementation"], "Vocal extractor")
        self.assertNotEqual(denoise["model"], "vocal-model")

    def test_work_unit_progress_is_preserved_on_the_authoritative_route(self):
        payload = server._progress_payload(
            self.command(),
            "cpu",
            20,
            "Processing chunk 3 of 8",
            {
                "node_id": "stems.vocals",
                "event": "progress",
                "work_units_completed": 3,
                "work_units_total": 8,
            },
            {},
        )
        route = payload["stage_routes"][0]
        self.assertEqual(route["stage_progress"], 38)
        self.assertEqual(route["work_units_completed"], 3)
        self.assertEqual(route["work_units_total"], 8)

    def test_historical_weight_requires_exact_node_implementation_and_device(self):
        command = {
            "node_weights": [
                {
                    "node_id": "stems.vocals",
                    "implementation": "RoFormer",
                    "actual_device": "cuda",
                    "duration_ms": 9000,
                },
                {
                    "node_id": "stems.vocals",
                    "implementation": "RoFormer",
                    "actual_device": "cpu",
                    "duration_ms": 24000,
                },
            ]
        }
        weights = {"stems.vocals": server.DEFAULT_NODE_WEIGHTS["stems.vocals"]}
        state = {}
        server._apply_matching_historical_weights(
            command,
            [{
                "node_id": "stems.vocals",
                "implementation": "RoFormer",
                "actual_device": "cpu",
            }],
            weights,
            state,
        )
        self.assertEqual(weights["stems.vocals"], 24000)

        server._apply_matching_historical_weights(
            command,
            [{
                "node_id": "stems.vocals",
                "implementation": "Fallback implementation",
                "actual_device": "cpu",
            }],
            weights,
            state,
        )
        self.assertEqual(
            weights["stems.vocals"],
            server.DEFAULT_NODE_WEIGHTS["stems.vocals"],
        )


class DedicatedLogTests(unittest.TestCase):
    def test_one_run_log_contains_structured_node_progress_and_raw_output(self):
        with tempfile.TemporaryDirectory() as root:
            path = pathlib.Path(root) / "run.jsonl"
            command = {"analysis_log_path": str(path), "engine": "whisper"}
            with server._analysis_log(command) as log:
                print("backend detail", flush=True)
                log.event({
                    "pct": 12,
                    "reported_pct": 4,
                    "node_id": "stems.vocals",
                    "event": "started",
                    "stage_progress": 0,
                    "msg": "Extracting vocals",
                })
            records = [
                __import__("json").loads(line)
                for line in path.read_text(encoding="utf-8").splitlines()
            ]
            self.assertTrue(any(record.get("message") == "backend detail" for record in records))
            event = next(record for record in records if record.get("record_type") == "node_event")
            self.assertEqual(event["node_id"], "stems.vocals")
            self.assertEqual(event["pct"], 12)


if __name__ == "__main__":
    unittest.main()
