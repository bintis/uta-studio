#!/usr/bin/env python3
"""Persistent analyzer server for Uta Studio.

Binds a TCP loopback socket and exchanges newline-delimited JSON with the
Rust client. Stdout/stderr are reserved for plain logs aside from one
handshake line emitted on startup.

Handshake (stdout, single line):
  {"event":"ready","port":N,"token":"...","device":"..."}

Wire protocol (NDJSON over TCP, one JSON object per line):
  Client -> server:
    {"type":"hello","token":"..."}
    {"type":"analyze","hash":"...","audio_path":"...","cache_path":"...", ...}
    {"type":"quit"}
  Server -> client:
    {"type":"hello_ack"}
    {"type":"progress","pct":N,"msg":"...","stage":"...",...}
    {"type":"done","hash":"..."}
    {"type":"error","kind":"oom"|"generic","msg":"..."}
"""

import json
import os
import secrets
import socket
import sys
import threading
import time
from contextlib import contextmanager

if os.name == "nt":
    import huggingface_hub.file_download as _hf_dl
    _hf_dl.are_symlinks_supported = lambda *_a, **_kw: False

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from gpu import end_of_song_cleanup, hard_free_gpu, log_vram, reset_peak_stats, vram_snapshot
from whisper_compat import detect_device, is_oom, set_align_backend, set_progress_sink
from audio import set_vocal_threshold_pct
from pipeline import run_pipeline


SEPARATOR_DETAILS = {
    "karaoke": ("UVR", "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956"),
    "demucs": ("Demucs", "htdemucs"),
    "openvino_demucs": ("OpenVINO", "OpenVINO Demucs"),
}

ALIGNMENT_DETAILS = {
    "whisperx": ("WhisperX", "Viterbi forced alignment"),
    "ctc": ("torchaudio", "CTC forced alignment"),
    "qwen": ("Qwen", "Qwen3 ForcedAligner"),
    "mms_karaoke": ("MMS Karaoke", "MMS CTC alignment"),
}

LANGUAGE_ALIASES = {
    "jp": "ja",
    "jpn": "ja",
    "eng": "en",
    "kor": "ko",
    "chi": "zh",
    "zho": "zh",
    "cn": "zh",
    "zh-cn": "zh",
    "zh-tw": "zh",
}


def _normalize_language(value):
    if value is None:
        return None
    normalized = str(value).strip().lower().replace("_", "-")
    return LANGUAGE_ALIASES.get(normalized, normalized) or None

STAGE_RANGES = {
    "preparing": (0, 4),
    "key_detection": (2, 4),
    "separation": (4, 52),
    "pitch": (51, 55),
    "audio_preprocessing": (54, 60),
    "transcription": (59, 80),
    "alignment": (80, 95),
    "finalizing": (95, 100),
    "complete": (100, 100),
}

NODE_STAGES = {
    "preflight": "preparing",
    "music.analysis": "key_detection",
    "music.key": "key_detection",
    "music.rhythm": "key_detection",
    "music.descriptors": "key_detection",
    "stems.separate": "separation",
    "stems.vocals": "separation",
    "vocals.denoise": "separation",
    "vocals.dereverb": "separation",
    "stems.instrumental": "separation",
    "instrumental.denoise": "separation",
    "instrumental.dereverb": "separation",
    "stems.karaoke": "separation",
    "stems.multistem": "separation",
    "stems.bind_analysis_outputs": "separation",
    "pitch.extract": "pitch",
    "lyrics.preprocess": "audio_preprocessing",
    "lyrics.transcribe": "transcription",
    "lyrics.align": "alignment",
    "lyrics.import_timed": "alignment",
    "chart.build_candidate": "finalizing",
}

NODE_OPERATIONS = {
    "preflight": "Source and runtime validation",
    "music.analysis": "Music analysis",
    "music.key": "Musical key detection",
    "music.rhythm": "Tempo and beat analysis",
    "music.descriptors": "Audio descriptor analysis",
    "stems.separate": "Stem processing plan",
    "stems.vocals": "Vocal extraction",
    "vocals.denoise": "Vocal denoise",
    "vocals.dereverb": "Vocal dereverb",
    "stems.instrumental": "Accompaniment extraction",
    "instrumental.denoise": "BGM denoise",
    "instrumental.dereverb": "BGM dereverb",
    "stems.karaoke": "Karaoke accompaniment extraction",
    "stems.multistem": "Multi-stem separation",
    "stems.bind_analysis_outputs": "Analysis audio binding",
    "pitch.extract": "Reference pitch extraction",
    "lyrics.preprocess": "Vocal-region preprocessing",
    "lyrics.transcribe": "Speech transcription",
    "lyrics.align": "Word timing alignment",
    "lyrics.import_timed": "Timed lyrics import",
    "chart.build_candidate": "Candidate chart commit",
}

