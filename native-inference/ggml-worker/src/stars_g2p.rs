use std::collections::BTreeMap;

use serde::Deserialize;
use uta_ggml_runtime::stars::PhonemeInput;

pub const PROFILE: &str = "stars-chinese-g2p-pypinyin-0.55.0-v1";

#[derive(Debug, Deserialize)]
struct RawAsset {
    phone_set: Vec<String>,
    characters: BTreeMap<String, Vec<String>>,
    phrases: BTreeMap<String, Vec<Vec<String>>>,
}

type PhoneSequence = Vec<String>;
type PhrasePronunciation = (Vec<char>, Vec<PhoneSequence>);

#[derive(Debug, Clone)]
pub struct ChineseG2pAsset {
    phone_ids: BTreeMap<String, i64>,
    characters: BTreeMap<char, Vec<String>>,
    phrases: BTreeMap<char, Vec<PhrasePronunciation>>,
}

impl ChineseG2pAsset {
    /// Parses the packaged immutable lexicon without a script runtime.
    pub fn load_embedded() -> Result<Self, String> {
        let raw: RawAsset =
            serde_json::from_slice(include_bytes!("../assets/stars-chinese-g2p-v1.json"))
                .map_err(|error| format!("STARS Chinese G2P asset is invalid: {error}"))?;
        if raw.phone_set.is_empty() || raw.characters.is_empty() {
            return Err("STARS Chinese G2P asset is empty".to_string());
        }
        let mut phone_ids = BTreeMap::new();
        for (index, phone) in raw.phone_set.into_iter().enumerate() {
            if phone.trim().is_empty() || phone_ids.insert(phone, index as i64 + 1).is_some() {
                return Err("STARS Chinese phone set is invalid".to_string());
            }
        }
        let mut characters = BTreeMap::new();
        for (text, phones) in raw.characters {
            let mut chars = text.chars();
            let character = chars
                .next()
                .filter(|_| chars.next().is_none())
                .ok_or_else(|| "STARS G2P character key is invalid".to_string())?;
            validate_phones(&phones, &phone_ids)?;
            characters.insert(character, phones);
        }
        let mut phrases: BTreeMap<char, Vec<PhrasePronunciation>> = BTreeMap::new();
        for (text, phones) in raw.phrases {
            let chars = text.chars().collect::<Vec<_>>();
            if chars.len() < 2 || chars.len() != phones.len() {
                return Err("STARS G2P phrase shape is invalid".to_string());
            }
            for row in &phones {
                validate_phones(row, &phone_ids)?;
            }
            phrases.entry(chars[0]).or_default().push((chars, phones));
        }
        for entries in phrases.values_mut() {
            entries
                .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));
        }
        Ok(Self {
            phone_ids,
            characters,
            phrases,
        })
    }

    pub fn phonemize_words(&self, words: &[String]) -> Result<PhonemeInput, String> {
        if words.is_empty() {
            return Err("STARS requires TimedTranscript words".to_string());
        }
        let mut phone_ids = Vec::new();
        let mut phone_to_word = Vec::new();
        for (word_index, word) in words.iter().enumerate() {
            let chars = word
                .chars()
                .filter(|character| !character.is_whitespace())
                .map(|character| if character == '嗯' { '蒽' } else { character })
                .collect::<Vec<_>>();
            if chars.is_empty() {
                return Err("TimedTranscript contains an empty STARS word".to_string());
            }
            let mut index = 0;
            let mut emitted = 0;
            while index < chars.len() {
                if is_punctuation(chars[index]) {
                    index += 1;
                    continue;
                }
                let phrase = self.phrases.get(&chars[index]).and_then(|entries| {
                    entries.iter().find(|(key, _)| {
                        index + key.len() <= chars.len()
                            && chars[index..index + key.len()] == key[..]
                    })
                });
                let (length, rows) = if let Some((key, rows)) = phrase {
                    (key.len(), rows.clone())
                } else {
                    let phones = self.characters.get(&chars[index]).ok_or_else(|| {
                        format!(
                            "STARS Chinese G2P has no packaged reading for {}",
                            chars[index]
                        )
                    })?;
                    (1, vec![phones.clone()])
                };
                for row in rows {
                    for phone in row {
                        phone_ids.push(*self.phone_ids.get(&phone).ok_or_else(|| {
                            "STARS G2P emitted a phone outside its phone set".to_string()
                        })?);
                        phone_to_word.push(word_index as i64);
                        emitted += 1;
                    }
                }
                index += length;
            }
            if emitted == 0 {
                return Err("TimedTranscript word has no STARS Chinese phones".to_string());
            }
        }
        Ok(PhonemeInput {
            phone_ids,
            phone_to_word,
        })
    }
}

fn validate_phones(phones: &[String], allowed: &BTreeMap<String, i64>) -> Result<(), String> {
    if phones.is_empty() || phones.iter().any(|phone| !allowed.contains_key(phone)) {
        Err("STARS G2P lexicon contains an unknown phone".to_string())
    } else {
        Ok(())
    }
}

fn is_punctuation(value: char) -> bool {
    matches!(
        value,
        '!' | ',' | '.' | '?' | ';' | ':' | '！' | '，' | '。' | '？' | '；' | '：'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_asset_handles_phrase_polyphony() {
        let asset = ChineseG2pAsset::load_embedded().unwrap();
        let result = asset
            .phonemize_words(&["你好".to_string(), "重庆".to_string()])
            .unwrap();
        assert_eq!(result.phone_ids, [34, 20, 19, 7, 10, 36, 39, 27]);
        assert_eq!(result.phone_to_word, [0, 0, 0, 0, 1, 1, 1, 1]);
    }

    #[test]
    fn unknown_reading_is_reported() {
        let asset = ChineseG2pAsset::load_embedded().unwrap();
        assert!(asset.phonemize_words(&["🙂".to_string()]).is_err());
    }
}
