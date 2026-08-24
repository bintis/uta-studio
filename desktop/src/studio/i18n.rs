//! Runtime UI localization for the native Bevy shell.
//!
//! English remains the source language and fallback.  The JSON catalogs are
//! embedded into the executable, so changing the interface language never
//! requires a network request or an external runtime dependency.

use std::{collections::HashMap, sync::OnceLock};

use crate::studio::*;

const ENGLISH_JSON: &str = include_str!("../../assets/i18n/en.json");
const SIMPLIFIED_CHINESE_JSON: &str = include_str!("../../assets/i18n/zh-CN.json");
const JAPANESE_JSON: &str = include_str!("../../assets/i18n/ja.json");

static ENGLISH_CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
static SIMPLIFIED_CHINESE_CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
static JAPANESE_CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
static DYNAMIC_TEMPLATES: OnceLock<Vec<UiTemplate>> = OnceLock::new();

#[derive(Debug)]
struct UiTemplate {
    source: String,
    literals: Vec<String>,
    slots: Vec<String>,
    literal_len: usize,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetUserDefaultLocaleName"]
    fn get_user_default_locale_name(locale_name: *mut u16, locale_name_capacity: i32) -> i32;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UiLocale {
    #[default]
    English,
    SimplifiedChinese,
    Japanese,
}

fn parse_catalog(source: &str) -> HashMap<String, String> {
    serde_json::from_str(source)
        .expect("embedded Uta Studio translation catalog must be valid JSON")
}

fn catalog(locale: UiLocale) -> &'static HashMap<String, String> {
    match locale {
        UiLocale::English => ENGLISH_CATALOG.get_or_init(|| parse_catalog(ENGLISH_JSON)),
        UiLocale::SimplifiedChinese => {
            SIMPLIFIED_CHINESE_CATALOG.get_or_init(|| parse_catalog(SIMPLIFIED_CHINESE_JSON))
        }
        UiLocale::Japanese => JAPANESE_CATALOG.get_or_init(|| parse_catalog(JAPANESE_JSON)),
    }
}

fn lookup(locale: UiLocale, source: &str) -> Option<&'static str> {
    catalog(locale).get(source).map(String::as_str)
}

fn parse_ui_template(source: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut literals = Vec::new();
    let mut slots = Vec::new();
    let mut literal_start = 0;
    let mut search_start = 0;

    while let Some(relative_open) = source[search_start..].find('{') {
        let open = search_start + relative_open;
        let Some(relative_close) = source[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + relative_close;
        let slot = &source[open + 1..close];
        let valid_slot = slot.is_empty()
            || slot.starts_with(':')
            || slot
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
                && slot.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '_' | ':' | '.' | '?' | '-' | '+')
                });
        if !valid_slot {
            search_start = close + 1;
            continue;
        }

        literals.push(source[literal_start..open].to_string());
        slots.push(slot.to_string());
        literal_start = close + 1;
        search_start = close + 1;
    }

    if slots.is_empty() {
        return None;
    }
    literals.push(source[literal_start..].to_string());
    Some((literals, slots))
}

fn dynamic_templates() -> &'static [UiTemplate] {
    DYNAMIC_TEMPLATES.get_or_init(|| {
        let mut templates = catalog(UiLocale::English)
            .iter()
            .filter(|(key, value)| !key.starts_with("message.") && key.as_str() == value.as_str())
            .filter_map(|(source, _)| {
                let (literals, slots) = parse_ui_template(source)?;
                let literal_len = literals.iter().map(String::len).sum();
                Some(UiTemplate {
                    source: source.clone(),
                    literals,
                    slots,
                    literal_len,
                })
            })
            .collect::<Vec<_>>();
        templates.sort_by(|left, right| {
            right
                .literal_len
                .cmp(&left.literal_len)
                .then_with(|| left.source.cmp(&right.source))
        });
        templates
    })
}

fn match_ui_template<'a>(template: &UiTemplate, source: &'a str) -> Option<Vec<&'a str>> {
    let mut cursor = template.literals.first()?.len();
    if !source.starts_with(template.literals.first()?) {
        return None;
    }

    let mut captures = Vec::with_capacity(template.slots.len());
    for literal in template.literals.iter().skip(1) {
        if literal.is_empty() {
            if captures.len() + 1 == template.slots.len() {
                captures.push(&source[cursor..]);
                cursor = source.len();
                continue;
            }
            return None;
        }
        let relative_end = source[cursor..].find(literal)?;
        let end = cursor + relative_end;
        captures.push(&source[cursor..end]);
        cursor = end + literal.len();
    }

    (cursor == source.len() && captures.len() == template.slots.len()).then_some(captures)
}