# Relative cold-run cost used only until real per-node duration history is
# available on the native side. Compound compatibility shells deliberately
# have zero weight: their children are the work shown in Full DAG view.
DEFAULT_NODE_WEIGHTS = {
    "preflight": 1,
    "music.analysis": 0,
    "music.key": 1,
    "music.rhythm": 1,
    "music.descriptors": 1,
    "stems.separate": 0,
    "stems.vocals": 12,
    "vocals.denoise": 8,
    "vocals.dereverb": 8,
    "stems.instrumental": 12,
    "instrumental.denoise": 8,
    "instrumental.dereverb": 8,
    "stems.karaoke": 12,
    "stems.multistem": 12,
    "stems.bind_analysis_outputs": 1,
    "pitch.extract": 5,
    "lyrics.preprocess": 2,
    "lyrics.transcribe": 9,
    "lyrics.align": 7,
    "lyrics.import_timed": 1,
    "chart.build_candidate": 1,
}

TERMINAL_NODE_EVENTS = {
    "completed",
    "reused",
    "skipped",
    "failed",
    "cancelled",
}


def _canonical_node_event(event):
    return {
        "node_started": "started",
        "node_progress": "progress",
        "node_completed": "completed",
        "artifact_reused": "reused",
        "node_skipped": "skipped",
        "node_failed": "failed",
        "node_cancelled": "cancelled",
    }.get(event, event)


def _planned_node_weights(cmd):
    weights = {
        node_id: DEFAULT_NODE_WEIGHTS[node_id]
        for node_id in ("preflight", "music.key", "music.rhythm", "music.descriptors")
    }
    if not bool(cmd.get("skip_separation", False)):
        audio_processing = cmd.get("audio_processing") or {}
        step_ids = [str(step.get("step_id") or "") for step in audio_processing.get("steps") or []]
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
        planned_stems = {node_for_step[step] for step in step_ids if step in node_for_step}
        if not planned_stems:
            planned_stems.add("stems.multistem")
        for node_id in planned_stems:
            weights[node_id] = DEFAULT_NODE_WEIGHTS[node_id]
        weights["stems.bind_analysis_outputs"] = DEFAULT_NODE_WEIGHTS["stems.bind_analysis_outputs"]
    if not bool(cmd.get("skip_pitch", False)):
        weights["pitch.extract"] = DEFAULT_NODE_WEIGHTS["pitch.extract"]
    if not bool(cmd.get("skip_transcription", False)):
        weights["lyrics.preprocess"] = DEFAULT_NODE_WEIGHTS["lyrics.preprocess"]
        if cmd.get("lyrics"):
            weights["lyrics.align"] = DEFAULT_NODE_WEIGHTS["lyrics.align"]
        else:
            weights["lyrics.transcribe"] = DEFAULT_NODE_WEIGHTS["lyrics.transcribe"]
            if cmd.get("engine", "whisper") != "parakeet":
                weights["lyrics.align"] = DEFAULT_NODE_WEIGHTS["lyrics.align"]
    weights["chart.build_candidate"] = DEFAULT_NODE_WEIGHTS["chart.build_candidate"]
    historical = cmd.get("node_weights") or {}
    # Compatibility for command payloads written by older native clients.
    if isinstance(historical, dict):
        for node_id in list(weights):
            measured = historical.get(node_id)
            if isinstance(measured, (int, float)) and measured > 0:
                weights[node_id] = measured
    return weights


