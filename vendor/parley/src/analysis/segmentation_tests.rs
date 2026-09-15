// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::{AnalysisDataSources, LineBreakOptions, LineBreakWordOption, LineSegmenter, WordBreak};
use alloc::vec::Vec;

fn words(text: &str) -> Vec<&str> {
    let sources = AnalysisDataSources::new();
    let boundaries: Vec<_> = sources.word_segmenter().segment_str(text).collect();
    boundaries
        .windows(2)
        .map(|range| &text[range[0]..range[1]])
        .collect()
}

#[test]
fn japanese_layout_loads_the_lexical_dictionary() {
    // ICU's Japanese dictionary fixture: the non-complex constructor cannot
    // produce these lexical boundaries. Test the actual Parley data source,
    // not the separate Analysis Engine segmenter or the desktop log filter.
    assert_eq!(words("うなぎうなじ"), ["うなぎ", "うなじ"]);
}

#[test]
fn mixed_cjk_layout_keeps_utf_boundaries_and_english_words() {
    let text = "Welcome龟山岛龟山岛Welcome";
    let units = words(text);
    assert_eq!(units, ["Welcome", "龟山岛", "龟山岛", "Welcome"]);
    assert_eq!(units.concat(), text);
    assert_eq!(words("Hello, world!"), ["Hello", ",", " ", "world", "!"]);
}

#[test]
fn dictionary_line_breaking_preserves_each_wrapping_policy() {
    let sources = AnalysisDataSources::new();
    for (policy, word_option) in [
        (WordBreak::Normal, LineBreakWordOption::Normal),
        (WordBreak::BreakAll, LineBreakWordOption::BreakAll),
        (WordBreak::KeepAll, LineBreakWordOption::KeepAll),
    ] {
        let mut options = LineBreakOptions::default();
        options.word_option = Some(word_option);
        let expected = LineSegmenter::new_dictionary(options);
        for text in ["日本語の歌詞、次の行。", "ภาษาไทยภาษาไทย", "Hello world!"] {
            let actual: Vec<_> = sources.line_segmenter(policy).segment_str(text).collect();
            assert_eq!(actual, expected.segment_str(text).collect::<Vec<_>>());
            assert_eq!(actual.last(), Some(&text.len()));
            assert!(actual.iter().all(|offset| text.is_char_boundary(*offset)));
        }
    }
}
