#!/usr/bin/env python3
"""Publish a rebuilt app bridge without truncating mapped runtime libraries.

Build-time utility only. Does not load the DSO, execute models, or fetch data.
"""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile


def publish(build: Path, runtime: Path) -> None:
    provenance = json.loads((build / "native-build-info.json").read_text())
    manifest_path = runtime / "runtime-manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else None
    if manifest is not None:
        manifest["native_source_commit"] = provenance["native_source_commit"]
        manifest["native_source_dirty"] = provenance["native_source_dirty"]
        # The replaced DSO no longer has the earlier diagnostic digest. These
        # records are not verified; never leave stale provenance for our file.
        manifest.get("libraries", {}).pop("lib/libuta_libtorch.so", None)
    destination = runtime / "lib/libuta_libtorch.so"
    destination.parent.mkdir(parents=True, exist_ok=True)
    pending = []
    try:
        descriptor, name = tempfile.mkstemp(prefix=".libuta_libtorch.", dir=destination.parent)
        pending.append(Path(name))
        with os.fdopen(descriptor, "wb") as output, (build / "libuta_libtorch.so").open("rb") as source:
            shutil.copyfileobj(source, output)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(name, 0o755)
        manifest_temporary = None
        if manifest is not None:
            descriptor, name = tempfile.mkstemp(prefix=".runtime-manifest.", dir=runtime)
            manifest_temporary = Path(name)
            pending.append(manifest_temporary)
            with os.fdopen(descriptor, "w") as output:
                json.dump(manifest, output, ensure_ascii=False, indent=2)
                output.write("\n")
                output.flush()
                os.fsync(output.fileno())
        # Readers holding the old inode keep its original bytes. The library
        # itself reports compiled provenance, so it remains authoritative if
        # publication of the separate descriptive manifest is interrupted.
        os.replace(pending[0], destination)
        if manifest_temporary is not None:
            os.replace(manifest_temporary, manifest_path)
    finally:
        for path in pending:
            path.unlink(missing_ok=True)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: publish-native-xpu.py BUILD_DIRECTORY RUNTIME_DIRECTORY")
    try:
        publish(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"Native bridge publication failed: {error}") from error
