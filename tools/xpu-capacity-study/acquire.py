#!/usr/bin/env python3
"""Stage only declared native wheel dependencies; no Python/model runtime install."""
import argparse
import json
from pathlib import Path
import shutil
import urllib.request
import zipfile

PACKAGES = {
    "intel-cmplr-lib-rt": "2026.0.0", "intel-cmplr-lib-ur": "2026.0.0",
    "intel-cmplr-lic-rt": "2026.0.0", "intel-sycl-rt": "2026.0.0",
    "oneccl": "2022.0.0", "oneccl-devel": "2022.0.0", "impi-rt": "2021.18.0",
    "onemkl-license": "2026.0.0", "onemkl-sycl-blas": "2026.0.0",
    "onemkl-sycl-dft": "2026.0.0", "onemkl-sycl-lapack": "2026.0.0",
    "mkl": "2026.0.0", "intel-openmp": "2026.0.0", "tbb": "2023.0.0",
    "tcmlib": "1.5.0", "umf": "1.1.0", "intel-pti": "0.17.0",
}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    libraries = root / "lib"
    libraries.mkdir(exist_ok=True)
    manifest = []
    for package, version in PACKAGES.items():
        metadata_url = f"https://pypi.org/pypi/{package}/{version}/json"
        with urllib.request.urlopen(metadata_url, timeout=90) as response:
            metadata = json.load(response)
        wheels = [item for item in metadata["urls"] if item["filename"].endswith(".whl")
                  and (("linux" in item["filename"] and "x86_64" in item["filename"])
                       or "none-any" in item["filename"])]
        if not wheels:
            raise RuntimeError(f"No Linux x86_64 wheel: {package} {version}")
        wheel = sorted(wheels, key=lambda item: item["filename"])[0]
        destination = root / wheel["filename"]
        print(f"DEPENDENCY {package} {version} {wheel['url']}", flush=True)
        if not destination.exists():
            temporary = destination.with_suffix(".download")
            with urllib.request.urlopen(wheel["url"], timeout=180) as source, temporary.open("wb") as target:
                shutil.copyfileobj(source, target, length=1024 * 1024)
            temporary.rename(destination)
        expanded = root / package
        expanded.mkdir(exist_ok=True)
        with zipfile.ZipFile(destination) as archive:
            for member in archive.infolist():
                target = (expanded / member.filename).resolve()
                if not target.is_relative_to(expanded):
                    raise RuntimeError("Unsafe wheel path")
            archive.extractall(expanded)
        for library in sorted(expanded.rglob("*")):
            if library.is_file() and (".so" in library.name):
                link = libraries / library.name
                if not link.exists():
                    link.symlink_to(library)
        manifest.append({"package": package, "version": version, "url": wheel["url"],
                         "wheel": str(destination), "bytes": destination.stat().st_size})
        (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"STAGED {len(manifest)} packages in {libraries}", flush=True)

if __name__ == "__main__":
    main()
