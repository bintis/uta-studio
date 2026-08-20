//! Native offline Documentation Center.
//!
//! Markdown is embedded at compile time and rendered as native Bevy text.
//! The body opts out of runtime source-string localization: the viewer
//! selects the correct document before rendering.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::studio::*;

const GUIDE_BUNDLE_JSON: &str = include_str!("../../assets/docs/docs.bundle.json");

#[derive(serde::Deserialize)]
struct EmbeddedGuideBundle {
    semantic_links: HashMap<String, String>,
    locales: HashMap<String, EmbeddedGuideLocale>,
}

#[derive(serde::Deserialize)]
struct EmbeddedGuideLocale {
    source: String,
}

fn guide_bundle() -> &'static EmbeddedGuideBundle {
    static BUNDLE: OnceLock<EmbeddedGuideBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        serde_json::from_str(GUIDE_BUNDLE_JSON)
            .expect("generated Uta Studio documentation bundle must be valid JSON")
    })
}

#[derive(Component)]
pub(crate) struct NoRuntimeLocalization;

#[derive(Component)]
pub(crate) struct DocumentationContent;

#[derive(Component)]
pub(crate) struct DocumentationSearchInput;

#[derive(Clone, Debug)]
pub(crate) struct DocumentationState {
    pub(crate) anchor: Option<String>,
    pub(crate) current_page_id: Option<String>,
    pub(crate) query: String,
    pub(crate) scroll_offset: f32,
    pub(crate) scroll_positions: std::collections::BTreeMap<String, f32>,
    pub(crate) back_stack: Vec<Option<String>>,
    pub(crate) forward_stack: Vec<Option<String>>,
    pub(crate) return_route: Option<StudioRoute>,
}

impl Default for DocumentationState {
    fn default() -> Self {
        Self {
            anchor: None,
            current_page_id: None,
            query: String::new(),
            scroll_offset: 0.0,
            scroll_positions: std::collections::BTreeMap::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            return_route: None,
        }
    }
}

impl DocumentationState {
    pub(crate) fn navigate(&mut self, target: Option<String>) {
        let target = target.map(|target| {
            guide_bundle()
                .semantic_links
                .get(&target)
                .cloned()
                .unwrap_or(target)
        });
        if self.anchor == target {
            return;
        }
        if self.anchor.is_some() {
            self.back_stack.push(self.anchor.clone());
        }
        self.forward_stack.clear();
        self.anchor = target.clone();
        self.current_page_id = target
            .as_deref()
            .and_then(|value| value.strip_prefix("guide:"))
            .map(|value| format!("guide:{value}"));
        self.scroll_offset = target
            .as_ref()
            .and_then(|key| self.scroll_positions.get(key))
            .copied()
            .unwrap_or(0.0);
    }

    pub(crate) fn go_back(&mut self) -> bool {
        let Some(previous) = self.back_stack.pop() else {
            return false;
        };
        self.forward_stack.push(self.anchor.clone());
        self.anchor = previous;
        true
    }

