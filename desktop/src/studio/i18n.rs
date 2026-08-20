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
    AccelerationSet,
    FlacEstimatedUpperBound,
    SeparationProfileApplied,
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
            Self::AccelerationSet => "message.acceleration_set",
            Self::FlacEstimatedUpperBound => "message.flac_estimated_upper_bound",
            Self::SeparationProfileApplied => "message.separation_profile_applied",
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
            Self::AccelerationSet => {
                "Acceleration set to {backend}. Reconfigure the runtime to apply it."
            }
            Self::FlacEstimatedUpperBound => {
                "Estimated upper bound before FLAC compression: {size} MiB."
            }
            Self::SeparationProfileApplied => {
                "{profile} separation profile applied. Existing stems change only after re-analysis."
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
}