def _apply_matching_historical_weights(cmd, routes, weights, runtime_state):
    samples = cmd.get("node_weights") or []
    if not isinstance(samples, list):
        return
    matched = runtime_state.setdefault("matched_historical_weights", {})
    for route in routes:
        node_id = route.get("node_id")
        implementation = route.get("implementation")
        actual_device = route.get("actual_device")
        if not node_id or not implementation or not actual_device:
            continue
        signature = (implementation, actual_device)
        if matched.get(node_id) == signature:
            continue
        sample = next(
            (
                item
                for item in samples
                if item.get("node_id") == node_id
                and item.get("implementation") == implementation
                and item.get("actual_device") == actual_device
                and isinstance(item.get("duration_ms"), (int, float))
                and item["duration_ms"] > 0
            ),
            None,
        )
        weights[node_id] = (
            sample["duration_ms"]
            if sample is not None
            else DEFAULT_NODE_WEIGHTS.get(node_id, weights.get(node_id, 1))
        )
        matched[node_id] = signature


def _aggregate_overall_progress(cmd, routes, runtime_state):
    weights = runtime_state.setdefault("node_weights", _planned_node_weights(cmd))
    _apply_matching_historical_weights(cmd, routes, weights, runtime_state)
    by_node = {
        route.get("node_id"): route
        for route in routes
        if route.get("node_id")
    }
    # A fallback route can appear at runtime (for example Parakeet falling
    # back to Whisper alignment). Add that real work to the denominator.
    for node_id in by_node:
        if node_id not in weights and node_id in DEFAULT_NODE_WEIGHTS:
            weights[node_id] = DEFAULT_NODE_WEIGHTS[node_id]
    total_weight = sum(max(0, weight) for weight in weights.values()) or 1
    completed = 0.0
    # Terminal means the node no longer has work left in this attempt, not
    # that it necessarily succeeded.  Pitch/descriptors are allowed to fail
    # without aborting the chart, so leaving their weight permanently at zero
    # would make an otherwise completed run jump from a stale percentage to
    # 100 only when Rust receives `done`.
    for node_id, weight in weights.items():
        route = by_node.get(node_id)
        if route is None:
            continue
        node_progress = route.get("stage_progress", 0)
        if _canonical_node_event(route.get("node_event")) in TERMINAL_NODE_EVENTS:
            node_progress = 100
        completed += max(0, weight) * min(max(int(node_progress), 0), 100) / 100.0
    calculated = min(99, int(completed * 100 / total_weight))
    previous = int(runtime_state.get("overall_progress", 0))
    overall = max(previous, calculated)
    runtime_state["overall_progress"] = overall
    return overall


def _classify_progress(pct, message):
    text = str(message).lower()
    if pct >= 100:
        return "complete", "Analysis complete"
    if (
        "musical key" in text
        or "chroma" in text
        or "tempo and beat" in text
        or "music analysis" in text
    ):
        return "key_detection", "Musical key and tempo analysis"
    if "pitch" in text or "singing guide" in text:
        return "pitch", "Reference pitch extraction"
    if "align" in text:
        return "alignment", "Word timing alignment"
    if "transcrib" in text or "parakeet" in text:
        return "transcription", "Speech transcription"
    if "language" in text:
        return "audio_preprocessing", "Language detection"
    if (
        "vocal region" in text
        or "loading lyrics" in text
        # Specifically transcribe.py:43's `f"Loading audio ({vocals_path})..."`
        # -- narrowed to the "(" that always follows in that message,
        # because the bare substring "loading audio" also matches
        # stems.py:85's `"Loading audio file..."` (pct 10, meant to classify
        # as "separation" per STAGE_RANGES and locked by
        # test_pipeline_cache.py's ClassifyProgressStageBaselineTests), a
        # real misclassification confirmed against both real call sites.
        or "loading audio (" in text
        or "detecting vocal" in text
    ):
        return "audio_preprocessing", "Vocal-region preprocessing"
    if "separat" in text or "stem" in text or 5 <= pct <= 54:
        if "loading" in text:
            return "separation", "Loading separation model"
        if "saving" in text or "cache" in text:
            return "separation", "Committing separated stems"
        return "separation", "Vocal stem separation"
    if "writing transcript" in text or pct >= 95:
        return "finalizing", "Committing analysis results"
    if pct <= 4:
        return "preparing", "Audio preprocessing"
    if pct >= 80:
        return "alignment", "Word timing alignment"
    if pct >= 55:
        return "transcription", "Preparing transcription"
    return "preparing", "Preparing analysis"


