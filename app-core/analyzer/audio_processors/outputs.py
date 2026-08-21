"""Semantic output mapping. Never inspects parenthesized filenames."""

from __future__ import annotations

from pathlib import Path

from audio_models.catalog import ModelSpec
from audio_models.errors import OutputContractError
from audio_processors.contracts import LoadedModelDescriptor, StemArtifact


def map_named_outputs(
    model_spec: ModelSpec,
    named_paths: dict[str, Path],
    *,
    sample_rate: int,
    channels: int,
    required_roles: tuple[str, ...] | None = None,
) -> dict[str, StemArtifact]:
    artifacts: dict[str, StemArtifact] = {}
    for stem_name, path in named_paths.items():
        if stem_name not in model_spec.output_contract:
            raise OutputContractError(
                f"{model_spec.id} returned unexpected stem {stem_name!r}",
                model_id=model_spec.id,
            )
        if not path.is_file():
            raise OutputContractError(
                f"{model_spec.id} stem {stem_name!r} is missing at {path}",
                model_id=model_spec.id,
            )
        try:
            output_size = path.stat().st_size
        except OSError as exc:
            raise OutputContractError(
                f"{model_spec.id} stem {stem_name!r} cannot be inspected at {path}: {exc}",
                model_id=model_spec.id,
            ) from exc
        if output_size == 0:
            raise OutputContractError(
                f"{model_spec.id} stem {stem_name!r} is empty at {path}",
                model_id=model_spec.id,
            )
        role = model_spec.output_contract[stem_name]
        artifacts[role] = StemArtifact(
            role=role,
            source_stem_name=stem_name,
            path=path,
            sample_rate=sample_rate,
            channels=channels,
        )
    expected = required_roles or tuple(model_spec.output_contract.values())
    missing = [role for role in expected if role not in artifacts]
    if missing:
        raise OutputContractError(
            f"{model_spec.id} missing required roles: {missing}",
            model_id=model_spec.id,
        )
    return artifacts


def descriptor_from_spec(model_spec: ModelSpec, step_id: str) -> LoadedModelDescriptor:
    from audio_processors.contracts import deterministic_output_names

    stems = model_spec.expected_stems
    return LoadedModelDescriptor(
        target_stem=model_spec.target_stem,
        source_stems=stems,
        output_names=deterministic_output_names(step_id, stems),
    )


def path_for_stem(work_dir: Path, descriptor: LoadedModelDescriptor, stem: str) -> Path:
    name = descriptor.output_names[stem]
    aliases = {name, name.replace("__", "_"), name.replace("_", "-")}
    for suffix in (".wav", ".flac"):
        for alias in aliases:
            candidate = work_dir / f"{alias}{suffix}"
            if candidate.is_file():
                return candidate
    return work_dir / f"{name}.wav"


def match_named_file(work_dir: Path, token: str, output_files: list[str] | tuple[str, ...] = ()) -> Path | None:
    aliases = {token, token.replace("__", "_")}
    for item in output_files:
        path = Path(item) if Path(item).is_absolute() else work_dir / item
        if path.is_file() and any(alias in path.stem for alias in aliases):
            return path
    for alias in aliases:
        for suffix in (".wav", ".flac"):
            candidate = work_dir / f"{alias}{suffix}"
            if candidate.is_file():
                return candidate
    return None
