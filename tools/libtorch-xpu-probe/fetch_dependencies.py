#!/usr/bin/env python3
"""Explicit, local-only acquisition of the native dependencies of the XPU probe.

Downloads official PyPI wheels with bounded parallel range requests. No automatic
retries, environment activation, global install, inference, or driver changes.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

PACKAGES = {
    "intel-sycl-rt": "2026.0.0", "intel-cmplr-lib-rt": "2026.0.0",
    "intel-cmplr-lib-ur": "2026.0.0", "intel-openmp": "2026.0.0",
    "intel-pti": "0.17.0", "umf": "1.1.0", "tcmlib": "1.5.0",
    "tbb": "2023.0.0", "oneccl": "2022.0.0", "impi-rt": "2021.18.0",
    "mkl": "2026.0.0", "onemkl-sycl-blas": "2026.0.0",
    "onemkl-sycl-dft": "2026.0.0", "onemkl-sycl-lapack": "2026.0.0",
}

def acquire(root: Path, package: str, version: str) -> Path:
    with urllib.request.urlopen(f"https://pypi.org/pypi/{package}/{version}/json", timeout=30) as response:
        metadata = json.load(response)
    choices = [item for item in metadata["urls"] if item["filename"].endswith(".whl")
               and "manylinux" in item["filename"] and "x86_64" in item["filename"]]
    if len(choices) != 1:
        raise RuntimeError(f"ambiguous Linux native wheel for {package}: {len(choices)}")
    item = choices[0]
    destination = root / item["filename"]
    if destination.exists():
        print("Retained prior wheel:", destination.name, flush=True)
        return destination
    pending = destination.with_suffix(".partial")
    if pending.exists():
        raise RuntimeError(f"Unfinished earlier download retained at {pending}; resolve explicitly before another attempt")
    size = item["size"]
    print("Acquire", package, version, "bytes", size, flush=True)
    descriptor = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    os.ftruncate(descriptor, size)
    block = 1024 * 1024
    spans = [(offset, min(offset + block, size) - 1) for offset in range(0, size, block)]
    def transfer(span):
        start, end = span
        request = urllib.request.Request(item["url"], headers={"Range": f"bytes={start}-{end}"})
        with urllib.request.urlopen(request, timeout=90) as response:
            expected = f"bytes {start}-{end}/{size}"
            if response.status != 206 or response.headers.get("Content-Range") != expected:
                raise RuntimeError(f"Unexpected range response: {response.status}, {response.headers.get('Content-Range')}")
            payload = response.read()
        if len(payload) != end - start + 1:
            raise RuntimeError("Truncated HTTP range")
        written = 0
        while written < len(payload):
            written += os.pwrite(descriptor, payload[written:], start + written)
        return len(payload)
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=32) as executor:
            total = sum(executor.map(transfer, spans))
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    if total != size:
        raise RuntimeError("Incomplete native wheel")
    pending.rename(destination)
    (root / (package + ".source.json")).write_text(json.dumps({
        "package": package, "version": version, "filename": item["filename"],
        "url": item["url"], "bytes": size, "source": "official PyPI release metadata",
    }, indent=2) + "\n")
    print("Completed", destination.name, flush=True)
    return destination

def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: fetch_dependencies.py PRIVATE_ROOT PACKAGE|all")
    root = Path(sys.argv[1]).resolve()
    wheels = root / "wheels"
    wheels.mkdir(exist_ok=True)
    packages = PACKAGES if sys.argv[2] == "all" else {sys.argv[2]: PACKAGES[sys.argv[2]]}
    for package, version in packages.items():
        wheel = acquire(wheels, package, version)
        subprocess.run([str(root / "venv/bin/python"), "-m", "pip", "install", "--disable-pip-version-check",
                        "--no-deps", "--no-index", str(wheel)], check=True)

if __name__ == "__main__":
    main()
