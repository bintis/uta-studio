"""§4.4 artifact split coverage: `lyrics.transcribe` (`run_transcription`)
must write real `{hash}_recognized_text.json` / `{hash}_asr_segments.json`,
and `chart.build_candidate` (`build_candidate_chart`) must write
`{hash}_timed_transcript.json` -- alongside the unchanged
`{hash}_transcript.json` compatibility file `run_pipeline` has always
written. docs/plan.md §4.4 / app-core/src/analysis_graph.rs's
`lyrics.transcribe -> [RecognizedText, AsrSegments]` /
`lyrics.align|lyrics.import_timed -> [TimedTranscript]` edges are the
target shape being locked down here, run against `pipeline.run_pipeline`
itself with only the real ML calls (`transcribe_vocals`/`align_lyrics`/
`run_music_analysis`/`run_stem_separation`/`analyze_pitch`) mocked out --
not a test of a smaller helper in isolation.
"""

from __future__ import annotations

import contextlib
import json
import os
import shutil
import tempfile
import unittest
from unittest import mock

try:
    import pipeline  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    pipeline = None
    pipeline_import_error = exc
else:
    pipeline_import_error = None


@contextlib.contextmanager
def _tmp_dir():
    d = tempfile.mkdtemp(prefix="uta-studio-transcript-split-test-")
    try:
        yield d
    finally:
        shutil.rmtree(d, ignore_errors=True)


def _music_analysis_mock():
    return mock.patch.object(
        pipeline,
        "run_music_analysis",
        return_value={"key": {"tonic": "C", "scale": "major"}, "rhythm": {"bpm": 120.0}},
    )


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class AsrRouteSplitTests(unittest.TestCase):
    def test_writes_all_three_split_files_and_leaves_transcript_json_unchanged(self):
        with _tmp_dir() as output_dir:
            file_hash = "songAsrSplit"
            fake_result = {
                "language": "ja",
                "source": "generated",
                "segments": [
                    {
                        "text": "hello",
                        "start": 0.0,
                        "end": 1.0,
                        "words": [{"word": "hello", "start": 0.0, "end": 1.0}],
                    }
                ],
                "_pre_alignment_segments": [{"text": "hello", "start": 0.0, "end": 1.2}],
            }

            with (
                _music_analysis_mock(),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
                mock.patch.object(pipeline, "analyze_pitch"),
                mock.patch.object(
                    pipeline, "transcribe_vocals", return_value=dict(fake_result)
                ) as mocked_transcribe,
            ):
                pipeline.run_pipeline("/tmp/song.flac", output_dir, file_hash, "cpu")
                mocked_transcribe.assert_called_once()

            recognized_text_path = os.path.join(output_dir, f"{file_hash}_recognized_text.json")
            asr_segments_path = os.path.join(output_dir, f"{file_hash}_asr_segments.json")
            timed_transcript_path = os.path.join(output_dir, f"{file_hash}_timed_transcript.json")
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")

            self.assertTrue(os.path.isfile(recognized_text_path))
            self.assertTrue(os.path.isfile(asr_segments_path))
            self.assertTrue(os.path.isfile(timed_transcript_path))

            with open(recognized_text_path, encoding="utf-8") as f:
                recognized_text = json.load(f)
            # recognized_text.json is the real pre-alignment ASR output, not
            # a duplicate of the final word-aligned segments.
            self.assertEqual(recognized_text["segments"], fake_result["_pre_alignment_segments"])
            self.assertEqual(recognized_text["language"], "ja")

            with open(asr_segments_path, encoding="utf-8") as f:
                asr_segments = json.load(f)
            self.assertEqual(asr_segments["segments"], fake_result["segments"])
            self.assertNotIn("_pre_alignment_segments", asr_segments)
            # Reflects the transcribe node's own output, not yet patched
            # with chart-build-time key/tempo/bpm.
            self.assertNotIn("key", asr_segments)

            with open(timed_transcript_path, encoding="utf-8") as f:
                timed_transcript = json.load(f)
            with open(transcript_path, encoding="utf-8") as f:
                transcript = json.load(f)
            self.assertEqual(timed_transcript, transcript)
            self.assertEqual(transcript["key"], "C")
            self.assertEqual(transcript["bpm"], 120.0)
            self.assertNotIn("_pre_alignment_segments", transcript)

    def test_parakeet_without_pre_alignment_segments_falls_back_to_final_segments(self):
        # Parakeet emits word timing natively -- transcribe.py never sets
        # `_pre_alignment_segments` for that engine. recognized_text.json
        # must still be produced, just from the same segments as
        # asr_segments.json, honestly reflecting that route's real
        # characteristics rather than fabricating a distinction that
        # doesn't exist for it.
        with _tmp_dir() as output_dir:
            file_hash = "songParakeetSplit"
            fake_result = {
                "language": "en",
                "source": "generated",
                "segments": [{"text": "hi", "start": 0.0, "end": 0.5, "words": []}],
            }

            with (
                _music_analysis_mock(),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
                mock.patch.object(pipeline, "analyze_pitch"),
                mock.patch.object(pipeline, "transcribe_vocals", return_value=dict(fake_result)),
            ):
                pipeline.run_pipeline("/tmp/song.flac", output_dir, file_hash, "cpu", engine="parakeet")

            recognized_text_path = os.path.join(output_dir, f"{file_hash}_recognized_text.json")
            asr_segments_path = os.path.join(output_dir, f"{file_hash}_asr_segments.json")
            with open(recognized_text_path, encoding="utf-8") as f:
                recognized_text = json.load(f)
            with open(asr_segments_path, encoding="utf-8") as f:
                asr_segments = json.load(f)
            self.assertEqual(recognized_text["segments"], asr_segments["segments"])


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class KnownLyricsRouteSplitTests(unittest.TestCase):
    def test_alignment_route_does_not_write_recognized_text_or_asr_segments(self):
        with _tmp_dir() as output_dir:
            file_hash = "songKnownLyricsSplit"
            lyrics_path = os.path.join(output_dir, f"{file_hash}_lyrics.txt")
            with open(lyrics_path, "w", encoding="utf-8") as f:
                f.write("hello world\n")

            fake_align_result = {
                "language": "en",
                "source": "lyrics",
                "alignment_backend_requested": "whisperx",
                "alignment_backend_used": "whisperx",
                "segments": [{"text": "hello world", "start": 0.0, "end": 1.0, "words": []}],
            }

            with (
                _music_analysis_mock(),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
                mock.patch.object(pipeline, "analyze_pitch"),
                mock.patch.object(
                    pipeline, "align_lyrics", return_value=dict(fake_align_result)
                ) as mocked_align,
            ):
                pipeline.run_pipeline(
                    "/tmp/song.flac", output_dir, file_hash, "cpu", lyrics_path=lyrics_path,
                )
                mocked_align.assert_called_once()

            recognized_text_path = os.path.join(output_dir, f"{file_hash}_recognized_text.json")
            asr_segments_path = os.path.join(output_dir, f"{file_hash}_asr_segments.json")
            timed_transcript_path = os.path.join(output_dir, f"{file_hash}_timed_transcript.json")

            # No ASR ran on the known-lyrics route -- lyrics.align only
            # produces TimedTranscript per the DAG model.
            self.assertFalse(os.path.isfile(recognized_text_path))
            self.assertFalse(os.path.isfile(asr_segments_path))
            self.assertTrue(os.path.isfile(timed_transcript_path))


