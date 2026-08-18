"""Offline Model Catalog. Never contacts the network."""

from __future__ import annotations

from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path
from typing import Any, Iterable, Mapping

from .errors import CatalogError, ModelConfigurationError
from .schema import (
    ALLOWED_ARCHITECTURES,
    ALLOWED_BACKENDS,
    ALLOWED_INPUT_ROLES,
    ALLOWED_OPERATIONS,
    ALLOWED_RUNNERS,
    SCHEMA_BY_ID,
    reject_placeholder,
)
from .yaml_util import RestrictedYamlError, load_restricted_yaml

CATALOG_FILENAME = "catalog.yaml"
REQUIRED_MODEL_IDS = (
    "bs_roformer_vocals_ep317",
    "melband_roformer_inst_v2",
    "htdemucs_6s",
    "melband_roformer_denoise_aufr33",
    "melband_roformer_dereverb_anvuew",
    "uvr_mdxnet_karaoke_2",
)
DEFAULT_LEGACY_KARAOKE_MODEL_ID = "melband_roformer_karaoke_aufr33_viperx"


@dataclass(frozen=True)
class ModelFileSpec:
    role: str
    filename: str
    source_id: str
    sha256: str
    url: str | None = None
    installed_name: str | None = None
    uvr_metadata_hash: str | None = None
    size_bytes: int | None = None

    @property
    def install_filename(self) -> str:
        return self.installed_name or self.filename


@dataclass(frozen=True)
class LicenseSpec:
    status: str
    source_attribution: str
    source_page: str | None = None
    redistribution: str = "user_download"
    review_date: str | None = None
    notes: str | None = None


@dataclass(frozen=True)
class ModelSpec:
    id: str
    display_name: str
    architecture: str
    operation: str
    runner: str
    accepted_roles: tuple[str, ...]
    channels: int
    sample_rate_policy: str
    files: tuple[ModelFileSpec, ...]
    expected_stems: tuple[str, ...]
    output_contract: Mapping[str, str]
    supported_backends: tuple[str, ...]
    parameter_schema_id: str
    license: LicenseSpec
    target_stem: str | None = None
    metadata_sha256: str | None = None
    purpose: str = ""
    estimated_bytes: int | None = None
    family: str = ""

    def file(self, role: str) -> ModelFileSpec:
        for item in self.files:
            if item.role == role:
                return item
        raise ModelConfigurationError(f"{self.id} is missing catalog file role {role}")

    def output_role_for_stem(self, stem_name: str) -> str:
        try:
            return self.output_contract[stem_name]
        except KeyError as exc:
            raise ModelConfigurationError(
                f"{self.id} has no output contract for stem {stem_name!r}",
                model_id=self.id,
            ) from exc


@dataclass(frozen=True)
class AudioModelCatalog:
    schema_version: int
    catalog_version: str
    models: tuple[ModelSpec, ...]
    sources: Mapping[str, Mapping[str, str]] = field(default_factory=dict)

    def get(self, model_id: str) -> ModelSpec:
        for model in self.models:
            if model.id == model_id:
                return model
        raise CatalogError(f"unknown audio model id: {model_id}")

    def ids(self) -> tuple[str, ...]:
        return tuple(model.id for model in self.models)

    def require(self, model_id: str) -> ModelSpec:
        return self.get(model_id)


def catalog_path() -> Path:
    return Path(__file__).with_name(CATALOG_FILENAME)


def _require_mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise CatalogError(f"{label} must be a mapping")
    return value


def _require_list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise CatalogError(f"{label} must be a list")
    return value


