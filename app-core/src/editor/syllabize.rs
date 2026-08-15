//! Splitting a word into the syllables a singer actually hits.
//!
//! A karaoke chart wants one note per syllable, but transcription hands back
//! whole words. Splitting them by hand is the slowest part of authoring, so
//! this does the mechanical part per language and leaves the judgement calls
//! to the user, who can still merge or re-split afterwards.
//!
//! These are heuristics, deliberately. Japanese kana and Han characters split
//! exactly, because their writing systems are syllabic; the Latin rule is the
//! usual vowel-group approximation and will be wrong on loanwords and names.

/// One piece of a split word: what to show, and how it is pronounced when that
/// differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    pub text: String,
    /// Kana or other pronunciation for this piece, when it is known.
    pub reading: Option<String>,
}

impl Syllable {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            reading: None,
        }
    }
}

/// Small kana and marks that belong to the mora before them rather than
/// starting one of their own.
const TRAILING_KANA: &[char] = &[
    'ゃ', 'ゅ', 'ょ', 'ぁ', 'ぃ', 'ぅ', 'ぇ', 'ぉ', 'ゎ', 'っ', 'ャ', 'ュ', 'ョ', 'ァ', 'ィ', 'ゥ',
    'ェ', 'ォ', 'ヮ', 'ッ', 'ー', 'ヵ', 'ヶ', '゛', '゜',
];

fn is_kana(character: char) -> bool {
    matches!(character, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9D}')
}

fn is_han(character: char) -> bool {
    matches!(character, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

fn is_hangul(character: char) -> bool {
    matches!(character, '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}')
}

/// Splits kana into morae. A small kana, a long-vowel mark, or a sokuon joins
/// the mora before it; `ん` stands on its own, as it does when sung.
pub fn kana_morae(text: &str) -> Vec<String> {
    let mut morae: Vec<String> = Vec::new();
    for character in text.chars() {
        if TRAILING_KANA.contains(&character)
            && let Some(last) = morae.last_mut()
        {
            last.push(character);
        } else {
            morae.push(character.to_string());
        }
    }
    morae
}

/// Splits a word into singable syllables. Returns one entry when the word is
/// already a syllable, or when the language's rules cannot split it safely.
///
/// `reading` is the pronunciation the aligner recovered, if any. For Japanese
/// it lets a kanji word split by its kana rather than by its characters.
pub fn syllables(text: &str, reading: Option<&str>, language: Option<&str>) -> Vec<Syllable> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let language = language.unwrap_or("").to_ascii_lowercase();
    if language.starts_with("ja") {
        return japanese_syllables(trimmed, reading);
    }
    if language.starts_with("zh") || language.starts_with("ko") {
        return character_syllables(trimmed);
    }
    // A mixed-script word splits by script even when the chart language is
    // Latin, because a Han or kana run is unambiguously one syllable each.
    if trimmed
        .chars()
        .any(|character| is_han(character) || is_kana(character) || is_hangul(character))
    {
        return japanese_syllables(trimmed, reading);
    }
    latin_syllables(trimmed)
}

fn japanese_syllables(text: &str, reading: Option<&str>) -> Vec<Syllable> {
    let characters = text.chars().collect::<Vec<_>>();
    // An all-kana word is its own reading, so the morae split it exactly.
    if characters.iter().copied().all(is_kana) {
        return kana_morae(text)
            .into_iter()
            .map(|mora| Syllable {
                reading: Some(mora.clone()),
                text: mora,
            })
            .collect();
    }
    let morae = reading.map(kana_morae).unwrap_or_default();
    if !morae.is_empty() && characters.len() == 1 {
        // A single kanji sung over several morae stays one syllable: splitting
        // the display text would invent characters that are not in the word.
        return vec![Syllable {
            text: text.to_string(),
            reading: Some(morae.concat()),
        }];
    }
    // Mixed kana and kanji: one syllable per character, sharing out the morae
    // when there are exactly as many as there are characters.
    let aligned = morae.len() == characters.len();
    characters
        .into_iter()
        .enumerate()
        .map(|(index, character)| Syllable {
            text: character.to_string(),
            reading: aligned.then(|| morae[index].clone()),
        })
        .collect()
}

fn character_syllables(text: &str) -> Vec<Syllable> {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| Syllable::plain(character.to_string()))
        .collect()
}

fn is_vowel(character: char) -> bool {
    matches!(
        character.to_ascii_lowercase(),
        'a' | 'e'
            | 'i'
            | 'o'
            | 'u'
            | 'y'
            | 'à'
            | 'á'
            | 'â'
            | 'ä'
            | 'è'
            | 'é'
            | 'ê'
            | 'ë'
            | 'í'
            | 'î'
            | 'ï'
            | 'ó'
            | 'ô'
            | 'ö'
            | 'ú'
            | 'û'
            | 'ü'
    )
}

