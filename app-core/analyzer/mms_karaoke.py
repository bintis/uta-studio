"""Japanese karaoke forced alignment inspired by FA-Kara.

This module adapts FA-Kara's useful alignment ideas to Uta Studio's in-process
analysis contract:

* lyrics are converted to display tokens plus Latin pronunciation units;
* silent gaps in the separated vocal are removed for the CTC pass and every
  timestamp is mapped back to the untouched source timeline;
* line-leading and sustained line-final vocals are restored after alignment;
* the result uses Uta Studio's existing ``segments[].words[]`` shape.

The optional model is never fetched here. Settings > Models & runtime must
install it first; ``local_files_only=True`` keeps analysis offline.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

import numpy as np
import torch

import cjk
from gpu import gpu_model
from whisper_compat import is_oom


MODEL_ID = "NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn"
SAMPLE_RATE = 16000

_ANNOTATION_RE = re.compile(r"(\{[^{}]*?\}|\[[^\[\]]*?\])")
_LATIN_RE = re.compile(r"[^a-z']+")
_SMALL_KANA = frozenset(
    "ゃゅょぁぃぅぇぉャュョァィゥェォー"
)
_SOKUON = frozenset("っッ")


class MmsKaraokeUnsupportedError(Exception):
    """The selected model cannot align this request."""


@dataclass(frozen=True)
class TimeMap:
    compressed_start: float
    compressed_end: float
    original_start: float
    original_end: float


def is_supported(language: str) -> bool:
    normalized = str(language or "").strip().lower().replace("_", "-")
    return normalized == "ja" or normalized.startswith("ja-")


def _normalize_latin(text: str) -> str:
    text = text.lower().replace("’", "'")
    return _LATIN_RE.sub("", text)


def _kana_to_latin(text: str) -> str:
    if not text:
        return ""
    try:
        converted = cjk._get_pykakasi().convert(text)
    except Exception:
        return ""
    return _normalize_latin("".join(part.get("hepburn", "") for part in converted))


def split_kana_morae(reading: str) -> list[str]:
    """Split kana into FA-Kara-compatible pronunciation units.

    Small kana and long-vowel marks stay with the previous kana. Sokuon also
    stays attached by default and is resolved against the following unit's
    consonant. Hatsuon (ん) remains an independent unit.
    """
    out: list[str] = []
    for char in reading:
        if (char in _SMALL_KANA or char in _SOKUON) and out:
            out[-1] += char
        else:
            out.append(char)
    return out


def _display_text(line: str) -> str:
    def replace(match: re.Match) -> str:
        content = match.group(0)[1:-1]
        surface, separator, _reading = content.partition("|")
        return surface if separator else match.group(0)

    return _ANNOTATION_RE.sub(replace, line).strip()


def _plain_display_tokens(text: str) -> list[dict]:
    tokens: list[dict] = []
    try:
        morphemes = cjk._get_fugashi()(text)
    except Exception:
        morphemes = []
    for morpheme in morphemes:
        surface = getattr(morpheme, "surface", None) or str(morpheme)
        feature = getattr(morpheme, "feature", None)
        reading = ""
        if feature is not None:
            # UniDic's pronunciation fields reflect spoken particles such as
            # は→ワ and へ→エ. That is more useful for acoustic alignment than
            # the orthographic kana fields used by the generic CJK backend.
            for attribute in ("pron", "pronBase", "kana", "kanaBase"):
                value = getattr(feature, attribute, None)
                if value and value != "*":
                    reading = value
                    break
        raw_units = split_kana_morae(reading) if reading else []
        explicit = None
        if not raw_units:
            latin = _normalize_latin(surface)
            explicit = [latin] if latin else []
        tokens.append(
            {
                "surface": surface,
                "raw_units": raw_units,
                "explicit_units": explicit,
            }
        )
    return tokens


def _annotated_token(value: str) -> dict:
    marker = value[0]
    content = value[1:-1]
    surface, separator, reading = content.partition("|")
    if not separator or not surface or not reading:
        return {"surface": value, "raw_units": [], "explicit_units": []}
    if marker == "{":
        return {
            "surface": surface,
            "raw_units": split_kana_morae(reading),
            "explicit_units": None,
        }
    latin = _normalize_latin(reading)
    return {
        "surface": surface,
        "raw_units": [],
        "explicit_units": [latin] if latin else [],
    }


def prepare_line(line: str) -> tuple[str, list[dict]]:
    """Return display text and display-token/alignment-unit mappings."""
    display_tokens: list[dict] = []
    for part in _ANNOTATION_RE.split(line):
        if not part:
            continue
        if _ANNOTATION_RE.fullmatch(part):
            display_tokens.append(_annotated_token(part))
        else:
            display_tokens.extend(_plain_display_tokens(part))

    # Resolve every pronunciation first, then apply Japanese sokuon using the
    # next audible unit, even when that unit belongs to a different morpheme.
    flat: list[tuple[dict, str | None, str]] = []
    for token in display_tokens:
        explicit_units = token["explicit_units"]
        if explicit_units is not None:
            for unit in explicit_units:
                if unit:
                    flat.append((token, unit, ""))
            continue
        for raw in token["raw_units"]:
            ordinary = raw.rstrip("っッ")
            roman = _kana_to_latin(ordinary)
            flat.append((token, None if raw[-1:] in _SOKUON else roman, roman))

    next_initial = ""
    resolved: list[tuple[dict, str]] = []
    for token, direct, base in reversed(flat):
        if direct is None:
            consonant = "t" if next_initial == "c" else next_initial
            roman = base + (consonant or "h")
        else:
            roman = direct
        roman = _normalize_latin(roman)
        if roman:
            next_initial = roman[0]
            resolved.append((token, roman))
    resolved.reverse()

    for token in display_tokens:
        token["units"] = []
    for token, roman in resolved:
        token["units"].append(roman)

    return _display_text(line), display_tokens


def _timed_display_characters(token: dict) -> list[dict]:
    """Project aligned pronunciation-unit spans onto display characters.

    MMS aligns Latin pronunciation units, while the editor's Japanese timing
    contract is one entry per displayed character. A morpheme can have a
    different number of pronunciation units and glyphs, so contiguous units
    are assigned proportionally. When there are fewer units than glyphs, the
    acoustic span is subdivided rather than duplicating one coarse timestamp.
    """
    surface = str(token.get("surface", ""))
    characters = [char for char in surface if not char.isspace()]
    timings = token.get("timings", [])
    units = token.get("units", [])
    if not characters:
        return []
    if not timings:
        return [{"word": surface, "_punct": True}]

    entries: list[dict] = []
    character_count = len(characters)
    timing_count = len(timings)
    total_start = float(timings[0][0])
    total_end = max(total_start, float(timings[-1][1]))

    for index, character in enumerate(characters):
        if timing_count >= character_count:
            first = index * timing_count // character_count
            last = max(first + 1, (index + 1) * timing_count // character_count)
            selected = timings[first:last]
            start = float(selected[0][0])
            end = max(start, float(selected[-1][1]))
            score = sum(float(value[2]) for value in selected) / len(selected)
            reading = "".join(units[first:last])
        else:
            start = total_start + (total_end - total_start) * index / character_count
            end = total_start + (total_end - total_start) * (index + 1) / character_count
            nearest = min(timing_count - 1, index * timing_count // character_count)
            score = float(timings[nearest][2])
            reading = units[nearest] if nearest < len(units) else ""

        entry = {
            "word": character,
            "start": start,
            "end": max(start, end),
            "score": score,
        }
        if reading:
            entry["reading"] = reading
        entries.append(entry)
    return entries


def detect_activity_ranges(
    audio: np.ndarray,
    sample_rate: int = SAMPLE_RATE,
    frame_seconds: float = 1.0,
    percentile: float = 90.0,
    threshold_ratio: float = 0.1,
) -> list[tuple[float, float]]:
    """Detect active vocal ranges with the percentile RMS rule used by FA-Kara."""
    values = np.asarray(audio, dtype=np.float32).reshape(-1)
    if values.size == 0:
        return []
    frame = max(1, int(sample_rate * frame_seconds))
    hop = max(1, frame // 2)
    starts = list(range(0, values.size, hop))
    rms = np.asarray(
        [
            float(np.sqrt(np.mean(values[start:min(start + frame, values.size)] ** 2)))
            for start in starts
        ],
        dtype=np.float64,
    )
    if rms.size == 0 or not np.isfinite(rms).any():
        return []
    threshold = float(np.percentile(rms, percentile)) * threshold_ratio
    active = rms > threshold
    if not active.any():
        return []

    duration = values.size / sample_rate
    padding = frame_seconds / 4.0
    ranges: list[tuple[float, float]] = []
    range_start: float | None = None
    for index, enabled in enumerate(active):
        time = starts[index] / sample_rate
        if enabled and range_start is None:
            range_start = max(0.0, time - padding)
        elif not enabled and range_start is not None:
            ranges.append((range_start, min(duration, time + padding)))
            range_start = None
    if range_start is not None:
        ranges.append((range_start, duration))
    return [(start, end) for start, end in ranges if end > start]


def _clip_ranges(
    ranges: list[tuple[float, float]], start: float, end: float
) -> list[tuple[float, float]]:
    clipped = [(max(start, left), min(end, right)) for left, right in ranges]
    return [(left, right) for left, right in clipped if right > left]


def compress_audio(
    audio: np.ndarray,
    ranges: list[tuple[float, float]],
    sample_rate: int = SAMPLE_RATE,
) -> tuple[np.ndarray, list[TimeMap]]:
    """Concatenate authorized ranges and retain an exact piecewise time map."""
    values = np.asarray(audio, dtype=np.float32).reshape(-1)
    pieces: list[np.ndarray] = []
    mapping: list[TimeMap] = []
    cursor = 0.0
    for original_start, original_end in ranges:
        first = max(0, int(round(original_start * sample_rate)))
        last = min(values.size, int(round(original_end * sample_rate)))
        if last <= first:
            continue
        piece = values[first:last]
        duration = piece.size / sample_rate
        pieces.append(piece)
        mapping.append(
            TimeMap(cursor, cursor + duration, first / sample_rate, last / sample_rate)
        )
        cursor += duration
    if not pieces:
        return np.asarray([], dtype=np.float32), []
    return np.ascontiguousarray(np.concatenate(pieces), dtype=np.float32), mapping


def map_compressed_time(value: float, mapping: list[TimeMap]) -> float:
    if not mapping:
        return max(0.0, value)
    for item in mapping:
        if value <= item.compressed_end:
            offset = min(
                max(value - item.compressed_start, 0.0),
                item.original_end - item.original_start,
            )
            return item.original_start + offset
    return mapping[-1].original_end


def _load_and_align(
    prepared_lines: list[tuple[str, list[dict]]],
    compressed_audio: np.ndarray,
    mapping: list[TimeMap],
    device: str,
) -> list[dict]:
    from transformers import AutoModelForCTC, AutoProcessor
    from ctc_align import _forced_align_segment

    with gpu_model(f"mms-karaoke:{device}") as held:
        processor = AutoProcessor.from_pretrained(MODEL_ID, local_files_only=True)
        model = AutoModelForCTC.from_pretrained(MODEL_ID, local_files_only=True)
        model = model.to(device)
        model.eval()
        held.append(model)

        encoded_units: list[tuple[int, int, list[int]]] = []
        flattened_ids: list[int] = []
        for line_index, (_text, tokens) in enumerate(prepared_lines):
            for token_index, token in enumerate(tokens):
                for unit in token["units"]:
                    ids = processor.tokenizer.encode(unit, add_special_tokens=False)
                    ids = [int(value) for value in ids]
                    if not ids:
                        continue
                    encoded_units.append((line_index, token_index, ids))
                    flattened_ids.extend(ids)

        if not flattened_ids:
            raise MmsKaraokeUnsupportedError(
                "lyrics contain no alignable Latin pronunciation units"
            )

        inputs = processor(
            audio=compressed_audio,
            sampling_rate=SAMPLE_RATE,
            return_tensors="pt",
        )
        inputs = {key: value.to(device) for key, value in inputs.items()}
        with torch.inference_mode():
            logits = model(**inputs).logits
            emission = torch.log_softmax(logits[0], dim=-1).detach().float().contiguous()

        blank = int(model.config.pad_token_id)
        char_spans = _forced_align_segment(emission, flattened_ids, blank)
        if char_spans is None or len(char_spans) != len(flattened_ids):
            raise RuntimeError("MMS Karaoke CTC could not align every pronunciation character")

        seconds_per_frame = compressed_audio.size / SAMPLE_RATE / emission.shape[0]
        cursor = 0
        for line_index, token_index, ids in encoded_units:
            spans = char_spans[cursor:cursor + len(ids)]
            cursor += len(ids)
            if not spans:
                continue
            start = map_compressed_time(spans[0]["start"] * seconds_per_frame, mapping)
            end = map_compressed_time(spans[-1]["end"] * seconds_per_frame, mapping)
            score = sum(float(span["score"]) for span in spans) / len(spans)
            token = prepared_lines[line_index][1][token_index]
            token.setdefault("timings", []).append((start, max(start, end), score))

    segments: list[dict] = []
    for text, tokens in prepared_lines:
        entries: list[dict] = []
        for token in tokens:
            entries.extend(_timed_display_characters(token))
        words = cjk.merge_punct(entries)
        words = [
            word
            for word in words
            if word.get("start") is not None and word.get("end") is not None
        ]
        if not words:
            continue
        for word in words:
            word["start"] = round(float(word["start"]), 3)
            word["end"] = round(max(float(word["start"]), float(word["end"])), 3)
            if "score" in word:
                word["score"] = round(float(word["score"]), 3)
        segments.append(
            {
                "text": text,
                "start": words[0]["start"],
                "end": words[-1]["end"],
                "words": words,
            }
        )
    return segments


def _restore_phrase_edges(
    segments: list[dict],
    coarse_ranges: list[tuple[float, float]],
    fine_ranges: list[tuple[float, float]],
    duration: float,
) -> None:
    """Restore clipped consonant attacks and sustained line-final vowels."""
    previous_end = 0.0
    for index, segment in enumerate(segments):
        words = segment["words"]
        first = words[0]
        last = words[-1]

        head_range = next(
            (
                (start, end)
                for start, end in coarse_ranges
                if start <= float(first["start"]) <= end
                or start <= float(first["end"]) <= end
            ),
            None,
        )
        if head_range is not None:
            candidate = max(previous_end, head_range[0])
            # A one-second RMS window includes useful consonant attack but can
            # also cover an earlier phrase. Keep this correction local.
            if 0.0 < float(first["start"]) - candidate <= 1.25:
                first["start"] = round(candidate, 3)
                segment["start"] = first["start"]

        next_start = (
            float(segments[index + 1]["start"]) - 0.02
            if index + 1 < len(segments)
            else duration
        )
        current_end = float(last["end"])
        extension = current_end
        for active_start, active_end in fine_ranges:
            if active_start <= current_end <= active_end:
                extension = min(active_end, next_start)
                break
            if current_end < active_start <= current_end + 0.08:
                extension = min(active_end, next_start)
                break
        if extension > current_end:
            last["end"] = round(extension, 3)
            segment["end"] = last["end"]
        previous_end = float(segment["end"])


def align_lyrics(
    lines: list[str],
    audio,
    language: str,
    vocal_start: float,
    vocal_end: float,
    device: str,
    pre_align_cleanup=None,
) -> list[dict]:
    if not is_supported(language):
        raise MmsKaraokeUnsupportedError(
            f"language '{language}' is not supported; MMS Karaoke currently supports Japanese"
        )

    values = np.ascontiguousarray(np.asarray(audio), dtype=np.float32).reshape(-1)
    duration = values.size / SAMPLE_RATE
    coarse_ranges = _clip_ranges(
        detect_activity_ranges(values, frame_seconds=1.0), vocal_start, vocal_end
    )
    if not coarse_ranges:
        coarse_ranges = [(max(0.0, vocal_start), min(duration, vocal_end))]
    compressed, mapping = compress_audio(values, coarse_ranges)
    if compressed.size < 400:
        raise MmsKaraokeUnsupportedError("the detected vocal region is too short to align")

    prepared_lines = [prepare_line(line) for line in lines]
    print(
        f"[uta-studio:LOG] MMS Karaoke input: {len(prepared_lines)} lines, "
        f"{sum(len(token['units']) for _, tokens in prepared_lines for token in tokens)} "
        f"pronunciation units, {compressed.size / SAMPLE_RATE:.1f}s active vocals",
        flush=True,
    )

    try:
        segments = _load_and_align(prepared_lines, compressed, mapping, device)
    except Exception as error:
        if device == "cpu" or not is_oom(error):
            raise
        print(
            f"[uta-studio:LOG] MMS Karaoke alignment OOM on {device}, retrying on CPU",
            flush=True,
        )
        if pre_align_cleanup:
            try:
                pre_align_cleanup()
            except Exception:
                pass
        segments = _load_and_align(prepared_lines, compressed, mapping, "cpu")

    fine_ranges = _clip_ranges(
        detect_activity_ranges(values, frame_seconds=0.02), vocal_start, vocal_end
    )
    _restore_phrase_edges(segments, coarse_ranges, fine_ranges, duration)
    print(
        f"[uta-studio:LOG] MMS Karaoke alignment: {len(segments)} lines, "
        f"{sum(len(segment['words']) for segment in segments)} display tokens",
        flush=True,
    )
    return segments
