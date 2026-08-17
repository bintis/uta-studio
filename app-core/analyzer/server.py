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
import time

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
    stage, operation = _classify_progress(int(pct), message)
    start, end = STAGE_RANGES.get(stage, (0, 100))
    stage_progress = 100 if end <= start else round((int(pct) - start) * 100 / (end - start))
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

    # A progress event may be the first event for a new stage. Clear the
    # previous stage's sticky execution metadata before resolving defaults,
    # otherwise the new route inherits (and then permanently stores) the old
    # implementation/model pair.
    if runtime_state.get("stage") != stage:
        for key in (
            "actual_device",
            "requested_device",
            "fallback_from",
            "fallback_reason",
            "implementation",
            "model",
            "backend_fallback_from",
            "backend_fallback_reason",
        ):
            runtime_state.pop(key, None)
        runtime_state["stage"] = stage

    implementation = str(
        metadata.get("implementation")
        or runtime_state.get("implementation")
        or implementation
    )
    model = str(metadata.get("model") or runtime_state.get("model") or model)

    requested_device = str(metadata.get("requested_device") or device)
    default_device = "cpu" if stage in (
        "preparing",
        "key_detection",
        "audio_preprocessing",
        "finalizing",
        "complete",
    ) else device
    effective_device = str(
        metadata.get("actual_device")
        or runtime_state.get("actual_device")
        or default_device
    )
    fallback_from = metadata.get("fallback_from") or runtime_state.get("fallback_from")
    fallback_reason = metadata.get("fallback_reason") or runtime_state.get("fallback_reason")
    backend_fallback_from = (
        metadata.get("backend_fallback_from")
        or runtime_state.get("backend_fallback_from")
    )
    backend_fallback_reason = (
        metadata.get("backend_fallback_reason")
        or runtime_state.get("backend_fallback_reason")
    )

    # Keep the execution route attached to all later events in this stage.
    runtime_state["actual_device"] = effective_device
    runtime_state["requested_device"] = requested_device
    runtime_state["implementation"] = implementation
    runtime_state["model"] = model
    if fallback_from:
        runtime_state["fallback_from"] = str(fallback_from)
    if fallback_reason:
        runtime_state["fallback_reason"] = str(fallback_reason)
    if backend_fallback_from:
        runtime_state["backend_fallback_from"] = str(backend_fallback_from)
    if backend_fallback_reason:
        runtime_state["backend_fallback_reason"] = str(backend_fallback_reason)

    # Keyed by the real node id when the call site has migrated to
    # `progress_node`/`artifact_reused` (analysis DAG redesign Phase 3),
    # falling back to the coarse bucket `stage` text for call sites that
    # haven't. Keying by node id (not just `stage`) matters for a compound
    # node's children -- e.g. music.key/music.rhythm/music.descriptors all
    # share the "preparing" bucket, and used to overwrite one shared dict
    # entry, silently losing all but the last child's route. Each real node
    # id now keeps its own entry regardless of how many nodes share a
    # bucket.
    node_id = metadata.get("node_id")
    node_event = metadata.get("event")
    stage_routes = runtime_state.setdefault("stage_routes", {})
    # Phase 3 gap closed: per-node Start/Finish timestamps. The route dict
    # is fully replaced (not merged) on every call, so `started_at_ms` has
    # to be read back from whatever was already recorded or it would reset
    # on every single progress update for the node, not just its first one.
    # Wall-clock time here (not something threaded from Rust) because this
    # runs in the same process as the actual node work -- it measures real
    # execution time, not socket/IPC latency. `artifact_reused` has no
    # `node_started` before it (a cache hit never "starts"), so its own
    # single event stamps both fields at once.
    existing_route = stage_routes.get(node_id or stage, {})
    event_at_ms = int(time.time() * 1000)
    started_at_ms = existing_route.get("started_at_ms")
    if started_at_ms is None or node_event in ("node_started", "artifact_reused"):
        started_at_ms = event_at_ms
    finished_at_ms = existing_route.get("finished_at_ms")
    if node_event in ("node_completed", "node_failed", "artifact_reused"):
        finished_at_ms = event_at_ms
    stage_routes[node_id or stage] = {
        "stage": stage,
        "node_id": node_id,
        "node_event": node_event,
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
    }

    payload = {
        "type": "progress",
        "pct": int(pct),
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
    if node_event == "artifact_reused":
        payload["artifact_reused_reason"] = metadata.get("reason")
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
        )
    finally:
        end_of_song_cleanup()
        log_vram("song_end")


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
                try:
                    progress_state = {}
                    set_progress_sink(
                        lambda pct, msg, metadata=None, current=cmd, state=progress_state: _send(
                            wfile,
                            _progress_payload(
                                current,
                                device,
                                pct,
                                msg,
                                metadata,
                                state,
                            ),
                        )
                    )
                    process_song(cmd, device)
                    _send(wfile, {"type": "done", "hash": cmd.get("hash", "")})
                except Exception as e:
                    import traceback
                    traceback.print_exc(file=sys.stderr)
                    err_str = str(e)
                    if is_oom(err_str):
                        snap = vram_snapshot()
                        if snap:
                            print(
                                f"[uta-studio:LOG] OOM in process_song; vram={snap}",
                                file=sys.stderr, flush=True,
                            )
                        end_of_song_cleanup()
                        _send(wfile, {"type": "error", "kind": "oom", "msg": err_str})
                    else:
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
