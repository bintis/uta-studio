"""User-triggered offline-after-download model installation."""

from __future__ import annotations

import hashlib
import json
import time
import urllib.request
from pathlib import Path
from typing import Callable

from .catalog import ModelSpec, installed_model_dir, load_catalog, manifest_path
from .errors import CatalogError, ModelIntegrityError
from .schema import reject_placeholder

Progress = Callable[[str], None]


def _sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _atomic_replace(src: Path, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    src.replace(dest)


def download_and_verify_file(
    url: str,
    dest: Path,
    expected_sha256: str,
    *,
    size_bytes: int | None = None,
    on_progress: Progress | None = None,
) -> None:
    reject_placeholder(expected_sha256, field=dest.name)
    part = dest.with_suffix(dest.suffix + ".part")
    if part.exists():
        part.unlink()
    if on_progress:
        on_progress(f"Downloading {dest.name}")
    request = urllib.request.Request(url, headers={"User-Agent": "UtaStudio-model-setup"})
    with urllib.request.urlopen(request) as response, part.open("wb") as handle:
        total = 0
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            handle.write(chunk)
            total += len(chunk)
    if size_bytes is not None and part.stat().st_size != size_bytes:
        part.unlink(missing_ok=True)
        raise ModelIntegrityError(f"{dest.name} length mismatch")
    actual = _sha256_path(part)
    if actual != expected_sha256.lower():
        part.unlink(missing_ok=True)
        raise ModelIntegrityError(f"{dest.name} SHA-256 mismatch")
    _atomic_replace(part, dest)


def write_manifest(models_dir: Path, model: ModelSpec) -> Path:
    directory = installed_model_dir(models_dir, model.id)
    directory.mkdir(parents=True, exist_ok=True)
    payload = {
        "schemaVersion": 1,
        "modelId": model.id,
        "catalogVersion": load_catalog().catalog_version,
        "files": [
            {
                "role": item.role,
                "filename": item.install_filename,
                "sha256": item.sha256,
            }
            for item in model.files
        ],
        "installedAtMs": int(time.time() * 1000),
    }
    path = manifest_path(models_dir, model.id)
    tmp = path.with_suffix(".json.part")
    tmp.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp.replace(path)
    return path


def model_install_status(models_dir: Path, model: ModelSpec) -> dict[str, object]:
    directory = installed_model_dir(models_dir, model.id)
    manifest = manifest_path(models_dir, model.id)
    files = []
    all_ok = True
    for item in model.files:
        path = directory / item.install_filename
        present = path.is_file()
        integrity = None
        if present:
            actual = _sha256_path(path)
            integrity = actual == item.sha256
            all_ok = all_ok and integrity
        else:
            all_ok = False
        files.append(
            {
                "role": item.role,
                "filename": item.install_filename,
                "present": present,
                "integrity": integrity,
                "sha256": item.sha256,
                "sizeBytes": item.size_bytes,
            }
        )
    state = "installed" if all_ok and manifest.is_file() else "missing"
    if any(item["present"] and item["integrity"] is False for item in files):
        state = "integrity_failed"
    return {
        "modelId": model.id,
        "displayName": model.display_name,
        "purpose": model.purpose,
        "architecture": model.architecture,
        "operation": model.operation,
        "runner": model.runner,
        "supportedBackends": list(model.supported_backends),
        "license": {
            "status": model.license.status,
            "sourceAttribution": model.license.source_attribution,
            "sourcePage": model.license.source_page,
        },
        "estimatedBytes": model.estimated_bytes,
        "state": state,
        "files": files,
        "catalogVersion": load_catalog().catalog_version,
    }


def list_audio_model_statuses(models_dir: Path) -> list[dict[str, object]]:
    catalog = load_catalog()
    return [model_install_status(models_dir, model) for model in catalog.models]


def _existing_user_file(models_dir: Path, item) -> Path | None:
    """Reuse an already-downloaded file from the legacy UVR model directory."""
    candidates = [
        models_dir.parent / "audio_separator" / item.filename,
        models_dir / "audio_separator" / item.filename,
        models_dir / item.filename,
    ]
    for candidate in candidates:
        if candidate.is_file() and _sha256_path(candidate) == item.sha256:
            return candidate
    return None


def install_audio_model(
    models_dir: Path,
    model_id: str,
    *,
    on_progress: Progress | None = None,
    allow_network: bool = False,
) -> dict[str, object]:
    catalog = load_catalog()
    model = catalog.get(model_id)
    directory = installed_model_dir(models_dir, model.id)
    directory.mkdir(parents=True, exist_ok=True)
    for item in model.files:
        dest = directory / item.install_filename
        if dest.is_file() and _sha256_path(dest) == item.sha256:
            if on_progress:
                on_progress(f"Using existing {item.install_filename}")
            continue
        if item.url and item.url.startswith("embedded:"):
            source = Path(__file__).with_name("configs") / item.filename
            if not source.is_file():
                raise CatalogError(f"{model.id} missing embedded file {item.filename}")
            dest.write_bytes(source.read_bytes())
            if _sha256_path(dest) != item.sha256:
                dest.unlink(missing_ok=True)
                raise ModelIntegrityError(f"{model.id} embedded file failed SHA-256")
            continue
        local = _existing_user_file(models_dir, item)
        if local is not None:
            if on_progress:
                on_progress(f"Importing {item.filename} from {local}")
            dest.write_bytes(local.read_bytes())
            if _sha256_path(dest) != item.sha256:
                dest.unlink(missing_ok=True)
                raise ModelIntegrityError(f"{model.id} imported file failed SHA-256")
            continue
        if not allow_network:
            raise CatalogError("audio model installation requires an explicit user action")
        if not item.url:
            raise CatalogError(f"{model.id} file {item.filename} has no catalog URL")
        download_and_verify_file(
            item.url,
            dest,
            item.sha256,
            size_bytes=item.size_bytes,
            on_progress=on_progress,
        )
    write_manifest(models_dir, model)
    return model_install_status(models_dir, model)


def remove_audio_model(models_dir: Path, model_id: str) -> None:
    catalog = load_catalog()
    model = catalog.get(model_id)
    directory = installed_model_dir(models_dir, model.id)
    if not directory.exists():
        return
    for child in sorted(directory.iterdir(), reverse=True):
        if child.is_file():
            child.unlink()
    directory.rmdir()