def _progress_payload(cmd, device, pct, message, metadata=None, runtime_state=None):
    metadata = metadata or {}
    runtime_state = runtime_state if runtime_state is not None else {}
    runtime_state["reported_pct"] = int(pct)
    node_id = metadata.get("node_id")
    node_event = _canonical_node_event(metadata.get("event"))
    if node_id is None and runtime_state.get("active_node_id"):
        node_id = runtime_state["active_node_id"]
        node_event = "progress"
        metadata = dict(metadata)
        metadata["node_id"] = node_id
        metadata["event"] = node_event
    if node_id is not None and node_event in ("started", "progress"):
        runtime_state["active_node_id"] = node_id
    elif (
        node_id is not None
        and node_event in TERMINAL_NODE_EVENTS
        and runtime_state.get("active_node_id") == node_id
    ):
        runtime_state.pop("active_node_id", None)

    classified_stage, classified_operation = _classify_progress(int(pct), message)
    stage = NODE_STAGES.get(node_id, classified_stage)
    operation = NODE_OPERATIONS.get(node_id, classified_operation)
    start, end = STAGE_RANGES.get(stage, (0, 100))
    stage_progress = 100 if end <= start else round((int(pct) - start) * 100 / (end - start))
    if metadata.get("node_progress_pct") is not None:
        stage_progress = int(metadata["node_progress_pct"])
    work_completed = metadata.get("work_units_completed")
    work_total = metadata.get("work_units_total")
    if (
        isinstance(work_completed, (int, float))
        and isinstance(work_total, (int, float))
        and work_total > 0
    ):
        stage_progress = round(work_completed * 100 / work_total)
    stage_progress = min(max(stage_progress, 0), 100)
    separator_impl, separator_model = SEPARATOR_DETAILS.get(
        cmd.get("separator", "karaoke"), (cmd.get("separator", "karaoke"), "")
    )
    align_impl, align_model = ALIGNMENT_DETAILS.get(
        cmd.get("align_backend", "whisperx"), (cmd.get("align_backend", "whisperx"), "")
    )

    if stage == "separation":
        implementation, model = separator_impl, separator_model
    elif stage == "pitch":
        implementation, model = "RMVPE", "RMVPE singing pitch model"
    elif stage == "transcription":
        implementation = cmd.get("engine", "whisper")
        model = "Parakeet v3" if implementation == "parakeet" else cmd.get("model", "large-v3")
    elif stage == "alignment":
        implementation, model = align_impl, align_model
    elif stage == "key_detection":
        implementation, model = "Essentia/NumPy FFT", "Key + rhythm analysis"
    elif stage == "audio_preprocessing":
        implementation, model = "Uta Studio audio DSP", "RMS region detection"
    elif stage == "finalizing":
        implementation, model = "Uta Studio chart pipeline", "Transcript and pitch evidence"
    else:
        implementation, model = "FFmpeg + Uta Studio", "Source preparation"

    stage_routes = runtime_state.setdefault("stage_routes", {})
    route_key = node_id or stage
    existing_route = stage_routes.get(route_key, {})

    implementation = str(
        metadata.get("implementation")
        or existing_route.get("implementation")
        or implementation
    )
    model = str(metadata.get("model") or existing_route.get("model") or model)

    requested_device = str(
        metadata.get("requested_device")
        or existing_route.get("requested_device")
        or device
    )
    default_device = "cpu" if stage in (
        "preparing",
        "key_detection",
        "audio_preprocessing",
        "finalizing",
        "complete",
    ) else device
    effective_device = str(
        metadata.get("actual_device")
        or existing_route.get("actual_device")
        or default_device
    )
    fallback_from = metadata.get("fallback_from") or existing_route.get("fallback_from")
    fallback_reason = metadata.get("fallback_reason") or existing_route.get("fallback_reason")
    backend_fallback_from = (
        metadata.get("backend_fallback_from")
        or existing_route.get("backend_fallback_from")
    )
    backend_fallback_reason = (
        metadata.get("backend_fallback_reason")
        or existing_route.get("backend_fallback_reason")
    )

    # Keyed by the real node id when the call site has migrated to
    # `progress_node`/`artifact_reused` (analysis DAG redesign Phase 3),
    # falling back to the coarse bucket `stage` text for call sites that
    # haven't. Keying by node id (not just `stage`) matters for a compound
    # node's children -- e.g. music.key/music.rhythm/music.descriptors all
    # share the "preparing" bucket, and used to overwrite one shared dict
    # entry, silently losing all but the last child's route. Each real node
    # id now keeps its own entry regardless of how many nodes share a
    # bucket.
    # Phase 3 gap closed: per-node Start/Finish timestamps. The route dict
    # is fully replaced (not merged) on every call, so `started_at_ms` has
    # to be read back from whatever was already recorded or it would reset
    # on every single progress update for the node, not just its first one.
    # Wall-clock time here (not something threaded from Rust) because this
    # runs in the same process as the actual node work -- it measures real
    # execution time, not socket/IPC latency. `artifact_reused` has no
    # `started` before it (a cache hit never "starts"), so its own
    # single event stamps both fields at once.
    event_at_ms = int(time.time() * 1000)
    started_at_ms = existing_route.get("started_at_ms")
    if started_at_ms is None or node_event in ("started", "reused"):
        started_at_ms = event_at_ms
    finished_at_ms = existing_route.get("finished_at_ms")
    if node_event in TERMINAL_NODE_EVENTS:
        finished_at_ms = event_at_ms
    committed_outputs = metadata.get("artifacts")
    if committed_outputs is None:
        committed_outputs = existing_route.get("committed_outputs", [])
    if not isinstance(committed_outputs, list):
        committed_outputs = []
    stage_routes[route_key] = {
        "stage": stage,
        "node_id": node_id,
        "node_event": node_event,
        "binding_kind": (
            metadata.get("reason")
            if node_event == "reused"
            else "bypassed" if node_event == "skipped" else None
        ),
        "committed_outputs": committed_outputs,
        "operation": operation,
        "implementation": implementation,
        "model": model,
        "stage_progress": stage_progress,
        "requested_device": requested_device,
        "actual_device": effective_device,
        "fallback_from": str(fallback_from) if fallback_from else None,
        "fallback_reason": str(fallback_reason) if fallback_reason else None,
        "backend_fallback_from": str(backend_fallback_from) if backend_fallback_from else None,
        "backend_fallback_reason": str(backend_fallback_reason) if backend_fallback_reason else None,
        "started_at_ms": started_at_ms,
        "finished_at_ms": finished_at_ms,
        "event_at_ms": event_at_ms,
        "work_units_completed": metadata.get("work_units_completed"),
        "work_units_total": metadata.get("work_units_total"),
    }

    overall_progress = _aggregate_overall_progress(
        cmd, list(stage_routes.values()), runtime_state
    )
    payload = {
        "type": "progress",
        "pct": overall_progress,
        "reported_pct": int(pct),
        "msg": str(message),
        "stage": stage,
        "stage_progress": stage_progress,
        "operation": operation,
        "implementation": str(implementation),
        "model": str(model),
        "device": str(effective_device),
        "requested_device": requested_device,
        "fallback_from": str(fallback_from) if fallback_from else None,
        "fallback_reason": str(fallback_reason) if fallback_reason else None,
        "backend_fallback_from": str(backend_fallback_from) if backend_fallback_from else None,
        "backend_fallback_reason": str(backend_fallback_reason) if backend_fallback_reason else None,
        "stage_routes": list(stage_routes.values()),
        # Structured Node event fields (analysis DAG redesign Phase 3).
        # `None` unless the emitter used `progress_node`/`artifact_reused`
        # (whisper_compat.py) -- everything above is the pre-Phase-3 Legacy
        # Adapter contract and is computed identically whether or not these
        # are present, so old consumers (today's desktop UI) are unaffected.
        "node_id": node_id,
        "event": node_event,
    }
    if node_event == "reused":
        payload["artifact_reused_reason"] = metadata.get("reason")
    if metadata.get("work_units_completed") is not None:
        payload["work_units_completed"] = metadata["work_units_completed"]
    if metadata.get("work_units_total") is not None:
        payload["work_units_total"] = metadata["work_units_total"]
    payload["event_at_ms"] = event_at_ms
    return payload


