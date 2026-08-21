"""Run exactly one PyTorch XPU audio model in a short-lived process."""

from __future__ import annotations

import ctypes
import json
import os
import signal
import subprocess
import sys
import traceback
from pathlib import Path
from typing import Any, Mapping

RESULT_FILENAME = ".uta-studio-xpu-worker-result.json"
_WORKER_ENV = "UTA_STUDIO_XPU_WORKER"


def run_isolated_xpu(request: Mapping[str, Any]) -> dict[str, Any]:
    """Execute one XPU model and return its file-only result payload."""
    work_dir = Path(str(request["work_dir"])).resolve()
    work_dir.mkdir(parents=True, exist_ok=True)
    result_path = work_dir / RESULT_FILENAME
    result_path.unlink(missing_ok=True)

    analyzer_dir = Path(__file__).resolve().parent.parent
    env = os.environ.copy()
    env[_WORKER_ENV] = "1"
    # Battlemage reports correlate permanent host-visible wedges with sustained
    # CCS/BCS work. Keep XPU enabled, but avoid the dedicated copy engine and
    # immediate command lists inside Uta Studio's short-lived worker context.
    # Explicit caller values remain available for upstream/runtime diagnosis.
    from audio_processors.xpu_segmented import intel_battlemage_present

    if intel_battlemage_present():
        env.setdefault("SYCL_UR_USE_LEVEL_ZERO_V2", "0")
        env.setdefault("UR_L0_USE_COPY_ENGINE", "0")
        env.setdefault("UR_L0_USE_IMMEDIATE_COMMANDLISTS", "0")
    worker_request = dict(request)
    worker_request["work_dir"] = str(work_dir)
    completed = subprocess.run(
        [sys.executable, "-m", "audio_processors.xpu_worker"],
        input=json.dumps(worker_request, ensure_ascii=False),
        text=True,
        check=False,
        cwd=str(analyzer_dir),
        env=env,
    )
    payload: dict[str, Any] = {}
    try:
        if result_path.is_file():
            loaded = json.loads(result_path.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                payload = loaded
    finally:
        result_path.unlink(missing_ok=True)

    if completed.returncode != 0:
        detail = str(payload.get("error") or "XPU model worker failed")
        raise RuntimeError(detail)
    if not payload:
        raise RuntimeError("XPU model worker exited without a result")
    return _validated_result(payload, work_dir)


def _validated_result(payload: Mapping[str, Any], work_dir: Path) -> dict[str, Any]:
    raw_stems = payload.get("stems")
    if not isinstance(raw_stems, Mapping):
        raise RuntimeError("XPU model worker returned an invalid stem map")
    stems: dict[str, str] = {}
    for stem, raw_path in raw_stems.items():
        path = Path(str(raw_path)).resolve()
        try:
            path.relative_to(work_dir)
        except ValueError as exc:
            raise RuntimeError("XPU model worker returned a path outside its work directory") from exc
        stems[str(stem)] = str(path)
    return {
        "stems": stems,
        "sample_rate": int(payload.get("sample_rate", 44100)),
        "channels": int(payload.get("channels", 2)),
    }


def _resolved_parameters(raw: object):
    from audio_models.parameters import ResolvedParameter, ResolvedParameters
    from audio_models.schema import coerce_parameter_value

    if not isinstance(raw, Mapping):
        raise RuntimeError("XPU model worker parameters must be a mapping")
    return ResolvedParameters(
        {
            str(key): ResolvedParameter(
                str(key), coerce_parameter_value(value), "isolated_xpu_worker"
            )
            for key, value in raw.items()
        }
    )


def _dispatch(request: Mapping[str, Any]) -> dict[str, Any]:
    from audio_models.catalog import load_catalog

    model = load_catalog().get(str(request["model_id"]))
    parameters = _resolved_parameters(request.get("parameters"))
    runner = str(request.get("runner") or "")
    if runner != model.runner:
        raise RuntimeError(
            f"XPU worker runner {runner!r} does not match model {model.id!r}"
        )
    if runner == "mdxc_torch":
        from audio_processors.runners.mdxc_torch import _separate_offline

        raw_names = request.get("descriptor_names")
        if not isinstance(raw_names, Mapping):
            raise RuntimeError("XPU MDXC worker output names must be a mapping")
        named = _separate_offline(
            model_spec=model,
            checkpoint=Path(str(request["checkpoint"])),
            config_path=Path(str(request["config_path"])),
            input_path=Path(str(request["input_path"])),
            work_dir=Path(str(request["work_dir"])),
            parameters=parameters,
            backend="torch_xpu",
            precision_policy=str(request.get("precision_policy") or "fp32"),
            descriptor_names={str(key): str(value) for key, value in raw_names.items()},
            process_isolated=True,
            require_all_outputs=not bool(request.get("allow_missing_stems")),
        )
        return {
            "stems": {stem: str(path) for stem, path in named.items()},
            "sample_rate": 44100,
            "channels": 2,
        }
    if runner == "demucs_torch":
        from audio_processors.runners.demucs_torch import _separate_demucs

        named, sample_rate, channels = _separate_demucs(
            yaml_path=Path(str(request["yaml_path"])),
            weight_path=Path(str(request["weight_path"])),
            input_path=Path(str(request["input_path"])),
            work_dir=Path(str(request["work_dir"])),
            parameters=parameters,
            backend="torch_xpu",
            expected=model.expected_stems,
            process_isolated=True,
        )
        return {
            "stems": {stem: str(path) for stem, path in named.items()},
            "sample_rate": sample_rate,
            "channels": channels,
        }
    raise RuntimeError(f"unsupported XPU worker runner: {runner!r}")


def _arm_parent_death_signal() -> None:
    """Do not leave an XPU inference orphaned if the analyzer is killed."""
    if not sys.platform.startswith("linux"):
        return
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(1, signal.SIGTERM, 0, 0, 0) != 0:  # PR_SET_PDEATHSIG
        errno = ctypes.get_errno()
        raise OSError(errno, os.strerror(errno))
    if os.getppid() == 1:
        raise RuntimeError("analyzer exited before the XPU worker started")


def _write_result(work_dir: Path, payload: Mapping[str, Any]) -> None:
    work_dir.mkdir(parents=True, exist_ok=True)
    result_path = work_dir / RESULT_FILENAME
    temporary = work_dir / f"{RESULT_FILENAME}.tmp"
    temporary.write_text(
        json.dumps(dict(payload), ensure_ascii=False, sort_keys=True),
        encoding="utf-8",
    )
    os.replace(temporary, result_path)


def main() -> int:
    work_dir: Path | None = None
    try:
        _arm_parent_death_signal()
        request = json.load(sys.stdin)
        if not isinstance(request, dict):
            raise RuntimeError("XPU model worker request must be an object")
        work_dir = Path(str(request["work_dir"])).resolve()
        _write_result(work_dir, _dispatch(request))
        return 0
    except BaseException as exc:
        traceback.print_exc(file=sys.stderr)
        if work_dir is not None:
            try:
                _write_result(work_dir, {"error": str(exc)})
            except Exception:
                pass
        return 1


if __name__ == "__main__":
    exit_code = main()
    sys.stdout.flush()
    sys.stderr.flush()
    # Avoid Python/C++ destructor-driven device work. All model-owned file
    # handles have closed and the result was atomically published above; the
    # operating system now tears down this process's complete Level Zero
    # context in one operation.
    os._exit(exit_code)
