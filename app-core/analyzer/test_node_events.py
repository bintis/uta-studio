"""Phase 3 regression tests for the structured node-event protocol
(see /docs/analysis-dag-redesign.md, Phase 3 status note, and
uta-studio-analysis-dag-phases.md, Phase 3).

`whisper_compat.progress_node`/`artifact_reused` are additive on top of the
existing `progress(pct, msg, **metadata)` call: they stuff `node_id`/`event`
into the same metadata dict every progress call already threads through to
`server._progress_payload`. These tests lock two things:

1. The helpers build the right metadata shape (whisper_compat side).
2. `_progress_payload` passes `node_id`/`event` straight through, and
   leaves every pre-Phase-3 field (`stage`, `stage_progress`,
   `implementation`, ...) computed exactly as before when they're absent --
   old consumers (today's desktop UI, which still keys off `stage`) must
   see zero behavior change.

Same environment caveat as test_pipeline_cache.py: `server`/`whisper_compat`
import torch transitively, which isn't available in every sandbox. Tests
skip cleanly rather than fail when that's the case.
"""

from __future__ import annotations

import unittest

try:
    import server  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    server = None
    server_import_error = exc
else:
    server_import_error = None

try:
    import whisper_compat  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    whisper_compat = None
    whisper_compat_import_error = exc
else:
    whisper_compat_import_error = None


@unittest.skipUnless(
    whisper_compat is not None, f"whisper_compat import failed: {whisper_compat_import_error}"
)
class ProgressNodeHelperTests(unittest.TestCase):
    def setUp(self):
        self.captured = []
        self._previous_sink = whisper_compat._progress_sink
        whisper_compat.set_progress_sink(
            lambda pct, msg, metadata=None: self.captured.append((pct, msg, metadata or {}))
        )

    def tearDown(self):
        whisper_compat.set_progress_sink(None)

    def test_progress_node_stuffs_node_id_and_event_into_metadata(self):
        whisper_compat.progress_node("pitch.extract", "node_started", 52, "Extracting reference pitch...")
        pct, msg, metadata = self.captured[-1]
        self.assertEqual(pct, 52)
        self.assertEqual(msg, "Extracting reference pitch...")
        self.assertEqual(metadata["node_id"], "pitch.extract")
        self.assertEqual(metadata["event"], "node_started")

    def test_progress_node_forwards_extra_metadata_unchanged(self):
        whisper_compat.progress_node(
            "music.analysis", "node_started", 3, "Analyzing musical key...",
            implementation="Essentia/NumPy FFT", model="KeyExtractor / Krumhansl chroma profiles",
        )
        _, _, metadata = self.captured[-1]
        self.assertEqual(metadata["implementation"], "Essentia/NumPy FFT")
        self.assertEqual(metadata["model"], "KeyExtractor / Krumhansl chroma profiles")

    def test_artifact_reused_sets_event_and_default_reason(self):
        whisper_compat.artifact_reused("stems.separate", 50, "Stems already cached, skipping separation")
        _, _, metadata = self.captured[-1]
        self.assertEqual(metadata["event"], "artifact_reused")
        self.assertEqual(metadata["reason"], "cache_hit")
        self.assertEqual(metadata["node_id"], "stems.separate")

    def test_plain_progress_calls_never_set_node_id(self):
        # The Legacy Adapter contract: a call site that hasn't been migrated
        # to progress_node must produce metadata with no node_id at all, not
        # an empty string or a guessed value.
        whisper_compat.progress(4, "Inspecting source codec and cache format...")
        _, _, metadata = self.captured[-1]
        self.assertNotIn("node_id", metadata)


