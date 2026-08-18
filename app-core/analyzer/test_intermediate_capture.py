from __future__ import annotations

import os
import tempfile
import unittest

try:
    import numpy as np
    import soundfile as sf
    from audio import write_capture_flac_atomic
except ImportError as exc:
    np = None
    sf = None
    write_capture_flac_atomic = None
    runtime_import_error = exc
else:
    runtime_import_error = None


@unittest.skipUnless(
    write_capture_flac_atomic is not None,
    f"audio capture runtime import failed: {runtime_import_error}",
)
class IntermediateCaptureTests(unittest.TestCase):
    def test_explicit_capture_materializes_real_lossless_audio_atomically(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-capture-") as root:
            destination = os.path.join(root, "preprocessed.flac")
            source = np.linspace(-0.25, 0.25, 1600, dtype=np.float32)
            write_capture_flac_atomic(destination, source, 16000)

            self.assertTrue(os.path.isfile(destination))
            decoded, sample_rate = sf.read(destination, dtype="float32")
            self.assertEqual(sample_rate, 16000)
            self.assertEqual(len(decoded), len(source))
            self.assertLess(float(np.max(np.abs(decoded - source))), 1e-5)
            self.assertFalse(any(name.endswith(".tmp") for name in os.listdir(root)))


if __name__ == "__main__":
    unittest.main()
