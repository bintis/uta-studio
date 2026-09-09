//! GPT-2 byte-level BPE with the official Qwen2 pre-tokenizer.
//! Ranked pairs use token ids to avoid repeated merge-string allocation.

use regex::Regex;
use std::collections::{HashMap, HashSet};

use super::model::MetadataReader;

pub struct Tokenizer {
    ids: HashMap<String, u32>,
    decoded: Vec<Vec<u8>>,
    byte_ids: [u32; 256],
    merges: HashMap<(u32, u32), (usize, u32)>,
    special: Vec<(String, u32)>,
    special_ids: HashSet<u32>,
    split: Regex,
}

impl Tokenizer {
    pub(crate) fn from_gguf(metadata: &MetadataReader<'_>) -> Result<Self, String> {
        let tokens = metadata.string_array("tokenizer.ggml.tokens")?;
        let merges = metadata.string_array("tokenizer.ggml.merges")?;
        let types = metadata.i32_array("tokenizer.ggml.token_type")?;
        if types.len() != tokens.len() {
            return Err("Qwen tokenizer token/type counts differ".to_string());
        }
        let of_type = |kind| {
            types
                .iter()
                .enumerate()
                .filter_map(|(id, &value)| (value == kind).then_some(id as u32))
                .collect::<Vec<_>>()
        };
        Self::from_parts(&tokens, &merges, &of_type(3), &of_type(4))
    }

