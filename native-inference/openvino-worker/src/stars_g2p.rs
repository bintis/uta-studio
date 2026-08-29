use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::path::Path;

use serde::Deserialize;
pub const PROFILE: &str = "stars-chinese-g2p-pypinyin-0.55.0-v1";
pub const ASSET_SHA256: &str = "433fcd2a7379cb9554a7a0dfe254746c3c7ee70bfd5de4fa18c1462757b888a5";
pub const SOURCE_REVISION: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
#[cfg(test)]
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratorIdentity {
    pypinyin: String,
    jieba: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAsset {
    schema_version: u32,
    profile: String,
    source_revision: String,
    generator: GeneratorIdentity,
    #[serde(rename = "phone_set_sha256")]
    _phone_set_sha256: String,
    phone_set: Vec<String>,
    characters: BTreeMap<String, Vec<String>>,
    phrases: BTreeMap<String, Vec<Vec<String>>>,
    runtime: String,
}

type PhoneSequence = Vec<String>;
type PhrasePronunciation = (Vec<char>, Vec<PhoneSequence>);

#[derive(Debug, Clone)]
pub struct ChineseG2pAsset {
    phone_ids: BTreeMap<String, i64>,
    characters: BTreeMap<char, Vec<String>>,
    phrases: BTreeMap<char, Vec<PhrasePronunciation>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonemeInput {
    pub phone_ids: Vec<i64>,
    /// Zero-based TimedTranscript word index for every phone.
    pub phone_to_word: Vec<i64>,
}

impl ChineseG2pAsset {
    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| format!("STARS Chinese G2P asset is unavailable: {error}"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_ASSET_BYTES
        {
            return Err("STARS Chinese G2P asset size is invalid".to_string());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("could not read STARS G2P asset: {error}"))?;
        Self::from_bytes(&bytes)
    }

    /// Load the packaged immutable asset without a filesystem or script runtime.
    pub fn load_embedded() -> Result<Self, String> {
        Self::from_bytes(include_bytes!("../assets/stars-chinese-g2p-v1.json"))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let raw: RawAsset = serde_json::from_slice(bytes)
            .map_err(|error| format!("STARS Chinese G2P asset is invalid: {error}"))?;
        if raw.schema_version != 1
            || raw.profile != PROFILE
            || raw.source_revision != SOURCE_REVISION
            || raw.generator.pypinyin != "0.55.0"
            || raw.generator.jieba != "0.42.1"
            || raw.runtime != "native_json_asset_only"
            || raw.phone_set.len() != 59
            || raw.phone_set.first().map(String::as_str) != Some("<SP>")
            || raw.phone_set.get(1).map(String::as_str) != Some("<AP>")
        {
            return Err("STARS Chinese G2P generation identity is invalid".to_string());
        }
        let mut phone_ids = BTreeMap::new();
        for (index, phone) in raw.phone_set.iter().enumerate() {
            if phone.trim().is_empty()
                || phone_ids.insert(phone.clone(), index as i64 + 1).is_some()
            {
                return Err("STARS Chinese phone set is invalid".to_string());
            }
        }
        let allowed = phone_ids
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut characters = BTreeMap::new();
        for (text, phones) in raw.characters {
            let mut chars = text.chars();
            let character = chars
                .next()
                .filter(|_| chars.next().is_none())
                .ok_or_else(|| "STARS G2P character key is invalid".to_string())?;
            validate_phones(&phones, &allowed)?;
            characters.insert(character, phones);
        }
        let mut phrases: BTreeMap<char, Vec<PhrasePronunciation>> = BTreeMap::new();
        for (text, phones) in raw.phrases {
            let chars = text.chars().collect::<Vec<_>>();
            if chars.len() < 2 || chars.len() != phones.len() {
                return Err("STARS G2P phrase shape is invalid".to_string());
            }
            for row in &phones {
                validate_phones(row, &allowed)?;
            }
            phrases.entry(chars[0]).or_default().push((chars, phones));
        }
        for entries in phrases.values_mut() {
            entries
                .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));
        }
        if characters.is_empty() {
            return Err("STARS Chinese G2P asset has no character lexicon".to_string());
        }
        Ok(Self {
            phone_ids,
            characters,
            phrases,
        })
    }

    /// Convert TimedTranscript words to the exact STARS phone IDs without a
    /// script runtime. Unknown Chinese characters fail closed.
    pub fn phonemize_words(&self, words: &[String]) -> Result<PhonemeInput, String> {
        if words.is_empty() {
            return Err("STARS P0 requires TimedTranscript words".to_string());
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
                            "STARS Chinese G2P has no pinned reading for {}",
                            chars[index]
                        )
                    })?;
                    (1, vec![phones.clone()])
                };
                for row in rows {
                    for phone in row {
                        phone_ids.push(*self.phone_ids.get(&phone).ok_or_else(|| {
                            "STARS G2P emitted a phone outside its pinned set".to_string()
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

fn validate_phones(phones: &[String], allowed: &BTreeSet<&str>) -> Result<(), String> {
    if phones.is_empty() || phones.iter().any(|phone| !allowed.contains(phone.as_str())) {
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
    fn native_asset_handles_phrase_polyphony() {
        let asset = ChineseG2pAsset::load_embedded().unwrap();
        let result = asset
            .phonemize_words(&["你好".to_string(), "重庆".to_string()])
            .unwrap();
        // Phone-set IDs include the leading <Blank> reserved by PhoneEncoder.
        assert_eq!(result.phone_ids, [34, 20, 19, 7, 10, 36, 39, 27]);
        assert_eq!(result.phone_to_word, [0, 0, 0, 0, 1, 1, 1, 1]);
    }

    #[test]
    fn native_asset_covers_the_representative_chinese_transcript() {
        let asset = ChineseG2pAsset::load_embedded().unwrap();
        let text = "拱桥月下谁在弹唱思念远方牵挂那年仲夏你背上行囊离开家古道旁却我最一愿望究要挥意坠网眼摇有水微回妄忧原味用轮圆忆随褪谁流憶遠";
        let result = asset.phonemize_words(&[text.to_string()]).unwrap();
        assert!(!result.phone_ids.is_empty());
        assert_eq!(result.phone_ids.len(), result.phone_to_word.len());
        assert!(result.phone_to_word.iter().all(|word| *word == 0));
    }

    #[test]
    fn unknown_readings_fail_closed() {
        let asset = ChineseG2pAsset::load_embedded().unwrap();
        assert!(asset.phonemize_words(&["🙂".to_string()]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn native_asset_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/stars-chinese-g2p-v1.json");
        let link = std::env::temp_dir().join(format!(
            "uta-stars-g2p-link-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        symlink(source, &link).unwrap();
        assert!(ChineseG2pAsset::load(&link).is_err());
        std::fs::remove_file(link).unwrap();
    }
}
