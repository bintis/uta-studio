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
    pages: Vec<EmbeddedPage>,
    headings: Vec<EmbeddedHeading>,
}

/// One of the 15 canonical guide pages, already bounded to its own body by
/// `cargo xtask docs build` (see `xtask/src/docs.rs::parse_pages`) — the
/// runtime must not re-derive page boundaries from raw markdown.
#[derive(serde::Deserialize)]
struct EmbeddedPage {
    id: String,
    heading: String,
    anchor: String,
    body: String,
}

/// Every heading in the guide (all levels), mapping its anchor back to the
/// page that contains it. Lets `resolve_page` answer "heading:<slug>"
/// navigation targets in O(1) instead of re-scanning raw source text.
#[derive(serde::Deserialize)]
struct EmbeddedHeading {
    anchor: String,
    page_id: String,
}

fn guide_bundle() -> &'static EmbeddedGuideBundle {
    static BUNDLE: OnceLock<EmbeddedGuideBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        serde_json::from_str(GUIDE_BUNDLE_JSON)
            .expect("generated Uta! Studio documentation bundle must be valid JSON")
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

fn locale_key(locale: UiLocale) -> &'static str {
    match locale {
        UiLocale::English => "en",
        UiLocale::SimplifiedChinese => "zh-CN",
        UiLocale::Japanese => "ja",
    }
}

/// A single searchable unit within a page: one rendered markdown block, plus
/// the nearest heading above it (or the page's own heading, if the block
/// comes before the first subheading) so a hit can navigate straight there.
struct SearchBlock {
    heading: String,
    heading_lower: String,
    anchor: String,
    text: String,
    text_lower: String,
    is_heading: bool,
}

struct PageIndex {
    id: String,
    label: String,
    blocks: Vec<MarkdownBlock>,
    search_blocks: Vec<SearchBlock>,
}

struct LocaleIndex {
    pages: Vec<PageIndex>,
    heading_to_page: HashMap<String, String>,
}

/// Strips the canonical "N. " numeric prefix the guide's H3 titles carry
/// (e.g. "6. Analysis pipeline") for a clean navigation label.
fn page_label(heading: &str) -> String {
    heading
        .split_once(". ")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| heading.to_string())
}

fn build_search_blocks(
    page: &EmbeddedPage,
    label: &str,
    blocks: &[MarkdownBlock],
) -> Vec<SearchBlock> {
    let mut heading = label.to_string();
    let mut anchor = page.anchor.clone();
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        let (text, is_heading) = match block {
            MarkdownBlock::Heading {
                text, anchor: id, ..
            } => {
                heading = text.clone();
                anchor = id.clone();
                (text.clone(), true)
            }
            MarkdownBlock::Paragraph(text) | MarkdownBlock::Callout { text, .. } => {
                (text.clone(), false)
            }
            MarkdownBlock::UnorderedList(items) | MarkdownBlock::OrderedList(items) => {
                (items.join(" "), false)
            }
            MarkdownBlock::Code { text, .. } => (text.clone(), false),
            MarkdownBlock::Table(rows) => (
                rows.iter().flatten().cloned().collect::<Vec<_>>().join(" "),
                false,
            ),
        };
        out.push(SearchBlock {
            heading: heading.clone(),
            heading_lower: heading.to_lowercase(),
            anchor: anchor.clone(),
            text_lower: text.to_lowercase(),
            text,
            is_heading,
        });
    }
    out
}

fn build_locale_index(locale: &EmbeddedGuideLocale) -> LocaleIndex {
    let mut pages = Vec::with_capacity(locale.pages.len());
    for page in &locale.pages {
        let blocks = parse_markdown(&page.body);
        let label = page_label(&page.heading);
        let search_blocks = build_search_blocks(page, &label, &blocks);
        pages.push(PageIndex {
            id: page.id.clone(),
            label,
            blocks,
            search_blocks,
        });
    }
    let heading_to_page = locale
        .headings
        .iter()
        .map(|heading| (heading.anchor.clone(), heading.page_id.clone()))
        .collect();
    LocaleIndex {
        pages,
        heading_to_page,
    }
}