def process_song(cmd, device):
    audio_path = os.path.abspath(cmd["audio_path"])
    output_dir = os.path.abspath(cmd["cache_path"])
    file_hash = cmd["hash"]
    model_name = cmd.get("model", "large-v3")
    beam_size = cmd.get("beam_size", 8)
    batch_size = cmd.get("batch_size", 8)
    separator = cmd.get("separator", "karaoke")
    separator_options = cmd.get("separator_options") or {}
    audio_processing = cmd.get("audio_processing")
    run_work_dir = cmd.get("run_work_dir")
    engine = cmd.get("engine", "whisper")
    lyrics_path = cmd.get("lyrics")
    language_override = _normalize_language(cmd.get("language"))
    skip_transcription = bool(cmd.get("skip_transcription", False))
    skip_separation = bool(cmd.get("skip_separation", False))
    skip_pitch = bool(cmd.get("skip_pitch", False))
    freeze_separation = bool(cmd.get("freeze_separation", False))
    freeze_pitch = bool(cmd.get("freeze_pitch", False))
    bypass_separation_with_original_mix = bool(
        cmd.get("bypass_separation_with_original_mix", False)
    )
    capture_preprocessed_audio = bool(cmd.get("capture_preprocessed_audio", False))

    set_align_backend(cmd.get("align_backend", "whisperx"))
    set_vocal_threshold_pct(cmd.get("vocal_detection_threshold_pct"))

    reset_peak_stats()
    log_vram("song_start")
    try:
        run_pipeline(
            audio_path, output_dir, file_hash, device,
            model_name=model_name,
            beam_size=beam_size,
            batch_size=batch_size,
            separator=separator,
            separator_options=separator_options,
            audio_processing=audio_processing,
            run_work_dir=run_work_dir,
            engine=engine,
            lyrics_path=lyrics_path,
            language_override=language_override,
            whisper_model=None,
            pre_align_cleanup=end_of_song_cleanup,
            free_gpu_fn=hard_free_gpu,
            skip_transcription=skip_transcription,
            skip_separation=skip_separation,
            skip_pitch=skip_pitch,
            freeze_separation=freeze_separation,
            freeze_pitch=freeze_pitch,
            bypass_separation_with_original_mix=bypass_separation_with_original_mix,
            capture_preprocessed_audio=capture_preprocessed_audio,
        )
    finally:
        end_of_song_cleanup()
        log_vram("song_end")