/// The usual vowel-group approximation: a syllable boundary falls between two
/// vowel groups, after the first of two consonants and before a lone one.
fn latin_syllables(text: &str) -> Vec<Syllable> {
    let characters = text.chars().collect::<Vec<_>>();
    if characters.len() < 4 {
        return vec![Syllable::plain(text)];
    }
    // Positions where a vowel group starts.
    let mut groups = Vec::new();
    let mut previous_vowel = false;
    for (index, character) in characters.iter().enumerate() {
        let vowel = is_vowel(*character);
        if vowel && !previous_vowel {
            groups.push(index);
        }
        previous_vowel = vowel;
    }
    // A silent final `e` is not a syllable of its own.
    if groups.len() > 1
        && characters
            .last()
            .is_some_and(|last| last.eq_ignore_ascii_case(&'e'))
        && groups.last() == Some(&(characters.len() - 1))
    {
        groups.pop();
    }
    if groups.len() < 2 {
        return vec![Syllable::plain(text)];
    }

    let mut cuts = Vec::new();
    for pair in groups.windows(2) {
        // Consonants between the end of one vowel group and the next.
        let mut end = pair[0];
        while end < characters.len() && is_vowel(characters[end]) {
            end += 1;
        }
        let consonants = pair[1].saturating_sub(end);
        let cut = match consonants {
            0 => pair[1],
            1 => pair[1] - 1,
            _ => end + 1,
        };
        if cut > 0 && cut < characters.len() {
            cuts.push(cut);
        }
    }
    cuts.dedup();
    if cuts.is_empty() {
        return vec![Syllable::plain(text)];
    }

    let mut pieces = Vec::new();
    let mut start = 0;
    for cut in cuts.into_iter().chain(std::iter::once(characters.len())) {
        if cut <= start {
            continue;
        }
        pieces.push(characters[start..cut].iter().collect::<String>());
        start = cut;
    }
    // Never leave a piece with no vowel to sing: fold it into its neighbour.
    let mut merged: Vec<String> = Vec::new();
    for piece in pieces {
        if !piece.chars().any(is_vowel)
            && let Some(last) = merged.last_mut()
        {
            last.push_str(&piece);
        } else {
            merged.push(piece);
        }
    }
    merged.into_iter().map(Syllable::plain).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(syllables: &[Syllable]) -> Vec<&str> {
        syllables
            .iter()
            .map(|syllable| syllable.text.as_str())
            .collect()
    }

    #[test]
    fn kana_keeps_small_kana_and_long_marks_with_their_mora() {
        assert_eq!(kana_morae("きょうは"), ["きょ", "う", "は"]);
        assert_eq!(kana_morae("ラーメン"), ["ラー", "メ", "ン"]);
        // Sokuon rides on the mora before it, matching the split the MMS
        // karaoke aligner produces, so a reading and its notes line up.
        assert_eq!(kana_morae("がっこう"), ["がっ", "こ", "う"]);
    }

    #[test]
    fn an_all_kana_word_splits_into_morae_that_are_their_own_reading() {
        let split = syllables("きょうは", None, Some("ja"));
        assert_eq!(texts(&split), ["きょ", "う", "は"]);
        assert_eq!(split[0].reading.as_deref(), Some("きょ"));
    }

    #[test]
    fn a_single_kanji_stays_whole_and_keeps_its_full_reading() {
        let split = syllables("空", Some("そら"), Some("ja"));
        assert_eq!(texts(&split), ["空"]);
        assert_eq!(split[0].reading.as_deref(), Some("そら"));
    }

    #[test]
    fn mixed_kanji_and_kana_split_per_character_and_share_the_reading() {
        let split = syllables("見る", Some("みる"), Some("ja"));
        assert_eq!(texts(&split), ["見", "る"]);
        assert_eq!(split[0].reading.as_deref(), Some("み"));
        assert_eq!(split[1].reading.as_deref(), Some("る"));
    }

    #[test]
    fn han_and_hangul_split_one_syllable_per_character() {
        assert_eq!(
            texts(&syllables("我爱你", None, Some("zh"))),
            ["我", "爱", "你"]
        );
        assert_eq!(
            texts(&syllables("사랑해", None, Some("ko"))),
            ["사", "랑", "해"]
        );
    }

    #[test]
    fn latin_words_split_between_vowel_groups() {
        assert_eq!(
            texts(&syllables("wonder", None, Some("en"))),
            ["won", "der"]
        );
        assert_eq!(texts(&syllables("later", None, Some("en"))), ["la", "ter"]);
        assert_eq!(
            texts(&syllables("remember", None, Some("en"))),
            ["re", "mem", "ber"]
        );
    }

    #[test]
    fn a_word_with_one_syllable_is_left_alone() {
        assert_eq!(texts(&syllables("love", None, Some("en"))), ["love"]);
        assert_eq!(texts(&syllables("sky", None, Some("en"))), ["sky"]);
        assert_eq!(texts(&syllables("a", None, Some("en"))), ["a"]);
    }

    #[test]
    fn every_piece_of_a_split_can_be_sung() {
        for word in ["strength", "rhythm", "beautiful", "morning", "yesterday"] {
            let split = syllables(word, None, Some("en"));
            assert_eq!(
                split
                    .iter()
                    .map(|syllable| syllable.text.as_str())
                    .collect::<String>(),
                word,
                "{word} lost characters"
            );
            assert!(
                split.iter().all(|syllable| !syllable.text.is_empty()),
                "{word} produced an empty piece"
            );
        }
    }

    #[test]
    fn a_mixed_script_word_splits_by_script_whatever_the_chart_language_says() {
        assert_eq!(texts(&syllables("愛", None, Some("en"))), ["愛"]);
        assert_eq!(texts(&syllables("あい", None, None)), ["あ", "い"]);
    }
}
