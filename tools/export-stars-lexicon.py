#!/usr/bin/env python3
"""Export offline linguistic data; this is not a model/runtime dependency.

Use pypinyin's tone normalization before splitting initials and strict finals.
The checkpoint phone order comes from the declared asset, never from a sort of
observed phones. Unsupported source readings are reported, not invented.
"""

import argparse
import json
from pathlib import Path

import pypinyin
from pypinyin.contrib.tone_convert import to_finals, to_initials, to_normal
from pypinyin.phrases_dict import phrases_dict
from pypinyin.pinyin_dict import pinyin_dict


def syllable_phones(reading, allowed):
    normalized = to_normal(reading)
    phones = [phone for phone in (to_initials(normalized), to_finals(normalized)) if phone]
    return phones if phones and all(phone in allowed for phone in phones) else None


def export_lexicon(phone_set, character_source, phrase_source):
    allowed = set(phone_set)
    characters = {}
    phrases = {}
    unsupported_characters = {}
    unsupported_phrases = {}
    for codepoint, readings in sorted(character_source.items()):
        character = chr(codepoint)
        reading = readings.split(",")[0]
        phones = syllable_phones(reading, allowed)
        if phones:
            characters[character] = phones
        else:
            unsupported_characters[character] = reading
    for phrase, readings in sorted(phrase_source.items()):
        selected = [choices[0] for choices in readings]
        rows = [syllable_phones(reading, allowed) for reading in selected]
        if len(phrase) >= 2 and len(rows) == len(phrase) and all(rows):
            phrases[phrase] = rows
        else:
            unsupported_phrases[phrase] = selected
    asset = {
        "profile": "stars-chinese-g2p",
        "generator": {"pypinyin": pypinyin.__version__, "tool": "tools/export-stars-lexicon.py"},
        "phone_set": phone_set,
        "characters": characters,
        "phrases": phrases,
        "runtime": "native_json_asset_only",
    }
    report = {
        "source_characters": len(character_source), "exported_characters": len(characters),
        "source_phrases": len(phrase_source), "exported_phrases": len(phrases),
        "unsupported_characters": unsupported_characters,
        "unsupported_phrases": unsupported_phrases,
    }
    return asset, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phone-set-asset", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    phone_set = json.loads(args.phone_set_asset.read_text())["phone_set"]
    asset, report = export_lexicon(phone_set, pinyin_dict, phrases_dict)
    args.output.write_text(json.dumps(asset, ensure_ascii=False, separators=(",", ":")) + "\n")
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key.startswith(("source_", "exported_"))}))


if __name__ == "__main__":
    main()
