"""Phase 4 executor-unification coverage: `run_pipeline`'s `skip_pitch`
parameter (app-core/src/analyzer.rs::run_analysis_plan's new
"Disable pitch.extract for this run" path -- see
docs/analysis-dag-redesign.md Phase 4 status note). `skip_transcription`/
`skip_separation` already had real production call sites (STEMS_ONLY/
PITCH_ONLY) exercising them end-to-end; `skip_pitch` is new and had no
integration coverage before this, so it is locked down here directly
against `run_pipeline` with everything else mocked out.
"""

from __future__ import annotations

import json
import os
import unittest
from unittest import mock

try:
    import pipeline  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    pipeline = None
    pipeline_import_error = exc
else:
    pipeline_import_error = None


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class SkipPitchFlagTests(unittest.TestCase):
    def _run(self, output_dir, file_hash, *, skip_pitch):
        transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
        with open(transcript_path, "w", encoding="utf-8") as f:
            json.dump({"segments": []}, f)

        with (
            mock.patch.object(
                pipeline,
                "run_music_analysis",
                return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
            ),
            mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
            mock.patch.object(pipeline, "analyze_pitch") as mocked_analyze_pitch,
        ):
            pipeline.run_pipeline(
                "/tmp/song.flac",
                output_dir,
                file_hash,
                "cpu",
                skip_transcription=True,
                skip_pitch=skip_pitch,
            )
            return mocked_analyze_pitch

    def test_skip_pitch_true_never_calls_analyze_pitch(self):
        with _tmp_dir() as output_dir:
            mocked_analyze_pitch = self._run(output_dir, "songSkipPitch", skip_pitch=True)
            mocked_analyze_pitch.assert_not_called()

    def test_skip_pitch_false_still_extracts_pitch_when_no_cache_exists(self):
        with _tmp_dir() as output_dir:
            mocked_analyze_pitch = self._run(output_dir, "songKeepPitch", skip_pitch=False)
            mocked_analyze_pitch.assert_called_once()

    def test_skip_pitch_true_still_patches_key_and_bpm_onto_the_transcript(self):
        # A disabled node must not silently break the run's other outputs --
        # only pitch.extract's own artifacts are affected.
        with _tmp_dir() as output_dir:
            file_hash = "songSkipPitchTranscript"
            self._run(output_dir, file_hash, skip_pitch=True)
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "r", encoding="utf-8") as f:
                transcript = json.load(f)
            self.assertEqual(transcript["key"], "C")
            self.assertEqual(transcript["bpm"], 120.0)


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class FreezeFlagTests(unittest.TestCase):
    """Phase 4 §4.5 Freeze consumer: `freeze_separation`/`freeze_pitch`
    (app-core/src/analyzer.rs::freeze_analysis_node_outputs_for_run's
    Python-side counterpart)."""

    def test_freeze_separation_reuses_existing_stems_without_calling_the_real_separator(self):
        with _tmp_dir() as output_dir:
            file_hash = "songFreezeStems"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": []}, f)
            vocals_path = os.path.join(output_dir, f"{file_hash}_vocals.flac")
            instrumental_path = os.path.join(output_dir, f"{file_hash}_instrumental.flac")
            with open(vocals_path, "wb") as f:
                f.write(b"fake-vocals")
            with open(instrumental_path, "wb") as f:
                f.write(b"fake-instrumental")

            with (
                mock.patch.object(
                    pipeline,
                    "run_music_analysis",
                    return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
                ),
                mock.patch.object(pipeline, "_try_execute_audio_plan") as mocked_separator,
                mock.patch.object(pipeline, "analyze_pitch"),
            ):
                pipeline.run_pipeline(
                    "/tmp/song.flac",
                    output_dir,
                    file_hash,
                    "cpu",
                    skip_transcription=True,
                    skip_pitch=True,
                    freeze_separation=True,
                )
                mocked_separator.assert_not_called()

    def test_freeze_separation_without_a_cached_stem_raises_instead_of_separating(self):
        with _tmp_dir() as output_dir:
            file_hash = "songFreezeStemsMissing"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": []}, f)

            with (
                mock.patch.object(
                    pipeline,
                    "run_music_analysis",
                    return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
                ),
                mock.patch.object(pipeline, "_try_execute_audio_plan") as mocked_separator,
            ):
                with self.assertRaises(RuntimeError):
                    pipeline.run_pipeline(
                        "/tmp/song.flac",
                        output_dir,
                        file_hash,
                        "cpu",
                        skip_transcription=True,
                        freeze_separation=True,
                    )
                mocked_separator.assert_not_called()

    def test_freeze_pitch_reuses_existing_guide_without_calling_analyze_pitch(self):
        with _tmp_dir() as output_dir:
            file_hash = "songFreezePitch"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": []}, f)
            with open(os.path.join(output_dir, f"{file_hash}_pitch_track.json"), "w") as f:
                json.dump({"frames": []}, f)
            with open(os.path.join(output_dir, f"{file_hash}_pitch_notes.json"), "w") as f:
                json.dump({"notes": []}, f)

            with (
                mock.patch.object(
                    pipeline,
                    "run_music_analysis",
                    return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
                ),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
                mock.patch.object(pipeline, "analyze_pitch") as mocked_analyze_pitch,
            ):
                pipeline.run_pipeline(
                    "/tmp/song.flac",
                    output_dir,
                    file_hash,
                    "cpu",
                    skip_transcription=True,
                    freeze_pitch=True,
                )
                mocked_analyze_pitch.assert_not_called()


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class BypassFlagTests(unittest.TestCase):
    """Phase 4 §4.5 Bypass consumer: `bypass_separation_with_original_mix`
    (app-core/src/analyzer.rs::bypass_analysis_node_with_original_mix_for_run's
    Python-side counterpart)."""

    def test_bypass_uses_the_original_mix_as_the_vocals_path_without_separating(self):
        with _tmp_dir() as output_dir:
            file_hash = "songBypassStems"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": []}, f)
            audio_path = "/tmp/original_mix.flac"

            with (
                mock.patch.object(
                    pipeline,
                    "run_music_analysis",
                    return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
                ),
                mock.patch.object(pipeline, "run_stem_separation") as mocked_separation,
                mock.patch.object(pipeline, "run_pitch_analysis") as mocked_pitch,
            ):
                pipeline.run_pipeline(
                    audio_path,
                    output_dir,
                    file_hash,
                    "cpu",
                    skip_transcription=True,
                    skip_separation=True,
                    bypass_separation_with_original_mix=True,
                )
                mocked_separation.assert_not_called()
                # The original mix -- not None -- must reach pitch analysis,
                # so a downstream node relying on "we have some vocals path"
                # doesn't quietly get skipped or crash on `None`.
                mocked_pitch.assert_called_once()
                called_vocals_path = mocked_pitch.call_args[0][0]
                self.assertEqual(called_vocals_path, audio_path)

    def test_bypass_without_skip_separation_leaves_the_real_separator_in_control(self):
        # bypass_separation_with_original_mix on its own (Rust never sends
        # this combination, but the Python function must still behave
        # sanely) must not short-circuit a real separation call -- only the
        # `elif` branch (skip_separation=True) substitutes the original mix.
        with _tmp_dir() as output_dir:
            file_hash = "songBypassIgnoredWithoutSkip"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": []}, f)

            with (
                mock.patch.object(
                    pipeline,
                    "run_music_analysis",
                    return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
                ),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/real_vocals.wav") as mocked_separation,
                mock.patch.object(pipeline, "run_pitch_analysis"),
            ):
                pipeline.run_pipeline(
                    "/tmp/song.flac",
                    output_dir,
                    file_hash,
                    "cpu",
                    skip_transcription=True,
                    skip_separation=False,
                    bypass_separation_with_original_mix=True,
                )
                mocked_separation.assert_called_once()


def _tmp_dir():
    import contextlib
    import shutil
    import tempfile

    @contextlib.contextmanager
    def _ctx():
        d = tempfile.mkdtemp(prefix="uta-studio-run-pipeline-skip-pitch-test-")
        try:
            yield d
        finally:
            shutil.rmtree(d, ignore_errors=True)

    return _ctx()


if __name__ == "__main__":
    unittest.main()