def _parse_file(raw: Mapping[str, Any], model_id: str) -> ModelFileSpec:
    role = str(raw.get("role") or "")
    filename = str(raw.get("filename") or "")
    source_id = str(raw.get("source_id") or "")
    sha256 = str(raw.get("sha256") or "")
    if not role or not filename or not source_id:
        raise CatalogError(f"{model_id} file entry is missing role, filename, or source_id")
    reject_placeholder(sha256, field=f"{model_id}:{filename}.sha256")
    uvr_hash = raw.get("uvr_metadata_hash")
    size = raw.get("size_bytes")
    return ModelFileSpec(
        role=role,
        filename=filename,
        source_id=source_id,
        sha256=sha256.lower(),
        url=str(raw["url"]) if raw.get("url") else None,
        installed_name=str(raw["installed_name"]) if raw.get("installed_name") else None,
        uvr_metadata_hash=str(uvr_hash) if uvr_hash else None,
        size_bytes=int(size) if size is not None else None,
    )


def _parse_license(raw: Mapping[str, Any], model_id: str) -> LicenseSpec:
    status = str(raw.get("status") or "")
    if status in {"", "unreviewed", "unreviewed production source"}:
        raise CatalogError(f"{model_id} license status is not production-ready: {status!r}")
    return LicenseSpec(
        status=status,
        source_attribution=str(raw.get("source_attribution") or ""),
        source_page=str(raw["source_page"]) if raw.get("source_page") else None,
        redistribution=str(raw.get("redistribution") or "user_download"),
        review_date=str(raw["review_date"]) if raw.get("review_date") else None,
        notes=str(raw["notes"]) if raw.get("notes") else None,
    )


def _parse_model(raw: Mapping[str, Any]) -> ModelSpec:
    model_id = str(raw.get("id") or "")
    if not model_id:
        raise CatalogError("model is missing id")
    architecture = str(raw.get("architecture") or "")
    if architecture not in ALLOWED_ARCHITECTURES:
        raise CatalogError(f"{model_id} has illegal architecture {architecture!r}")
    runner = str(raw.get("runner") or "")
    if runner not in ALLOWED_RUNNERS:
        raise CatalogError(f"{model_id} has illegal runner {runner!r}")
    operation = str(raw.get("operation") or "")
    if operation not in ALLOWED_OPERATIONS:
        raise CatalogError(f"{model_id} has illegal operation {operation!r}")
    input_contract = _require_mapping(raw.get("input_contract"), f"{model_id}.input_contract")
    accepted = tuple(str(role) for role in _require_list(input_contract.get("accepted_roles"), f"{model_id}.accepted_roles"))
    if not accepted or any(role not in ALLOWED_INPUT_ROLES for role in accepted):
        raise CatalogError(f"{model_id} has illegal input roles: {accepted}")
    files = tuple(
        _parse_file(_require_mapping(item, f"{model_id}.files[]"), model_id)
        for item in _require_list(raw.get("files"), f"{model_id}.files")
    )
    if not files:
        raise CatalogError(f"{model_id} is missing files")
    roles = [item.role for item in files]
    if len(roles) != len(set(roles)):
        raise CatalogError(f"{model_id} has duplicate file roles")
    metadata = _require_mapping(raw.get("model_metadata") or {}, f"{model_id}.model_metadata")
    expected = tuple(str(stem) for stem in _require_list(metadata.get("expected_stems"), f"{model_id}.expected_stems"))
    output_contract_raw = _require_mapping(raw.get("output_contract"), f"{model_id}.output_contract")
    output_contract = {str(key): str(value) for key, value in output_contract_raw.items()}
    if not output_contract:
        raise CatalogError(f"{model_id} is missing output_contract")
    if len(set(output_contract.values())) != len(output_contract):
        raise CatalogError(f"{model_id} output roles must be unique")
    for stem in expected:
        if stem not in output_contract:
            raise CatalogError(f"{model_id} expected stem {stem!r} has no output contract")
    backends = tuple(str(item) for item in _require_list(raw.get("supported_backends"), f"{model_id}.supported_backends"))
    if not backends or any(item not in ALLOWED_BACKENDS for item in backends):
        raise CatalogError(f"{model_id} has illegal backends: {backends}")
    schema_id = str(raw.get("parameter_schema_id") or "")
    if schema_id not in SCHEMA_BY_ID:
        raise CatalogError(f"{model_id} references unknown parameter schema {schema_id!r}")
    metadata_sha = metadata.get("normalized_metadata_sha256")
    if metadata_sha:
        reject_placeholder(str(metadata_sha), field=f"{model_id}.normalized_metadata_sha256")
    return ModelSpec(
        id=model_id,
        display_name=str(raw.get("display_name") or model_id),
        architecture=architecture,
        operation=operation,
        runner=runner,
        accepted_roles=accepted,
        channels=int(input_contract.get("channels") or 2),
        sample_rate_policy=str(input_contract.get("sample_rate_policy") or "model_native"),
        files=files,
        expected_stems=expected,
        output_contract=output_contract,
        supported_backends=backends,
        parameter_schema_id=schema_id,
        license=_parse_license(_require_mapping(raw.get("license"), f"{model_id}.license"), model_id),
        target_stem=str(metadata["target_stem"]) if metadata.get("target_stem") else None,
        metadata_sha256=str(metadata_sha).lower() if metadata_sha else None,
        purpose=str(raw.get("purpose") or ""),
        estimated_bytes=int(raw["estimated_bytes"]) if raw.get("estimated_bytes") is not None else None,
        family=str(raw.get("family") or ""),
    )