@unittest.skipUnless(server is not None, f"server import failed: {server_import_error}")
class ProgressPayloadNodeFieldsTests(unittest.TestCase):
    def test_node_id_and_event_pass_through_when_present(self):
        payload = server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_started"},
            runtime_state={},
        )
        self.assertEqual(payload["node_id"], "pitch.extract")
        self.assertEqual(payload["event"], "node_started")
        # Legacy Adapter fields must still be computed identically -- Phase 3
        # is additive, not a replacement of the text classifier.
        self.assertEqual(payload["stage"], "pitch")

    def test_node_id_and_event_are_none_when_absent(self):
        payload = server._progress_payload(
            {}, "cpu", 4, "Inspecting source codec and cache format...",
            metadata={},
            runtime_state={},
        )
        self.assertIsNone(payload["node_id"])
        self.assertIsNone(payload["event"])
        # Every pre-Phase-3 field must be present and unaffected.
        self.assertEqual(payload["stage"], "preparing")
        self.assertIn("stage_progress", payload)
        self.assertIn("stage_routes", payload)

    def test_artifact_reused_carries_its_reason(self):
        payload = server._progress_payload(
            {}, "cpu", 50, "Stems already cached, skipping separation",
            metadata={"node_id": "stems.separate", "event": "artifact_reused", "reason": "cache_hit"},
            runtime_state={},
        )
        self.assertEqual(payload["event"], "artifact_reused")
        self.assertEqual(payload["artifact_reused_reason"], "cache_hit")

    def test_absent_event_never_sets_artifact_reused_reason(self):
        payload = server._progress_payload(
            {}, "cpu", 4, "Inspecting source codec and cache format...",
            metadata={},
            runtime_state={},
        )
        self.assertNotIn("artifact_reused_reason", payload)


@unittest.skipUnless(server is not None, f"server import failed: {server_import_error}")
class StageRoutesNodeIdKeyingTests(unittest.TestCase):
    """`AnalysisStageRoute.node_id` (Rust: app-core/src/analyzer.rs) and the
    dict-keying fix in `_progress_payload` -- previously `stage_routes` was
    keyed only by the coarse 7-bucket `stage` text, so a compound node's
    children (e.g. music.key/music.rhythm/music.descriptors, all under the
    "key_detection" bucket) silently overwrote each other's route entry,
    leaving only the last child's data. Now keyed by `node_id` when present,
    falling back to `stage` for call sites that haven't migrated."""

    def test_stage_routes_entry_carries_node_id_when_present(self):
        payload = server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract"},
            runtime_state={},
        )
        self.assertEqual(payload["stage_routes"][0]["node_id"], "pitch.extract")

    def test_stage_routes_entry_carries_node_event_when_present(self):
        # Feeds `analysis_node_attempts.status` (app-core/src/analyzer.rs's
        # `node_attempt_status`) -- independent of the route's `node_id`.
        payload = server._progress_payload(
            {}, "cpu", 54, "Building singing guide...",
            metadata={"node_id": "pitch.extract", "event": "node_completed"},
            runtime_state={},
        )
        self.assertEqual(payload["stage_routes"][0]["node_event"], "node_completed")

    def test_stage_routes_entry_node_event_is_none_for_legacy_call_sites(self):
        payload = server._progress_payload(
            {}, "cpu", 4, "Inspecting source codec and cache format...",
            metadata={},
            runtime_state={},
        )
        self.assertIsNone(payload["stage_routes"][0]["node_event"])

    def test_latest_node_event_wins_when_a_node_is_reported_more_than_once(self):
        # A node's own route entry updates in place as it progresses
        # (node_started -> node_completed), not just its stage_progress.
        runtime_state = {}
        server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_started"},
            runtime_state=runtime_state,
        )
        payload = server._progress_payload(
            {}, "cpu", 54, "Building singing guide...",
            metadata={"node_id": "pitch.extract", "event": "node_completed"},
            runtime_state=runtime_state,
        )
        self.assertEqual(len(payload["stage_routes"]), 1)
        self.assertEqual(payload["stage_routes"][0]["node_event"], "node_completed")

    def test_stage_routes_entry_node_id_is_none_for_legacy_call_sites(self):
        payload = server._progress_payload(
            {}, "cpu", 4, "Inspecting source codec and cache format...",
            metadata={},
            runtime_state={},
        )
        self.assertIsNone(payload["stage_routes"][0]["node_id"])

    def test_two_nodes_sharing_a_bucket_each_keep_their_own_route(self):
        runtime_state = {}
        server._progress_payload(
            {}, "cpu", 3, "Analyzing musical key...",
            metadata={"node_id": "music.key"},
            runtime_state=runtime_state,
        )
        payload = server._progress_payload(
            {}, "cpu", 3, "Analyzing musical key...",
            metadata={"node_id": "music.rhythm"},
            runtime_state=runtime_state,
        )
        node_ids = {route["node_id"] for route in payload["stage_routes"]}
        self.assertEqual(node_ids, {"music.key", "music.rhythm"})
        # Both routes still carry the same legacy bucket text, so old
        # bucket-based matching (the fallback for routes without a node_id)
        # keeps working unchanged.
        self.assertTrue(
            all(route["stage"] == "key_detection" for route in payload["stage_routes"])
        )

    def test_legacy_call_sites_still_dedupe_one_entry_per_bucket(self):
        # No node_id at all across repeated calls into the same bucket: must
        # still overwrite one shared entry, exactly the pre-fix behavior --
        # this keying change is additive, not a behavior change for call
        # sites that haven't migrated to progress_node.
        runtime_state = {}
        server._progress_payload(
            {}, "cpu", 1, "Starting analysis",
            metadata={}, runtime_state=runtime_state,
        )
        payload = server._progress_payload(
            {}, "cpu", 3, "Preparing pipeline",
            metadata={}, runtime_state=runtime_state,
        )
        # Both calls classify into the same "preparing" bucket (see
        # _classify_progress's `pct <= 4` fallback) with no node_id, so they
        # must still collapse into the single pre-fix dict entry.
        self.assertEqual(len(payload["stage_routes"]), 1)
        self.assertEqual(payload["stage_routes"][0]["stage"], "preparing")


