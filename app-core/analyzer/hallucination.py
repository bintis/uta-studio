"""Hallucination detection and filtering for WhisperX transcripts."""

import re

BANNED_WORDS = {
    "dimatorzok", "dimatorsok", "dima_torzok",
    "amara.org",
}

HALLUCINATION_PHRASES = [
    "продолжение следует",
    "подписывайтесь на канал",
    "редактор субтитров",
    "корректор субтитров",
    "субтитры подогнал",
    "субтитры подогнала",
    "субтитры сделал",
    "субтитры сделала",
    "thanks for watching",
    "please subscribe",
    "like and subscribe",
    "satsang with mooji",
    "song lyrics",
    "all rights reserved",
    "subtitles by",
    "captions by",
]

ATTRIBUTION_WORDS = {
    "субтитры", "субтитр", "подписи", "титры",
    "сделал", "сделала", "сделали",
    "создал", "создала", "создали",
    "делал", "делала", "делали",
    "создавал", "создавала", "создавали",
    "подготовил", "подготовила", "подготовили",
    "редактировал", "редактировала", "редактировали",
    "выполнил", "выполнила", "выполнили",
    "подогнал", "подогнала", "подогнали",
    "перевёл", "перевела", "перевели",
    "редактор", "корректор", "переводчик",
    "subtitles", "subtitle", "captions", "caption",
    "transcribed", "transcript", "transcription",
    "editor", "translator", "proofreader",
}


def is_hallucination(seg: dict) -> bool:
    """Check if a transcribed segment is a known hallucination or attribution."""
    text = seg.get("text", "").strip()
    if not text:
        return True

    text_lower = text.lower()
    for phrase in HALLUCINATION_PHRASES:
        if phrase in text_lower:
            return True

    words = text.split()
    if not words:
        return True

    attr_count = 0
    for w in words:
        clean = re.sub(r"[.,!?;:\"\']", "", w).lower()
        if clean in BANNED_WORDS or clean in ATTRIBUTION_WORDS:
            attr_count += 1

    if attr_count == len(words):
        return True
    if len(words) >= 3 and attr_count / len(words) >= 0.5:
        return True

    return False


def remove_hallucinated_words(all_words: list[dict]) -> list[dict]:
    """Remove individual hallucinated/attribution words and their neighbors."""
    if not all_words:
        return all_words

    to_remove: set[int] = set()

    for i, w in enumerate(all_words):
        clean = re.sub(r"[.,!?;:\"\']", "", w["word"]).lower()
        if clean in BANNED_WORDS:
            to_remove.add(i)
            for j in range(max(0, i - 4), i):
                neighbor = re.sub(r"[.,!?;:\"\']", "", all_words[j]["word"]).lower()
                if neighbor in ATTRIBUTION_WORDS:
                    to_remove.add(j)
            for j in range(i + 1, min(len(all_words), i + 3)):
                neighbor = re.sub(r"[.,!?;:\"\']", "", all_words[j]["word"]).lower()
                if neighbor in ATTRIBUTION_WORDS:
                    to_remove.add(j)

    if to_remove:
        removed_text = " ".join(all_words[i]["word"] for i in sorted(to_remove))
        print(f"[uta-studio:LOG] Removed hallucination ({len(to_remove)} words): {removed_text}", flush=True)

    return [w for i, w in enumerate(all_words) if i not in to_remove]
