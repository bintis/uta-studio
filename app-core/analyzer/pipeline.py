"""Shared analysis pipeline used by both server.py and analyze.py.

Phase 4 §4.2 of the analysis DAG redesign (docs/plan.md /
uta-studio-analysis-dag-phases.md) asks for `run_pipeline` to be split into
independently named node functions. That split lives here:
`run_preflight`, `run_music_analysis`, `run_stem_separation`,
`run_pitch_analysis`, `run_transcription`, `run_alignment`, and
`build_candidate_chart` are each callable on their own, each has a clear
input/output artifact contract, and each reports its own cache-hit /
success / failure via `progress_node`/`artifact_reused`.

Two things are deliberately *not* split further, both documented rather
than silently skipped:

- `run_audio_preprocessing` (vocal-region detection, resampling) remains
  inside `transcribe_vocals`/`align_lyrics`, where its arrays are actually
  consumed. That boundary does emit dedicated events and, only when an
  explicit frozen request is present, atomically materializes the exact
  float-audio array as lossless FLAC for immutable capture. It is not
  retained during ordinary runs.
- `run_timed_lyrics_import` has no Python counterpart: the Timed LRC path
  never enters this pipeline at all (it's fully handled on the Rust side,
  see `lyrics.rs::record_timed_lyrics_import`), so there is nothing to
  extract here.

`transcribe_or_align` remains as a thin dispatcher between `run_alignment`
(known-lyrics path) and `run_transcription` (ASR path) -- that branch is
the one real, code-visible fork in the lyrics route, and collapsing it
into `run_pipeline` itself would just move the same decision to a less
reusable place.
"""

import glob
import json
import os
import subprocess
import tempfile

from gpu import hard_free_gpu, log_vram
from whisper_compat import artifact_reused, progress, progress_node


def _committed_artifact(kind, path, slot, binding_kind="produced", algorithm_version="1"):
    """Structured output boundary consumed by Rust's immutable store."""
    return {
        "slot": slot,
        "artifact_kind": kind,
        "path": os.path.abspath(path),
        "binding_kind": binding_kind,
        "config_hash": "",
        "algorithm_version": algorithm_version,
    }
from key_detect import analyze_extra_descriptors, detect_key_structured, format_key
from rhythm import analyze_rhythm
from stems import (
    separate_stems,
    separate_stems_openvino_demucs,
)


def _models_dir(output_dir):
    torch_home = os.environ.get("TORCH_HOME", "")
    if torch_home:
        return os.path.dirname(torch_home)
    return output_dir


def _audio_plan_node_ids(audio_processing):
    node_for_step = {
        "extract_vocals": "stems.vocals",
        "denoise_vocals": "vocals.denoise",
        "dereverb_vocals": "vocals.dereverb",
        "extract_accompaniment": "stems.instrumental",
        "denoise_accompaniment": "instrumental.denoise",
        "dereverb_accompaniment": "instrumental.dereverb",
        "extract_karaoke": "stems.karaoke",
        "separate_6s": "stems.multistem",
        "legacy_htdemucs": "stems.multistem",
    }
    nodes = []
    for step in (audio_processing or {}).get("steps") or []:
        node_id = node_for_step.get(str(step.get("step_id") or ""))
        if node_id and node_id not in nodes:
            nodes.append(node_id)
    return nodes or ["stems.multistem"]


def _report_stem_plan_reused(audio_processing, message, reason, artifacts):
    for node_id in _audio_plan_node_ids(audio_processing):
        artifact_reused(
            node_id, 50, message, reason=reason,
            requested_device="cache", actual_device="cache",
            node_progress_pct=100,
        )
    artifact_reused(
        "stems.bind_analysis_outputs", 50, "Analysis audio bindings reused",
        reason=reason, requested_device="cache", actual_device="cache",
        node_progress_pct=100, artifacts=artifacts,
    )


def _audio_artifact_kind(node_id, role):
    normalized = str(role).lower()
    if node_id == "stems.vocals":
        return "RawVocalStem"
    if node_id == "vocals.denoise":
        return "DenoisedVocalStem"
    if node_id == "vocals.dereverb":
        return "DereverbedVocalStem"
    if node_id == "stems.instrumental":
        return "HighQualityInstrumentalStem"
    if node_id == "instrumental.denoise":
        return "DenoisedInstrumentalStem"
    if node_id == "instrumental.dereverb":
        return "DereverbedInstrumentalStem"
    if node_id == "stems.karaoke":
        return "KaraokeInstrumentalStem"
    if node_id == "stems.multistem":
        return {
            "vocals": "VocalStem",
            "vocal": "VocalStem",
            "drums": "DrumStem",
            "drum": "DrumStem",
            "bass": "BassStem",
            "guitar": "GuitarStem",
            "piano": "PianoStem",
            "other": "OtherStem",
        }.get(normalized, "OtherStem")
    return "VocalStem"


_AUDIO_STEP_NODES = {
    "extract_vocals": "stems.vocals",
    "denoise_vocals": "vocals.denoise",
    "dereverb_vocals": "vocals.dereverb",
    "extract_accompaniment": "stems.instrumental",
    "denoise_accompaniment": "instrumental.denoise",
    "dereverb_accompaniment": "instrumental.dereverb",
    "extract_karaoke": "stems.karaoke",
    "separate_6s": "stems.multistem",
    "legacy_htdemucs": "stems.multistem",
}


