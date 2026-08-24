#!/usr/bin/env python3
"""Build the versioned native Chinese G2P asset consumed by STARS P0.

Python/pypinyin is a conversion-time tool only. The packaged worker reads the
resulting JSON and never starts Python for G2P.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from importlib.metadata import version
from pathlib import Path
from typing import Any

PHONE_SET_SHA256 = "8767ab69222297499de3c109598fcfcabaf9585211a2ed4f5797dc944dca82a7"
PROFILE = "stars-chinese-g2p-pypinyin-0.55.0-v1"
SOURCE_REVISION = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n")
    with temporary.open("rb") as handle:
        os.fsync(handle.fileno())
    temporary.replace(path)


def phones(text: str) -> list[list[str]]:
    from pypinyin import Style, pinyin

    text = text.replace("嗯", "蒽")
    initials = [row[0] for row in pinyin(text, style=Style.INITIALS, strict=False)]
    finals = [row[0] for row in pinyin(text, style=Style.FINALS, strict=False)]
    result = []
    for initial, final in zip(initials, finals):
        if initial == final:
            value = [initial] if initial else []
        else:
            value = [item for item in (initial, final) if item]
        result.append(value)
    return result


def build(arguments: argparse.Namespace) -> None:
    import pypinyin.constants as constants

    if version("pypinyin") != "0.55.0":
        raise SystemExit("STARS G2P asset requires pypinyin 0.55.0")
    if version("jieba") != "0.42.1":
        raise SystemExit("STARS G2P provenance requires jieba 0.42.1")
    if sha256(arguments.phone_set) != PHONE_SET_SHA256:
        raise SystemExit("STARS Chinese phone-set identity mismatch")
    phone_set = json.loads(arguments.phone_set.read_text())
    allowed = set(phone_set)
    characters: dict[str, list[str]] = {}
    phrases: dict[str, list[list[str]]] = {}
    character_keys = [chr(codepoint) for codepoint in sorted(constants.PINYIN_DICT)]
    phrase_keys = sorted(constants.PHRASES_DICT)
    for character in character_keys:
        value = phones(character)[0]
        if value and all(item in allowed for item in value):
            characters[character] = value
    for phrase in phrase_keys:
        if len(phrase) < 2 or any(not ("\u4e00" <= item <= "\u9fff") for item in phrase):
            continue
        value = phones(phrase)
        if len(value) == len(phrase) and all(row and all(item in allowed for item in row) for row in value):
            phrases[phrase] = value
    output = {
        "schema_version": 1,
        "profile": PROFILE,
        "source_revision": SOURCE_REVISION,
        "generator": {"pypinyin": "0.55.0", "jieba": "0.42.1"},
        "phone_set_sha256": PHONE_SET_SHA256,
        "phone_set": phone_set,
        "characters": characters,
        "phrases": phrases,
        "runtime": "native_json_asset_only",
    }
    atomic_json(arguments.output, output)
    print(
        json.dumps(
            {
                "output": str(arguments.output),
                "sha256": sha256(arguments.output),
                "characters": len(characters),
                "phrases": len(phrases),
                "profile": PROFILE,
            },
            sort_keys=True,
        )
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    result.add_argument("--phone-set", type=Path, required=True)
    result.add_argument("--output", type=Path, required=True)
    return result


if __name__ == "__main__":
    build(parser().parse_args())