    fn from_parts(
        tokens: &[String],
        merges: &[String],
        special: &[u32],
        user_defined: &[u32],
    ) -> Result<Self, String> {
        // Current converters retain these semantic markers as NORMAL=1,
        // losing Hugging Face's added-token classification. Recover only the
        // known literals, not arbitrary normal tokens or padding.
        let mut added = user_defined.to_vec();
        for marker in ["<asr_text>", "<timestamp>"] {
            if let Some(id) = tokens.iter().position(|token| token == marker)
                && !added.contains(&(id as u32))
            {
                added.push(id as u32);
            }
        }
        let user_defined = added.as_slice();
        let byte_map = byte_encoder();
        let inverse: HashMap<char, u8> = byte_map
            .iter()
            .enumerate()
            .map(|(byte, &character)| (character, byte as u8))
            .collect();
        let ids: HashMap<String, u32> = tokens
            .iter()
            .enumerate()
            .map(|(id, token)| (token.clone(), id as u32))
            .collect();
        let special_ids: HashSet<_> = special.iter().copied().collect();
        let added_ids: HashSet<_> = special.iter().chain(user_defined).copied().collect();
        let mut decoded = Vec::with_capacity(tokens.len());
        for (id, token) in tokens.iter().enumerate() {
            decoded.push(if added_ids.contains(&(id as u32)) {
                token.as_bytes().to_vec()
            } else {
                token
                    .chars()
                    .map(|character| {
                        inverse.get(&character).copied().ok_or_else(|| {
                            format!(
                                "Qwen token {id} contains a non-byte-map character {character:?}"
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            });
        }
        let mut byte_ids = [0; 256];
        for (byte, &character) in byte_map.iter().enumerate() {
            byte_ids[byte] = *ids
                .get(&character.to_string())
                .ok_or_else(|| format!("Qwen byte vocabulary is missing byte {byte}"))?;
        }
        let mut ranked = HashMap::with_capacity(merges.len());
        for (rank, merge) in merges.iter().enumerate() {
            let (first, second) = merge
                .split_once(' ')
                .ok_or_else(|| format!("Malformed Qwen BPE pair at rank {rank}"))?;
            let id = |text: &str| {
                ids.get(text)
                    .copied()
                    .ok_or_else(|| format!("Qwen BPE rank {rank}: absent token {text:?}"))
            };
            ranked.insert(
                (id(first)?, id(second)?),
                (rank, id(&format!("{first}{second}"))?),
            );
        }
        let special = special
            .iter()
            .chain(user_defined)
            .map(|&id| {
                tokens
                    .get(id as usize)
                    .map(|token| (token.clone(), id))
                    .ok_or_else(|| format!("Qwen special token {id} is outside vocabulary"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Rust regex lacks look-around. The final two whitespace alternatives
        // are implemented explicitly by `pretokenize` with equivalent backtracking.
        let split = Regex::new(
            r"^(?:(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+)",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            ids,
            decoded,
            byte_ids,
            merges: ranked,
            special,
            special_ids,
            split,
        })
    }

    pub fn id(&self, text: &str) -> Result<u32, String> {
        self.ids
            .get(text)
            .copied()
            .ok_or_else(|| format!("Qwen vocabulary has no token {text:?}"))
    }

    pub fn decode(&self, ids: &[u32], skip_special: bool) -> Result<String, String> {
        let mut bytes = Vec::new();
        for &id in ids {
            if skip_special && self.special_ids.contains(&id) {
                continue;
            }
            bytes.extend_from_slice(
                self.decoded
                    .get(id as usize)
                    .ok_or_else(|| format!("Qwen token id {id} is outside vocabulary"))?,
            );
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn encode(&self, mut text: &str) -> Result<Vec<u32>, String> {
        let mut output = Vec::new();
        while !text.is_empty() {
            let next = self
                .special
                .iter()
                .filter(|(special, _)| !special.is_empty())
                .filter_map(|(special, id)| text.find(special).map(|at| (at, special, *id)))
                .min_by(|first, second| {
                    first
                        .0
                        .cmp(&second.0)
                        .then_with(|| second.1.len().cmp(&first.1.len()))
                });
            if let Some((at, special, id)) = next {
                self.encode_ordinary(&text[..at], &mut output)?;
                output.push(id);
                text = &text[at + special.len()..];
            } else {
                self.encode_ordinary(text, &mut output)?;
                break;
            }
        }
        Ok(output)
    }

    fn pretokenize<'a>(&self, mut text: &'a str) -> Result<Vec<&'a str>, String> {
        let mut parts = Vec::new();
        while !text.is_empty() {
            let end = if let Some(found) = self.split.find(text) {
                found.end()
            } else {
                let whitespace: Vec<_> = text
                    .char_indices()
                    .take_while(|(_, character)| character.is_whitespace())
                    .collect();
                let &(last_at, last) = whitespace
                    .last()
                    .ok_or("Qwen pre-tokenizer did not consume input")?;
                let end = last_at + last.len_utf8();
                if end < text.len() && whitespace.len() > 1 {
                    last_at
                } else {
                    end
                }
            };
            parts.push(&text[..end]);
            text = &text[end..];
        }
        Ok(parts)
    }

    fn encode_ordinary(&self, text: &str, output: &mut Vec<u32>) -> Result<(), String> {
        for part in self.pretokenize(text)? {
            let mut pieces: Vec<u32> = part
                .bytes()
                .map(|byte| self.byte_ids[byte as usize])
                .collect();
            loop {
                let candidate = pieces
                    .windows(2)
                    .enumerate()
                    .filter_map(|(index, pair)| {
                        self.merges
                            .get(&(pair[0], pair[1]))
                            .map(|&(rank, id)| (rank, index, id))
                    })
                    .min_by_key(|&(rank, index, _)| (rank, index));
                let Some((_, at, merged)) = candidate else {
                    break;
                };
                pieces[at] = merged;
                pieces.remove(at + 1);
            }
            output.extend(pieces);
        }
        Ok(())
    }
}

fn byte_encoder() -> [char; 256] {
    let mut next = 256;
    std::array::from_fn(|byte| {
        let code = if matches!(byte, 33..=126 | 161..=172 | 174..=255) {
            byte
        } else {
            let code = next;
            next += 1;
            code
        };
        char::from_u32(code as u32).expect("GPT-2 byte map is valid Unicode")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenizer() -> Tokenizer {
        let mut tokens: Vec<_> = byte_encoder().iter().map(char::to_string).collect();
        tokens.extend(
            [
                "ab",
                "abc",
                "bc",
                "<|stop|>",
                "<|stop|>tail",
                "",
                "<asr_text>",
                "你好",
                "<timestamp>",
                "<|unused_999|>",
            ]
            .map(String::from),
        );
        Tokenizer::from_parts(
            &tokens,
            &["a b".into(), "ab c".into(), "b c".into()],
            &[259, 260, 261],
            &[263],
        )
        .unwrap()
    }

    #[test]
    fn non_special_added_markers_remain_atomic_and_are_not_skipped() {
        let tokenizer = tokenizer();
        let ids = tokenizer.encode("ab<asr_text>你好").unwrap();
        assert_eq!(ids, [256, 262, 263]);
        assert_eq!(tokenizer.decode(&ids, true).unwrap(), "ab<asr_text>你好");
    }

    #[test]
    fn normal_typed_qwen_marker_is_not_confused_with_normal_padding() {
        let tokenizer = tokenizer();
        assert_eq!(
            tokenizer.encode("<asr_text><timestamp>").unwrap(),
            [262, 264]
        );
        assert_eq!(
            tokenizer.decode(&[262, 264], true).unwrap(),
            "<asr_text><timestamp>"
        );
        assert_ne!(tokenizer.encode("<|unused_999|>").unwrap(), [265]);
    }

    #[test]
    fn all_bytes_have_reversible_gpt2_mapping() {
        let tokenizer = tokenizer();
        for byte in 0..256 {
            assert_eq!(
                tokenizer.decoded[tokenizer.byte_ids[byte] as usize],
                [byte as u8]
            );
        }
        assert_eq!(byte_encoder()[b' ' as usize], 'Ġ');
        assert_eq!(byte_encoder()[b'\n' as usize], 'Ċ');
    }

    #[test]
    fn bpe_obeys_rank_and_leftmost_position() {
        let tokenizer = tokenizer();
        assert_eq!(
            tokenizer.encode("abc").unwrap(),
            [tokenizer.id("abc").unwrap()]
        );
        assert_eq!(
            tokenizer.encode("ababc").unwrap(),
            [tokenizer.id("ab").unwrap(), tokenizer.id("abc").unwrap()]
        );
    }

    #[test]
    fn unicode_byte_roundtrip_and_incomplete_token_decode() {
        let tokenizer = tokenizer();
        for text in [
            "世界 你好！",
            "日本語かな",
            "한국어",
            "Ελληνικά",
            "🙂👩‍🎤",
            "  line\r\n\t\n",
            "We're 123",
        ] {
            assert_eq!(
                tokenizer
                    .decode(&tokenizer.encode(text).unwrap(), false)
                    .unwrap(),
                text
            );
        }
        assert_eq!(
            tokenizer
                .decode(&[tokenizer.byte_ids[0xf0]], false)
                .unwrap(),
            "�"
        );
    }

    #[test]
    fn qwen2_pretokenization_whitespace_contractions_and_numbers() {
        let tokenizer = tokenizer();
        assert_eq!(
            tokenizer.pretokenize("We're  123 世界!\r\n  end").unwrap(),
            [
                "We", "'re", " ", " ", "1", "2", "3", " 世界", "!\r\n", " ", " end"
            ]
        );
        assert_eq!(
            tokenizer.pretokenize("one\t\t two  ").unwrap(),
            ["one", "\t\t", " two", "  "]
        );
    }

    #[test]
    fn explicit_controls_keep_literal_text_and_use_longest_match() {
        let tokenizer = tokenizer();
        let ids = tokenizer.encode("ab<|stop|>tailabc").unwrap();
        assert_eq!(
            ids,
            [
                tokenizer.id("ab").unwrap(),
                260,
                tokenizer.id("abc").unwrap()
            ]
        );
        assert_eq!(tokenizer.decode(&ids, false).unwrap(), "ab<|stop|>tailabc");
        assert_eq!(tokenizer.decode(&ids, true).unwrap(), "ababc");
        assert!(tokenizer.encode("").unwrap().is_empty());
        assert!(!tokenizer.encode("a").unwrap().contains(&261));
        assert!(tokenizer.decode(&[99_999], false).is_err());
    }
}
