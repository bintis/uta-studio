#!/usr/bin/env python3
"""Developer-only catalog importer. Never called by the application runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ANALYZER = ROOT / "app-core" / "analyzer"
if str(ANALYZER) not in sys.path:
    sys.path.insert(0, str(ANALYZER))

from audio_models.catalog import load_catalog  # noqa: E402
from audio_models.schema import is_sha256  # noqa: E402


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def uvr_metadata_hash(path: Path) -> str:
    data = path.read_bytes()
    return hashlib.md5(data[-10000 * 1024 :]).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify or refresh Uta Studio audio catalog hashes. Offline after files exist."
    )
    parser.add_argument("--models-dir", type=Path, help="Directory of already-downloaded files")
    parser.add_argument("--print-catalog", action="store_true")
    args = parser.parse_args()
    catalog = load_catalog()
    print(f"catalog {catalog.catalog_version} models={len(catalog.models)}")
    for model in catalog.models:
        for item in model.files:
            if not is_sha256(item.sha256):
                raise SystemExit(f"{model.id} {item.filename} has an invalid SHA-256")
            print(f"  {model.id:36} {item.role:22} {item.sha256}")
            if args.models_dir:
                candidate = args.models_dir / item.filename
                if candidate.is_file():
                    actual = sha256_file(candidate)
                    ok = actual == item.sha256
                    print(f"    local {candidate.name} {'OK' if ok else 'MISMATCH ' + actual}")
                    if item.filename.endswith(".onnx"):
                        print(f"    uvr_metadata_hash {uvr_metadata_hash(candidate)}")
    if args.print_catalog:
        print(json.dumps({"catalogVersion": catalog.catalog_version, "ids": list(catalog.ids())}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
