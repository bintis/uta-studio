use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const DOCUMENT_REVISION: &str = "2026-08-24";
const LOCALES: [(&str, &str); 3] = [("en", "English"), ("zh-CN", "简体中文"), ("ja", "日本語")];
const PAGE_IDS: [&str; 15] = [
    "guide:about",
    "guide:installation",
    "guide:getting-started",
    "guide:quick-start",
    "guide:library",
    "guide:analysis",
    "guide:lyrics",
    "guide:editor",
    "guide:export",
    "guide:storage",
    "guide:diagnostics",
    "guide:troubleshooting",
    "guide:privacy",
    "guide:documentation",
    "guide:artifacts",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Heading {
    level: usize,
    title: String,
    anchor: String,
    page_id: String,
}

#[derive(Clone, Debug)]
struct LocaleDocument {
    locale: &'static str,
    label: &'static str,
    source: String,
    headings: Vec<Heading>,
    pages: Vec<(String, String, String)>,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let check = match args.first().map(String::as_str) {
        Some("build") => false,
        Some("check") => true,
        _ => return Err("usage: cargo xtask docs <build|check>".to_string()),
    };
    let root = workspace_root()?;
    let app_version = package_version(&root)?;
    let documents = load_documents(&root, &app_version)?;
    let combined = build_combined(&documents, &app_version);
    let bundle = build_bundle(&documents, &app_version);
    let outputs = [
        (root.join("docs/USER_GUIDE.md"), combined),
        (root.join("desktop/assets/docs/docs.bundle.json"), bundle),
    ];
    if check {
        let mut stale = Vec::new();
        for (path, expected) in &outputs {
            if fs::read_to_string(path).ok().as_deref() != Some(expected.as_str()) {
                stale.push(relative(&root, path));
            }
        }
        if stale.is_empty() {
            println!("documentation outputs are current");
            Ok(())
        } else {
            Err(format!(
                "documentation outputs are stale: {}\nrun `cargo xtask docs build`",
                stale.join(", ")
            ))
        }
    } else {
        for (path, content) in outputs {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("generated {}", relative(&root, &path));
        }
        Ok(())
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live below the workspace root".to_string())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn package_version(root: &Path) -> Result<String, String> {
    let manifest =
        fs::read_to_string(root.join("desktop/Cargo.toml")).map_err(|e| e.to_string())?;
    manifest
        .lines()
        .find_map(|line| {
            let value = line.trim().strip_prefix("version = ")?.trim();
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_string)
        })
        .ok_or_else(|| "desktop package version is missing".to_string())
}

fn load_documents(root: &Path, app_version: &str) -> Result<Vec<LocaleDocument>, String> {
    let mut documents = Vec::new();
    let mut expected_pages: Option<Vec<String>> = None;
    for (locale, label) in LOCALES {
        let path = root.join(format!("docs/user-guide/{locale}.md"));
        let canonical = fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        validate_canonical_source(locale, &canonical)?;
        let source = canonical
            .replace("{{APP_VERSION}}", app_version)
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let headings = parse_headings(locale, &source)?;
        validate_links(locale, &source, &headings)?;
        let pages = parse_pages(locale, &source, &headings)?;
        let page_ids = pages.iter().map(|page| page.0.clone()).collect::<Vec<_>>();
        if let Some(expected) = &expected_pages {
            if expected != &page_ids {
                return Err(format!(
                    "{locale}: page IDs differ from en: {page_ids:?} != {expected:?}"
                ));
            }
        } else {
            expected_pages = Some(page_ids);
        }
        documents.push(LocaleDocument {
            locale,
            label,
            source,
            headings,
            pages,
        });
    }
    Ok(documents)
}

fn validate_canonical_source(locale: &str, source: &str) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err(format!("{locale}: document is empty"));
    }
    if !source.starts_with("# Uta! Studio") {
        return Err(format!(
            "{locale}: document must start with the Uta! Studio title"
        ));
    }
    if !source.contains("{{APP_VERSION}}") {
        return Err(format!("{locale}: {{APP_VERSION}} placeholder is required"));
    }
    for marker in [
        "uta-studio-0.",
        "uta-studio_0.",
        "Release 0.",
        "版本 0.",
        "0.4.0",
    ] {
        if source.contains(marker) {
            return Err(format!(
                "{locale}: stale hard-coded release marker `{marker}`"
            ));
        }
    }
    Ok(())
}