    pub(crate) fn go_forward(&mut self) -> bool {
        let Some(next) = self.forward_stack.pop() else {
            return false;
        };
        self.back_stack.push(self.anchor.clone());
        self.anchor = next;
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentationSearchHit {
    pub(crate) heading: String,
    pub(crate) line: String,
    pub(crate) anchor: String,
    pub(crate) score: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MarkdownBlock {
    Heading {
        level: u8,
        text: String,
        anchor: String,
    },
    Paragraph(String),
    UnorderedList(Vec<String>),
    OrderedList(Vec<String>),
    Code {
        language: Option<String>,
        text: String,
    },
    Table(Vec<Vec<String>>),
    Callout {
        kind: String,
        text: String,
    },
}

fn source_for(locale: UiLocale) -> &'static str {
    let key = match locale {
        UiLocale::English => "en",
        UiLocale::SimplifiedChinese => "zh-CN",
        UiLocale::Japanese => "ja",
    };
    guide_bundle()
        .locales
        .get(key)
        .or_else(|| guide_bundle().locales.get("en"))
        .map(|locale| locale.source.as_str())
        .expect("generated documentation bundle must contain English")
}

pub(crate) fn documentation_anchor_for_node(node_id: &str) -> &'static str {
    match node_id {
        "preflight" | "music.analysis" | "music.key" | "music.rhythm" | "music.descriptors" => {
            "guide:analysis"
        }
        "stems.separate" => "guide:analysis",
        "pitch.extract" => "guide:analysis",
        "lyrics.preprocess" | "lyrics.transcribe" | "lyrics.align" | "lyrics.import_timed" => {
            "guide:lyrics"
        }
        "chart.build_candidate" => "guide:editor",
        _ => "guide:getting-started",
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn safe_link_target(target: &str) -> bool {
    target.starts_with('#')
        || target.starts_with("guide:")
        || target.starts_with("node:")
        || target.starts_with("artifact:")
        || target.starts_with("problem:")
        || target.starts_with("https://")
}

fn plain_inline(markdown: &str) -> String {
    let chars = markdown.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '!'
            && chars.get(index + 1) == Some(&'[')
            && let Some(close) = chars[index + 2..].iter().position(|ch| *ch == ']')
        {
            let after = index + 2 + close + 1;
            if chars.get(after) == Some(&'(')
                && let Some(end) = chars[after + 1..].iter().position(|ch| *ch == ')')
            {
                index = after + end + 2;
                continue;
            }
        }
        if chars[index] == '['
            && let Some(close) = chars[index + 1..].iter().position(|ch| *ch == ']')
        {
            let close = index + 1 + close;
            if chars.get(close + 1) == Some(&'(')
                && let Some(end) = chars[close + 2..].iter().position(|ch| *ch == ')')
            {
                let end = close + 2 + end;
                let target = chars[close + 2..end].iter().collect::<String>();
                if safe_link_target(&target) {
                    output.extend(chars[index + 1..close].iter());
                }
                index = end + 1;
                continue;
            }
        }
        if chars[index] == '*' || chars[index] == '_' || chars[index] == '`' {
            index += 1;
            continue;
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn parse_markdown(source: &str) -> Vec<MarkdownBlock> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        if let Some(fence) = trimmed.strip_prefix("```") {
            let language = (!fence.trim().is_empty()).then(|| fence.trim().to_string());
            index += 1;
            let mut code = Vec::new();
            while index < lines.len() && !lines[index].trim_start().starts_with("```") {
                code.push(lines[index]);
                index += 1;
            }
            index += usize::from(index < lines.len());
            blocks.push(MarkdownBlock::Code {
                language,
                text: code.join("\n"),
            });
            continue;
        }
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        if (1..=4).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
            let text = plain_inline(trimmed[level + 1..].trim());
            blocks.push(MarkdownBlock::Heading {
                level: level as u8,
                anchor: slug(&text),
                text,
            });
            index += 1;
            continue;
        }
        if let Some(kind) = trimmed
            .strip_prefix("> [!")
            .and_then(|value| value.split_once(']'))
        {
            let mut text = vec![kind.1.trim().to_string()];
            index += 1;
            while index < lines.len() {
                let Some(value) = lines[index].trim().strip_prefix('>') else {
                    break;
                };
                text.push(value.trim().to_string());
                index += 1;
            }
            blocks.push(MarkdownBlock::Callout {
                kind: kind.0.to_string(),
                text: plain_inline(&text.join(" ")),
            });
            continue;
        }
        if trimmed.starts_with('<') && trimmed.ends_with('>') {
            index += 1;
            continue;
        }
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            let mut rows = Vec::new();
            while index < lines.len() {
                let row = lines[index].trim();
                if !row.starts_with('|') || !row.ends_with('|') {
                    break;
                }
                let cells = row[1..row.len() - 1]
                    .split('|')
                    .map(|cell| plain_inline(cell.trim()))
                    .collect::<Vec<_>>();
                let separator = cells.iter().all(|cell| {
                    let cell = cell.trim_matches(':').trim();
                    !cell.is_empty() && cell.chars().all(|ch| ch == '-')
                });
                if !separator {
                    rows.push(cells);
                }
                index += 1;
            }
            blocks.push(MarkdownBlock::Table(rows));
            continue;
        }
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let mut items = Vec::new();
            while index < lines.len() {
                let row = lines[index].trim();
                let Some(item) = row.strip_prefix("- ").or_else(|| row.strip_prefix("* ")) else {
                    break;
                };
                items.push(plain_inline(item));
                index += 1;
            }
            blocks.push(MarkdownBlock::UnorderedList(items));
            continue;
        }
        if ordered_item(trimmed).is_some() {
            let mut items = Vec::new();
            while index < lines.len() {
                let Some(item) = ordered_item(lines[index].trim()) else {
                    break;
                };
                items.push(plain_inline(item));
                index += 1;
            }
            blocks.push(MarkdownBlock::OrderedList(items));
            continue;
        }
        let mut paragraph = vec![trimmed];
        index += 1;
        while index < lines.len() {
            let next = lines[index].trim();
            if next.is_empty()
                || next.starts_with('#')
                || next.starts_with("```")
                || next.starts_with("- ")
                || next.starts_with("* ")
                || next.starts_with('|')
                || next.starts_with("> [!")
                || ordered_item(next).is_some()
            {
                break;
            }
            paragraph.push(next);
            index += 1;
        }
        blocks.push(MarkdownBlock::Paragraph(plain_inline(&paragraph.join(" "))));
    }
    blocks
}

