#!/usr/bin/env python3
"""Isolated bridge publication/build tests; no real CMake, DSO load, or GPU."""
import importlib.util
import json
import mmap
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "native-inference/libtorch-runtime/rebuild-native-xpu.sh"
spec = importlib.util.spec_from_file_location(
    "native_publisher", ROOT / "native-inference/libtorch-runtime/publish-native-xpu.py"
)
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class NativeBridgeTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(prefix="uta-native-publication-")
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.runtime = self.root / "runtime"
        self.build = self.root / "build"
        self.build.mkdir()
        (self.runtime / "lib").mkdir(parents=True)
        self.library = self.runtime / "lib/libuta_libtorch.so"
        self.library.write_bytes(b"original mapped library")
        self.manifest = self.runtime / "runtime-manifest.json"
        self.manifest.write_text(json.dumps({
            "backend": "libtorch_xpu", "native_library": "lib/libuta_libtorch.so",
            "source_commit": "upstream-source", "native_source_commit": "previous-app-source",
            "environment": {"ONEAPI_DEVICE_SELECTOR": "level_zero:gpu"},
            "libraries": {"lib/libuta_libtorch.so": "obsolete-native-digest", "torch/lib/libc10.so": "retained"},
        }))
        (self.build / "libuta_libtorch.so").write_bytes(b"rebuilt bridge")
        (self.build / "native-build-info.json").write_text(json.dumps({
            "native_source_commit": "fixture-source", "native_source_dirty": False,
        }))

    def test_publication_never_truncates_a_mapped_library(self):
        with self.library.open("rb") as source, mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as mapping:
            inode = self.library.stat().st_ino
            publisher.publish(self.build, self.runtime)
            self.assertEqual(mapping[:], b"original mapped library")
            self.assertNotEqual(self.library.stat().st_ino, inode)
        self.assertEqual(self.library.read_bytes(), b"rebuilt bridge")
        manifest = json.loads(self.manifest.read_text())
        self.assertEqual(manifest["native_source_commit"], "fixture-source")
        self.assertFalse(manifest["native_source_dirty"])
        self.assertEqual(manifest["source_commit"], "upstream-source")
        self.assertEqual(manifest["environment"], {"ONEAPI_DEVICE_SELECTOR": "level_zero:gpu"})
        self.assertNotIn("lib/libuta_libtorch.so", manifest["libraries"])
        self.assertEqual(manifest["libraries"]["torch/lib/libc10.so"], "retained")

    def test_missing_build_output_preserves_installation_and_cleans_temporaries(self):
        (self.build / "libuta_libtorch.so").unlink()
        original = self.manifest.read_bytes()
        with self.assertRaises(OSError):
            publisher.publish(self.build, self.runtime)
        self.assertEqual(self.library.read_bytes(), b"original mapped library")
        self.assertEqual(self.manifest.read_bytes(), original)
        self.assertFalse(list((self.runtime / "lib").glob(".libuta_libtorch.*")))

    def test_native_rebuild_uses_installed_headers_and_never_runs_the_library(self):
        headers = self.runtime / "torch/include/ATen"
        headers.mkdir(parents=True)
        (headers / "ATen.h").write_text("fixture headers")
        bin_path = self.root / "bin"
        bin_path.mkdir()
        fake = bin_path / "cmake"
        fake.write_text("""#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['UTA_NATIVE_TEST_COMMANDS'], 'a') as output:
    output.write(json.dumps(args) + '\\n')
if '--build' in args:
    build = pathlib.Path(args[args.index('--build') + 1])
    (build / 'libuta_libtorch.so').write_bytes(b'rebuilt bridge')
else:
    build = pathlib.Path(args[args.index('-B') + 1])
    build.mkdir(parents=True, exist_ok=True)
    (build / 'native-build-info.json').write_text(json.dumps({
        'native_source_commit': 'fixture-source', 'native_source_dirty': False}))
""")
        fake.chmod(0o755)
        commands = self.root / "commands.jsonl"
        env = dict(os.environ, PATH=str(bin_path) + os.pathsep + os.environ["PATH"],
                   UTA_STUDIO_LIBTORCH_RUNTIME_DIR=str(self.runtime),
                   UTA_STUDIO_LIBTORCH_WORK_DIR=str(self.root / "work"),
                   UTA_NATIVE_TEST_COMMANDS=str(commands))
        env.pop("UTA_STUDIO_LIBTORCH_INCLUDE_DIR", None)
        result = subprocess.run(["bash", str(SCRIPT)], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [json.loads(line) for line in commands.read_text().splitlines()]
        self.assertEqual(len(calls), 2)
        self.assertIn("-DTORCH_INCLUDE_ROOT=" + str(self.runtime / "torch/include"), calls[0])
        self.assertIn("-DUTA_LIBTORCH_BACKEND=xpu", calls[0])
        self.assertIn("uta_libtorch", calls[1])
        self.assertEqual(self.library.read_bytes(), b"rebuilt bridge")


if __name__ == "__main__":
    unittest.main()