fn parse_headings(locale: &str, source: &str) -> Result<Vec<Heading>, String> {
    let mut headings = Vec::new();
    let mut anchors = BTreeSet::new();
    let mut current_page = PAGE_IDS[0].to_string();
    for line in source.lines() {
        let Some((level, title)) = heading_line(line) else {
            continue;
        };
        if level == 3 {
            let number = title
                .split_once('.')
                .and_then(|(number, _)| number.trim().parse::<usize>().ok())
                .ok_or_else(|| {
                    format!("{locale}: H3 must start with a numeric page prefix: {title}")
                })?;
            current_page = PAGE_IDS
                .get(number.saturating_sub(1))
                .ok_or_else(|| format!("{locale}: unsupported page number {number}"))?
                .to_string();
        }
        let anchor = slug(title);
        if anchor.is_empty() || !anchors.insert(anchor.clone()) {
            return Err(format!(
                "{locale}: empty or duplicate heading anchor `{anchor}`"
            ));
        }
        headings.push(Heading {
            level,
            title: title.to_string(),
            anchor,
            page_id: current_page.clone(),
        });
    }
    if headings.is_empty() {
        return Err(format!("{locale}: no headings"));
    }
    Ok(headings)
}

fn parse_pages(
    locale: &str,
    source: &str,
    headings: &[Heading],
) -> Result<Vec<(String, String, String)>, String> {
    let h3 = headings
        .iter()
        .filter(|heading| heading.level == 3)
        .collect::<Vec<_>>();
    if h3.len() != PAGE_IDS.len() {
        return Err(format!(
            "{locale}: expected {} non-empty pages, found {}",
            PAGE_IDS.len(),
            h3.len()
        ));
    }
    let mut pages = Vec::new();
    for (index, heading) in h3.iter().enumerate() {
        let marker = format!("### {}", heading.title);
        let start = source
            .find(&marker)
            .ok_or_else(|| format!("{locale}: page heading disappeared: {}", heading.title))?;
        let end = h3
            .get(index + 1)
            .and_then(|next| source.find(&format!("### {}", next.title)))
            .unwrap_or(source.len());
        let body = source[start + marker.len()..end].trim().to_string();
        if body.is_empty() {
            return Err(format!("{locale}: empty page {}", heading.page_id));
        }
        pages.push((heading.page_id.clone(), heading.title.clone(), body));
    }
    Ok(pages)
}

fn heading_line(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=4).contains(&level) || trimmed.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, trimmed[level + 1..].trim()))
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn validate_links(locale: &str, source: &str, headings: &[Heading]) -> Result<(), String> {
    let anchors = headings
        .iter()
        .map(|heading| heading.anchor.as_str())
        .collect::<BTreeSet<_>>();
    for target in markdown_link_targets(source) {
        if let Some(anchor) = target.strip_prefix('#') {
            if !anchors.contains(anchor) {
                return Err(format!("{locale}: broken internal link `{target}`"));
            }
        } else if target.starts_with("https://") || target.starts_with("guide:") {
            continue;
        } else {
            return Err(format!(
                "{locale}: unsupported or unresolved link `{target}`"
            ));
        }
    }
    Ok(())
}

fn markdown_link_targets(source: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut remaining = source;
    while let Some(close_label) = remaining.find("](") {
        remaining = &remaining[close_label + 2..];
        let Some(end) = remaining.find(')') else {
            break;
        };
        targets.push(&remaining[..end]);
        remaining = &remaining[end + 1..];
    }
    targets
}

fn build_combined(documents: &[LocaleDocument], app_version: &str) -> String {
    let mut output = format!(
        "# Uta! Studio User Guide / 用户说明书 / ユーザーガイド\n\n**Applies to:** Uta! Studio {app_version}\n**Document revision:** {DOCUMENT_REVISION}\n**License:** Documentation distributed with the GPL-3.0 project.\n\n[English](#english) · [简体中文](#简体中文) · [日本語](#日本語)\n\n> This file is generated from `docs/user-guide/*.md`. Do not edit it directly.\n"
    );
    for document in documents {
        output.push_str("\n---\n\n## ");
        output.push_str(document.label);
        output.push_str("\n\n");
        output.push_str(
            document
                .source
                .trim_start_matches("# Uta! Studio User Guide")
                .trim(),
        );
        output.push('\n');
    }
    output
}

fn json_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 16);
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }
    output
}

