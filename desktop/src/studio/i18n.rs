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

fn translate_diagnostic_summary(locale: UiLocale, source: &str) -> Option<String> {
    let parts = source.split(" · ").collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }
    let passed = parts[0].strip_suffix(" passed")?;
    let failed = parts[1].strip_suffix(" failed")?;
    let skipped = parts[2].strip_suffix(" skipped")?;
    let apis = parts[3].strip_suffix(" APIs")?;
    render_template(
        locale,
        "{passed} passed · {failed} failed · {skipped} skipped · {apis} APIs",
        &[
            ("{passed}", passed),
            ("{failed}", failed),
            ("{skipped}", skipped),
            ("{apis}", apis),
        ],
    )
}

fn translate_dynamic(locale: UiLocale, source: &str) -> Option<String> {
    for (prefix, key) in [
        ("Version ", "Version {value}"),
        ("Font size: ", "Font size: {value}"),
        ("Missing components: ", "Missing components: {value}"),
        ("Latest scan: ", "Latest scan: {value}"),
        (
            "Recalculating in background. Latest scan: ",
            "Recalculating in background. Latest scan: {value}",
        ),
        (
            "Cache stats failed to calculate: ",
            "Cache stats failed to calculate: {value}",
        ),
        (
            "Could not read this folder: ",
            "Could not read this folder: {value}",
        ),
        ("WATCHED LOCATIONS · ", "WATCHED LOCATIONS · {value}"),
        (
            "No application log exists yet at ",
            "No application log exists yet at {value}",
        ),
        ("Opened ", "Opened {value}"),
        (
            "Could not preview transcript audio: ",
            "Could not preview transcript audio: {value}",
        ),
        (
            "Could not load transcript audio: ",
            "Could not load transcript audio: {value}",
        ),
        (
            "Could not load transcript waveform: ",
            "Could not load transcript waveform: {value}",
        ),
        ("AUDIO WAVEFORM · ", "AUDIO WAVEFORM · {value}"),
    ] {
        if let Some(value) = source.strip_prefix(prefix) {
            return render_template(locale, key, &[("{value}", value)]);
        }
    }

    if let Some(value) = source
        .strip_prefix("Previewing transcript at ")
        .and_then(|value| value.strip_suffix('.'))
    {
        return render_template(
            locale,
            "Previewing transcript at {value}.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source.strip_suffix(" is not numeric") {
        return render_template(locale, "{value} is not numeric", &[("{value}", value)]);
    }

    if let Some(value) = source
        .strip_prefix("Language set to ")
        .and_then(|value| value.strip_suffix("; reprocessing queued."))
    {
        return render_template(
            locale,
            "Language set to {value}; reprocessing queued.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source
        .strip_prefix("Recorded ")
        .and_then(|value| value.strip_suffix(" artifact revision(s) from disk."))
    {
        return render_template(
            locale,
            "Recorded {value} artifact revision(s) from disk.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source
        .strip_prefix("Stopped watching ")
        .and_then(|value| value.strip_suffix(". No source media was moved or deleted."))
    {
        return render_template(
            locale,
            "Stopped watching {value}. No source media was moved or deleted.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source
        .strip_prefix("Acceleration set to ")
        .and_then(|value| value.strip_suffix(". Reconfigure the runtime to apply it."))
    {
        return render_template(
            locale,
            "Acceleration set to {value}. Reconfigure the runtime to apply it.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source
        .strip_prefix("Estimated upper bound before FLAC compression: ")
        .and_then(|value| value.strip_suffix(" MiB."))
    {
        return render_template(
            locale,
            "Estimated upper bound before FLAC compression: {value} MiB.",
            &[("{value}", value)],
        );
    }

    if let Some(value) = source
        .strip_suffix(" separation profile applied. Existing stems change only after re-analysis.")
    {
        return render_template(
            locale,
            "{value} separation profile applied. Existing stems change only after re-analysis.",
            &[("{value}", value)],
        );
    }

    if let Some(value) =
        source.strip_suffix(" selected. Existing charts change only after re-analysis.")
    {
        return render_template(
            locale,
            "{value} selected. Existing charts change only after re-analysis.",
            &[("{value}", value)],
        );
    }

    translate_diagnostic_summary(locale, source)
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

    translate_dynamic(locale, source)
}

pub(crate) fn localize_ui_text(
    session: Res<StudioSession>,
    mut texts: Query<
        &mut Text,
        (
            Changed<Text>,
            Without<EditableText>,
            Without<NoRuntimeLocalization>,
        ),
    >,
) {
    let locale = effective_ui_locale(&session.config);
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
        assert!(english.iter().all(|(key, value)| key == value));
        assert!(chinese.values().all(|value| !value.trim().is_empty()));
        assert!(japanese.values().all(|value| !value.trim().is_empty()));
    }

    #[test]
    fn exact_and_dynamic_copy_is_localized() {
        assert_eq!(
            translate_ui(UiLocale::SimplifiedChinese, "Settings").as_deref(),
            Some("设置")
        );
        let version = env!("CARGO_PKG_VERSION");
        let english = format!("Version {version}");
        let japanese = format!("バージョン {version}");
        assert_eq!(
            translate_ui(UiLocale::Japanese, &english).as_deref(),
            Some(japanese.as_str())
        );
        assert_eq!(
            translate_ui(
                UiLocale::SimplifiedChinese,
                "2 passed · 1 failed · 3 skipped · 17 APIs"
            )
            .as_deref(),
            Some("2 项通过 · 1 项失败 · 3 项跳过 · 17 个 API")
        );
        assert_eq!(translate_ui(UiLocale::Japanese, "user supplied text"), None);
    }
}