fn ordered_item(line: &str) -> Option<&str> {
    let (number, rest) = line.split_once(". ")?;
    (!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())).then_some(rest)
}

pub(crate) fn search_documentation(locale: UiLocale, query: &str) -> Vec<DocumentationSearchHit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut heading = String::new();
    let mut anchor = String::new();
    let mut hits = Vec::new();
    for block in parse_markdown(source_for(locale)) {
        let (line, is_heading) = match block {
            MarkdownBlock::Heading {
                text, anchor: id, ..
            } => {
                heading = text.clone();
                anchor = id;
                (text, true)
            }
            MarkdownBlock::Paragraph(text) | MarkdownBlock::Callout { text, .. } => (text, false),
            MarkdownBlock::UnorderedList(items) | MarkdownBlock::OrderedList(items) => {
                (items.join(" "), false)
            }
            MarkdownBlock::Code { text, .. } => (text, false),
            MarkdownBlock::Table(rows) => (
                rows.into_iter().flatten().collect::<Vec<_>>().join(" "),
                false,
            ),
        };
        let line_lower = line.to_lowercase();
        let heading_lower = heading.to_lowercase();
        if line_lower.contains(&query) || heading_lower.contains(&query) {
            let heading_bonus = usize::from(is_heading || heading_lower.contains(&query)) * 100;
            hits.push(DocumentationSearchHit {
                heading: heading.clone(),
                line,
                anchor: anchor.clone(),
                score: heading_bonus + query.chars().count(),
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.heading.cmp(&b.heading))
    });
    hits.dedup_by(|a, b| a.anchor == b.anchor);
    hits.truncate(24);
    hits
}