@unittest.skipUnless(pipeline is not None, f"pipeline import failed: {pipeline_import_error}")
class TimedLrcRouteSplitTests(unittest.TestCase):
    def test_skip_transcription_route_writes_timed_transcript_only(self):
        with _tmp_dir() as output_dir:
            file_hash = "songTimedLrcSplit"
            transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump({"segments": [], "source": "lrc"}, f)

            with (
                _music_analysis_mock(),
                mock.patch.object(pipeline, "run_stem_separation", return_value="/tmp/vocals.wav"),
                mock.patch.object(pipeline, "analyze_pitch"),
            ):
                pipeline.run_pipeline(
                    "/tmp/song.flac", output_dir, file_hash, "cpu", skip_transcription=True,
                )

            recognized_text_path = os.path.join(output_dir, f"{file_hash}_recognized_text.json")
            asr_segments_path = os.path.join(output_dir, f"{file_hash}_asr_segments.json")
            timed_transcript_path = os.path.join(output_dir, f"{file_hash}_timed_transcript.json")

            self.assertFalse(os.path.isfile(recognized_text_path))
            self.assertFalse(os.path.isfile(asr_segments_path))
            self.assertTrue(os.path.isfile(timed_transcript_path))
            with open(timed_transcript_path, encoding="utf-8") as f:
                timed_transcript = json.load(f)
            with open(transcript_path, encoding="utf-8") as f:
                transcript = json.load(f)
            self.assertEqual(timed_transcript, transcript)


if __name__ == "__main__":
    unittest.main()