def parse_catalog_document(raw: Mapping[str, Any]) -> AudioModelCatalog:
    schema_version = int(raw.get("schema_version") or 0)
    if schema_version != 1:
        raise CatalogError(f"unsupported catalog schema_version: {schema_version}")
    catalog_version = str(raw.get("catalog_version") or "")
    if not catalog_version:
        raise CatalogError("catalog_version is required")
    models = tuple(
        _parse_model(_require_mapping(item, "models[]"))
        for item in _require_list(raw.get("models"), "models")
    )
    ids = [model.id for model in models]
    if len(ids) != len(set(ids)):
        raise CatalogError("duplicate model IDs in catalog")
    missing = [model_id for model_id in REQUIRED_MODEL_IDS if model_id not in ids]
    if missing:
        raise CatalogError(f"catalog is missing required models: {missing}")
    sources_raw = raw.get("sources") or {}
    sources = {
        str(key): {str(inner): str(value) for inner, value in _require_mapping(item, f"sources.{key}").items()}
        for key, item in _require_mapping(sources_raw, "sources").items()
    }
    return AudioModelCatalog(
        schema_version=schema_version,
        catalog_version=catalog_version,
        models=models,
        sources=sources,
    )


def load_catalog_text(text: str) -> AudioModelCatalog:
    try:
        raw = load_restricted_yaml(text)
    except RestrictedYamlError as exc:
        raise CatalogError(f"catalog YAML is invalid: {exc}") from exc
    if not isinstance(raw, dict):
        raise CatalogError("catalog root must be a mapping")
    return parse_catalog_document(raw)


@lru_cache(maxsize=1)
def load_catalog(path: Path | None = None) -> AudioModelCatalog:
    catalog_file = path or catalog_path()
    try:
        text = catalog_file.read_text(encoding="utf-8")
    except OSError as exc:
        raise CatalogError(f"could not read catalog: {catalog_file}") from exc
    return load_catalog_text(text)


def installed_model_dir(models_dir: Path, model_id: str) -> Path:
    return Path(models_dir) / "audio-processing" / model_id


def manifest_path(models_dir: Path, model_id: str) -> Path:
    return installed_model_dir(models_dir, model_id) / "install-manifest.json"


def iter_required_models(catalog: AudioModelCatalog | None = None) -> Iterable[ModelSpec]:
    loaded = catalog or load_catalog()
    return (loaded.get(model_id) for model_id in REQUIRED_MODEL_IDS)