fn build_bundle(documents: &[LocaleDocument], app_version: &str) -> String {
    let mut locale_json = Vec::new();
    for document in documents {
        let pages = document
            .pages
            .iter()
            .map(|(id, heading, body)| {
                format!(
                    "{{\"id\":\"{}\",\"heading\":\"{}\",\"anchor\":\"{}\",\"body\":\"{}\"}}",
                    json_escape(id),
                    json_escape(heading),
                    json_escape(&slug(heading)),
                    json_escape(body)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let headings = document
            .headings
            .iter()
            .map(|heading| {
                format!(
                    "{{\"level\":{},\"title\":\"{}\",\"anchor\":\"{}\",\"page_id\":\"{}\"}}",
                    heading.level,
                    json_escape(&heading.title),
                    json_escape(&heading.anchor),
                    json_escape(&heading.page_id)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        locale_json.push(format!(
            "\"{}\":{{\"source\":\"{}\",\"pages\":[{}],\"headings\":[{}]}}",
            document.locale,
            json_escape(&document.source),
            pages,
            headings
        ));
    }
    let mut semantic_links = BTreeMap::new();
    semantic_links.insert("artifact:SourceMedia", "guide:artifacts");
    semantic_links.insert("artifact:MusicAnalysis", "guide:artifacts");
    semantic_links.insert("artifact:KeyAnalysis", "guide:artifacts");
    semantic_links.insert("artifact:RhythmAnalysis", "guide:artifacts");
    semantic_links.insert("artifact:AudioDescriptors", "guide:artifacts");
    semantic_links.insert("artifact:VocalStem", "guide:artifacts");
    semantic_links.insert("artifact:InstrumentalStem", "guide:artifacts");
    semantic_links.insert("artifact:PitchTrack", "guide:artifacts");
    semantic_links.insert("artifact:PitchNoteCandidates", "guide:artifacts");
    semantic_links.insert("artifact:AuthoredChart", "guide:artifacts");
    semantic_links.insert("artifact:LyricsInput", "guide:artifacts");
    semantic_links.insert("artifact:PreprocessedAudio", "guide:artifacts");
    semantic_links.insert("artifact:RecognizedText", "guide:artifacts");
    semantic_links.insert("artifact:AsrSegments", "guide:artifacts");
    semantic_links.insert("artifact:TimedTranscript", "guide:artifacts");
    semantic_links.insert("artifact:CandidateChart", "guide:artifacts");
    semantic_links.insert("node:chart.build_candidate", "guide:editor");
    semantic_links.insert("node:lyrics.align", "guide:lyrics");
    semantic_links.insert("node:lyrics.import_timed", "guide:lyrics");
    semantic_links.insert("node:lyrics.preprocess", "guide:lyrics");
    semantic_links.insert("node:lyrics.transcribe", "guide:lyrics");
    semantic_links.insert("node:music.analysis", "guide:analysis");
    semantic_links.insert("node:music.key", "guide:analysis");
    semantic_links.insert("node:music.rhythm", "guide:analysis");
    semantic_links.insert("node:music.descriptors", "guide:analysis");
    semantic_links.insert("node:pitch.extract", "guide:analysis");
    semantic_links.insert("node:preflight", "guide:analysis");
    semantic_links.insert("node:stems.separate", "guide:analysis");
    semantic_links.insert("node:stems.vocals", "guide:analysis");
    semantic_links.insert("node:vocals.denoise", "guide:analysis");
    semantic_links.insert("node:vocals.dereverb", "guide:analysis");
    semantic_links.insert("node:stems.instrumental", "guide:analysis");
    semantic_links.insert("node:instrumental.denoise", "guide:analysis");
    semantic_links.insert("node:instrumental.dereverb", "guide:analysis");
    semantic_links.insert("node:stems.bind_analysis_outputs", "guide:analysis");
    semantic_links.insert("problem:OverlappingNotes", "guide:editor");
    semantic_links.insert("problem:NoteTooShort", "guide:editor");
    semantic_links.insert("problem:MissingPitchTarget", "guide:editor");
    semantic_links.insert("problem:UnresolvedContinuation", "guide:editor");
    semantic_links.insert("problem:EmptyLyric", "guide:editor");
    semantic_links.insert("problem:ScorableNoteWithoutLyric", "guide:editor");
    semantic_links.insert("problem:LyricWithoutPitch", "guide:editor");
    semantic_links.insert("problem:LargeIntervalLeap", "guide:editor");
    semantic_links.insert("problem:PhrasesTouch", "guide:editor");
    semantic_links.insert("problem:UnusualGoldenShare", "guide:editor");
    let semantic_links = semantic_links
        .into_iter()
        .map(|(key, value)| format!("\"{}\":\"{}\"", json_escape(key), json_escape(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":1,\"app_version_range\":\">=0.6.0,<0.7.0\",\"built_for_version\":\"{}\",\"document_revision\":\"{}\",\"semantic_links\":{{{}}},\"locales\":{{{}}}}}\n",
        json_escape(app_version),
        DOCUMENT_REVISION,
        semantic_links,
        locale_json.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_anchors_are_rejected() {
        let source = "# Uta! Studio User Guide\n### 1. Same\ntext\n#### 1. Same\ntext\n";
        assert!(parse_headings("en", source)
            .unwrap_err()
            .contains("duplicate"));
    }

    #[test]
    fn broken_links_are_rejected() {
        let source = "# Uta! Studio User Guide\n### 1. Start\n[bad](#missing)\n";
        let headings = parse_headings("en", source).unwrap();
        assert!(validate_links("en", source, &headings).is_err());
    }

    #[test]
    fn version_placeholder_expands_deterministically() {
        let source = "uta-studio-{{APP_VERSION}}";
        assert_eq!(
            source.replace("{{APP_VERSION}}", "0.5.0"),
            "uta-studio-0.5.0"
        );
    }

    #[test]
    fn json_escape_preserves_cjk_and_escapes_structure() {
        assert_eq!(json_escape("解析\n\"x\""), "解析\\n\\\"x\\\"");
    }
}