class _AnalysisLog:
    def __init__(self, handle):
        self.handle = handle
        self.lock = threading.Lock()
        self.active_node = None

    def write(self, record_type, **fields):
        if self.handle is None:
            return
        record = {
            "timestamp_ms": int(time.time() * 1000),
            "record_type": record_type,
            **fields,
        }
        with self.lock:
            self.handle.write(
                json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n"
            )
            self.handle.flush()

    def event(self, payload):
        logged = dict(payload)
        logged.pop("stage_routes", None)
        self.write("node_event", **logged)
        event = _canonical_node_event(payload.get("event"))
        with self.lock:
            if event in ("started", "progress"):
                self.active_node = payload.get("node_id")
            elif event in TERMINAL_NODE_EVENTS and self.active_node == payload.get("node_id"):
                self.active_node = None

    def process_output(self, stream, message):
        with self.lock:
            node_id = self.active_node
        self.write(
            "process_output",
            node_id=node_id,
            stream=stream,
            message=message.rstrip("\r\n"),
        )

    def terminal(self, status, message=""):
        self.write("run_terminal", status=status, message=str(message))


def _capture_fd_lines(read_fd, analysis_log, stream):
    with os.fdopen(read_fd, "r", encoding="utf-8", errors="replace") as reader:
        for line in reader:
            analysis_log.process_output(stream, line)


@contextmanager
def _analysis_log(cmd):
    path = cmd.get("analysis_log_path")
    if not path:
        yield _AnalysisLog(None)
        return
    path = os.path.abspath(os.fspath(path))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    handle = open(path, "a", encoding="utf-8", buffering=1)
    analysis_log = _AnalysisLog(handle)
    saved_stdout = os.dup(1)
    saved_stderr = os.dup(2)
    stdout_read, stdout_write = os.pipe()
    stderr_read, stderr_write = os.pipe()
    stdout_thread = threading.Thread(
        target=_capture_fd_lines,
        args=(stdout_read, analysis_log, "stdout"),
        daemon=True,
    )
    stderr_thread = threading.Thread(
        target=_capture_fd_lines,
        args=(stderr_read, analysis_log, "stderr"),
        daemon=True,
    )
    try:
        analysis_log.write(
            "python_attached",
            device=cmd.get("_resolved_device", ""),
            engine=cmd.get("engine", "whisper"),
            separator=cmd.get("separator", "karaoke"),
        )
        config_snapshot = {
            key: value
            for key, value in cmd.items()
            if key not in {"analysis_log_path", "run_work_dir"}
        }
        analysis_log.write("config_summary", config=config_snapshot)
        sys.stdout.flush()
        sys.stderr.flush()
        stdout_thread.start()
        stderr_thread.start()
        os.dup2(stdout_write, 1)
        os.dup2(stderr_write, 2)
        os.close(stdout_write)
        os.close(stderr_write)
        yield analysis_log
    finally:
        try:
            sys.stdout.flush()
            sys.stderr.flush()
        finally:
            os.dup2(saved_stdout, 1)
            os.dup2(saved_stderr, 2)
            os.close(saved_stdout)
            os.close(saved_stderr)
            stdout_thread.join(timeout=5)
            stderr_thread.join(timeout=5)
            handle.close()


