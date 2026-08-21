"""§4.4: `_build_result_from_raw_segments` must freeze real ASR output
before forced alignment. Alignment is intentionally deferred until
`pipeline.run_transcription` has atomically committed the ASR artifacts.
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
    def test_captures_pre_alignment_segments_and_defers_alignment(self):
        raw_segments = [
            {"text": "hello world", "start": 0.0, "end": 1.5, "id": "whisperx-internal-field"},
        ]
        with (
            mock.patch.object(transcribe, "_filter_hallucinations", side_effect=lambda segs, _dur: segs),
            mock.patch.object(transcribe, "_align_and_build") as mocked_align,
        ):
            result = transcribe._build_result_from_raw_segments(
                raw_segments, full_audio=[0.0] * 16000, language="en",
                duration_secs=1.5, device="cpu", pre_align_cleanup=None, engine_used="whisper",
            )
            mocked_align.assert_not_called()

        self.assertEqual(result["segments"], result["_pre_alignment_segments"])
        self.assertEqual(result["source"], "generated")
        self.assertEqual(result["_alignment_raw_segments"], raw_segments)

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
            mock.patch.object(transcribe, "_align_and_build"),
        ):
            result = transcribe._build_result_from_raw_segments(
                raw_segments, full_audio=[0.0] * 16000, language="en",
                duration_secs=1.1, device="cpu", pre_align_cleanup=None, engine_used="whisper",
            )

        self.assertEqual(len(result["_pre_alignment_segments"]), 1)
        self.assertEqual(result["_pre_alignment_segments"][0]["text"], "real lyric")


if __name__ == "__main__":
    unittest.main()