fn render_ui_template(
    template: &str,
    expected_slots: &[String],
    captures: &[&str],
) -> Option<String> {
    let (literals, slots) = parse_ui_template(template)?;
    if slots != expected_slots || captures.len() != slots.len() {
        return None;
    }

    let mut rendered = String::new();
    for (index, literal) in literals.iter().enumerate() {
        rendered.push_str(literal);
        if let Some(capture) = captures.get(index) {
            rendered.push_str(capture);
        }
    }
    Some(rendered)
}

fn translate_dynamic_ui(locale: UiLocale, source: &str) -> Option<String> {
    dynamic_templates().iter().find_map(|template| {
        let captures = match_ui_template(template, source)?;
        let translated = lookup(locale, &template.source)?;
        render_ui_template(translated, &template.slots, &captures)
    })
}

pub(crate) fn parse_ui_locale_hint(value: &str) -> Option<UiLocale> {
    // LANGUAGE may contain a colon-separated preference list.  Use the first
    // supported entry; encoding and modifier suffixes are irrelevant here.
    for candidate in value.split(':') {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }
        let candidate = candidate.split('.').next().unwrap_or(candidate);
        let candidate = candidate.split('@').next().unwrap_or(candidate);
        let normalized = candidate.replace('_', "-").to_ascii_lowercase();
        if normalized == "en" || normalized.starts_with("en-") {
            return Some(UiLocale::English);
        }
        if normalized == "ja" || normalized == "jp" || normalized.starts_with("ja-") {
            return Some(UiLocale::Japanese);
        }
        // The first Chinese catalog is Simplified Chinese.  Every zh locale
        // uses it for now rather than silently falling back to English.
        if normalized == "zh" || normalized.starts_with("zh-") {
            return Some(UiLocale::SimplifiedChinese);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_user_ui_locale() -> Option<UiLocale> {
    // LOCALE_NAME_MAX_LENGTH from the Windows API includes the terminator.
    let mut locale_name = [0_u16; 85];
    let length =
        unsafe { get_user_default_locale_name(locale_name.as_mut_ptr(), locale_name.len() as i32) };
    if length <= 1 {
        return None;
    }
    let value = String::from_utf16_lossy(&locale_name[..length as usize - 1]);
    parse_ui_locale_hint(&value)
}

pub(crate) fn effective_ui_locale(config: &AppConfig) -> UiLocale {
    if let Ok(value) = std::env::var("UTA_STUDIO_LOCALE")
        && let Some(locale) = parse_ui_locale_hint(&value)
    {
        return locale;
    }

    if let Some(value) = config.ui_language.as_deref()
        && let Some(locale) = parse_ui_locale_hint(value)
    {
        return locale;
    }

    for name in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(value) = std::env::var(name)
            && let Some(locale) = parse_ui_locale_hint(&value)
        {
            return locale;
        }
    }

    #[cfg(target_os = "windows")]
    if let Some(locale) = windows_user_ui_locale() {
        return locale;
    }

    UiLocale::English
}

fn render_template(locale: UiLocale, key: &str, replacements: &[(&str, &str)]) -> Option<String> {
    let mut rendered = lookup(locale, key)?.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    Some(rendered)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiMessage {
    AppVersion,
    RuntimeMissingComponents,
    PathOpened,
    FontSize,
    LatestScan,
    CacheRecalculating,
    CacheStatsFailed,
    FolderReadFailed,
    WatchedLocations,
    LogMissing,
    TranscriptPreviewFailed,
    TranscriptLoadFailed,
    TranscriptWaveformFailed,
    AudioWaveform,
    TranscriptPreviewing,
    TimingNotNumeric,
    LanguageReprocessQueued,
    ArtifactRevisionsRecorded,
    FolderStoppedWatching,
    FlacEstimatedUpperBound,
    AnalysisEngineSelected,
    DiagnosticsSummary,
}

impl UiMessage {
    const fn id(self) -> &'static str {
        match self {
            Self::AppVersion => "message.app_version",
            Self::RuntimeMissingComponents => "message.runtime_missing_components",
            Self::PathOpened => "message.path_opened",
            Self::FontSize => "message.font_size",
            Self::LatestScan => "message.latest_scan",
            Self::CacheRecalculating => "message.cache_recalculating",
            Self::CacheStatsFailed => "message.cache_stats_failed",
            Self::FolderReadFailed => "message.folder_read_failed",
            Self::WatchedLocations => "message.watched_locations",
            Self::LogMissing => "message.log_missing",
            Self::TranscriptPreviewFailed => "message.transcript_preview_failed",
            Self::TranscriptLoadFailed => "message.transcript_load_failed",
            Self::TranscriptWaveformFailed => "message.transcript_waveform_failed",
            Self::AudioWaveform => "message.audio_waveform",
            Self::TranscriptPreviewing => "message.transcript_previewing",
            Self::TimingNotNumeric => "message.timing_not_numeric",
            Self::LanguageReprocessQueued => "message.language_reprocess_queued",
            Self::ArtifactRevisionsRecorded => "message.artifact_revisions_recorded",
            Self::FolderStoppedWatching => "message.folder_stopped_watching",
            Self::FlacEstimatedUpperBound => "message.flac_estimated_upper_bound",
            Self::AnalysisEngineSelected => "message.analysis_engine_selected",
            Self::DiagnosticsSummary => "message.diagnostics_summary",
        }
    }

    const fn english(self) -> &'static str {
        match self {
            Self::AppVersion => "Version {version}",
            Self::RuntimeMissingComponents => "Missing components: {components}",
            Self::PathOpened => "Opened {path}",
            Self::FontSize => "Font size: {size}",
            Self::LatestScan => "Latest scan: {size}",
            Self::CacheRecalculating => "Recalculating in background. Latest scan: {size}",
            Self::CacheStatsFailed => "Cache stats failed to calculate: {error}",
            Self::FolderReadFailed => "Could not read this folder: {error}",
            Self::WatchedLocations => "WATCHED LOCATIONS · {count}",
            Self::LogMissing => "No application log exists yet at {path}",
            Self::TranscriptPreviewFailed => "Could not preview transcript audio: {error}",
            Self::TranscriptLoadFailed => "Could not load transcript audio: {error}",
            Self::TranscriptWaveformFailed => "Could not load transcript waveform: {error}",
            Self::AudioWaveform => "AUDIO WAVEFORM · {duration}",
            Self::TranscriptPreviewing => "Previewing transcript at {position}.",
            Self::TimingNotNumeric => "{field} is not numeric",
            Self::LanguageReprocessQueued => "Language set to {language}; reprocessing queued.",
            Self::ArtifactRevisionsRecorded => "Recorded {count} artifact revision(s) from disk.",
            Self::FolderStoppedWatching => {
                "Stopped watching {path}. No source media was moved or deleted."
            }
            Self::FlacEstimatedUpperBound => {
                "Estimated upper bound before FLAC compression: {size} MiB."
            }
            Self::AnalysisEngineSelected => {
                "{engine} selected. Existing charts change only after re-analysis."
            }
            Self::DiagnosticsSummary => {
                "{passed} passed · {failed} failed · {skipped} skipped · {apis} APIs"
            }
        }
    }
}

