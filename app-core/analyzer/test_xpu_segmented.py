from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import numpy as np
import soundfile as sf

from audio_processors.xpu_segmented import (
    MAX_WINDOW_SECONDS,
    audio_windows,
    intel_battlemage_present,
    run_segmented_mdxc_xpu,
)


class SegmentedXpuTests(unittest.TestCase):
    def test_battlemage_detection_reads_pci_ids_without_xpu(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            drm_root = Path(raw)
            device_root = drm_root / "renderD129" / "device"
            device_root.mkdir(parents=True)
            (device_root / "vendor").write_text("0x8086\n", encoding="ascii")
            (device_root / "device").write_text("0xe20b\n", encoding="ascii")

            self.assertTrue(intel_battlemage_present(drm_root))
            (device_root / "device").write_text("0x56a0\n", encoding="ascii")
            self.assertFalse(intel_battlemage_present(drm_root))

    def test_windows_are_bounded_and_overlap_by_two_seconds(self) -> None:
        windows = audio_windows(25 * 44100, 44100)

        self.assertEqual(
            [(window.start_frame, window.end_frame) for window in windows],
            [
                (0, 12 * 44100),
                (10 * 44100, 22 * 44100),
                (20 * 44100, 25 * 44100),
            ],
        )
        self.assertTrue(
            all(window.frame_count <= MAX_WINDOW_SECONDS * 44100 for window in windows)
        )

    def test_long_audio_uses_fresh_workers_and_crossfades_to_exact_length(self) -> None:
        sample_rate = 44100
        frames = 25 * sample_rate
        time_axis = np.arange(frames, dtype=np.float32) / sample_rate
        source_audio = np.stack(
            (
                0.1 * np.sin(2 * np.pi * 220 * time_axis),
                0.1 * np.sin(2 * np.pi * 330 * time_axis),
            ),
            axis=1,
        )

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source.wav"
            attempt = root / "attempt"
            attempt.mkdir()
            sf.write(
                str(source),
                source_audio,
                sample_rate,
                format="WAV",
                subtype="FLOAT",
            )
            requests: list[dict[str, object]] = []
            input_durations: list[float] = []

            def worker(request):
                request = dict(request)
                requests.append(request)
                data, actual_rate = sf.read(
                    str(request["input_path"]), dtype="float32", always_2d=True
                )
                self.assertEqual(actual_rate, sample_rate)
                input_durations.append(len(data) / actual_rate)
                worker_dir = Path(str(request["work_dir"]))
                worker_dir.mkdir(parents=True)
                stems = {}
                for stem, scale in (("Vocals", 1.0), ("Instrumental", 0.5)):
                    # A silent semantic may be omitted by audio-separator for a
                    # short window. The merger must materialize silence rather
                    # than reject an otherwise valid complete song.
                    if stem == "Instrumental" and len(requests) == 2:
                        continue
                    path = worker_dir / f"{stem.lower()}.wav"
                    sf.write(
                        str(path),
                        data * scale,
                        sample_rate,
                        format="WAV",
                        subtype="FLOAT",
                    )
                    stems[stem] = str(path)
                return {"stems": stems, "sample_rate": sample_rate, "channels": 2}

            progress = []
            outputs = run_segmented_mdxc_xpu(
                request={"runner": "mdxc_torch"},
                input_path=source,
                attempt_dir=attempt,
                descriptor_names={
                    "Vocals": "step_test__vocals",
                    "Instrumental": "step_test__instrumental",
                },
                expected_stems=("Vocals", "Instrumental"),
                run_worker=worker,
                progress_sink=lambda percent, message, **metadata: progress.append(
                    (percent, message, metadata)
                ),
                force_segmented=True,
            )

            self.assertEqual(len(requests), 3)
            self.assertTrue(all(request["allow_missing_stems"] for request in requests))
            self.assertTrue(
                all(duration <= MAX_WINDOW_SECONDS for duration in input_durations)
            )
            vocals, vocals_rate = sf.read(
                str(outputs["Vocals"]), dtype="float32", always_2d=True
            )
            instrumental, instrumental_rate = sf.read(
                str(outputs["Instrumental"]), dtype="float32", always_2d=True
            )
            self.assertEqual(vocals_rate, sample_rate)
            self.assertEqual(instrumental_rate, sample_rate)
            self.assertEqual(len(vocals), frames)
            self.assertEqual(len(instrumental), frames)
            np.testing.assert_allclose(vocals, source_audio, atol=2e-6)
            self.assertTrue(np.isfinite(instrumental).all())
            self.assertEqual(len(progress), 3)
            self.assertFalse((attempt / "xpu-windows").exists())

    def test_failed_window_does_not_publish_partial_merged_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source.wav"
            attempt = root / "attempt"
            attempt.mkdir()
            sf.write(
                str(source),
                np.zeros((13 * 44100, 2), dtype=np.float32),
                44100,
                format="WAV",
                subtype="FLOAT",
            )
            calls = 0

            def worker(_request):
                nonlocal calls
                calls += 1
                raise RuntimeError("simulated XPU worker failure")

            with self.assertRaisesRegex(RuntimeError, "simulated XPU worker failure"):
                run_segmented_mdxc_xpu(
                    request={"runner": "mdxc_torch"},
                    input_path=source,
                    attempt_dir=attempt,
                    descriptor_names={
                        "Vocals": "step_test__vocals",
                        "Instrumental": "step_test__instrumental",
                    },
                    expected_stems=("Vocals", "Instrumental"),
                    run_worker=worker,
                    force_segmented=True,
                )

            self.assertEqual(calls, 1)
            self.assertFalse((attempt / "step_test__vocals.wav").exists())
            self.assertFalse((attempt / "step_test__instrumental.wav").exists())
            self.assertEqual(list(attempt.glob(".*.segmented.tmp")), [])

    def test_resampled_windows_preserve_exact_song_duration(self) -> None:
        input_rate = 48000
        output_rate = 44100
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source.wav"
            attempt = root / "attempt"
            attempt.mkdir()
            sf.write(
                str(source),
                np.zeros((13 * input_rate, 2), dtype=np.float32),
                input_rate,
                format="WAV",
                subtype="FLOAT",
            )
            calls = 0

            def worker(request):
                nonlocal calls
                calls += 1
                info = sf.info(str(request["input_path"]))
                output_frames = round(info.frames * output_rate / info.samplerate)
                worker_dir = Path(str(request["work_dir"]))
                worker_dir.mkdir(parents=True)
                stems = {}
                for stem in ("Vocals", "Instrumental"):
                    path = worker_dir / f"{stem.lower()}.wav"
                    sf.write(
                        str(path),
                        np.zeros((output_frames, 2), dtype=np.float32),
                        output_rate,
                        format="WAV",
                        subtype="FLOAT",
                    )
                    stems[stem] = str(path)
                return {"stems": stems, "sample_rate": output_rate, "channels": 2}

            outputs = run_segmented_mdxc_xpu(
                request={"runner": "mdxc_torch"},
                input_path=source,
                attempt_dir=attempt,
                descriptor_names={
                    "Vocals": "step_test__vocals",
                    "Instrumental": "step_test__instrumental",
                },
                expected_stems=("Vocals", "Instrumental"),
                run_worker=worker,
                force_segmented=True,
            )

            self.assertEqual(calls, 2)
            for path in outputs.values():
                info = sf.info(str(path))
                self.assertEqual(info.samplerate, output_rate)
                self.assertEqual(info.frames, 13 * output_rate)


if __name__ == "__main__":
    unittest.main()
