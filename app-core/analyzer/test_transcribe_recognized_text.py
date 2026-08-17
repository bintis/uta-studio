"""§4.4: `transcribe.py::_build_result_from_raw_segments` must capture the
real pre-alignment ASR output (`_pre_alignment_segments`) before wav2vec2
forced alignment refines it into word-level timing, and that transient key
must never leak past `pipeline.py::run_transcription`, which pops it off.
This file only covers the capture point itself (`transcribe.py`); the
pop-and-write behavior in `pipeline.py` is covered by
`test_transcript_artifacts.py`.
"""

from __future__ import annotations

import unittest
from unittest import mock

try:
    import transcribe  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    transcribe = None
    transcribe_import_error = exc
else:
    transcribe_import_error = None


@unittest.skipUnless(transcribe is not None, f"transcribe import failed: {transcribe_import_error}")
class PreAlignmentSegmentsCaptureTests(unittest.TestCase):
    def test_captures_pre_alignment_segments_before_alignment_and_they_survive_onto_the_result(self):
        raw_segments = [
            {"text": "hello world", "start": 0.0, "end": 1.5, "id": "whisperx-internal-field"},
        ]
        aligned_result = {
            "language": "en",
            "segments": [
                {
                    "text": "hello world",
                    "start": 0.0,
                    "end": 1.5,
                    "words": [
                        {"word": "hello", "start": 0.0, "end": 0.6},
                        {"word": "world", "start": 0.7, "end": 1.5},
                    ],
                }
            ],
        }

        with (
            mock.patch.object(transcribe, "_filter_hallucinations", side_effect=lambda segs, _dur: segs),
            mock.patch.object(transcribe, "_align_and_build", return_value=dict(aligned_result)) as mocked_align,
        ):
            result = transcribe._build_result_from_raw_segments(
                raw_segments, full_audio=[0.0] * 16000, language="en",
                duration_secs=1.5, device="cpu", pre_align_cleanup=None, engine_used="whisper",
            )
            mocked_align.assert_called_once()

        # The final result is the aligned (word-level) segments, unchanged.
        self.assertEqual(result["segments"], aligned_result["segments"])
        self.assertEqual(result["source"], "generated")

        # But the real pre-alignment ASR output was captured too -- coarse
        # (no word-level timing), stripped of whisperx's internal fields,
        # matching what recognized_text.json should contain.
        pre_alignment = result["_pre_alignment_segments"]
        self.assertEqual(
            pre_alignment,
            [{"text": "hello world", "start": 0.0, "end": 1.5}],
        )
        self.assertNotIn("id", pre_alignment[0])

    def test_pre_alignment_segments_reflect_hallucination_filtering(self):
        # The capture point sits after `_filter_hallucinations`, not before
        # -- a hallucinated segment ASR itself already discarded must not
        # reappear in recognized_text.json.
        raw_segments = [
            {"text": "real lyric", "start": 0.0, "end": 1.0},
            {"text": "[hallucinated]", "start": 1.0, "end": 1.1},
        ]
        filtered = raw_segments[:1]

        with (
            mock.patch.object(transcribe, "_filter_hallucinations", return_value=filtered),
            mock.patch.object(
                transcribe, "_align_and_build",
                return_value={"language": "en", "segments": filtered},
            ),
        ):
            result = transcribe._build_result_from_raw_segments(
                raw_segments, full_audio=[0.0] * 16000, language="en",
                duration_secs=1.1, device="cpu", pre_align_cleanup=None, engine_used="whisper",
            )

        self.assertEqual(len(result["_pre_alignment_segments"]), 1)
        self.assertEqual(result["_pre_alignment_segments"][0]["text"], "real lyric")


if __name__ == "__main__":
    unittest.main()