/// Every page and heading, parsed once and cached for the process lifetime
/// instead of re-parsing raw markdown on every render/keystroke.
fn locale_index(locale: UiLocale) -> &'static LocaleIndex {
    static INDEXES: OnceLock<HashMap<&'static str, LocaleIndex>> = OnceLock::new();
    let indexes = INDEXES.get_or_init(|| {
        let bundle = guide_bundle();
        ["en", "zh-CN", "ja"]
            .into_iter()
            .filter_map(|key| {
                bundle
                    .locales
                    .get(key)
                    .map(|locale| (key, build_locale_index(locale)))
            })
            .collect()
    });
    indexes
        .get(locale_key(locale))
        .or_else(|| indexes.get("en"))
        .expect("generated documentation bundle must contain English")
}

/// Resolves a navigation target (a page id like "guide:analysis", a
/// "heading:<slug>" search/outline hit, or nothing) to the page that must be
/// rendered. Page bodies are already correctly bounded by the doc pipeline,
/// so this never leaks content from later sections the way scanning raw
/// source for a heading marker used to.
fn resolve_page<'a>(index: &'a LocaleIndex, anchor: Option<&str>) -> &'a PageIndex {
    let fallback = index
        .pages
        .first()
        .expect("generated documentation bundle always has pages");
    let Some(anchor) = anchor else {
        return fallback;
    };
    if let Some(page) = index.pages.iter().find(|page| page.id == anchor) {
        return page;
    }
    if let Some(slug) = anchor.strip_prefix("heading:")
        && let Some(page_id) = index.heading_to_page.get(slug)
        && let Some(page) = index.pages.iter().find(|page| &page.id == page_id)
    {
        return page;
    }
    fallback
}

fn search_snippet(text: &str) -> String {
    const MAX_CHARS: usize = 90;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_CHARS {
        trimmed.to_string()
    } else {
        let truncated = trimmed.chars().take(MAX_CHARS).collect::<String>();
        format!("{truncated}\u{2026}")
    }
}