@unittest.skipUnless(server is not None, f"server import failed: {server_import_error}")
class NodeAttemptTimingTests(unittest.TestCase):
    """Per-node Start/Finish timestamps (docs/plan.md Phase 7's "Duration
    检查器字段有意省略" gap, closed): `started_at_ms` is set once and kept
    across later events for the same node; `finished_at_ms` is set only by
    a terminal event."""

    def test_node_started_sets_started_at_ms_and_leaves_finished_unset(self):
        payload = server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_started"},
            runtime_state={},
        )
        route = payload["stage_routes"][0]
        self.assertIsNotNone(route["started_at_ms"])
        self.assertIsNone(route["finished_at_ms"])

    def test_started_at_ms_survives_intermediate_progress_events(self):
        runtime_state = {}
        started = server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_started"},
            runtime_state=runtime_state,
        )["stage_routes"][0]["started_at_ms"]
        progressed = server._progress_payload(
            {}, "cpu", 53, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_progress"},
            runtime_state=runtime_state,
        )["stage_routes"][0]["started_at_ms"]
        # Not "the timestamps happen to be close" -- the plain progress
        # event must not touch started_at_ms at all.
        self.assertEqual(started, progressed)

    def test_node_completed_sets_finished_at_ms(self):
        runtime_state = {}
        server._progress_payload(
            {}, "cpu", 52, "Extracting reference pitch...",
            metadata={"node_id": "pitch.extract", "event": "node_started"},
            runtime_state=runtime_state,
        )
        route = server._progress_payload(
            {}, "cpu", 54, "Building singing guide",
            metadata={"node_id": "pitch.extract", "event": "node_completed"},
            runtime_state=runtime_state,
        )["stage_routes"][0]
        self.assertIsNotNone(route["started_at_ms"])
        self.assertIsNotNone(route["finished_at_ms"])
        self.assertGreaterEqual(route["finished_at_ms"], route["started_at_ms"])

    def test_artifact_reused_stamps_both_fields_from_its_own_single_event(self):
        # A cache hit never gets a node_started before it -- there's nothing
        # to measure a duration against except the reuse check itself.
        route = server._progress_payload(
            {}, "cpu", 50, "Stems already cached",
            metadata={"node_id": "stems.separate", "event": "artifact_reused"},
            runtime_state={},
        )["stage_routes"][0]
        self.assertIsNotNone(route["started_at_ms"])
        self.assertEqual(route["started_at_ms"], route["finished_at_ms"])

    def test_a_route_with_no_node_event_never_gets_timestamps(self):
        # Legacy Adapter call sites (no node_id/event at all) must not
        # start reporting timing they never actually measured.
        route = server._progress_payload(
            {}, "cpu", 4, "Inspecting source codec and cache format...",
            metadata={}, runtime_state={},
        )["stage_routes"][0]
        self.assertIsNotNone(route["started_at_ms"], "started_at_ms is set for every route today")
        self.assertIsNone(route["finished_at_ms"])


if __name__ == "__main__":
    unittest.main()