/// Renders a stable message ID before a `Text` entity is created.
pub(crate) fn localized_message(
    config: &AppConfig,
    message: UiMessage,
    replacements: &[(&str, &str)],
) -> String {
    let locale = effective_ui_locale(config);
    render_template(locale, message.id(), replacements).unwrap_or_else(|| {
        let mut rendered = message.english().to_string();
        for (placeholder, value) in replacements {
            rendered = rendered.replace(placeholder, value);
        }
        rendered
    })
}

pub(crate) fn translate_ui(locale: UiLocale, source: &str) -> Option<String> {
    if locale == UiLocale::English {
        return None;
    }

    if let Some(translated) = lookup(locale, source) {
        return Some(translated.to_string());
    }

    if let Some(translated) = translate_dynamic_ui(locale, source) {
        return Some(translated);
    }

    // Several setting descriptions append a user-selected path after a blank
    // line.  Translate the stable copy while preserving that path verbatim.
    if let Some((head, tail)) = source.split_once("\n\n")
        && let Some(translated) = lookup(locale, head)
    {
        return Some(format!("{translated}\n\n{tail}"));
    }

    None
}

type LocalizableTexts<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        Changed<Text>,
        Without<EditableText>,
        Without<NoRuntimeLocalization>,
    ),
>;