def _send(wfile, payload):
    try:
        wfile.write(json.dumps(payload, ensure_ascii=False) + "\n")
        wfile.flush()
    except (BrokenPipeError, OSError) as e:
        print(f"[uta-studio:LOG] Failed to send message: {e}", file=sys.stderr, flush=True)


def main():
    device = detect_device()
    token = secrets.token_hex(16)

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    port = srv.getsockname()[1]

    print(
        json.dumps({"event": "ready", "port": port, "token": token, "device": device}),
        flush=True,
    )

    srv.settimeout(60.0)
    try:
        conn, _addr = srv.accept()
    except socket.timeout:
        print("[uta-studio:LOG] Timed out waiting for client connection", file=sys.stderr, flush=True)
        return
    finally:
        srv.close()

    conn.settimeout(None)
    rfile = conn.makefile("r", encoding="utf-8", newline="\n")
    wfile = conn.makefile("w", encoding="utf-8", newline="\n")

    hello_line = rfile.readline()
    if not hello_line:
        return
    try:
        hello = json.loads(hello_line)
    except json.JSONDecodeError:
        return
    if hello.get("type") != "hello" or hello.get("token") != token:
        print("[uta-studio:LOG] Auth failed, closing connection", file=sys.stderr, flush=True)
        return
    _send(wfile, {"type": "hello_ack"})

    try:
        for line in rfile:
            line = line.strip()
            if not line:
                continue
            try:
                cmd = json.loads(line)
            except json.JSONDecodeError as e:
                _send(wfile, {"type": "error", "kind": "generic", "msg": f"Invalid JSON: {e}"})
                continue

            ctype = cmd.get("type")
            if ctype == "quit":
                break
            if ctype == "analyze":
                cmd["_resolved_device"] = device
                with _analysis_log(cmd) as analysis_log:
                    try:
                        progress_state = {}

                        def send_progress(pct, msg, metadata=None, current=cmd, state=progress_state):
                            payload = _progress_payload(
                                current, device, pct, msg, metadata, state
                            )
                            analysis_log.event(payload)
                            _send(wfile, payload)

                        set_progress_sink(send_progress)
                        process_song(cmd, device)
                        analysis_log.terminal("completed")
                        _send(wfile, {"type": "done", "hash": cmd.get("hash", "")})
                    except Exception as e:
                        import traceback
                        traceback.print_exc(file=sys.stderr)
                        err_str = str(e)
                        active_node = progress_state.get("active_node_id")
                        if active_node:
                            failed_payload = _progress_payload(
                                cmd,
                                device,
                                progress_state.get("reported_pct", 0),
                                err_str,
                                {
                                    "node_id": active_node,
                                    "event": "failed",
                                },
                                progress_state,
                            )
                            analysis_log.event(failed_payload)
                            _send(wfile, failed_payload)
                        if is_oom(err_str):
                            snap = vram_snapshot()
                            if snap:
                                print(
                                    f"[uta-studio:LOG] OOM in process_song; vram={snap}",
                                    file=sys.stderr, flush=True,
                                )
                            end_of_song_cleanup()
                            analysis_log.terminal("oom", err_str)
                            _send(wfile, {"type": "error", "kind": "oom", "msg": err_str})
                        else:
                            analysis_log.terminal("failed", err_str)
                            _send(wfile, {"type": "error", "kind": "generic", "msg": err_str})
            else:
                _send(wfile, {"type": "error", "kind": "generic", "msg": f"Unknown command: {ctype!r}"})
    finally:
        try:
            wfile.close()
        except Exception:
            pass
        try:
            rfile.close()
        except Exception:
            pass
        try:
            conn.close()
        except Exception:
            pass


if __name__ == "__main__":
    main()