def _audio_node_for_step(step_id):
    try:
        return _AUDIO_STEP_NODES[step_id]
    except KeyError as error:
        raise RuntimeError(
            f"audio processing step {step_id!r} has no authoritative DAG node mapping"
        ) from error


def _try_execute_audio_plan(
    audio_processing,
    audio_path,
    work_dir,
    output_dir,
    device,
    separator,
    separator_options,
):
    """Run a frozen catalog plan when it contains real steps.

    Empty snapshots (legacy karaoke / openvino_demucs) fall through to the
    existing separator implementations so current results do not change.
    """
    if not audio_processing:
        return None
    if not audio_processing.get("steps"):
        from audio_models.plan import legacy_plan_from_separator

        rebuilt = legacy_plan_from_separator(separator, separator_options=separator_options)
        audio_processing = rebuilt.as_json()
        if not audio_processing.get("steps"):
            return None
    from pathlib import Path

    from audio_models.plan import plan_from_json
    from audio_processors.executor import execute_audio_processing_plan

    plan = plan_from_json(audio_processing)
    step_indexes = {step.step_id: index for index, step in enumerate(plan.steps)}
    started_steps = set()

    def report_step_progress(percent, message, **metadata):
        step_id = str(metadata.get("step_id") or "")
        model_id = str(metadata.get("model_id") or step_id)
        node_id = _audio_node_for_step(step_id)
        local = max(0, min(100, int(percent)))
        count = max(1, len(plan.steps))
        index = step_indexes.get(step_id, 0)
        overall = 4 + round(((index + local / 100.0) / count) * 46)
        lifecycle = metadata.get("lifecycle")
        if step_id not in started_steps:
            started_steps.add(step_id)
            progress_node(
                node_id,
                "node_started",
                overall,
                message,
                implementation=metadata.get("implementation") or "Audio Model Catalog",
                model=model_id,
                node_progress_pct=local,
            )
        if lifecycle == "step_completed":
            artifacts = [
                _committed_artifact(
                    _audio_artifact_kind(
                        node_id, artifact.get("role", f"output:{artifact_index}")
                    ),
                    artifact["path"],
                    f"output:{artifact_index}",
                )
                for artifact_index, artifact in enumerate(metadata.get("artifacts") or [])
                if artifact.get("path")
            ]
            progress_node(
                node_id,
                "node_completed",
                overall,
                message,
                implementation=metadata.get("implementation") or "Audio Model Catalog",
                model=model_id,
                requested_device=metadata.get("requested_device"),
                actual_device=metadata.get("actual_device"),
                fallback_from=metadata.get("fallback_from"),
                fallback_reason=metadata.get("fallback_reason"),
                node_progress_pct=100,
                artifacts=artifacts,
            )
            return
        progress_node(
            node_id,
            "node_progress",
            overall,
            message,
            implementation=metadata.get("implementation") or "Audio Model Catalog",
            model=model_id,
            requested_device=metadata.get("requested_device"),
            actual_device=metadata.get("actual_device"),
            node_progress_pct=local,
        )

    result = execute_audio_processing_plan(
        plan,
        source_path=Path(audio_path),
        work_root=Path(work_dir),
        models_dir=Path(_models_dir(output_dir)),
        progress_sink=report_step_progress,
    )
    vocals = result.binding("vocals").path
    instrumental = result.binding("instrumental").path
    progress_node(
        "stems.bind_analysis_outputs",
        "node_started",
        50,
        "Resolving analysis vocal and instrumental outputs...",
        node_progress_pct=0,
    )
    return str(vocals), str(instrumental)
from transcribe import transcribe_vocals
from align import align_lyrics
from pitch import analyze_pitch


def ffmpeg_bin():
    return os.environ.get("FFMPEG_PATH", "ffmpeg")


LOSSLESS_EXTENSIONS = {".flac", ".wav", ".wave", ".aif", ".aiff", ".alac"}


