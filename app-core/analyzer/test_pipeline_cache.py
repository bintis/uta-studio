"""Phase 0 regression baseline for the analysis DAG redesign
(see /docs/analysis-dag-redesign.md, section 13).

Locks down cache-signature and stage-classification behavior that the
redesign phases (2 and 3) will replace with an explicit ArtifactRevision
signature and a structured event protocol. Until then, these tests are the
contract that must keep holding: stem cache identity must stay independent
of detected key/tempo, legacy stem discovery must stay read-only, and the
current text-classification stage mapping must not silently drift while
later phases are implemented on top of it.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path

try:
    import pipeline  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    pipeline = None
    pipeline_import_error = exc
else:
    pipeline_import_error = None

try:
    import server  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    server = None
    server_import_error = exc
else:
    server_import_error = None


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class SeparatorCacheSignatureTests(unittest.TestCase):
    """Stem identity must be (separator, options) only -- never key/BPM."""

    def test_matches_when_separator_and_options_are_identical(self):
        with _tmp_dir() as output_dir:
            pipeline._write_separator_marker(output_dir, "songA", "karaoke", {"segment_size": 256})
            self.assertTrue(
                pipeline._cached_separator_matches(
                    output_dir, "songA", "karaoke", {"segment_size": 256}
                )
            )

    def test_does_not_match_a_different_separator_backend(self):
        with _tmp_dir() as output_dir:
            pipeline._write_separator_marker(output_dir, "songA", "karaoke", {"segment_size": 256})
            self.assertFalse(
                pipeline._cached_separator_matches(
                    output_dir, "songA", "demucs", {"segment_size": 256}
                )
            )

    def test_does_not_match_different_options_for_the_same_separator(self):
        with _tmp_dir() as output_dir:
            pipeline._write_separator_marker(output_dir, "songA", "karaoke", {"segment_size": 256})
            self.assertFalse(
                pipeline._cached_separator_matches(
                    output_dir, "songA", "karaoke", {"segment_size": 512}
                )
            )

    def test_signature_has_no_key_or_tempo_parameter_at_all(self):
        # The function signature itself is the guarantee: a BPM/key
        # algorithm update has no argument to plumb through here, so it
        # structurally cannot affect stem cache validity.
        import inspect

        params = list(inspect.signature(pipeline._cached_separator_matches).parameters)
        self.assertEqual(params, ["output_dir", "file_hash", "separator", "options"])
        self.assertNotIn("key", params)
        self.assertNotIn("tempo", params)
        self.assertNotIn("bpm", params)


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class LegacyStemCacheDiscoveryTests(unittest.TestCase):
    """Pre-decoupling stem filenames must still be found, and never touched."""

    def test_finds_legacy_pair_at_tempo_1_0(self):
        with _tmp_dir() as output_dir:
            vocals = Path(output_dir) / "songA_vocals_Cmaj_1.0.flac"
            instrumental = Path(output_dir) / "songA_instrumental_Cmaj_1.0.flac"
            vocals.write_bytes(b"fake-audio")
            instrumental.write_bytes(b"fake-audio")

            result = pipeline._find_legacy_stem_cache(output_dir, "songA", "flac")

            self.assertIsNotNone(result)
            found_vocals, found_instrumental = result
            self.assertEqual(Path(found_vocals), vocals)
            self.assertEqual(Path(found_instrumental), instrumental)

    def test_ignores_a_non_default_tempo_variant(self):
        with _tmp_dir() as output_dir:
            vocals = Path(output_dir) / "songA_vocals_Cmaj_1.2.flac"
            instrumental = Path(output_dir) / "songA_instrumental_Cmaj_1.2.flac"
            vocals.write_bytes(b"fake-audio")
            instrumental.write_bytes(b"fake-audio")

            result = pipeline._find_legacy_stem_cache(output_dir, "songA", "flac")

            self.assertIsNone(result)

    def test_never_mutates_what_it_finds(self):
        with _tmp_dir() as output_dir:
            vocals = Path(output_dir) / "songA_vocals_Cmaj_1.0.flac"
            instrumental = Path(output_dir) / "songA_instrumental_Cmaj_1.0.flac"
            vocals.write_bytes(b"fake-audio")
            instrumental.write_bytes(b"fake-audio")
            before_vocals = vocals.read_bytes()
            before_instrumental = instrumental.read_bytes()

            pipeline._find_legacy_stem_cache(output_dir, "songA", "flac")

            self.assertTrue(vocals.is_file())
            self.assertTrue(instrumental.is_file())
            self.assertEqual(vocals.read_bytes(), before_vocals)
            self.assertEqual(instrumental.read_bytes(), before_instrumental)

    def test_returns_none_when_no_legacy_pair_exists(self):
        with _tmp_dir() as output_dir:
            result = pipeline._find_legacy_stem_cache(output_dir, "songA", "flac")
            self.assertIsNone(result)


@unittest.skipUnless(server is not None, f"server import failed: {server_import_error}")
class ClassifyProgressStageBaselineTests(unittest.TestCase):
    """Locks the current text-classification stage mapping (server.py::
    _classify_progress) for a fixed table of real production progress
    messages. This function is the thing Phase 3's structured event
    protocol replaces outright -- until then, incidental message-text
    edits during Phase 1/2 implementation must not silently reclassify a
    node's stage.
    """

    # (pct, message) -> expected stage id, taken verbatim from real
    # progress(...) call sites in pipeline.py/stems.py/transcribe.py/align.py
    # at the time of this audit (file:line noted per case).
    CASES = [
        (4, "Inspecting source codec and cache format...", "preparing"),  # pipeline.py:136
        (3, "Analyzing musical key...", "key_detection"),  # pipeline.py:292
        (10, "Loading audio file...", "separation"),  # stems.py:85
        (55, "Loading audio (/tmp/vocals.wav)...", "audio_preprocessing"),  # transcribe.py:43
        (52, "Extracting reference pitch...", "pitch"),  # pipeline.py:376
        (56, "Detecting vocal region...", "audio_preprocessing"),  # transcribe.py:48
        (60, "Transcribing vocals...", "transcription"),  # transcribe.py:253
        (90, "MMS Karaoke alignment complete: 42 segments", "alignment"),  # align.py:118 (shape)
        (95, "Writing transcript...", "finalizing"),  # pipeline.py:403/428
        (100, "Analysis complete", "complete"),  # server.py: pct >= 100 always completes
    ]

    def test_known_messages_classify_to_their_documented_stage(self):
        for pct, message, expected_stage in self.CASES:
            with self.subTest(message=message):
                stage, _label = server._classify_progress(pct, message)
                self.assertEqual(stage, expected_stage)

    def test_stage_ranges_cover_every_classified_stage(self):
        # STAGE_RANGES is the companion table _classify_progress's return
        # values are looked up against (server.py:73-83); every id it can
        # return must have a range, or downstream stage_progress rescaling
        # breaks silently.
        for _pct, message, expected_stage in self.CASES:
            self.assertIn(expected_stage, server.STAGE_RANGES)


def _tmp_dir():
    import contextlib
    import shutil
    import tempfile

    @contextlib.contextmanager
    def _ctx():
        d = tempfile.mkdtemp(prefix="uta-studio-pipeline-cache-test-")
        try:
            yield d
        finally:
            shutil.rmtree(d, ignore_errors=True)

    return _ctx()


if __name__ == "__main__":
    unittest.main()