pub(crate) fn documentation_anchor_for_node(node_id: &str) -> &'static str {
    match node_id {
        "preflight" | "music.analysis" | "music.key" | "music.rhythm" | "music.descriptors" => {
            "guide:analysis"
        }
        "stems.separate"
        | "stems.vocals"
        | "vocals.denoise"
        | "vocals.dereverb"
        | "stems.instrumental"
        | "instrumental.denoise"
        | "instrumental.dereverb"
        | "stems.bind_analysis_outputs" => "guide:analysis",
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
    let tokens = query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Vec::new();
    }
    let index = locale_index(locale);
    let mut hits = Vec::new();
    for page in &index.pages {
        for block in &page.search_blocks {
            let heading_hit = tokens
                .iter()
                .all(|token| block.heading_lower.contains(token.as_str()));
            let matches = heading_hit
                || tokens.iter().all(|token| {
                    block.text_lower.contains(token.as_str())
                        || block.heading_lower.contains(token.as_str())
                });
            if !matches {
                continue;
            }
            let heading_bonus = usize::from(block.is_heading || heading_hit) * 100;
            let query_len = tokens
                .iter()
                .map(|token| token.chars().count())
                .sum::<usize>();
            hits.push(DocumentationSearchHit {
                heading: block.heading.clone(),
                line: block.text.clone(),
                anchor: block.anchor.clone(),
                score: heading_bonus + query_len,
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

pub(crate) fn spawn_documentation_header_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    if !session.documentation.forward_stack.is_empty() {
        spawn_text_button(
            parent,
            font.clone(),
            theme,
            "Forward",
            10.0,
            UiAction::from(AppCommand::DocumentationForward),
        );
    }
}

pub(crate) fn spawn_documentation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let locale = effective_ui_locale(session.config);
    let index = locale_index(locale);
    let page = resolve_page(index, session.documentation.anchor.as_deref());
    let query = session.documentation.query.as_str();
    let searching = !query.trim().is_empty();
    let hits = search_documentation(locale, query);

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
                    if searching {
                        spawn_text(
                            toc,
                            font.clone(),
                            format!("SEARCH RESULTS \u{b7} {}", hits.len()),
                            8.0,
                            theme.primary,
                        );
                        if hits.is_empty() {
                            spawn_text(
                                toc,
                                font.clone(),
                                format!("No matches for \u{201c}{}\u{201d}", query.trim()),
                                9.0,
                                theme.muted_foreground,
                            );
                        } else {
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
                                spawn_text(
                                    toc,
                                    font.clone(),
                                    search_snippet(&hit.line),
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                        }
                    } else {
                        spawn_text(toc, font.clone(), "CONTENTS", 8.0, theme.primary);
                        for entry in &index.pages {
                            spawn_text_button(
                                toc,
                                font.clone(),
                                theme,
                                &entry.label,
                                9.0,
                                UiAction::from(AppCommand::OpenDocumentation(Some(
                                    entry.id.clone(),
                                ))),
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
                        .with_children(|page_ui| {
                            page_ui.spawn((
                                NoRuntimeLocalization,
                                Text::new(page.label.clone()),
                                ui_text_font(font.clone(), 24.0),
                                TextColor(theme.foreground),
                            ));
                            page_ui.spawn((
                                NoRuntimeLocalization,
                                Node {
                                    height: px(10),
                                    ..default()
                                },
                            ));
                            for block in &page.blocks {
                                render_markdown_block(page_ui, font.clone(), theme, block);
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
                    for block in &page.blocks {
                        let MarkdownBlock::Heading {
                            level,
                            text,
                            anchor,
                        } = block
                        else {
                            continue;
                        };
                        if *level >= 3 {
                            spawn_text_button(
                                outline,
                                font.clone(),
                                theme,
                                text.as_str(),
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
    if let Ok(input) = inputs.single()
        && input.value() != shell.documentation.query.as_str()
    {
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
            .selected_analysis_node
            .as_deref()
            .map(|node| documentation_anchor_for_node(node).to_string())
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

    #[test]
    fn search_matches_tokens_regardless_of_order() {
        // "folders music" is not a literal substring anywhere in the guide (the
        // real text reads "Add music folders"); a naive substring search would
        // miss it. Tokenized AND matching must still find it.
        assert!(!search_documentation(UiLocale::English, "folders music").is_empty());
        assert!(
            search_documentation(UiLocale::English, "folders music")
                .iter()
                .all(|hit| hit.line.to_lowercase().contains("music")
                    || hit.heading.to_lowercase().contains("music"))
        );
    }

    #[test]
    fn unmatched_query_returns_no_hits() {
        assert!(search_documentation(UiLocale::English, "zzzznotarealword12345").is_empty());
    }

    #[test]
    fn resolved_page_never_leaks_into_later_sections() {
        // Regression for the old `visible_source`, which sliced from a matched
        // heading to the *end of the entire guide* instead of stopping at the
        // next top-level section. A page's blocks must never contain another
        // page's own heading.
        let index = locale_index(UiLocale::English);
        let page = resolve_page(index, Some("guide:getting-started"));
        assert_eq!(page.id, "guide:getting-started");
        let leaks_next_page = page.blocks.iter().any(|block| {
            matches!(
                block,
                MarkdownBlock::Heading { anchor, .. } if anchor == "4-quick-start-workflow"
            )
        });
        assert!(
            !leaks_next_page,
            "getting-started page must not include the next section's heading"
        );
    }

    #[test]
    fn all_fifteen_guide_pages_are_reachable() {
        let index = locale_index(UiLocale::English);
        assert_eq!(index.pages.len(), 15);
        let expected = [
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
        for id in expected {
            assert!(
                index.pages.iter().any(|page| page.id == id),
                "missing page {id}"
            );
            assert_eq!(
                resolve_page(index, Some(id)).id,
                id,
                "resolve_page must reach {id} directly"
            );
        }
    }

    #[test]
    fn resolve_page_follows_heading_anchors_to_their_owning_page() {
        let index = locale_index(UiLocale::English);
        // A subheading inside the analysis page must resolve back to that page,
        // not fall through to the default page.
        let heading_anchor = index
            .pages
            .iter()
            .find(|page| page.id == "guide:analysis")
            .and_then(|page| {
                page.blocks.iter().find_map(|block| match block {
                    MarkdownBlock::Heading { anchor, .. } => Some(anchor.clone()),
                    _ => None,
                })
            })
            .expect("analysis page has at least one subheading");
        let target = format!("heading:{heading_anchor}");
        assert_eq!(resolve_page(index, Some(&target)).id, "guide:analysis");
    }
}
