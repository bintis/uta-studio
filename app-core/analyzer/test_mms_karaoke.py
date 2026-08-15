from __future__ import annotations

import unittest

import numpy as np

import mms_karaoke


class MmsKaraokeTextTests(unittest.TestCase):
    def test_kana_morae_keep_small_kana_and_sokuon_with_previous_unit(self) -> None:
        self.assertEqual(
            mms_karaoke.split_kana_morae("きゃっかん"),
            ["きゃっ", "か", "ん"],
        )

    def test_explicit_kana_and_romaji_annotations_preserve_display_text(self) -> None:
        text, tokens = mms_karaoke.prepare_line("{阻|はば}{む|む}[3σ|srisigma]")
        self.assertEqual(text, "阻む3σ")
        self.assertEqual([token["surface"] for token in tokens], ["阻", "む", "3σ"])
        self.assertEqual(tokens[-1]["units"], ["srisigma"])
        self.assertTrue(all(token["units"] for token in tokens))


class MmsKaraokeTimingTests(unittest.TestCase):
    def test_compressed_timestamps_map_back_across_removed_silence(self) -> None:
        audio = np.arange(100, dtype=np.float32)
        compressed, mapping = mms_karaoke.compress_audio(
            audio,
            [(0.0, 0.2), (0.6, 1.0)],
            sample_rate=100,
        )
        self.assertEqual(compressed.size, 60)
        self.assertAlmostEqual(mms_karaoke.map_compressed_time(0.1, mapping), 0.1)
        self.assertAlmostEqual(mms_karaoke.map_compressed_time(0.3, mapping), 0.7)
        self.assertAlmostEqual(mms_karaoke.map_compressed_time(0.6, mapping), 1.0)

    def test_tail_restoration_never_crosses_the_next_line(self) -> None:
        segments = [
            {
                "start": 1.0,
                "end": 1.5,
                "words": [{"word": "あ", "start": 1.0, "end": 1.5}],
            },
            {
                "start": 1.8,
                "end": 2.0,
                "words": [{"word": "い", "start": 1.8, "end": 2.0}],
            },
        ]
        mms_karaoke._restore_phrase_edges(
            segments,
            [(0.9, 2.1)],
            [(1.4, 2.1)],
            3.0,
        )
        self.assertLessEqual(segments[0]["end"], 1.78)
        self.assertEqual(segments[0]["words"][-1]["end"], segments[0]["end"])


if __name__ == "__main__":
    unittest.main()