fn render_markdown_block(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    block: &MarkdownBlock,
) {
    let (size, color, text) = match block {
        MarkdownBlock::Heading { level, text, .. } => (
            match level {
                1 => 24.0,
                2 => 20.0,
                3 => 16.0,
                _ => 13.0,
            },
            theme.foreground,
            text.clone(),
        ),
        MarkdownBlock::Paragraph(text) => (10.0, theme.muted_foreground, text.clone()),
        MarkdownBlock::UnorderedList(items) => (
            10.0,
            theme.foreground,
            items
                .iter()
                .map(|item| format!("• {item}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        MarkdownBlock::OrderedList(items) => (
            10.0,
            theme.foreground,
            items
                .iter()
                .enumerate()
                .map(|(index, item)| format!("{}. {item}", index + 1))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        MarkdownBlock::Code { language, text } => (
            9.0,
            theme.primary,
            language
                .as_ref()
                .map(|language| format!("{language}\n{text}"))
                .unwrap_or_else(|| text.clone()),
        ),
        MarkdownBlock::Table(rows) => (
            9.0,
            theme.foreground,
            rows.iter()
                .map(|row| row.join("   ·   "))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        MarkdownBlock::Callout { kind, text } => (
            10.0,
            theme.primary,
            format!("{}  {text}", kind.to_uppercase()),
        ),
    };
    parent.spawn((
        NoRuntimeLocalization,
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        Node {
            max_width: percent(100),
            margin: UiRect::bottom(px(7)),
            ..default()
        },
    ));
}

fn section_number_for_anchor(anchor: &str) -> Option<&'static str> {
    match anchor {
        "guide:getting-started" => Some("### 3."),
        "guide:analysis" => Some("### 6."),
        "guide:lyrics" => Some("### 7."),
        "guide:editor" => Some("### 8."),
        "guide:export" => Some("### 9."),
        "guide:troubleshooting" => Some("### 12."),
        "guide:documentation" => Some("### 14."),
        "guide:artifacts" => Some("### 15."),
        _ => None,
    }
}

fn visible_source<'a>(source: &'a str, anchor: Option<&str>) -> &'a str {
    let Some(anchor) = anchor else {
        return source;
    };
    if let Some(prefix) = section_number_for_anchor(anchor)
        && let Some(index) = source.find(prefix)
    {
        return &source[index..];
    }
    if let Some(slugged) = anchor.strip_prefix("heading:") {
        let mut offset = 0usize;
        for line in source.lines() {
            let line_with_newline = source[offset..]
                .find('\n')
                .map(|end| &source[offset..offset + end + 1])
                .unwrap_or(&source[offset..]);
            if line.trim_start().starts_with('#') {
                let heading = line.trim_start_matches('#').trim();
                if slug(heading) == slugged {
                    return &source[offset..];
                }
            }
            offset += line_with_newline.len();
        }
    }
    source
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum DocumentationLayout {
    Wide,
    Narrow,
}

#[cfg(test)]
fn documentation_layout_for_width(width: f32) -> DocumentationLayout {
    if width >= 980.0 {
        DocumentationLayout::Wide
    } else {
        DocumentationLayout::Narrow
    }
}

pub(crate) fn spawn_documentation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let locale = effective_ui_locale(&session.config);
    let source = source_for(locale);
    let visible_source = visible_source(source, session.documentation.anchor.as_deref());
    let hits = search_documentation(locale, &session.documentation.query);

    parent
        .spawn(Node {
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100),
                    min_height: px(76),
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    padding: UiRect::axes(px(26), px(16)),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BackgroundColor(theme.card.with_alpha(0.55)),
                BorderColor::all(theme.border.with_alpha(0.55)),
            ))
            .with_children(|header| {
                spawn_text_button(
                    header,
                    font.clone(),
                    theme,
                    "Back",
                    10.0,
                    UiAction::from(AppCommand::DocumentationBack),
                );
                if !session.documentation.forward_stack.is_empty() {
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "Forward",
                        10.0,
                        UiAction::from(AppCommand::DocumentationForward),
                    );
                }
                header
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), "Documentation", 21.0, theme.foreground);
                        spawn_text(
                            copy,
                            font.clone(),
                            match locale {
                                UiLocale::English => "Offline user guide · English",
                                UiLocale::SimplifiedChinese => "离线使用说明 · 简体中文",
                                UiLocale::Japanese => "オフラインユーザーガイド · 日本語",
                            },
                            9.0,
                            theme.muted_foreground,
                        );
                    });
                header.spawn((
                    DocumentationSearchInput,
                    EditableText {
                        visible_width: Some(30.0),
                        max_characters: Some(120),
                        ..EditableText::new(session.documentation.query.as_str())
                    },
                    Node {
                        width: px(280),
                        min_height: px(34),
                        padding: UiRect::axes(px(10), px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.72)),
                    BorderColor::all(theme.border),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.foreground),
                    TextCursorStyle {
                        color: theme.primary,
                        ..default()
                    },
                ));
            });

            root.spawn(Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|body| {
                body.spawn((
                    Node {
                        width: px(245),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(18)),
                        row_gap: px(7),
                        border: UiRect::right(px(1)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    BackgroundColor(theme.card.with_alpha(0.32)),
                    BorderColor::all(theme.border.with_alpha(0.45)),
                ))
                .with_children(|toc| {
                    spawn_text(toc, font.clone(), "CONTENTS", 8.0, theme.primary);
                    for (label, anchor) in [
                        ("Getting started", "guide:getting-started"),
                        ("Analysis pipeline", "guide:analysis"),
                        ("Lyrics & language", "guide:lyrics"),
                        ("Chart editor", "guide:editor"),
                        ("Export", "guide:export"),
                        ("Documentation Center", "guide:documentation"),
                        ("Analysis artifacts", "guide:artifacts"),
                        ("Troubleshooting", "guide:troubleshooting"),
                    ] {
                        spawn_text_button(
                            toc,
                            font.clone(),
                            theme,
                            label,
                            9.0,
                            UiAction::from(AppCommand::OpenDocumentation(Some(anchor.to_string()))),
                        );
                    }
                    if !hits.is_empty() {
                        toc.spawn(Node {
                            height: px(9),
                            ..default()
                        });
                        spawn_text(toc, font.clone(), "SEARCH RESULTS", 8.0, theme.primary);
                        for hit in &hits {
                            spawn_text_button(
                                toc,
                                font.clone(),
                                theme,
                                &hit.heading,
                                9.0,
                                UiAction::from(AppCommand::OpenDocumentation(Some(format!(
                                    "heading:{}",
                                    hit.anchor
                                )))),
                            );
                        }
                    }
                });

                body.spawn((
                    DocumentationContent,
                    ScrollPosition(Vec2::new(0.0, session.documentation.scroll_offset)),
                    Node {
                        min_width: px(0),
                        flex_basis: px(480),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|scroll| {
                    scroll
                        .spawn(Node {
                            width: percent(100),
                            max_width: px(980),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::axes(px(38), px(30)),
                            ..default()
                        })
                        .with_children(|page| {
                            if let Some(anchor) = session.documentation.anchor.as_deref() {
                                page.spawn((
                                    NoRuntimeLocalization,
                                    Text::new(format!("Context: {anchor}")),
                                    ui_text_font(font.clone(), 8.0),
                                    TextColor(theme.primary),
                                ));
                                page.spawn((
                                    NoRuntimeLocalization,
                                    Node {
                                        height: px(8),
                                        ..default()
                                    },
                                ));
                            }
                            for block in parse_markdown(visible_source) {
                                render_markdown_block(page, font.clone(), theme, &block);
                            }
                        });
                });

                body.spawn((
                    Node {
                        width: px(220),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(18)),
                        row_gap: px(6),
                        border: UiRect::left(px(1)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    BackgroundColor(theme.card.with_alpha(0.24)),
                    BorderColor::all(theme.border.with_alpha(0.4)),
                ))
                .with_children(|outline| {
                    spawn_text(outline, font.clone(), "ON THIS PAGE", 8.0, theme.primary);
                    for block in parse_markdown(visible_source) {
                        let MarkdownBlock::Heading {
                            level,
                            text,
                            anchor,
                        } = block
                        else {
                            continue;
                        };
                        if level >= 3 {
                            spawn_text_button(
                                outline,
                                font.clone(),
                                theme,
                                &text,
                                8.0,
                                UiAction::from(AppCommand::OpenDocumentation(Some(format!(
                                    "heading:{anchor}"
                                )))),
                            );
                        }
                    }
                });
            });
        });
}

pub(crate) fn sync_documentation_search(
    mut shell: ResMut<ShellState>,
    inputs: Query<&EditableText, With<DocumentationSearchInput>>,
    contents: Query<&ScrollPosition, With<DocumentationContent>>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if shell.route != StudioRoute::Documentation {
        return;
    }
    let Ok(input) = inputs.single() else { return };
    if input.value() != shell.documentation.query.as_str() {
        shell.documentation.query = input.value().to_string();
        invalidated.invalidate(UiDirtyRegion::Documentation);
    }
    if let Ok(position) = contents.single()
        && (position.0.y - shell.documentation.scroll_offset).abs() > 0.5
    {
        shell.documentation.scroll_offset = position.0.y;
        if let Some(anchor) = shell.documentation.anchor.clone() {
            let offset = shell.documentation.scroll_offset;
            shell.documentation.scroll_positions.insert(anchor, offset);
        }
    }
}

pub(crate) fn handle_documentation_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut shell: ResMut<ShellState>,
    analysis: Res<AnalysisUiState>,
    editor: Res<EditorUiState>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if keys.just_pressed(KeyCode::F1) {
        let anchor = analysis
            .selected_analysis_stage
            .as_deref()
            .map(|stage| {
                let (node, _) = stage_primary_node_and_artifact(analysis_stage_index(stage));
                documentation_anchor_for_node(node).to_string()
            })
            .or_else(|| match shell.route {
                StudioRoute::Editor => Some("guide:editor".to_string()),
                StudioRoute::SongDetail => Some("guide:lyrics".to_string()),
                _ => Some("guide:getting-started".to_string()),
            });
        let origin = shell.route;
        if origin != StudioRoute::Documentation {
            shell.documentation.return_route = Some(origin);
            shell.documentation.back_stack.clear();
            shell.documentation.forward_stack.clear();
            shell.documentation.anchor = None;
        }
        shell.documentation.navigate(anchor);
        if shell.route == StudioRoute::Editor
            && editor.editor.as_ref().is_some_and(|editor| editor.dirty)
        {
            dialogs.pending_leave = Some(PendingLeave::Documentation);
        } else {
            shell.route = StudioRoute::Documentation;
        }
        invalidated.invalidate(UiDirtyRegion::Documentation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_supports_english_and_cjk_substrings() {
        assert!(!search_documentation(UiLocale::English, "analysis").is_empty());
        assert!(!search_documentation(UiLocale::SimplifiedChinese, "分析").is_empty());
        assert!(!search_documentation(UiLocale::Japanese, "解析").is_empty());
    }

    #[test]
    fn node_help_is_stable() {
        assert_eq!(
            documentation_anchor_for_node("lyrics.align"),
            "guide:lyrics"
        );
        assert_eq!(
            documentation_anchor_for_node("pitch.extract"),
            "guide:analysis"
        );
    }

    #[test]
    fn markdown_ast_removes_supported_punctuation_and_keeps_structure() {
        let blocks = parse_markdown(
            "## **Heading**\n\nA *quiet* `code` [link](https://example.com).\n\n- one\n- two\n\n```rs\nlet x = 1;\n```\n",
        );
        assert_eq!(
            blocks[0],
            MarkdownBlock::Heading {
                level: 2,
                text: "Heading".to_string(),
                anchor: "heading".to_string(),
            }
        );
        assert_eq!(
            blocks[1],
            MarkdownBlock::Paragraph("A quiet code link.".to_string())
        );
        assert_eq!(
            blocks[2],
            MarkdownBlock::UnorderedList(vec!["one".to_string(), "two".to_string()])
        );
        assert!(matches!(blocks[3], MarkdownBlock::Code { .. }));
    }

    #[test]
    fn unsupported_link_schemes_and_remote_images_are_not_rendered() {
        assert_eq!(plain_inline("[local](file:///tmp/x)"), "");
        assert_eq!(plain_inline("[script](javascript:evil)"), "");
        assert_eq!(plain_inline("![remote](https://example.com/x.png)"), "");
        assert!(!safe_link_target("file:///tmp/x"));
        assert!(!safe_link_target("javascript:evil"));
    }

    #[test]
    fn responsive_layout_switches_before_three_columns_overlap() {
        assert_eq!(
            documentation_layout_for_width(1280.0),
            DocumentationLayout::Wide
        );
        assert_eq!(
            documentation_layout_for_width(760.0),
            DocumentationLayout::Narrow
        );
    }

    #[test]
    fn documentation_history_has_real_back_and_forward_stacks() {
        let mut state = DocumentationState::default();
        state.navigate(Some("guide:analysis".to_string()));
        state.navigate(Some("guide:lyrics".to_string()));
        assert!(state.go_back());
        assert_eq!(state.anchor.as_deref(), Some("guide:analysis"));
        assert!(state.go_forward());
        assert_eq!(state.anchor.as_deref(), Some("guide:lyrics"));
    }
}