def source_is_lossless(path):
    extension = os.path.splitext(path)[1].lower()
    if extension in LOSSLESS_EXTENSIONS:
        return True
    if extension not in {".m4a", ".mp4", ".mov"}:
        return False
    probe = subprocess.run(
        [ffmpeg_bin(), "-hide_banner", "-i", path],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    return "Audio: alac" in probe.stderr


def convert_to_cache_audio(src, destination, *, lossless):
    codec = ["-c:a", "flac", "-compression_level", "8"] if lossless else ["-c:a", "libmp3lame", "-q:a", "2"]
    directory = os.path.dirname(destination) or "."
    suffix = os.path.splitext(destination)[1]
    prefix = f".{os.path.basename(destination)}."
    fd, temporary = tempfile.mkstemp(prefix=prefix, suffix=f".tmp{suffix}", dir=directory)
    os.close(fd)
    try:
        subprocess.run(
            [ffmpeg_bin(), "-y", "-i", src, *codec, "-v", "error", temporary],
            check=True,
        )
        os.replace(temporary, destination)
        if os.path.isfile(src):
            os.remove(src)
    except Exception:
        try:
            os.remove(temporary)
        except OSError:
            pass
        raise


def normalize_tempo(tempo):
    try:
        t = float(tempo)
    except (TypeError, ValueError):
        return 1.0
    if t <= 0:
        return 1.0
    return round(t + 1e-8, 1)


def _separator_marker_path(output_dir, file_hash):
    return os.path.join(output_dir, f"{file_hash}_separator.json")


def _active_separator_options(separator, options):
    options = options or {}
    if separator == "karaoke":
        segment_size = options.get("segment_size")
        return {
            "segment_size": None if segment_size is None else max(64, min(1024, int(segment_size))),
            "overlap": max(2, min(32, int(options.get("overlap", 8)))),
            "batch_size": max(1, min(8, int(options.get("batch_size", 1)))),
            "normalization_pct": max(1, min(100, int(options.get("normalization_pct", 90)))),
        }
    if separator == "demucs":
        return {
            "shifts": max(1, min(8, int(options.get("demucs_shifts", 1)))),
            "overlap_pct": max(1, min(95, int(options.get("demucs_overlap_pct", 25)))),
        }
    return {}


def _cached_separator_matches(output_dir, file_hash, separator, options):
    try:
        with open(_separator_marker_path(output_dir, file_hash), encoding="utf-8") as marker:
            data = json.load(marker)
            return data.get("separator") == separator and data.get("options", {}) == options
    except (OSError, ValueError, AttributeError):
        return False


def _write_separator_marker(output_dir, file_hash, separator, options):
    path = _separator_marker_path(output_dir, file_hash)
    _atomic_write_json(path, {"separator": separator, "options": options})


def _find_legacy_stem_cache(output_dir, file_hash, extension):
    """Finds a stem cache from before separation was decoupled from
    detected key/tempo (`{file_hash}_vocals_{key}_{tempo}.ext`). Only
    `tempo == 1.0` candidates count — that's what the default separation
    always used; a different tempo means a deliberately shifted variant,
    not the base separation this is trying to recognize.

    Returns `(vocals, instrumental)` or `None`. Never deletes, renames, or
    otherwise touches what it finds — an update to the key/tempo detection
    algorithm must not force a costly re-separation for existing libraries,
    but nothing else may still depend on the old filename either, so it's
    left in place.
    """
    prefix = os.path.join(output_dir, f"{file_hash}_vocals_")
    for vocals in sorted(glob.glob(f"{prefix}*_1.0.{extension}")):
        instrumental = os.path.join(
            output_dir, f"{file_hash}_instrumental_{vocals[len(prefix):]}"
        )
        if os.path.isfile(instrumental):
            return vocals, instrumental
    return None


def run_stem_separation(
    audio_path, output_dir, file_hash, separator, device,
    separator_options=None, audio_processing=None, free_gpu_fn=None, freeze=False,
    run_work_dir=None,
):
    """Execute the stem plan and report only its real child/binding nodes.
    ``stems.separate`` is a UI aggregate derived from those events.
    Returns the vocals path.

    Stem identity is the source file hash, separator backend, and separator
    options — never a detected key or tempo (see `_cached_separator_matches`
    below), so a BPM/key algorithm update never forces a re-separation.

    ``freeze`` is the analyzer's Phase 4 §4.5 "Freeze current outputs"
    signal (app-core/src/analyzer.rs::freeze_analysis_node_outputs_for_run)
    -- distinct from the ordinary cache-hit check just below. Cache-hit
    reuse only fires when the *current* separator options still match what
    produced the cached file; Freeze exists specifically for the opposite
    case -- the user changed separator options but explicitly wants to keep
    the old stems anyway -- so it force-reuses the existing files without
    checking the marker at all. The Rust caller already verified both files
    exist before setting this flag, so a missing file here is a genuine
    inconsistency, not a normal "nothing to freeze yet" case -- raised
    rather than silently falling through to a real (unwanted) separation.
    """
    lossless = source_is_lossless(audio_path)
    cache_extension = "flac" if lossless else "mp3"
    final_vocals = os.path.join(output_dir, f"{file_hash}_vocals.{cache_extension}")
    final_instrumental = os.path.join(output_dir, f"{file_hash}_instrumental.{cache_extension}")

    if freeze:
        if not (os.path.isfile(final_vocals) and os.path.isfile(final_instrumental)):
            raise RuntimeError(
                "stems.separate was frozen for this run, but no cached vocal/instrumental "
                "stem exists to reuse"
            )
        artifacts = [
            _committed_artifact("VocalStem", final_vocals, "output:0", "frozen"),
            _committed_artifact("InstrumentalStem", final_instrumental, "output:1", "frozen"),
        ]
        _report_stem_plan_reused(
            audio_processing, "Stem output frozen and reused", "frozen", artifacts
        )
        return final_vocals

    active_options = _active_separator_options(separator, separator_options)
    separator_matches = _cached_separator_matches(output_dir, file_hash, separator, active_options)

    if os.path.isfile(final_vocals) and os.path.isfile(final_instrumental) and separator_matches:
        artifacts = [
            _committed_artifact("VocalStem", final_vocals, "output:0", "reused"),
            _committed_artifact("InstrumentalStem", final_instrumental, "output:1", "reused"),
        ]
        _report_stem_plan_reused(
            audio_processing, "Stem output already cached", "cache_hit", artifacts
        )
        return final_vocals

    if separator_matches:
        legacy = _find_legacy_stem_cache(output_dir, file_hash, cache_extension)
        if legacy is not None:
            artifacts = [
                _committed_artifact("VocalStem", legacy[0], "output:0", "reused"),
                _committed_artifact("InstrumentalStem", legacy[1], "output:1", "reused"),
            ]
            _report_stem_plan_reused(
                audio_processing, "Legacy stem output already cached", "cache_hit", artifacts
            )
            return legacy[0]

    if run_work_dir:
        os.makedirs(run_work_dir, exist_ok=True)
    used_catalog_plan = False
    with tempfile.TemporaryDirectory(prefix="uta_studio_", dir=run_work_dir) as work_dir:
        plan_result = _try_execute_audio_plan(
            audio_processing,
            audio_path,
            work_dir,
            output_dir,
            device,
            separator,
            separator_options,
        )
        if plan_result is not None:
            used_catalog_plan = True
            vp, ip = plan_result
        elif separator == "karaoke":
            raise RuntimeError(
                "catalog karaoke plan produced no stems; install "
                "melband_roformer_karaoke_aufr33_viperx in Settings > Models & runtime"
            )
        elif separator == "openvino_demucs":
            progress_node(
                "stems.multistem", "node_started", 5,
                "Loading OpenVINO Demucs...", node_progress_pct=0,
            )
            vp, ip = separate_stems_openvino_demucs(audio_path, work_dir, os.environ.get("OPENVINO_SEPARATOR_MODEL_DIR", output_dir))
        else:
            vp, ip = separate_stems(
                audio_path,
                work_dir,
                device,
                shifts=active_options.get("shifts", 1),
                overlap=active_options.get("overlap_pct", 25) / 100.0,
            )
        if not used_catalog_plan:
            progress_node(
                "stems.bind_analysis_outputs", "node_started", 50,
                "Resolving analysis vocal and instrumental outputs...",
                node_progress_pct=0,
            )
        progress_node(
            "stems.bind_analysis_outputs", "node_progress", 51,
            "Saving analysis audio to cache...", node_progress_pct=75,
        )
        convert_to_cache_audio(vp, final_vocals, lossless=lossless)
        convert_to_cache_audio(ip, final_instrumental, lossless=lossless)

    if not used_catalog_plan:
        progress_node(
            "stems.multistem", "node_completed", 50,
            "Legacy multi-stem separation complete", node_progress_pct=100,
            artifacts=[
                _committed_artifact("VocalStem", final_vocals, "output:0"),
                _committed_artifact("InstrumentalStem", final_instrumental, "output:1"),
            ],
        )

    progress_node(
        "stems.bind_analysis_outputs", "node_completed", 51,
        "Analysis audio outputs resolved", node_progress_pct=100,
        artifacts=[
            _committed_artifact("VocalStem", final_vocals, "output:0"),
            _committed_artifact("InstrumentalStem", final_instrumental, "output:1"),
        ],
    )

    _write_separator_marker(output_dir, file_hash, separator, active_options)

    if free_gpu_fn:
        free_gpu_fn()

    return final_vocals


# Back-compat alias: the pre-Phase-4.2 name. No other module in this
# analyzer imports it directly (only `run_pipeline` below does), kept so
# any external tooling built against the old name keeps working.
separate_and_cache = run_stem_separation


def run_transcription(
    vocals_path, audio_path, device, output_dir, file_hash, *,
    model_name, beam_size=5, batch_size=16, engine="whisper",
    language_override=None, whisper_model=None, pre_align_cleanup=None,
    capture_preprocessed_path=None,
):
    """Node: lyrics.transcribe. ASR path (no known lyrics available).

    Writes the §4.4 split artifacts this node owns:
    `recognized_text.json` (pre-alignment ASR output -- see
    `transcribe.py::_build_result_from_raw_segments`'s
    `_pre_alignment_segments`) and `asr_segments.json` (the ASR segment
    evidence committed before alignment starts). Parakeet has no separate
    pre-alignment stage (native word timing, no wav2vec2 pass), so
    `_pre_alignment_segments` is absent for that engine and
    `recognized_text.json` falls back to the same content as
    `asr_segments.json` -- honest given that route's real characteristics,
    not a fabricated distinction (mirrors the direct
    `lyrics.transcribe -> chart.build_candidate` graph edge for Parakeet).
    """
    result = transcribe_vocals(
        vocals_path, audio_path, device,
        model_name=model_name,
        beam_size=beam_size,
        batch_size=batch_size,
        engine=engine,
        language_override=language_override,
        whisper_model=whisper_model,
        pre_align_cleanup=pre_align_cleanup,
        capture_preprocessed_path=capture_preprocessed_path,
    )
    pre_alignment_segments = result.pop("_pre_alignment_segments", None)
    alignment_raw_segments = result.pop("_alignment_raw_segments", None)
    alignment_audio = result.pop("_alignment_audio", None)
    alignment_device = result.pop("_alignment_device", None)
    alignment_cleanup = result.pop("_pre_align_cleanup", None)
    recognized_text_path, asr_segments_path, timed_transcript_path = _transcript_split_paths(
        output_dir, file_hash
    )
    recognized_text = {
        "language": result.get("language"),
        "source": result.get("source"),
        "segments": pre_alignment_segments if pre_alignment_segments is not None else result.get("segments"),
    }
    _atomic_write_json(recognized_text_path, recognized_text)
    asr_segments = dict(result)
    if pre_alignment_segments is not None:
        asr_segments["segments"] = pre_alignment_segments
    _atomic_write_json(asr_segments_path, asr_segments)
    progress_node(
        "lyrics.transcribe", "completed", 90, "Transcription complete",
        node_progress_pct=100,
        artifacts=[
            _committed_artifact("RecognizedText", recognized_text_path, "output:0"),
            _committed_artifact("AsrSegments", asr_segments_path, "output:1"),
        ],
    )
    if alignment_raw_segments is not None:
        progress_node(
            "lyrics.align", "started", 80, "Aligning word timestamps...",
            node_progress_pct=0,
        )
        from transcribe import _align_and_build

        result = _align_and_build(
            alignment_raw_segments,
            alignment_audio,
            result.get("language"),
            alignment_device,
            alignment_cleanup,
        )
        result["source"] = "generated"
        progress_node(
            "lyrics.align", "progress", 90, "Word timing alignment computed",
            node_progress_pct=95,
        )
        _atomic_write_json(timed_transcript_path, result)
        progress_node(
            "lyrics.align", "completed", 90, "Word timing alignment complete",
            node_progress_pct=100,
            artifacts=[_committed_artifact(
                "TimedTranscript", timed_transcript_path, "output:0"
            )],
        )
    return result


def run_alignment(
    lyrics_path, vocals_path, device, output_dir, file_hash, *,
    model_name, language_override=None, whisper_model=None, pre_align_cleanup=None,
    capture_preprocessed_path=None,
):
    """Node: lyrics.align. Known-lyrics path (Plain lyrics / LRCLIB text,
    not Timed LRC -- Timed LRC skips this pipeline entirely)."""
    print(f"[uta-studio:LOG] Using pre-fetched lyrics: {lyrics_path}", flush=True)
    result = align_lyrics(
        lyrics_path, vocals_path, device,
        model_name=model_name,
        language_override=language_override,
        whisper_model=whisper_model,
        pre_align_cleanup=pre_align_cleanup,
        capture_preprocessed_path=capture_preprocessed_path,
    )
    _, _, timed_transcript_path = _transcript_split_paths(output_dir, file_hash)
    _atomic_write_json(timed_transcript_path, result)
    progress_node(
        "lyrics.align", "node_completed", 90, "Alignment complete",
        node_progress_pct=100,
        artifacts=[_committed_artifact(
            "TimedTranscript", timed_transcript_path, "output:0"
        )],
    )
    return result


def transcribe_or_align(
    vocals_path, audio_path, device, output_dir, file_hash, *,
    model_name, beam_size=5, batch_size=16,
    engine="whisper",
    lyrics_path=None, language_override=None,
    whisper_model=None, pre_align_cleanup=None,
    capture_preprocessed_path=None,
):
    """Dispatches to `run_alignment` or `run_transcription` -- the one
    real, code-visible branch point in the lyrics route today. Kept as a
    thin dispatcher (rather than inlined into `run_pipeline`) so any other
    caller that just wants "whichever lyrics node applies" doesn't have to
    duplicate this branch.
    """
    if lyrics_path and os.path.isfile(lyrics_path):
        return run_alignment(
            lyrics_path, vocals_path, device, output_dir, file_hash,
            model_name=model_name,
            language_override=language_override,
            whisper_model=whisper_model,
            pre_align_cleanup=pre_align_cleanup,
            capture_preprocessed_path=capture_preprocessed_path,
        )
    return run_transcription(
        vocals_path, audio_path, device, output_dir, file_hash,
        model_name=model_name,
        beam_size=beam_size,
        batch_size=batch_size,
        engine=engine,
        language_override=language_override,
        whisper_model=whisper_model,
        pre_align_cleanup=pre_align_cleanup,
        capture_preprocessed_path=capture_preprocessed_path,
    )


MUSIC_ANALYSIS_VERSION = 1


def _music_analysis_path(output_dir, file_hash):
    return os.path.join(output_dir, f"{file_hash}_music_analysis.json")


def _read_music_analysis_cache(path):
    """Loads a previously written `music_analysis.json`, or `None` if it's
    missing, corrupt, or from an older version — regenerated rather than
    trusted in any of those cases."""
    if not os.path.isfile(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, ValueError) as e:
        print(f"[uta-studio:LOG] Music analysis cache unreadable, regenerating: {e}", flush=True)
        return None
    if (
        not isinstance(data, dict)
        or data.get("version") != MUSIC_ANALYSIS_VERSION
        or not isinstance(data.get("key"), dict)
        or not isinstance(data.get("rhythm"), dict)
    ):
        return None
    return data


def _write_music_analysis_cache(path, data):
    """Temp-file-plus-atomic-replace so a crash or kill mid-write never
    leaves a half-written (and therefore corrupt-looking, triggering a
    pointless re-analysis) JSON file behind."""
    directory = os.path.dirname(path) or "."
    prefix = f".{os.path.basename(path)}."
    fd, temp_path = tempfile.mkstemp(prefix=prefix, suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        os.replace(temp_path, path)
    except OSError as e:
        print(f"[uta-studio:LOG] Failed to write music analysis cache: {e}", flush=True)
        try:
            os.remove(temp_path)
        except OSError:
            pass


def _atomic_write_json(path, data):
    """Same temp-file-plus-atomic-replace strategy as
    `_write_music_analysis_cache`, generalized for the §4.4 split artifacts
    (`recognized_text.json`/`asr_segments.json`/`timed_transcript.json`) and
    compatibility files so a crash or kill mid-write never leaves a
    half-written output behind.
    """
    directory = os.path.dirname(path) or "."
    prefix = f".{os.path.basename(path)}."
    fd, temp_path = tempfile.mkstemp(prefix=prefix, suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        os.replace(temp_path, path)
    except OSError as e:
        print(f"[uta-studio:LOG] Failed to write {path}: {e}", flush=True)
        try:
            os.remove(temp_path)
        except OSError:
            pass


def _transcript_split_paths(output_dir, file_hash):
    return (
        os.path.join(output_dir, f"{file_hash}_recognized_text.json"),
        os.path.join(output_dir, f"{file_hash}_asr_segments.json"),
        os.path.join(output_dir, f"{file_hash}_timed_transcript.json"),
    )


def run_music_analysis(audio_path, output_dir, file_hash):
    """Run music.key / music.rhythm / music.descriptors. The
    ``music.analysis`` UI aggregate is derived from those child events.
    Key + rhythm (+ a few extra descriptors when
    Essentia is installed), cached to `{file_hash}_music_analysis.json`.
    Reuses a valid existing cache instead of re-running analysis:
    realigning lyrics or re-running transcription must not repeat key/BPM
    detection, and re-running this alone must not touch stems or the
    transcript.

    Always returns a dict shaped like the cache file (`version`/`key`/
    `rhythm`, `descriptors` only when available) — key/rhythm fields are
    the explicit "unknown" shape on failure, never a fabricated default.
    """
    path = _music_analysis_path(output_dir, file_hash)
    cached = _read_music_analysis_cache(path)
    if cached is not None:
        artifacts = [_committed_artifact("MusicAnalysis", path, "output:0", "reused")]
        for node_id in ("music.key", "music.rhythm", "music.descriptors"):
            artifact_reused(
                node_id, 4, "Music analysis already cached",
                node_progress_pct=100,
                artifacts=artifacts if node_id == "music.descriptors" else None,
            )
        return cached

    progress_node(
        "music.key", "node_started", 3, "Analyzing musical key...",
        implementation="Essentia/NumPy FFT", model="KeyExtractor / Krumhansl chroma profiles",
        node_progress_pct=0,
    )
    key = detect_key_structured(audio_path)
    progress_node(
        "music.key", "node_completed", 3, "Musical key analysis complete",
        implementation="Essentia/NumPy FFT", model="KeyExtractor / Krumhansl chroma profiles",
        node_progress_pct=100,
    )

    progress_node(
        "music.rhythm", "node_started", 3, "Analyzing tempo and beat positions...",
        implementation="Essentia/NumPy FFT", model="RhythmExtractor2013 / onset autocorrelation",
        node_progress_pct=0,
    )
    rhythm = analyze_rhythm(audio_path)
    progress_node(
        "music.rhythm", "node_completed", 4, "Tempo and beat analysis complete",
        implementation="Essentia/NumPy FFT", model="RhythmExtractor2013 / onset autocorrelation",
        node_progress_pct=100,
    )

    result = {"version": MUSIC_ANALYSIS_VERSION, "key": key, "rhythm": rhythm}

    progress_node(
        "music.descriptors", "node_started", 4, "Analyzing audio descriptors...",
        implementation="Essentia/NumPy FFT", model="Audio descriptors",
        node_progress_pct=0,
    )
    try:
        descriptors = analyze_extra_descriptors(audio_path)
        if descriptors:
            result["descriptors"] = descriptors
        descriptor_event = "completed"
        descriptor_message = "Audio descriptor analysis complete"
        descriptor_reason = None
    except Exception as e:
        # Never let an optional descriptor pass take key/rhythm down with it.
        print(f"[uta-studio:LOG] Extra descriptors unavailable: {e}", flush=True)
        descriptor_event = "skipped"
        descriptor_message = f"Audio descriptors unavailable: {e}"
        descriptor_reason = "unavailable"

    _write_music_analysis_cache(path, result)
    progress_node(
        "music.descriptors", descriptor_event, 4, descriptor_message,
        node_progress_pct=100,
        reason=descriptor_reason,
        implementation="Essentia/NumPy FFT",
        model="Audio descriptors",
        artifacts=[_committed_artifact("MusicAnalysis", path, "output:0")],
    )
    return result


# Back-compat alias: the pre-Phase-4.2 name.
analyze_music = run_music_analysis


def _pitch_cache_paths(output_dir, file_hash):
    return (
        os.path.join(output_dir, f"{file_hash}_pitch_track.json"),
        os.path.join(output_dir, f"{file_hash}_pitch_notes.json"),
    )


def run_pitch_analysis(
    vocals_path, output_dir, file_hash, pitch_model_dir, *, skip_pitch=False, freeze=False,
):
    """Node: pitch.extract. A failed guide must not make an otherwise
    playable karaoke analysis fail -- failure is logged and reported as
    `node_failed`, never raised; the existing runtime pitchy-based
    reference detector remains the fallback for songs without these cache
    files. Self-contained cache-hit check: safe to call independently of
    `run_pipeline`'s own orchestration.

    ``skip_pitch`` is the analyzer's Phase 4 "disable pitch.extract for
    this run" signal (app-core/src/analyzer.rs::run_analysis_plan) --
    distinct from the cache-reuse check below, which is about *not
    redoing* already-cached work, not about honoring an explicit disable.

    ``freeze`` is the Phase 4 §4.5 "Freeze current outputs" signal
    (app-core/src/analyzer.rs::freeze_analysis_node_outputs_for_run).
    Pitch extraction has no parameter-driven cache invalidation today (the
    check just below is pure file-existence), so in practice this behaves
    the same as an ordinary cache hit -- wired anyway for symmetry with
    `run_stem_separation` and so it stays correct if pitch ever grows its
    own invalidation. Like the stems case, the Rust caller already verified
    both files exist before setting this flag.
    """
    if skip_pitch or vocals_path is None:
        return
    track_path, notes_path = _pitch_cache_paths(output_dir, file_hash)
    if freeze:
        if not (os.path.isfile(track_path) and os.path.isfile(notes_path)):
            raise RuntimeError(
                "pitch.extract was frozen for this run, but no cached pitch guide exists to reuse"
            )
        artifact_reused(
            "pitch.extract",
            54,
            "Pitch extraction frozen, reusing existing pitch guide",
            reason="frozen",
            implementation="RMVPE",
            model="RMVPE singing pitch model",
            requested_device="cache",
            actual_device="cache",
            node_progress_pct=100,
            artifacts=[
                _committed_artifact("PitchTrack", track_path, "output:0", "frozen"),
                _committed_artifact("PitchNoteCandidates", notes_path, "output:1", "frozen"),
            ],
        )
        return
    if os.path.isfile(track_path) and os.path.isfile(notes_path):
        artifact_reused(
            "pitch.extract",
            54,
            "Pitch guide already cached, skipping extraction",
            implementation="RMVPE",
            model="RMVPE singing pitch model",
            requested_device="cache",
            actual_device="cache",
            node_progress_pct=100,
            artifacts=[
                _committed_artifact("PitchTrack", track_path, "output:0", "reused"),
                _committed_artifact("PitchNoteCandidates", notes_path, "output:1", "reused"),
            ],
        )
        return
    try:
        progress_node(
            "pitch.extract", "node_started", 52, "Extracting reference pitch...",
            node_progress_pct=0,
        )
        analyze_pitch(vocals_path, output_dir, file_hash, pitch_model_dir)
        progress_node(
            "pitch.extract", "node_completed", 54, "Building singing guide...",
            node_progress_pct=100,
            artifacts=[
                _committed_artifact("PitchTrack", track_path, "output:0"),
                _committed_artifact("PitchNoteCandidates", notes_path, "output:1"),
            ],
        )
    except Exception as e:
        print(f"[uta-studio:LOG] Pitch guide unavailable: {e}", flush=True)
        progress_node(
            "pitch.extract", "node_failed", 54, f"Pitch guide unavailable: {e}",
            node_progress_pct=100,
        )


def build_candidate_chart(
    transcript_path, transcript, detected_key, tempo, detected_bpm,
    timed_transcript_path=None,
):
    """Node: chart.build_candidate. Patches the detected key/tempo/BPM onto
    a transcript dict and writes it to `{file_hash}_transcript.json`.
    Shared by both the Timed-LRC path (patches an already-provided
    transcript in place) and the transcribe/align path (patches a freshly
    built one) — both end up producing the same candidate-chart-shaped
    artifact, just from a different `transcript` input.

    Also writes the same content to `{file_hash}_timed_transcript.json`
    (the §4.4 `TimedTranscript` artifact `chart.build_candidate` is modeled
    as consuming in `app-core/src/analysis_graph.rs`) when
    ``timed_transcript_path`` is given -- every lyrics route (ASR,
    known-lyrics alignment, Timed-LRC) converges here, so this is the one
    place that can write it uniformly. `transcript_path`'s write above is
    unchanged and kept as the permanent compatibility file.
    """
    transcript["key"] = detected_key
    transcript["tempo"] = normalize_tempo(tempo)
    transcript["bpm"] = detected_bpm
    progress_node("chart.build_candidate", "node_started", 95, "Writing transcript...")
    _atomic_write_json(transcript_path, transcript)
    if timed_transcript_path is not None:
        _atomic_write_json(timed_transcript_path, transcript)
    artifacts = [_committed_artifact("CandidateChart", transcript_path, "output:0")]
    progress_node(
        "chart.build_candidate", "node_completed", 100, "Transcript written",
        artifacts=artifacts,
    )
    return transcript


def run_preflight(output_dir, device):
    """Node: preflight. Ensures the cache directory exists and reports the
    resolved compute device. `AlwaysRequired` per `analysis_plan.rs` --
    every other node's output directory depends on this having run first.
    """
    os.makedirs(output_dir, exist_ok=True)
    progress_node(
        "preflight", "node_started", 1,
        "Inspecting source audio and existing analysis cache...", node_progress_pct=0,
    )
    progress(2, f"Using device: {device}")
    progress_node(
        "preflight", "node_completed", 2, f"Using device: {device}",
        node_progress_pct=100,
    )


def run_pipeline(
    audio_path, output_dir, file_hash, device, *,
    model_name="large-v3", beam_size=5, batch_size=16,
    separator="karaoke", separator_options=None, audio_processing=None, engine="whisper",
    run_work_dir=None,
    lyrics_path=None, language_override=None,
    whisper_model=None, pre_align_cleanup=None, free_gpu_fn=None,
    skip_transcription=False,
    skip_separation=False,
    skip_pitch=False,
    freeze_separation=False,
    freeze_pitch=False,
    bypass_separation_with_original_mix=False,
    capture_preprocessed_audio=False,
):
    """Full analysis pipeline: preflight -> music analysis -> stem
    separation -> pitch extraction -> transcription/alignment -> candidate
    chart. Orchestrates the node functions above in sequence; see the
    module docstring for what's still merged rather than split out.

    When ``skip_transcription`` is set, an existing (LRC-provided) transcript is
    kept as-is: only key detection and stem separation run, and the detected key
    is patched into the transcript. When ``skip_separation`` is also set, stem
    separation is skipped too (the song plays over its original mix); only the
    key is detected and stamped onto the provided transcript. ``skip_pitch``
    is the analyzer's Phase 4 "disable pitch.extract for this run" signal --
    see `run_pitch_analysis`. ``freeze_separation``/``freeze_pitch`` are the
    Phase 4 §4.5 "Freeze current outputs" signal -- see `run_stem_separation`/
    `run_pitch_analysis`. The Rust caller (`analyzer.rs::pipeline_flags_from_plan`)
    guarantees `skip_separation` and `freeze_separation` (respectively
    `skip_pitch`/`freeze_pitch`) are never both true: a frozen node must
    still be "run" here so its cached output reaches whatever downstream
    node needs it. ``bypass_separation_with_original_mix`` is the Phase 4
    §4.5 "Choose bypass" signal
    (app-core/src/analyzer.rs::bypass_analysis_node_with_original_mix_for_run):
    unlike Freeze, a bypassed `stems.separate` genuinely never runs --
    ``skip_separation`` is true alongside it -- but instead of leaving
    ``vocals_path`` unset, the full original mix is used in its place, so
    pitch extraction and transcription/alignment run against the whole song
    rather than an isolated vocal stem.
    """
    os.makedirs(output_dir, exist_ok=True)

    transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
    transcript_exists = os.path.isfile(transcript_path)
    _, _, timed_transcript_path = _transcript_split_paths(output_dir, file_hash)
    recognized_text_path, asr_segments_path, _ = _transcript_split_paths(
        output_dir, file_hash
    )

    run_preflight(output_dir, device)

    try:
        log_vram("phase:start")
        music = run_music_analysis(audio_path, output_dir, file_hash)
        detected_key = format_key(music["key"])
        detected_bpm = music["rhythm"]["bpm"]
        tempo = 1.0

        vocals_path = None
        if not skip_separation:
            vocals_path = run_stem_separation(
                audio_path, output_dir, file_hash, separator, device,
                separator_options=separator_options,
                audio_processing=audio_processing,
                run_work_dir=run_work_dir,
                free_gpu_fn=free_gpu_fn,
                freeze=freeze_separation,
            )
            log_vram("phase:after_separation")
        elif bypass_separation_with_original_mix:
            for node_id in [*_audio_plan_node_ids(audio_processing), "stems.bind_analysis_outputs"]:
                progress_node(
                    node_id, "node_skipped", 50,
                    "Stem processing bypassed, using the original mix",
                    reason="bypassed", node_progress_pct=100,
                )
            vocals_path = audio_path

        run_pitch_analysis(
            vocals_path, output_dir, file_hash,
            os.environ.get("PITCH_MODEL_DIR", os.path.join(output_dir, "pitch-model")),
            skip_pitch=skip_pitch,
            freeze=freeze_pitch,
        )

        if (
            transcript_exists
            and not skip_transcription
            and not capture_preprocessed_audio
        ):
            progress_node(
                "lyrics.preprocess", "node_skipped", 90,
                "Preprocessing skipped because committed lyric artifacts are cached",
                reason="downstream_cache_hit", node_progress_pct=100,
            )
            if os.path.isfile(recognized_text_path) and os.path.isfile(asr_segments_path):
                artifact_reused(
                    "lyrics.transcribe", 90, "Transcription artifacts already cached",
                    artifacts=[
                        _committed_artifact(
                            "RecognizedText", recognized_text_path, "output:0", "reused"
                        ),
                        _committed_artifact(
                            "AsrSegments", asr_segments_path, "output:1", "reused"
                        ),
                    ],
                )
            elif not lyrics_path:
                progress_node(
                    "lyrics.transcribe", "node_skipped", 90,
                    "Legacy transcript cache has no split ASR artifacts",
                    reason="legacy_cache", node_progress_pct=100,
                )
            if os.path.isfile(timed_transcript_path):
                artifact_reused(
                    "lyrics.align", 94, "Timed transcript already cached",
                    artifacts=[_committed_artifact(
                        "TimedTranscript", timed_transcript_path, "output:0", "reused"
                    )],
                )
            elif engine != "parakeet":
                progress_node(
                    "lyrics.align", "node_skipped", 94,
                    "Legacy transcript cache has no split timing artifact",
                    reason="legacy_cache", node_progress_pct=100,
                )
            artifact_reused(
                "chart.build_candidate", 99, "Candidate chart already committed",
                artifacts=[_committed_artifact(
                    "CandidateChart", transcript_path, "output:0", "reused"
                )],
            )
            return

        if skip_transcription:
            # Keep the provided LRC transcript; only stamp key/tempo onto it.
            transcript = {}
            if transcript_exists:
                try:
                    with open(transcript_path, "r", encoding="utf-8") as f:
                        transcript = json.load(f)
                except (OSError, ValueError) as e:
                    print(f"[uta-studio:LOG] Failed to read provided transcript: {e}", flush=True)
            build_candidate_chart(
                transcript_path, transcript, detected_key, tempo, detected_bpm,
                timed_transcript_path=timed_transcript_path,
            )
            return

        if callable(whisper_model):
            whisper_model = whisper_model()

        capture_preprocessed_path = None
        if capture_preprocessed_audio:
            capture_preprocessed_path = os.path.join(
                output_dir, f"{file_hash}_preprocessed_audio.flac"
            )
        transcript = transcribe_or_align(
            vocals_path, audio_path, device, output_dir, file_hash,
            model_name=model_name,
            beam_size=beam_size,
            batch_size=batch_size,
            engine=engine,
            lyrics_path=lyrics_path,
            language_override=language_override,
            whisper_model=whisper_model,
            pre_align_cleanup=pre_align_cleanup,
            capture_preprocessed_path=capture_preprocessed_path,
        )
        log_vram("phase:after_transcribe_or_align")

        build_candidate_chart(
            transcript_path, transcript, detected_key, tempo, detected_bpm,
            timed_transcript_path=timed_transcript_path,
        )
    finally:
        hard_free_gpu("pipeline_end")