pub(crate) fn localize_ui_text(shell: Res<ShellState>, mut texts: LocalizableTexts) {
    let locale = effective_ui_locale(&shell.config);
    if locale == UiLocale::English {
        return;
    }

    for mut text in &mut texts {
        if let Some(translated) = translate_ui(locale, &text.0)
            && translated.as_str() != text.0.as_str()
        {
            text.0 = translated;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use super::*;

    #[test]
    fn locale_hints_cover_supported_aliases() {
        assert_eq!(parse_ui_locale_hint("en_US.UTF-8"), Some(UiLocale::English));
        assert_eq!(
            parse_ui_locale_hint("zh_CN.UTF-8"),
            Some(UiLocale::SimplifiedChinese)
        );
        assert_eq!(
            parse_ui_locale_hint("zh-Hant:ja:en"),
            Some(UiLocale::SimplifiedChinese)
        );
        assert_eq!(parse_ui_locale_hint("ja_JP"), Some(UiLocale::Japanese));
        assert_eq!(parse_ui_locale_hint("C.UTF-8"), None);
    }

    #[test]
    fn catalogs_have_identical_non_empty_keys() {
        let english = parse_catalog(ENGLISH_JSON);
        let chinese = parse_catalog(SIMPLIFIED_CHINESE_JSON);
        let japanese = parse_catalog(JAPANESE_JSON);
        let keys =
            |catalog: &HashMap<String, String>| catalog.keys().cloned().collect::<BTreeSet<_>>();
        assert_eq!(keys(&english), keys(&chinese));
        assert_eq!(keys(&english), keys(&japanese));
        assert!(
            english
                .iter()
                .all(|(key, value)| key.starts_with("message.") || key == value)
        );
        assert!(chinese.values().all(|value| !value.trim().is_empty()));
        assert!(japanese.values().all(|value| !value.trim().is_empty()));
        for (key, english_value) in &english {
            let template = if key.starts_with("message.") {
                english_value.as_str()
            } else {
                key.as_str()
            };
            let Some((_, expected_slots)) = parse_ui_template(template) else {
                continue;
            };
            for (locale, localized) in [
                ("zh-CN", chinese.get(key).expect("Chinese key exists")),
                ("ja", japanese.get(key).expect("Japanese key exists")),
            ] {
                let (_, localized_slots) = parse_ui_template(localized).unwrap_or_default();
                assert_eq!(
                    localized_slots, expected_slots,
                    "{locale} placeholders differ for {key}"
                );
            }
        }
    }

    #[test]
    fn exact_static_copy_is_localized() {
        assert_eq!(
            translate_ui(UiLocale::SimplifiedChinese, "Settings").as_deref(),
            Some("设置")
        );
        assert_eq!(translate_ui(UiLocale::Japanese, "user supplied text"), None);
    }

    #[test]
    fn stable_message_ids_render_arguments_before_text_creation() {
        let config = AppConfig {
            ui_language: Some("ja".to_string()),
            ..AppConfig::default()
        };
        assert_eq!(
            localized_message(
                &config,
                UiMessage::PathOpened,
                &[("{path}", "C:\\Music\\song.flac")]
            ),
            "C:\\Music\\song.flac を開きました"
        );
        let chinese = AppConfig {
            ui_language: Some("zh-CN".to_string()),
            ..AppConfig::default()
        };
        assert_eq!(
            localized_message(
                &chinese,
                UiMessage::DiagnosticsSummary,
                &[
                    ("{passed}", "2"),
                    ("{failed}", "1"),
                    ("{skipped}", "3"),
                    ("{apis}", "17"),
                ]
            ),
            "2 项通过 · 1 项失败 · 3 项跳过 · 17 个 API"
        );
    }

    #[test]
    fn formatted_static_copy_is_localized_after_values_are_inserted() {
        assert_eq!(
            translate_ui(UiLocale::SimplifiedChinese, "Font size: 14px").as_deref(),
            Some("字体大小：14px")
        );
        assert_eq!(
            translate_ui(
                UiLocale::Japanese,
                "12 selected. Existing charts change only after re-analysis."
            )
            .as_deref(),
            Some("12 を選択しました。既存譜面は再解析後にのみ変わります。")
        );
        assert_eq!(
            translate_ui(UiLocale::SimplifiedChinese, "3 tracks").as_deref(),
            Some("3 首曲目")
        );
    }
}
