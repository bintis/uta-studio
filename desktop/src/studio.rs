use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use app_core::{
    AppConfig, LibraryFolderEntry, LibraryMenuFilters, LibrarySource, LoadSongsParams, Song,
    SongsMeta, SongsStore,
};
use bevy::{
    asset::RenderAssetUsages,
    color::Mix,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    input_focus::{AutoFocus, InputFocus, tab_navigation::TabIndex},
    log::{DEFAULT_FILTER, LogPlugin},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    window::{MonitorSelection, PrimaryWindow, WindowMode, WindowTheme},
};

use crate::theme::StudioTheme;

const FONT_PATH: &str = "desktop/assets/fonts/NotoSansCJKsc-Regular.otf";
const LOGO_PATH: &str = "icon.png";
const ICON_ATLAS_PATH: &str = "desktop/assets/icons/ui-icons.png";
const ICON_CELL: f32 = 24.0;
const SIDEBAR_WIDTH: f32 = 265.0;
const SETTINGS_CONTROL_WIDTH: f32 = 230.0;
const EDITOR_PITCH_TOP_PERCENT: f32 = 20.0;
const EDITOR_PITCH_HEIGHT_PERCENT: f32 = 76.0;
const UI_FONT_SCALE_MIN_PERCENT: u32 = 80;
const UI_FONT_SCALE_MAX_PERCENT: u32 = 140;
const UI_FONT_BASE_SIZE_PX: u32 = 12;
const UI_FONT_SIZE_MIN_PX: u32 = 10;
const UI_FONT_SIZE_MAX_PX: u32 = 18;
const UI_FONT_SIZE_STEP_PX: u32 = 1;

static GLOBAL_UI_FONT_SCALE_BITS: AtomicU32 = AtomicU32::new(f32::to_bits(1.0));

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum UiIcon {
    Home = 0,
    Queue = 1,
    CircleCheck = 2,
    Video = 3,
    Artists = 4,
    Albums = 5,
    List = 6,
    Folder = 7,
    Settings = 8,
    Monitor = 10,
    Database = 11,
    Box = 12,
    Sparkles = 13,
    ArrowLeft = 14,
    Undo = 15,
    Redo = 16,
    PanelRight = 17,
    PanelBottom = 18,
    Save = 19,
    Play = 20,
    Pause = 21,
    Add = 22,
    Scissors = 23,
    Combine = 24,
    Copy = 25,
    Clipboard = 26,
    Trash = 27,
    Grid = 28,
    ZoomOut = 29,
    ZoomIn = 30,
    ChevronDown = 31,
    Search = 32,
    Close = 34,
    Music = 35,
    Repair = 36,
    Check = 38,
    Previous = 40,
    Next = 41,
    Shuffle = 42,
    Repeat = 43,
    Volume = 44,
}

impl UiIcon {
    fn rect(self) -> Rect {
        let left = f32::from(self as u8) * ICON_CELL;
        Rect::new(left, 0.0, left + ICON_CELL, ICON_CELL)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StudioRoute {
    #[default]
    Library,
    Folders,
    SongDetail,
    Settings,
    Editor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    General,
    Storage,
    Models,
    Analysis,
}

impl SettingsTab {
    fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Storage => 1,
            Self::Models => 2,
            Self::Analysis => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSelectKind {
    ComputeBackend,
    Separator,
    SeparatorPreset,
    AsrEngine,
    WhisperModel,
    AlignBackend,
    PitchModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnalysisAdvancedSection {
    Separation,
    Transcription,
    Pitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibrarySelectKind {
    Status,
    TranscriptSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorDockSelectKind {
    AudioSource,
    SnapGrid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LibraryView {
    #[default]
    All,
    Queue,
    Completed,
    Videos,
    Artists,
    Albums,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LibraryFacet {
    Artist { value: String, label: String },
    Album { value: String, label: String },
    Playlist { value: String, label: String },
}

impl LibraryFacet {
    fn label(&self) -> &str {
        match self {
            Self::Artist { label, .. }
            | Self::Album { label, .. }
            | Self::Playlist { label, .. } => label,
        }
    }
}

impl LibraryView {
    fn title(self) -> &'static str {
        match self {
            Self::All => "Song Library",
            Self::Queue => "Analysis Queue",
            Self::Completed => "Completed Charts",
            Self::Videos => "Video Library",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
        }
    }

    fn eyebrow(self) -> &'static str {
        match self {
            Self::All => "ALL MUSIC",
            Self::Queue => "IN PROGRESS",
            Self::Completed => "READY TO AUTHOR",
            Self::Videos => "VIDEO SOURCES",
            Self::Artists | Self::Albums => "MY LIBRARY",
        }
    }

    fn filters(self) -> LibraryMenuFilters {
        LibraryMenuFilters {
            query: match self {
                Self::Queue => Some("queued".to_string()),
                Self::Completed => Some("analysed".to_string()),
                Self::Videos => Some("videos".to_string()),
                Self::All | Self::Artists | Self::Albums => None,
            },
            ..default()
        }
    }
}

#[derive(Resource)]
struct StudioSession {
    config: AppConfig,
    meta: SongsMeta,
    songs: SongsStore,
    scanning: bool,
    route: StudioRoute,
    settings_tab: SettingsTab,
    library_view: LibraryView,
    library_search: Option<String>,
    library_status: Option<String>,
    library_transcript_source: Option<String>,
    library_facet: Option<LibraryFacet>,
    menu_items: app_core::LibraryMenuItems,
    notice: Option<String>,
    selected_song: Option<String>,
    editor: Option<NativeEditor>,
    folder_browser: FolderBrowser,
    song_context: Option<SongContextMenu>,
    pending_setup: Option<SetupRequest>,
    diagnostic_report: Option<uta_studio_diagnostics::DiagnosticReport>,
    lyrics_editor: Option<NativeLyricsEditor>,
    pending_cache_delete: Option<String>,
    authoring_busy: bool,
    language_editor: Option<NativeLanguageEditor>,
    pending_cache_clear: Option<CacheClearScope>,
    pending_leave: Option<PendingLeave>,
    open_settings_select: Option<SettingsSelectKind>,
    open_analysis_advanced: Option<AnalysisAdvancedSection>,
    settings_scroll_offsets: [f32; 4],
    open_library_select: Option<LibrarySelectKind>,
    export_all_open: bool,
    open_editor_select: Option<EditorDockSelectKind>,
    analysis_tasks: Vec<app_core::AnalysisTask>,
    analysis_history: Vec<app_core::AnalysisRunHistory>,
    selected_analysis_history: Option<i64>,
    selected_analysis_stage: Option<String>,
    pending_analysis_history_clear: bool,
    request_cache_stats_refresh: bool,
    search_open: bool,
    activity_open: bool,
    about_open: bool,
    library_playback: LibraryPlayback,
    export_job: NativeExportJob,
    editor_load_job: NativeEditorLoadJob,
    lyrics_search_job: NativeLyricsSearchJob,
}

impl StudioSession {
    fn load() -> Self {
        let config = AppConfig::load();
        let folder_browser = FolderBrowser::new(&config);
        Self {
            config,
            meta: SongsStore::load_meta(),
            songs: load_songs(LibraryView::All.filters()),
            scanning: false,
            route: StudioRoute::Library,
            settings_tab: SettingsTab::General,
            library_view: LibraryView::All,
            library_search: None,
            library_status: None,
            library_transcript_source: None,
            library_facet: None,
            menu_items: app_core::load_library_menu_items().unwrap_or_default(),
            notice: None,
            selected_song: None,
            editor: None,
            folder_browser,
            song_context: None,
            pending_setup: None,
            diagnostic_report: None,
            lyrics_editor: None,
            pending_cache_delete: None,
            authoring_busy: false,
            language_editor: None,
            pending_cache_clear: None,
            pending_leave: None,
            open_settings_select: None,
            open_analysis_advanced: None,
            settings_scroll_offsets: [0.0; 4],
            open_library_select: None,
            export_all_open: false,
            open_editor_select: None,
            analysis_tasks: app_core::load_analysis_tasks(),
            analysis_history: app_core::load_analysis_history(100),
            selected_analysis_history: None,
            selected_analysis_stage: None,
            pending_analysis_history_clear: false,
            request_cache_stats_refresh: false,
            search_open: false,
            activity_open: false,
            about_open: false,
            library_playback: LibraryPlayback::default(),
            export_job: NativeExportJob::default(),
            editor_load_job: NativeEditorLoadJob::default(),
            lyrics_search_job: NativeLyricsSearchJob::default(),
        }
    }

    fn refresh_library(&mut self) {
        self.meta = SongsStore::load_meta();
        self.songs = load_songs(self.library_filters());
        self.menu_items = app_core::load_library_menu_items().unwrap_or_default();
    }

    fn load_more_songs(&mut self) {
        let next = SongsStore::load(&LoadSongsParams {
            search: None,
            filters: self.library_filters(),
            skip: self.songs.processed.len(),
            take: 500,
        });
        self.songs.processed.extend(next.processed);
        self.songs.count = next.count;
        self.songs.processed_count = self.songs.processed.len();
    }

    fn selected_song(&self) -> Option<Song> {
        let hash = self.selected_song.as_deref()?;
        app_core::load_song_by_hash(hash).ok().flatten()
    }

    fn library_filters(&self) -> LibraryMenuFilters {
        let mut filters = self.library_view.filters();
        filters.search = self.library_search.clone();
        filters.status = self.library_status.clone();
        filters.transcript_source = self.library_transcript_source.clone();
        match self.library_facet.as_ref() {
            Some(LibraryFacet::Artist { value, .. }) => filters.artist = Some(value.clone()),
            Some(LibraryFacet::Album { value, .. }) => filters.album = Some(value.clone()),
            Some(LibraryFacet::Playlist { value, .. }) => filters.playlist = Some(value.clone()),
            None => {}
        }
        filters
    }

    fn library_title(&self) -> &str {
        self.library_facet
            .as_ref()
            .map(LibraryFacet::label)
            .unwrap_or_else(|| self.library_view.title())
    }
}

#[derive(Clone, Copy)]
struct SetupRequest {
    target: Option<app_core::ModelDownloadTarget>,
}

#[derive(Clone)]
struct FolderContextMenu {
    entry: LibraryFolderEntry,
    position: Vec2,
}

#[derive(Clone)]
struct SongContextMenu {
    song: Song,
    position: Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LyricsInputMode {
    Plain,
    TimedLrc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheClearScope {
    Generated,
    Models,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingLeave {
    Exit,
    Back,
    Home,
}

struct NativeLyricsEditor {
    file_hash: String,
    mode: LyricsInputMode,
    separate_stems: bool,
    initial_text: String,
    candidates: Vec<app_core::LrclibCandidate>,
    candidate_index: usize,
    searching: bool,
}

struct NativeLanguageEditor {
    file_hash: String,
    initial_language: String,
    force_transcribe: bool,
    picker_open: bool,
}

const ANALYSIS_LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("auto", "Automatic detection"),
    ("ja", "Japanese"),
    ("en", "English"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("id", "Indonesian"),
    ("vi", "Vietnamese"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
    ("nl", "Dutch"),
    ("sv", "Swedish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

fn canonical_analysis_language(language: &str) -> String {
    match language.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "jp" | "jpn" => "ja".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        "chi" | "zho" | "cn" | "zh-cn" | "zh-tw" => "zh".into(),
        language
            if ANALYSIS_LANGUAGE_OPTIONS
                .iter()
                .any(|(code, _)| *code == language) =>
        {
            language.to_string()
        }
        _ => "auto".into(),
    }
}

fn analysis_language_label(language: &str) -> &'static str {
    ANALYSIS_LANGUAGE_OPTIONS
        .iter()
        .find_map(|(code, label)| (*code == language).then_some(*label))
        .unwrap_or("Automatic detection")
}

#[derive(Default)]
struct FolderBrowser {
    root: Option<PathBuf>,
    current: Option<PathBuf>,
    entries: Vec<LibraryFolderEntry>,
    error: Option<String>,
    context_menu: Option<FolderContextMenu>,
    pending_remove: Option<PathBuf>,
}

impl FolderBrowser {
    fn new(config: &AppConfig) -> Self {
        let root = config.library_paths().into_iter().next();
        let mut browser = Self {
            root: root.clone(),
            current: root,
            ..default()
        };
        browser.refresh();
        browser
    }

    fn select_root(&mut self, path: PathBuf) {
        self.root = Some(path.clone());
        self.current = Some(path);
        self.context_menu = None;
        self.refresh();
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        let Some(current) = self.current.as_deref() else {
            return;
        };
        match app_core::list_library_folder(current) {
            Ok(entries) => self.entries = entries,
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn parent(&self) -> Option<PathBuf> {
        let root = self.root.as_deref()?;
        let current = self.current.as_deref()?;
        if current == root {
            return None;
        }
        let parent = current.parent()?;
        parent.starts_with(root).then(|| parent.to_path_buf())
    }
}

struct NativeEditor {
    chart: app_core::ChartDocument,
    waveform: app_core::ChartWaveform,
    audio_source: String,
    visible_position: f64,
    audio_status: uta_studio_audio::EditorAudioStatus,
    last_audio_sync: Instant,
    viewport_start: f64,
    viewport_duration: f64,
    pitch_min: f64,
    pitch_max: f64,
    lyrics_hidden: bool,
    inspector_open: bool,
    selected_note: Option<usize>,
    selected_notes: BTreeSet<usize>,
    selected_word: Option<WordSelection>,
    selected_words: BTreeSet<WordSelection>,
    word_edit_focus: Option<WordSelection>,
    snap_seconds: f64,
    dirty: bool,
    manual_scroll_until: Instant,
    undo: Vec<ChartSnapshot>,
    redo: Vec<ChartSnapshot>,
    clipboard_notes: Vec<serde_json::Value>,
}

struct LibraryPlayback {
    file_hash: Option<String>,
    visible_position: f64,
    status: uta_studio_audio::EditorAudioStatus,
    last_audio_sync: Instant,
    queue: Vec<String>,
    queue_index: Option<usize>,
    queue_open: bool,
    shuffle: bool,
    shuffle_seed: u64,
    repeat: LibraryRepeatMode,
    volume: f64,
    volume_before_mute: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LibraryRepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl LibraryRepeatMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat queue",
            Self::One => "Repeat one",
        }
    }
}

impl Default for LibraryPlayback {
    fn default() -> Self {
        Self {
            file_hash: None,
            visible_position: 0.0,
            status: uta_studio_audio::EditorAudioStatus::default(),
            last_audio_sync: Instant::now(),
            queue: Vec::new(),
            queue_index: None,
            queue_open: false,
            shuffle: false,
            shuffle_seed: 0x9e37_79b9_7f4a_7c15,
            repeat: LibraryRepeatMode::Off,
            volume: 0.8,
            volume_before_mute: 0.8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct WordSelection {
    segment: usize,
    word: usize,
}

#[derive(Clone)]
struct ChartSnapshot {
    transcript: serde_json::Value,
    pitch_notes: serde_json::Value,
}

impl NativeEditor {
    fn new(
        chart: app_core::ChartDocument,
        audio_status: uta_studio_audio::EditorAudioStatus,
        waveform: app_core::ChartWaveform,
        audio_source: impl Into<String>,
    ) -> Self {
        let notes = chart_notes(&chart);
        let pitch_min = notes
            .iter()
            .map(|note| note.midi)
            .reduce(f64::min)
            .unwrap_or(48.0)
            .floor()
            - 2.0;
        let pitch_max = notes
            .iter()
            .map(|note| note.midi)
            .reduce(f64::max)
            .unwrap_or(72.0)
            .ceil()
            + 2.0;
        Self {
            chart,
            waveform,
            audio_source: audio_source.into(),
            visible_position: audio_status.position_secs,
            audio_status,
            last_audio_sync: Instant::now(),
            viewport_start: 0.0,
            viewport_duration: 12.0,
            pitch_min,
            pitch_max: pitch_max.max(pitch_min + 12.0),
            lyrics_hidden: false,
            inspector_open: false,
            selected_note: None,
            selected_notes: BTreeSet::new(),
            selected_word: None,
            selected_words: BTreeSet::new(),
            word_edit_focus: None,
            snap_seconds: 0.05,
            dirty: false,
            manual_scroll_until: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard_notes: Vec::new(),
        }
    }

    fn viewport_end(&self) -> f64 {
        self.viewport_start + self.viewport_duration
    }

    fn snapshot(&self) -> ChartSnapshot {
        ChartSnapshot {
            transcript: self.chart.transcript.clone(),
            pitch_notes: self.chart.pitch_notes.clone(),
        }
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, snapshot: ChartSnapshot) {
        self.chart.transcript = snapshot.transcript;
        self.chart.pitch_notes = snapshot.pitch_notes;
        self.selected_note = None;
        self.selected_notes.clear();
        self.selected_word = None;
        self.selected_words.clear();
        self.word_edit_focus = None;
        self.dirty = true;
    }

    fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    fn select_only_note(&mut self, index: usize) {
        self.selected_note = Some(index);
        self.selected_notes.clear();
        self.selected_notes.insert(index);
        self.selected_word = None;
        self.selected_words.clear();
        self.word_edit_focus = None;
    }

    fn selected_note_indices(&self) -> BTreeSet<usize> {
        if self.selected_notes.is_empty() {
            self.selected_note.into_iter().collect()
        } else {
            self.selected_notes.clone()
        }
    }

    fn select_only_word(&mut self, selection: WordSelection) {
        if self.selected_word != Some(selection) {
            self.word_edit_focus = None;
        }
        self.selected_word = Some(selection);
        self.selected_words.clear();
        self.selected_words.insert(selection);
        self.selected_note = None;
        self.selected_notes.clear();
    }

    fn selected_word_indices(&self) -> BTreeSet<WordSelection> {
        if self.selected_words.is_empty() {
            self.selected_word.into_iter().collect()
        } else {
            self.selected_words.clone()
        }
    }
}

#[derive(Clone, Debug)]
struct ChartNoteView {
    index: usize,
    start: f64,
    end: f64,
    midi: f64,
    confidence: f64,
    kind: String,
}

#[derive(Clone, Debug)]
struct ChartPitchFrame {
    time: f64,
    midi: f64,
    confidence: f64,
}

#[derive(Clone, Debug)]
struct ChartLyricView {
    segment: usize,
    word: usize,
    start: f64,
    end: f64,
    text: String,
    lane: usize,
    guided: bool,
}

fn load_songs(filters: LibraryMenuFilters) -> SongsStore {
    SongsStore::load(&LoadSongsParams {
        search: None,
        filters,
        skip: 0,
        take: 500,
    })
}

#[derive(Resource)]
struct NativeAudio(#[allow(dead_code)] Arc<uta_studio_audio::EditorAudioPlayer>);

#[derive(Resource)]
struct NativeLibraryAudio(Arc<uta_studio_audio::EditorAudioPlayer>);

#[derive(Resource, Default)]
struct LocalImages {
    covers: HashMap<PathBuf, Handle<Image>>,
    ambient_covers: HashMap<PathBuf, Handle<Image>>,
}

#[derive(Resource, Default)]
struct UiInvalidated(bool);

/// The authored background of a button before transient hover/press feedback.
///
/// UI rebuilds create buttons with intentionally different resting surfaces
/// (transparent text actions, quiet outlined controls, primary actions, and
/// full-screen dismiss backdrops). Keeping that value prevents interaction
/// feedback from flattening every button to the same transparent background.
#[derive(Component, Clone, Copy)]
struct RestingButtonBackground(Color);

#[derive(Resource)]
struct LibraryRefreshTimer(Timer);

#[derive(Resource)]
struct AnalysisRefreshTimer(Timer);

#[derive(Resource)]
struct EditorAudioSyncTimer(Timer);

#[derive(Resource)]
struct LibraryAudioSyncTimer(Timer);

#[derive(Resource, Default)]
struct NativeSetup {
    receiver: Option<Mutex<mpsc::Receiver<SetupEvent>>>,
    progress: Option<app_core::SetupProgress>,
    logs: Vec<String>,
}

enum SetupEvent {
    Progress(app_core::SetupProgress),
    Log(String),
    Complete(Result<(), String>),
}

#[derive(Resource, Default)]
struct NativeDiagnostics {
    receiver: Option<Mutex<mpsc::Receiver<uta_studio_diagnostics::DiagnosticReport>>>,
}

#[derive(Resource, Default)]
struct NativeAuthoringJob {
    receiver: Option<Mutex<mpsc::Receiver<AuthoringEvent>>>,
}

#[derive(Default)]
struct NativeExportJob {
    receiver: Option<Mutex<mpsc::Receiver<String>>>,
}

#[derive(Default)]
struct NativeEditorLoadJob {
    receiver: Option<Mutex<mpsc::Receiver<Result<NativeEditor, String>>>>,
}

#[derive(Default)]
struct NativeLyricsSearchJob {
    receiver: Option<Mutex<mpsc::Receiver<Vec<app_core::LrclibCandidate>>>>,
}

#[derive(Resource, Default)]
struct CacheStatsJob {
    receiver: Option<Mutex<mpsc::Receiver<app_core::CacheStats>>>,
    current: Option<app_core::CacheStats>,
    error: Option<String>,
}

struct AuthoringEvent {
    result: Result<app_core::ShiftResult, String>,
    kind: &'static str,
}

#[derive(Component)]
struct StudioUiRoot;

#[derive(Component)]
struct EditorPlayhead;

#[derive(Component)]
struct EditorClockText;

#[derive(Component)]
struct LibraryPlayerProgress;

#[derive(Component)]
struct LibraryPlayerClockText;

#[derive(Component)]
struct EditorTimelineSurface;

#[derive(Component)]
struct EditorLyricsSurface;

#[derive(Component)]
struct EditorNoteNode(usize);

#[derive(Clone, Copy)]
enum NoteEdge {
    Start,
    End,
}

#[derive(Component)]
struct EditorNoteResizeHandle {
    index: usize,
    edge: NoteEdge,
}

#[derive(Component)]
struct EditorLyricNode {
    selection: WordSelection,
}

#[derive(Component)]
struct EditorLyricResizeHandle {
    selection: WordSelection,
    edge: NoteEdge,
}

#[derive(Component)]
struct FolderEntryList;

#[derive(Component)]
struct SettingsContent;

#[derive(Component, Clone, Copy)]
enum NumericSetting {
    SeparatorSegmentSize,
    SeparatorOverlap,
    SeparatorBatchSize,
    SeparatorNormalization,
    DemucsShifts,
    DemucsOverlap,
    BeamSize,
    BatchSize,
    VocalThreshold,
}

#[derive(Component)]
struct LibrarySongList;

#[derive(Component)]
struct SongDetailContent;

#[derive(Component)]
struct LyricsEditorInput;

#[derive(Component)]
struct LanguageEditorInput;

#[derive(Component)]
struct LibrarySearchInput;

#[derive(Component)]
struct EditorWordInput(WordSelection);

#[derive(Component)]
struct InlineEditorWordInput;

#[derive(Resource, Default)]
struct EditorPointerCapture {
    drag: Option<EditorDrag>,
    last_surface_click: Option<(Instant, Vec2)>,
    last_lyric_click: Option<(Instant, WordSelection)>,
}

#[derive(Clone)]
enum EditorDrag {
    Note {
        pointer_start: Vec2,
        originals: Vec<EditorNoteOriginal>,
        viewport_duration: f64,
        pitch_span: f64,
    },
    ResizeNote {
        index: usize,
        edge: NoteEdge,
        pointer_start: Vec2,
        original_start: f64,
        original_end: f64,
        viewport_duration: f64,
    },
    Lyric {
        pointer_start: Vec2,
        originals: Vec<EditorWordOriginal>,
        viewport_duration: f64,
    },
    ResizeLyric {
        selection: WordSelection,
        edge: NoteEdge,
        pointer_start: Vec2,
        original_start: f64,
        original_end: f64,
        viewport_duration: f64,
    },
    Pan {
        pointer_start: Vec2,
        viewport_start: f64,
        pitch_min: f64,
        pitch_max: f64,
    },
    Marquee {
        pointer_start: Vec2,
        base: BTreeSet<usize>,
        viewport_start: f64,
        viewport_duration: f64,
        pitch_min: f64,
        pitch_max: f64,
    },
}

#[derive(Clone)]
struct EditorNoteOriginal {
    index: usize,
    start: f64,
    end: f64,
    midi: f64,
}

#[derive(Clone)]
struct EditorWordOriginal {
    selection: WordSelection,
    start: f64,
    end: f64,
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
enum UiAction {
    Back,
    Home,
    SetLibraryView(LibraryView),
    SetLibraryFacet(LibraryFacet),
    LoadMoreSongs,
    ApplyLibrarySearch,
    ClearLibrarySearch,
    ToggleLibraryLayout,
    ToggleExportAllMenu,
    ExportAllUtz,
    ExportAllUltraStar,
    ToggleGlobalSearch,
    OpenLibrarySelect(LibrarySelectKind),
    SelectLibraryValue(LibrarySelectKind, String),
    AnalyzeAll,
    Folders,
    Settings,
    ToggleActivity,
    CloseActivity,
    SelectAnalysisHistory(Option<i64>),
    SelectAnalysisStage(String),
    RequestClearAnalysisHistory,
    CancelClearAnalysisHistory,
    ConfirmClearAnalysisHistory,
    OpenAbout,
    CloseAbout,
    SettingsTab(SettingsTab),
    ToggleFullscreen,
    OpenLog,
    RunDiagnostics,
    RefreshRuntimeStatus,
    OpenSettingsSelect(SettingsSelectKind),
    SelectSettingsValue(SettingsSelectKind, String),
    ToggleAnalysisAdvanced(AnalysisAdvancedSection),
    RequestSetup(Option<app_core::ModelDownloadTarget>),
    CancelSetup,
    ConfirmSetup,
    RescanLibrary,
    ToggleTheme,
    ChooseFolder,
    ChooseExportFolder,
    ClearExportFolder,
    SelectFolderRoot(PathBuf),
    FolderUp,
    OpenFolderEntry(PathBuf),
    RevealFolderEntry(PathBuf),
    DismissFolderContext,
    RequestRemoveFolder(PathBuf),
    CancelRemoveFolder,
    ConfirmRemoveFolder,
    AdjustBeamSize(i8),
    AdjustBatchSize(i8),
    AdjustSeparatorSegmentSize(i32),
    AdjustSeparatorOverlap(i32),
    AdjustSeparatorBatchSize(i32),
    AdjustSeparatorNormalization(i32),
    AdjustDemucsShifts(i32),
    AdjustDemucsOverlap(i32),
    AdjustUiFontScale(i8),
    ToggleAutoAnalyze,
    AdjustVocalThreshold(i8),
    RestoreAnalysisDefaults,
    RequestClearCache(CacheClearScope),
    CancelClearCache,
    ConfirmClearCache,
    OpenSong(String),
    AnalyzeSong(String),
    OpenEditor(String),
    ExportUtz(String),
    ExportUltraStar(String),
    OpenSource(PathBuf),
    RevealSource(PathBuf),
    DismissSongContext,
    OpenLyricsEditor(String),
    CloseLyricsEditor,
    ToggleLyricsInputMode,
    ToggleLyricsSeparateStems,
    SearchLrclibLyrics,
    PreviousLrclibCandidate,
    NextLrclibCandidate,
    UseLrclibPlain,
    UseLrclibTimed,
    SaveLyricsEditor,
    OpenLanguageEditor(String),
    CloseLanguageEditor,
    ToggleLanguageReprocess,
    ToggleLanguagePicker,
    SelectAnalysisLanguage(String),
    SaveLanguageEditor,
    RealignSong(String),
    ReanalyzeTranscript(String),
    ForceTranscribe(String),
    ReanalyzePitch(String),
    ReanalyzeFull(String),
    RequestDeleteSongCache(String),
    CancelDeleteSongCache,
    ConfirmDeleteSongCache,
    CancelLeave,
    ConfirmLeave,
    ShiftSongKey(String, i8),
    ShiftSongTempo(String, i8),
    PlayLibrarySong(String),
    ToggleLibraryPlayback,
    SeekLibraryRelative(i8),
    PreviousLibrarySong,
    NextLibrarySong,
    ToggleLibraryShuffle,
    CycleLibraryRepeat,
    AdjustLibraryVolume(i8),
    ToggleLibraryMute,
    ToggleLibraryQueue,
    TogglePlayback,
    SeekEditorStart,
    OpenEditorSelect(EditorDockSelectKind),
    SelectEditorValue(EditorDockSelectKind, String),
    ToggleLyrics,
    ToggleInspector,
    SaveEditor,
    EditorUndo,
    EditorRedo,
    AddEditorNote,
    DeleteEditorNote,
    SplitEditorNote,
    MergeEditorNotes,
    QuantizeEditorNotes,
    DuplicateEditorNotes,
    RepairEditorChart,
    AdjustEditorTimeZoom(i8),
    PanEditorPitch(i8),
    AdjustEditorPitchZoom(i8),
    ShiftWholeChart(i8),
    CopyEditorNote,
    PasteEditorNote,
    CycleEditorNoteKind,
    SelectEditorWord(usize, usize, u64),
    AdjustEditorWordStart(i8),
    AdjustEditorWordEnd(i8),
    AddEditorWord,
    DeleteEditorWord,
    ShiftEditorWord(i8),
    SplitEditorWord,
    MergeEditorWord,
    SplitEditorPhrase,
    MergeEditorPhrase,
}

pub fn run() {
    let session = StudioSession::load();
    let native_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let native_library_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let theme = StudioTheme::new(session.config.dark_mode.unwrap_or(false));
    set_ui_font_scale(session.config.font_scale());
    let window = studio_window(&session.config, theme.dark);

    App::new()
        .insert_resource(ClearColor(theme.background))
        .insert_resource(theme)
        .insert_resource(session)
        .insert_resource(NativeAudio(native_audio))
        .insert_resource(NativeLibraryAudio(native_library_audio))
        .insert_resource(LocalImages::default())
        .insert_resource(EditorPointerCapture::default())
        .insert_resource(UiInvalidated::default())
        .insert_resource(LibraryRefreshTimer(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .insert_resource(AnalysisRefreshTimer(Timer::from_seconds(
            0.75,
            TimerMode::Repeating,
        )))
        .insert_resource(EditorAudioSyncTimer(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        )))
        .insert_resource(LibraryAudioSyncTimer(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        )))
        .insert_resource(NativeSetup::default())
        .insert_resource(NativeDiagnostics::default())
        .insert_resource(NativeAuthoringJob::default())
        .insert_resource(CacheStatsJob::default())
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    // Parley 0.9 asks ICU for non-complex word segmentation even
                    // for no-wrap labels. ICU 2.2 logs that expected fallback once
                    // per CJK text node; keep real ICU errors while avoiding that
                    // misleading warning storm in the native shell.
                    filter: studio_log_filter(),
                    ..default()
                })
                .set(AssetPlugin {
                    // During the transition, use the canonical repository logo
                    // and the same bundled CJK font as the current desktop UI.
                    // Keeping the source paths explicit also makes the later
                    // package asset-copy step auditable.
                    file_path: asset_root(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(window),
                    close_when_requested: false,
                    ..default()
                }),
        )
        .add_systems(Startup, setup)
        .add_systems(Update, handle_actions)
        .add_systems(Update, handle_cache_stats_request)
        .add_systems(Update, handle_window_close_requests)
        .add_systems(Update, handle_fullscreen_shortcut)
        .add_systems(Update, refresh_library_while_scanning)
        .add_systems(Update, refresh_analysis_activity)
        .add_systems(Update, poll_native_setup)
        .add_systems(Update, poll_native_diagnostics)
        .add_systems(Update, poll_cache_stats)
        .add_systems(Update, poll_authoring_job)
        .add_systems(Update, poll_export_job)
        .add_systems(Update, poll_editor_load_job)
        .add_systems(Update, poll_lyrics_search_job)
        .add_systems(Update, sync_numeric_settings)
        .add_systems(Update, sync_editor_word_input)
        .add_systems(Update, finish_inline_lyric_edit)
        .add_systems(Update, handle_library_search_keyboard)
        .add_systems(Update, rebuild_ui)
        .add_systems(Update, update_button_visuals)
        .add_systems(Update, handle_editor_keyboard)
        .add_systems(Update, handle_editor_wheel)
        .add_systems(Update, handle_editor_pointer_capture)
        .add_systems(Update, handle_folder_scroll)
        .add_systems(Update, handle_library_scroll)
        .add_systems(Update, handle_song_detail_scroll)
        .add_systems(Update, handle_settings_scroll)
        .add_systems(Update, sync_editor_audio)
        .add_systems(Update, sync_library_audio)
        .add_systems(Update, update_editor_geometry)
        .add_systems(Update, update_editor_playhead)
        .add_systems(Update, update_library_player_ui)
        .run();
}

fn studio_log_filter() -> String {
    format!("{DEFAULT_FILTER},icu_provider=error")
}

fn asset_root() -> String {
    if let Some(path) = std::env::var_os("UTA_STUDIO_ASSET_PATH") {
        return path.to_string_lossy().into_owned();
    }

    if let Ok(executable) = std::env::current_exe()
        && let Some(prefix) = executable.parent().and_then(std::path::Path::parent)
    {
        let packaged = prefix.join("share/uta-studio");
        if packaged.join(LOGO_PATH).is_file() && packaged.join(FONT_PATH).is_file() {
            return packaged.to_string_lossy().into_owned();
        }
    }

    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("desktop crate must remain inside the Uta Studio workspace")
        .to_string_lossy()
        .into_owned()
}

fn studio_window(config: &AppConfig, dark: bool) -> Window {
    Window {
        title: "Uta Studio".to_string(),
        name: Some("com.uta-studio.desktop".to_string()),
        resolution: (1280, 720).into(),
        decorations: true,
        transparent: false,
        resizable: true,
        mode: if config.fullscreen.unwrap_or(false) {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        },
        window_theme: Some(if dark {
            WindowTheme::Dark
        } else {
            WindowTheme::Light
        }),
        ..default()
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut local_images: ResMut<LocalImages>,
    session: Res<StudioSession>,
    native_setup: Res<NativeSetup>,
    cache_stats: Res<CacheStatsJob>,
    theme: Res<StudioTheme>,
) {
    commands.spawn(Camera2d);
    render_ui(
        &mut commands,
        &asset_server,
        &mut images,
        &mut local_images,
        &session,
        &native_setup,
        &cache_stats,
        &theme,
    );
}

// Bevy systems expose each independently tracked resource/query as a parameter.
#[allow(clippy::too_many_arguments)]
fn rebuild_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut local_images: ResMut<LocalImages>,
    session: Res<StudioSession>,
    native_setup: Res<NativeSetup>,
    cache_stats: Res<CacheStatsJob>,
    theme: Res<StudioTheme>,
    mut invalidated: ResMut<UiInvalidated>,
    roots: Query<Entity, With<StudioUiRoot>>,
) {
    if !invalidated.0 {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    render_ui(
        &mut commands,
        &asset_server,
        &mut images,
        &mut local_images,
        &session,
        &native_setup,
        &cache_stats,
        &theme,
    );
    invalidated.0 = false;
}

fn render_ui(
    commands: &mut Commands,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    let font = asset_server.load(FONT_PATH);
    let icons = asset_server.load(ICON_ATLAS_PATH);
    commands
        .spawn((
            StudioUiRoot,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|root| {
            if session.route == StudioRoute::Editor {
                spawn_editor(root, font.clone(), icons.clone(), session, theme);
            } else {
                root.spawn(Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    spawn_sidebar(
                        body,
                        font.clone(),
                        icons.clone(),
                        asset_server,
                        session,
                        theme,
                    );
                    spawn_workspace(
                        body,
                        font.clone(),
                        asset_server,
                        images,
                        local_images,
                        session,
                        native_setup,
                        cache_stats,
                        icons.clone(),
                        theme,
                    );
                });
            }
            if session.activity_open {
                spawn_activity_center(root, font.clone(), icons.clone(), session, theme);
            }
            if session.about_open {
                spawn_about_dialog(root, font.clone(), asset_server, theme);
            }
            if let Some(destination) = session.pending_leave {
                spawn_leave_confirmation(root, font, theme, session, destination);
            }
        });
}

fn spawn_leave_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    destination: PendingLeave,
) {
    let dirty = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let (title, action) = match destination {
        PendingLeave::Exit => ("Close Uta Studio?", "Close"),
        PendingLeave::Back | PendingLeave::Home => ("Leave the editor?", "Leave"),
    };
    let description = if dirty {
        "This chart has unsaved edits. Leaving now discards those edits. Source media is never changed."
    } else {
        "A scan, setup, diagnostic, or rendering task is still active. Closing now interrupts that work. Source media is never changed."
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.8)),
        ZIndex(120),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(description),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelLeave,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Stay"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmLeave,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new(action),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn spawn_icon(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    icon: UiIcon,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::new(atlas)
            .with_rect(icon.rect())
            .with_color(color),
        Pickable::IGNORE,
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_icon_button(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    action: UiAction,
    active: bool,
    destructive: bool,
    size: f32,
) {
    let color = if destructive {
        theme.destructive
    } else if active {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    parent
        .spawn((
            Button,
            action,
            Node {
                width: px(size),
                height: px(size),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if active {
                theme.foreground.with_alpha(0.07)
            } else {
                Color::NONE
            }),
        ))
        .with_children(|button| spawn_icon(button, atlas, icon, 16.0, color));
}

fn spawn_activity_button(
    parent: &mut ChildSpawnerCommands,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    panel_open: bool,
    has_active_analysis: bool,
) {
    let emphasized = panel_open || has_active_analysis;
    let color = if has_active_analysis {
        theme.primary
    } else if panel_open {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    parent
        .spawn((
            Node {
                width: px(34),
                height: px(34),
                flex_shrink: 0.0,
                border: UiRect::all(px(if emphasized { 1 } else { 0 })),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if has_active_analysis {
                theme.primary.with_alpha(0.075)
            } else if panel_open {
                theme.foreground.with_alpha(0.07)
            } else {
                Color::NONE
            }),
            BorderColor::all(if has_active_analysis {
                theme.primary.with_alpha(0.18)
            } else {
                theme.border.with_alpha(0.42)
            }),
        ))
        .with_children(|slot| {
            slot.spawn((
                Button,
                UiAction::ToggleActivity,
                Node {
                    width: percent(100),
                    height: percent(100),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|button| spawn_icon(button, atlas, UiIcon::Queue, 16.0, color));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_toolbar_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    atlas: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: impl Into<String>,
    action: UiAction,
    destructive: bool,
) {
    let color = if destructive {
        theme.destructive
    } else {
        theme.foreground
    };
    parent
        .spawn((
            Button,
            action,
            Node {
                height: px(32),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(9)),
                column_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.44)),
        ))
        .with_children(|button| {
            spawn_icon(button, atlas, icon, 14.0, color);
            spawn_text(button, font, label, 9.0, color);
        });
}

fn spawn_sidebar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: px(SIDEBAR_WIDTH),
                height: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(10), px(16)),
                ..default()
            },
            BackgroundColor(theme.sidebar),
        ))
        .with_children(|sidebar| {
            sidebar
                .spawn(Node {
                    width: percent(100),
                    height: px(82),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn((
                            Button,
                            UiAction::OpenAbout,
                            Node {
                                height: px(68),
                                align_items: AlignItems::Center,
                                padding: UiRect::right(px(8)),
                                column_gap: px(10),
                                ..default()
                            },
                        ))
                        .with_children(|brand| {
                            brand.spawn((
                                Node {
                                    width: px(66),
                                    height: px(66),
                                    flex_shrink: 0.0,
                                    overflow: Overflow::clip(),
                                    border_radius: BorderRadius::all(px(12)),
                                    ..default()
                                },
                                ImageNode::new(asset_server.load(LOGO_PATH)),
                            ));
                            spawn_text(
                                brand,
                                font.clone(),
                                "Uta Studio",
                                18.0,
                                theme.sidebar_foreground,
                            );
                        });
                    header.spawn(Node {
                        min_width: px(4),
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon_button(
                        header,
                        icons.clone(),
                        theme,
                        UiIcon::Settings,
                        UiAction::Settings,
                        false,
                        false,
                        30.0,
                    );
                });

            spawn_section_label(sidebar, font.clone(), theme, "BROWSE");
            let analysis_count = session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                    )
                })
                .count();
            for (view, icon, label, count) in [
                (
                    LibraryView::All,
                    UiIcon::Home,
                    "All music",
                    session.meta.songs_count,
                ),
                (
                    LibraryView::Queue,
                    UiIcon::Queue,
                    "Analysis Queue",
                    analysis_count,
                ),
                (
                    LibraryView::Completed,
                    UiIcon::CircleCheck,
                    "Completed Charts",
                    session.meta.analyzed_count,
                ),
                (
                    LibraryView::Videos,
                    UiIcon::Video,
                    "Video Library",
                    session.meta.videos_count,
                ),
            ] {
                spawn_sidebar_filter_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    Some(icon),
                    label,
                    count,
                    UiAction::SetLibraryView(view),
                    session.route == StudioRoute::Library && session.library_view == view,
                );
            }
            spawn_section_label(sidebar, font.clone(), theme, "MY LIBRARY");
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Artists,
                "Artists",
                UiAction::SetLibraryView(LibraryView::Artists),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Artists
                    && session.library_facet.is_none(),
            );
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Albums,
                "Albums",
                UiAction::SetLibraryView(LibraryView::Albums),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Albums
                    && session.library_facet.is_none(),
            );
            if !session.menu_items.playlists.is_empty() {
                spawn_sidebar_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::List,
                    "Playlists",
                    0,
                );
                for item in session.menu_items.playlists.iter().take(2) {
                    let facet = LibraryFacet::Playlist {
                        value: item.value.clone(),
                        label: item.label.clone(),
                    };
                    spawn_sidebar_filter_item(
                        sidebar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        None,
                        format!("  {}", item.label),
                        item.count.try_into().unwrap_or(usize::MAX),
                        UiAction::SetLibraryFacet(facet.clone()),
                        session.route == StudioRoute::Library
                            && session.library_facet.as_ref() == Some(&facet),
                    );
                }
            }
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Folder,
                "Folders",
                UiAction::Folders,
                session.route == StudioRoute::Folders,
            );
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Settings,
                "Settings",
                UiAction::Settings,
                session.route == StudioRoute::Settings,
            );
            sidebar.spawn(Node {
                min_height: px(14),
                flex_grow: 1.0,
                ..default()
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_sidebar_filter_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: Option<UiIcon>,
    label: impl Into<String>,
    count: usize,
    action: UiAction,
    active: bool,
) {
    let label = label.into();
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(32),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            if let Some(icon) = icon {
                spawn_icon(
                    row,
                    icons,
                    icon,
                    15.0,
                    if active {
                        theme.primary
                    } else {
                        theme.sidebar_foreground.with_alpha(0.62)
                    },
                );
                row.spawn(Node {
                    width: px(9),
                    ..default()
                });
            }
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|label_node| {
                label_node.spawn((
                    Text::new(label),
                    ui_text_font(font.clone(), 12.0),
                    TextColor(if active {
                        theme.sidebar_foreground.with_alpha(0.68)
                    } else {
                        theme.sidebar_foreground
                    }),
                    TextLayout::no_wrap(),
                ));
            });
            if count > 0 {
                row.spawn(Node {
                    width: px(28),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|count_node| {
                    spawn_text(
                        count_node,
                        font,
                        count.to_string(),
                        10.0,
                        theme.sidebar_foreground.with_alpha(0.38),
                    );
                });
            }
        });
}

fn spawn_section_label(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
) {
    parent.spawn((
        Node {
            margin: UiRect::new(px(8), px(0), px(18), px(8)),
            ..default()
        },
        children![(
            Text::new(label),
            ui_text_font(font, 9.0),
            TextColor(theme.sidebar_foreground.with_alpha(0.42)),
        )],
    ));
}

fn spawn_sidebar_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    count: usize,
) {
    parent
        .spawn(Node {
            width: percent(100),
            height: px(32),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            ..default()
        })
        .with_children(|row| {
            spawn_icon(
                row,
                icons,
                icon,
                15.0,
                theme.sidebar_foreground.with_alpha(0.58),
            );
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(row, font.clone(), label, 12.0, theme.sidebar_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if count > 0 {
                spawn_text(
                    row,
                    font,
                    count.to_string(),
                    10.0,
                    theme.sidebar_foreground.with_alpha(0.38),
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_sidebar_nav_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    action: UiAction,
    active: bool,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(32),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            spawn_icon(
                row,
                icons,
                icon,
                15.0,
                if active {
                    theme.primary
                } else {
                    theme.sidebar_foreground.with_alpha(0.62)
                },
            );
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(
                row,
                font,
                label,
                12.0,
                if active {
                    theme.sidebar_foreground.with_alpha(0.68)
                } else {
                    theme.sidebar_foreground
                },
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_workspace(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    icons: Handle<Image>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|workspace| {
            spawn_top_bar(workspace, font.clone(), icons.clone(), session, theme);
            match session.route {
                StudioRoute::Library if session.config.library_paths().is_empty() => {
                    spawn_empty_library(workspace, font.clone(), session.scanning, theme);
                }
                StudioRoute::Library
                    if session.library_facet.is_none()
                        && matches!(
                            session.library_view,
                            LibraryView::Artists | LibraryView::Albums
                        ) =>
                {
                    spawn_library_collection(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::Library => spawn_library(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Folders => {
                    spawn_folders(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::SongDetail => spawn_song_detail(
                    workspace,
                    font.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Settings => spawn_settings(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    session,
                    native_setup,
                    cache_stats,
                    theme,
                ),
                StudioRoute::Editor => {}
            }
            spawn_library_player(
                workspace,
                font,
                icons,
                asset_server,
                images,
                local_images,
                session,
                theme,
            );
        });
}

fn spawn_top_bar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(56),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(12),
                    right: px(12),
                    ..default()
                },
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.72)),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|bar| {
            spawn_icon_button(
                bar,
                icons.clone(),
                theme,
                UiIcon::ArrowLeft,
                UiAction::Back,
                false,
                false,
                34.0,
            );
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(title) = match session.route {
                StudioRoute::Folders => Some("Folders"),
                StudioRoute::SongDetail => Some("Song"),
                StudioRoute::Editor => Some("Editor"),
                StudioRoute::Library | StudioRoute::Settings => None,
            } {
                spawn_text(bar, font.clone(), title, 11.0, theme.muted_foreground);
            }
            bar.spawn(Node {
                position_type: PositionType::Relative,
                width: px(34),
                height: px(34),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|search| {
                spawn_icon_button(
                    search,
                    icons.clone(),
                    theme,
                    UiIcon::Search,
                    UiAction::ToggleGlobalSearch,
                    session.search_open || session.library_search.is_some(),
                    false,
                    34.0,
                );
                if session.search_open {
                    search
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(0),
                                top: px(40),
                                width: px(410),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(12)),
                                row_gap: px(9),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(7)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.98)),
                            BorderColor::all(theme.border.with_alpha(0.86)),
                            BoxShadow::new(
                                Color::srgba(0.0, 0.0, 0.0, 0.24),
                                px(0),
                                px(14),
                                px(30),
                                px(-10),
                            ),
                            ZIndex(75),
                        ))
                        .with_children(|popover| {
                            popover.spawn((
                                LibrarySearchInput,
                                EditableText {
                                    visible_width: Some(38.0),
                                    max_characters: Some(120),
                                    ..EditableText::new(
                                        session.library_search.as_deref().unwrap_or(""),
                                    )
                                },
                                Node {
                                    width: percent(100),
                                    height: px(42),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(12)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                ui_text_font(font.clone(), 12.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                                TextCursorStyle {
                                    color: theme.primary,
                                    selected_text_color: Some(theme.primary_foreground),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.72)),
                                BorderColor::all(theme.border.with_alpha(0.68)),
                                TabIndex(0),
                                AutoFocus,
                            ));
                            popover
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|footer| {
                                    spawn_wrapped_text(
                                        footer,
                                        font.clone(),
                                        "Search tracks, artists, albums, and playlists · Enter",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                    footer.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    if session.library_search.is_some() {
                                        spawn_text_button(
                                            footer,
                                            font.clone(),
                                            theme,
                                            "Clear",
                                            9.0,
                                            UiAction::ClearLibrarySearch,
                                        );
                                    }
                                    spawn_text_button(
                                        footer,
                                        font.clone(),
                                        theme,
                                        "Search",
                                        9.0,
                                        UiAction::ApplyLibrarySearch,
                                    );
                                });
                        });
                }
            });
            let has_active_analysis = session.analysis_tasks.iter().any(|task| {
                matches!(
                    task.status,
                    app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                )
            });
            spawn_activity_button(
                bar,
                icons.clone(),
                theme,
                session.activity_open,
                has_active_analysis,
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_library_player(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let playback_song = session
        .library_playback
        .file_hash
        .as_deref()
        .and_then(|hash| app_core::load_song_by_hash(hash).ok().flatten());
    let Some(song) = playback_song.or_else(|| session.selected_song()) else {
        return;
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(82),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(18)),
                column_gap: px(14),
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.72)),
            BorderColor::all(theme.border.with_alpha(0.52)),
        ))
        .with_children(|player| {
            let cover = album_art_handle(&song, asset_server, images, local_images);
            let current = session.library_playback.file_hash.as_deref()
                == Some(song.file_hash.as_str())
                && session.library_playback.status.loaded;
            let position = if current {
                library_visible_position(&session.library_playback)
            } else {
                0.0
            };
            let duration = if current && session.library_playback.status.duration_secs > 0.0 {
                session.library_playback.status.duration_secs
            } else {
                song.duration_secs
            };
            player
                .spawn(Node {
                    width: px(300),
                    min_width: px(180),
                    flex_shrink: 1.0,
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    ..default()
                })
                .with_children(|now_playing| {
                    now_playing.spawn((
                        Node {
                            width: px(52),
                            height: px(52),
                            flex_shrink: 0.0,
                            overflow: Overflow::clip(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        ImageNode::new(cover),
                        BorderColor::all(theme.border.with_alpha(0.58)),
                    ));
                    now_playing
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|identity| {
                            identity.spawn((
                                Text::new(song.title.clone()),
                                ui_text_font(font.clone(), 11.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            identity.spawn((
                                Text::new(if song.artist.trim().is_empty() {
                                    "Unknown artist".to_string()
                                } else {
                                    song.artist.clone()
                                }),
                                ui_text_font(font.clone(), 9.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                });
            player
                .spawn(Node {
                    min_width: px(280),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|transport| {
                    transport
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            column_gap: px(6),
                            ..default()
                        })
                        .with_children(|controls| {
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Shuffle,
                                UiAction::ToggleLibraryShuffle,
                                session.library_playback.shuffle,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Previous,
                                UiAction::PreviousLibrarySong,
                                false,
                                false,
                                30.0,
                            );
                            if current {
                                spawn_text_button(
                                    controls,
                                    font.clone(),
                                    theme,
                                    "−10",
                                    9.0,
                                    UiAction::SeekLibraryRelative(-10),
                                );
                            }
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                if current && session.library_playback.status.playing {
                                    UiIcon::Pause
                                } else {
                                    UiIcon::Play
                                },
                                if current {
                                    UiAction::ToggleLibraryPlayback
                                } else {
                                    UiAction::PlayLibrarySong(song.file_hash.clone())
                                },
                                true,
                                false,
                                34.0,
                            );
                            if current {
                                spawn_text_button(
                                    controls,
                                    font.clone(),
                                    theme,
                                    "+10",
                                    9.0,
                                    UiAction::SeekLibraryRelative(10),
                                );
                            }
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Next,
                                UiAction::NextLibrarySong,
                                false,
                                false,
                                30.0,
                            );
                            spawn_icon_button(
                                controls,
                                icons.clone(),
                                theme,
                                UiIcon::Repeat,
                                UiAction::CycleLibraryRepeat,
                                session.library_playback.repeat != LibraryRepeatMode::Off,
                                false,
                                30.0,
                            );
                            if session.library_playback.repeat == LibraryRepeatMode::One {
                                spawn_text(controls, font.clone(), "1", 7.0, theme.primary);
                            }
                        });
                    transport
                        .spawn(Node {
                            width: percent(100),
                            max_width: px(560),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        })
                        .with_children(|timeline| {
                            timeline
                                .spawn((
                                    Node {
                                        min_width: px(100),
                                        height: px(3),
                                        flex_grow: 1.0,
                                        overflow: Overflow::clip(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted.with_alpha(0.82)),
                                ))
                                .with_children(|track| {
                                    track.spawn((
                                        LibraryPlayerProgress,
                                        Node {
                                            width: percent(
                                                ((position / duration.max(0.001)) * 100.0)
                                                    .clamp(0.0, 100.0)
                                                    as f32,
                                            ),
                                            height: percent(100),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(theme.primary.with_alpha(0.88)),
                                    ));
                                });
                            timeline.spawn((
                                LibraryPlayerClockText,
                                Text::new(format_editor_clock(position, duration)),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                });
            let source_format = song
                .path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(str::to_ascii_uppercase)
                .unwrap_or_else(|| "AUDIO".to_string());
            player
                .spawn(Node {
                    width: px(270),
                    min_width: px(190),
                    flex_shrink: 1.0,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(5),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|source| {
                    source
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: px(5),
                            ..default()
                        })
                        .with_children(|quality| {
                            spawn_text(
                                quality,
                                font.clone(),
                                "ORIGINAL",
                                7.0,
                                theme.muted_foreground.with_alpha(0.72),
                            );
                            spawn_text(
                                quality,
                                font.clone(),
                                source_format,
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    source
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: px(2),
                            ..default()
                        })
                        .with_children(|output| {
                            spawn_icon_button(
                                output,
                                icons.clone(),
                                theme,
                                UiIcon::Volume,
                                UiAction::ToggleLibraryMute,
                                session.library_playback.volume == 0.0,
                                false,
                                28.0,
                            );
                            spawn_text_button(
                                output,
                                font.clone(),
                                theme,
                                "−",
                                10.0,
                                UiAction::AdjustLibraryVolume(-5),
                            );
                            spawn_text(
                                output,
                                font.clone(),
                                format!("{}%", (session.library_playback.volume * 100.0).round()),
                                8.0,
                                theme.muted_foreground,
                            );
                            spawn_text_button(
                                output,
                                font.clone(),
                                theme,
                                "+",
                                10.0,
                                UiAction::AdjustLibraryVolume(5),
                            );
                            spawn_icon_button(
                                output,
                                icons.clone(),
                                theme,
                                UiIcon::Queue,
                                UiAction::ToggleLibraryQueue,
                                session.library_playback.queue_open,
                                false,
                                30.0,
                            );
                        });
                });
        });

    if session.library_playback.queue_open {
        spawn_library_play_queue(parent, font, icons, session, theme);
    }
}

fn spawn_library_play_queue(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(14),
                bottom: px(90),
                width: px(390),
                max_height: px(390),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(7),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.88)),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.26),
                px(0),
                px(14),
                px(32),
                px(-10),
            ),
            ZIndex(85),
        ))
        .with_children(|queue| {
            queue
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons.clone(), UiIcon::Queue, 15.0, theme.primary);
                    spawn_text(header, font.clone(), "Play queue", 14.0, theme.foreground);
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{} tracks", session.library_playback.queue.len()),
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_icon_button(
                        header,
                        icons.clone(),
                        theme,
                        UiIcon::Close,
                        UiAction::ToggleLibraryQueue,
                        false,
                        false,
                        28.0,
                    );
                });
            if session.library_playback.queue.is_empty() {
                spawn_wrapped_text(
                    queue,
                    font,
                    "Play a song to create a queue from the current library view.",
                    10.0,
                    theme.muted_foreground,
                );
                return;
            }
            let current = session.library_playback.queue_index.unwrap_or(0);
            let first = current.saturating_sub(1);
            for (index, file_hash) in session
                .library_playback
                .queue
                .iter()
                .enumerate()
                .skip(first)
                .take(8)
            {
                let song = session
                    .songs
                    .processed
                    .iter()
                    .find(|song| song.file_hash == *file_hash)
                    .cloned()
                    .or_else(|| app_core::load_song_by_hash(file_hash).ok().flatten());
                let (title, artist) = song
                    .map(|song| (song.title, song.artist))
                    .unwrap_or_else(|| ("Unavailable track".to_string(), String::new()));
                let active = index == current;
                queue
                    .spawn((
                        Button,
                        UiAction::PlayLibrarySong(file_hash.clone()),
                        Node {
                            width: percent(100),
                            min_height: px(38),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(9)),
                            column_gap: px(9),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(if active {
                            theme.primary.with_alpha(0.1)
                        } else {
                            Color::NONE
                        }),
                    ))
                    .with_children(|row| {
                        spawn_text(
                            row,
                            font.clone(),
                            if active { "NOW" } else { "NEXT" },
                            7.0,
                            if active {
                                theme.primary
                            } else {
                                theme.muted_foreground.with_alpha(0.7)
                            },
                        );
                        row.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|identity| {
                            identity.spawn((
                                Text::new(title),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            identity.spawn((
                                Text::new(artist),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            ));
                        });
                    });
            }
        });
}

fn spawn_activity_center(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseActivity,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.54)),
        ZIndex(100),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(420),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20)),
                row_gap: px(12),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.9)),
            ZIndex(101),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(9),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons, UiIcon::Queue, 17.0, theme.primary);
                    spawn_text(header, font.clone(), "Activity", 18.0, theme.foreground);
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::CloseActivity,
                    );
                });
            spawn_wrapped_text(
                panel,
                font.clone(),
                "Live analysis work and the most recent native operation.",
                10.0,
                theme.muted_foreground,
            );
            panel.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.64)),
            ));
            spawn_text(
                panel,
                font.clone(),
                format!("JOBS  ·  {}", session.analysis_tasks.len()),
                9.0,
                theme.muted_foreground,
            );
            if session.analysis_tasks.is_empty() {
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            padding: UiRect::all(px(18)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.55)),
                    ))
                    .with_children(|empty| {
                        spawn_wrapped_text(
                            empty,
                            font.clone(),
                            "Nothing is running. Requested analyses and failures appear here.",
                            10.0,
                            theme.muted_foreground,
                        );
                    });
            } else {
                for task in session.analysis_tasks.iter().take(10) {
                    let (status, progress, failed) = analysis_status_copy(&task.status);
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(11)),
                                row_gap: px(4),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.36)),
                            BorderColor::all(if failed {
                                theme.destructive.with_alpha(0.62)
                            } else {
                                theme.border.with_alpha(0.58)
                            }),
                        ))
                        .with_children(|card| {
                            card.spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.title.clone(),
                                        11.0,
                                        theme.foreground,
                                    );
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.artist.clone(),
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                });
                                spawn_text(
                                    row,
                                    font.clone(),
                                    status,
                                    9.0,
                                    if failed {
                                        theme.destructive
                                    } else {
                                        theme.primary
                                    },
                                );
                            });
                            if let Some(live) = task.live.as_ref() {
                                spawn_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}%", live.operation, live.stage_progress),
                                    9.0,
                                    theme.primary,
                                );
                                spawn_wrapped_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}", live.implementation, live.detail),
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                            if let Some(progress) = progress {
                                card.spawn((
                                    Node {
                                        position_type: PositionType::Relative,
                                        width: percent(100),
                                        height: px(3),
                                        margin: UiRect::top(px(4)),
                                        overflow: Overflow::clip(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted),
                                ))
                                .with_children(|rail| {
                                    rail.spawn((
                                        Node {
                                            width: percent(progress.clamp(0, 100) as f32),
                                            height: percent(100),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(theme.primary),
                                    ));
                                });
                            }
                        });
                }
            }
            panel.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(notice) = session.notice.as_deref() {
                spawn_wrapped_text(panel, font.clone(), notice, 9.0, theme.muted_foreground);
            }
            spawn_action_button(
                panel,
                font,
                theme,
                "Open analysis queue",
                UiAction::SetLibraryView(LibraryView::Queue),
            );
        });
}

fn analysis_status_copy(status: &app_core::QueuedStatus) -> (String, Option<usize>, bool) {
    match status {
        app_core::QueuedStatus::Queued => ("Queued".to_string(), None, false),
        app_core::QueuedStatus::Analyzing(progress) => {
            (format!("Analyzing · {progress}%"), Some(*progress), false)
        }
        app_core::QueuedStatus::Failed(message) => (
            if message.trim().is_empty() {
                "Failed".to_string()
            } else {
                format!("Failed · {message}")
            },
            None,
            true,
        ),
    }
}

fn analysis_stage_index(stage: &str) -> usize {
    match stage {
        "preparing" | "key_detection" => 0,
        "separation" => 1,
        "pitch" => 2,
        "audio_preprocessing" => 3,
        "transcription" => 4,
        "alignment" => 5,
        "finalizing" | "complete" => 6,
        _ => 0,
    }
}

fn analysis_stage_matches(route_stage: &str, selected_stage: &str) -> bool {
    route_stage == selected_stage
        || (selected_stage == "preparing" && route_stage == "key_detection")
        || (selected_stage == "finalizing" && route_stage == "complete")
}

fn analysis_stage_details(stage: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match stage {
        "preparing" => (
            "Prepare",
            "Validates the source, resolves analysis settings, and detects musical context before model execution.",
            "Authorized source media and analysis profile",
            "Validated audio, runtime plan, tempo and key context",
        ),
        "separation" => (
            "Separate",
            "Extracts a vocal-focused stem while preserving the original source unchanged.",
            "Validated source audio",
            "Lossless vocal and instrumental analysis stems",
        ),
        "pitch" => (
            "Pitch",
            "Tracks the sung fundamental frequency and converts the contour into editable note guidance.",
            "Separated vocal stem",
            "Pitch contour and note candidates",
        ),
        "audio_preprocessing" => (
            "Preprocess",
            "Normalizes the analysis signal and prepares model-specific audio windows without rewriting source media.",
            "Vocal analysis stem",
            "Model-ready audio windows and vocal regions",
        ),
        "transcription" => (
            "Transcribe",
            "Recognizes lyric text and produces the timing evidence supported by the selected speech model.",
            "Preprocessed vocal regions and language preference",
            "Recognized lyric tokens and provisional timestamps",
        ),
        "alignment" => (
            "Align",
            "Refines recognized or supplied lyrics against the audio into editor-ready character and word timing.",
            "Lyrics, provisional timestamps, and vocal audio",
            "Character and word-level aligned lyrics",
        ),
        "finalizing" => (
            "Finalize",
            "Validates and commits generated analysis assets before the song becomes available for authoring.",
            "Aligned lyrics, pitch data, metadata, and stems",
            "Cached chart analysis and library metadata",
        ),
        _ => (
            "Analysis step",
            "Executes one stage of the configured analysis pipeline.",
            "Previous stage output",
            "Next stage input",
        ),
    }
}

fn spawn_analysis_session_overview(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let active_task = session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .find(|task| matches!(task.status, app_core::QueuedStatus::Queued))
        });
    let history = session
        .selected_analysis_history
        .and_then(|id| session.analysis_history.iter().find(|history| history.id == id))
        .or_else(|| active_task.is_none().then(|| session.analysis_history.first()).flatten());
    let history_task = history.map(|history| app_core::AnalysisTask {
        file_hash: history.file_hash.clone(),
        title: history.title.clone(),
        artist: history.artist.clone(),
        status: app_core::QueuedStatus::Analyzing(if history.status == "completed" {
            100
        } else {
            0
        }),
        live: Some(history.snapshot.clone()),
    });
    let Some(task) = history_task.as_ref().or(active_task) else {
        return;
    };
    let viewing_history = history_task.is_some();

    let progress = match &task.status {
        app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
        _ => 0,
    };
    let stage = task
        .live
        .as_ref()
        .map(|live| live.stage.as_str())
        .unwrap_or("preparing");
    let stage_index = analysis_stage_index(stage);
    let operation = task
        .live
        .as_ref()
        .map(|live| live.operation.as_str())
        .unwrap_or("Waiting for the analysis runtime");
    let detail = task
        .live
        .as_ref()
        .map(|live| live.detail.as_str())
        .unwrap_or("The task is queued and will start when the current analysis completes.");
    let selected_stage = session.selected_analysis_stage.as_deref().unwrap_or(stage);
    let selected_stage_index = analysis_stage_index(selected_stage);
    let selected_route = task.live.as_ref().and_then(|live| {
        live.stage_routes
            .iter()
            .rev()
            .find(|route| analysis_stage_matches(&route.stage, selected_stage))
    });
    let selected_is_current = analysis_stage_matches(stage, selected_stage);
    let selected_progress = selected_route
        .map(|route| route.stage_progress.clamp(0, 100))
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.stage_progress.clamp(0, 100))
                    .unwrap_or(0)
            })
        })
        .unwrap_or_else(|| if selected_stage_index < stage_index { 100 } else { 0 });
    let selected_trace_missing = selected_route.is_none() && selected_progress >= 100;
    let selected_pending_copy = if selected_trace_missing {
        "Not recorded in this analysis session"
    } else {
        "Pending"
    };
    let (selected_label, selected_purpose, selected_input, selected_output) =
        analysis_stage_details(selected_stage);
    let selected_status = if selected_progress >= 100 {
        "COMPLETE"
    } else if selected_is_current {
        "RUNNING"
    } else if selected_stage_index < stage_index {
        "COMPLETE"
    } else {
        "WAITING"
    };
    let selected_operation = selected_route
        .map(|route| route.operation.as_str())
        .or_else(|| selected_is_current.then_some(operation))
        .unwrap_or("This step has not started yet.");
    let selected_implementation = selected_route
        .map(|route| route.implementation.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.implementation.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_model = selected_route
        .map(|route| route.model.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.model.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_requested_device = selected_route
        .map(|route| route.requested_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.requested_device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_actual_device = selected_route
        .map(|route| route.actual_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_device_fallback = selected_route
        .and_then(|route| route.fallback_from.as_deref().zip(route.fallback_reason.as_deref()));
    let selected_backend_fallback = selected_route.and_then(|route| {
        route
            .backend_fallback_from
            .as_deref()
            .zip(route.backend_fallback_reason.as_deref())
    });
    let history_error = history.and_then(|history| history.error_message.as_deref());

    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(30), px(26)),
                row_gap: px(16),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|session_card| {
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(20),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                if viewing_history {
                                    "ANALYSIS SESSION HISTORY"
                                } else {
                                    "LIVE ANALYSIS SESSION"
                                },
                                9.0,
                                theme.primary,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                task.title.clone(),
                                25.0,
                                theme.foreground,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                task.artist.clone(),
                                11.0,
                                theme.muted_foreground,
                            );
                        });
                    if viewing_history && active_task.is_some() {
                        spawn_text_button(
                            header,
                            font.clone(),
                            theme,
                            "View live",
                            9.0,
                            UiAction::SelectAnalysisHistory(None),
                        );
                    }
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{progress:02}%"),
                        30.0,
                        theme.foreground,
                    );
                });

            session_card
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|current| {
                    spawn_text(
                        current,
                        font.clone(),
                        "CURRENT OPERATION",
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_text(current, font.clone(), operation, 18.0, theme.foreground);
                    spawn_wrapped_text(
                        current,
                        font.clone(),
                        detail,
                        10.0,
                        theme.muted_foreground,
                    );
                    if let Some(live) = task.live.as_ref() {
                        if let Some(fallback_from) = live.fallback_from.as_deref() {
                            current.spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                column_gap: px(10),
                                margin: UiRect::top(px(8)),
                                ..default()
                            })
                            .with_children(|route| {
                                spawn_text(
                                    route,
                                    font.clone(),
                                    "EXECUTION FALLBACK",
                                    8.0,
                                    theme.editor_warning,
                                );
                                route
                                    .spawn((
                                        Node {
                                            min_width: px(58),
                                            padding: UiRect::axes(px(10), px(6)),
                                            justify_content: JustifyContent::Center,
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.editor_warning.with_alpha(0.08)),
                                        BorderColor::all(theme.editor_warning.with_alpha(0.48)),
                                    ))
                                    .with_children(|source| {
                                        spawn_text(
                                            source,
                                            font.clone(),
                                            fallback_from.to_ascii_uppercase(),
                                            9.0,
                                            theme.editor_warning,
                                        );
                                    });
                                route.spawn((
                                    Node {
                                        width: px(34),
                                        height: px(2),
                                        ..default()
                                    },
                                    BackgroundColor(theme.editor_warning.with_alpha(0.68)),
                                ));
                                spawn_text(
                                    route,
                                    font.clone(),
                                    ">",
                                    10.0,
                                    theme.editor_warning,
                                );
                                route
                                    .spawn((
                                        Node {
                                            min_width: px(58),
                                            padding: UiRect::axes(px(10), px(6)),
                                            justify_content: JustifyContent::Center,
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.pitch_contour.with_alpha(0.09)),
                                        BorderColor::all(theme.pitch_contour.with_alpha(0.52)),
                                    ))
                                    .with_children(|destination| {
                                        spawn_text(
                                            destination,
                                            font.clone(),
                                            live.device.to_ascii_uppercase(),
                                            9.0,
                                            theme.pitch_contour,
                                        );
                                    });
                                if let Some(reason) = live.fallback_reason.as_deref() {
                                    spawn_wrapped_text(
                                        route,
                                        font.clone(),
                                        reason,
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                }
                            });
                        }
                    }
                });

            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(5),
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.72)),
                ))
                .with_children(|rail| {
                    rail.spawn((
                        Node {
                            width: percent(progress as f32),
                            height: percent(100),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                    ));
                });

            let stages = [
                ("preparing", "Prepare"),
                ("separation", "Separate"),
                ("pitch", "Pitch"),
                ("audio_preprocessing", "Preprocess"),
                ("transcription", "Transcribe"),
                ("alignment", "Align"),
                ("finalizing", "Finalize"),
            ];
            let active_stage_progress = task
                .live
                .as_ref()
                .map(|live| live.stage_progress.clamp(0, 100))
                .unwrap_or(0);
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|track| {
                    for (index, (stage_id, label)) in stages.into_iter().enumerate() {
                        if index > 0 {
                            track.spawn((
                                Node {
                                    min_width: px(12),
                                    height: px(2),
                                    flex_grow: 0.16,
                                    margin: UiRect::top(px(25)),
                                    ..default()
                                },
                                BackgroundColor(if index <= stage_index {
                                    theme.pitch_contour.with_alpha(0.72)
                                } else {
                                    theme.border.with_alpha(0.55)
                                }),
                            ));
                        }
                        let completed = index < stage_index;
                        let active = index == stage_index;
                        let step_progress = if completed {
                            100
                        } else if active {
                            active_stage_progress
                        } else {
                            0
                        };
                        let stage_route = task.live.as_ref().and_then(|live| {
                            live.stage_routes.iter().rev().find(|route| {
                                analysis_stage_matches(&route.stage, stage_id)
                            })
                        });
                        let selected = selected_stage == stage_id;
                        track
                            .spawn((
                                Button,
                                UiAction::SelectAnalysisStage(stage_id.to_string()),
                                Node {
                                    min_width: px(108),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(11)),
                                    row_gap: px(9),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.primary.with_alpha(0.18)
                                } else if active {
                                    theme.primary.with_alpha(0.12)
                                } else {
                                    theme.background.with_alpha(0.24)
                                }),
                                BorderColor::all(if selected {
                                    theme.primary.with_alpha(0.88)
                                } else if active {
                                    theme.primary.with_alpha(0.62)
                                } else if completed {
                                    theme.pitch_contour.with_alpha(0.42)
                                } else {
                                    theme.border.with_alpha(0.42)
                                }),
                            ))
                            .with_children(|step| {
                                step.spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(7),
                                    ..default()
                                })
                                .with_children(|heading| {
                                    heading.spawn((
                                        Node {
                                            width: px(22),
                                            height: px(22),
                                            flex_shrink: 0.0,
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(if active {
                                            theme.primary
                                        } else if completed {
                                            theme.pitch_contour
                                        } else {
                                            theme.muted
                                        }),
                                    ))
                                    .with_children(|badge| {
                                        spawn_text(
                                            badge,
                                            font.clone(),
                                            format!("{:02}", index + 1),
                                            7.0,
                                            if active || completed {
                                                theme.background
                                            } else {
                                                theme.muted_foreground
                                            },
                                        );
                                    });
                                    heading
                                        .spawn(Node {
                                            min_width: px(0),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            ..default()
                                        })
                                        .with_children(|copy| {
                                            spawn_text(
                                                copy,
                                                font.clone(),
                                                label,
                                                9.0,
                                                if active || completed {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                            spawn_text(
                                                copy,
                                                font.clone(),
                                                if active {
                                                    "RUNNING".to_string()
                                                } else if completed {
                                                    "COMPLETE".to_string()
                                                } else {
                                                    "WAITING".to_string()
                                                },
                                                7.0,
                                                if active {
                                                    theme.primary
                                                } else if completed {
                                                    theme.pitch_contour
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                        });
                                });
                                step.spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|meter| {
                                    meter
                                        .spawn((
                                            Node {
                                                min_width: px(0),
                                                height: px(3),
                                                flex_grow: 1.0,
                                                overflow: Overflow::clip(),
                                                border_radius: BorderRadius::MAX,
                                                ..default()
                                            },
                                            BackgroundColor(theme.muted.with_alpha(0.72)),
                                        ))
                                        .with_children(|rail| {
                                            rail.spawn((
                                                Node {
                                                    width: percent(step_progress as f32),
                                                    height: percent(100),
                                                    border_radius: BorderRadius::MAX,
                                                    ..default()
                                                },
                                                BackgroundColor(if completed {
                                                    theme.pitch_contour
                                                } else {
                                                    theme.primary
                                                }),
                                            ));
                                        });
                                    spawn_text(
                                        meter,
                                        font.clone(),
                                        format!("{step_progress}%"),
                                        8.0,
                                        if active || completed {
                                            theme.foreground
                                        } else {
                                            theme.muted_foreground
                                        },
                                    );
                                });
                                let route_copy = stage_route
                                    .map(|route| {
                                        route
                                            .fallback_from
                                            .as_ref()
                                            .map(|from| {
                                                format!(
                                                    "{} > {}",
                                                    from.to_ascii_uppercase(),
                                                    route.actual_device.to_ascii_uppercase()
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                route.actual_device.to_ascii_uppercase()
                                            })
                                    })
                                    .unwrap_or_else(|| {
                                        if completed {
                                            "TELEMETRY NOT RECORDED".to_string()
                                        } else {
                                            "NOT STARTED".to_string()
                                        }
                                    });
                                spawn_text(
                                    step,
                                    font.clone(),
                                    route_copy,
                                    7.0,
                                    if stage_route
                                        .is_some_and(|route| route.fallback_from.is_some())
                                    {
                                        theme.editor_warning
                                    } else if stage_route.is_some() {
                                        theme.pitch_contour
                                    } else {
                                        theme.muted_foreground
                                    },
                                );
                                let backend_copy = stage_route
                                    .map(|route| {
                                        route
                                            .backend_fallback_from
                                            .as_ref()
                                            .map(|from| {
                                                format!(
                                                    "{} > {}",
                                                    from.to_ascii_uppercase(),
                                                    route.implementation.to_ascii_uppercase()
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                if route.model.trim().is_empty() {
                                                    route.implementation.clone()
                                                } else {
                                                    format!("{} · {}", route.implementation, route.model)
                                                }
                                            })
                                    })
                                    .unwrap_or_else(|| {
                                        if completed {
                                            "LEGACY SESSION · NO MODEL TRACE".to_string()
                                        } else {
                                            "MODEL PENDING".to_string()
                                        }
                                    });
                                spawn_wrapped_text(
                                    step,
                                    font.clone(),
                                    backend_copy,
                                    7.0,
                                    if stage_route.is_some_and(|route| {
                                        route.backend_fallback_from.is_some()
                                    }) {
                                        theme.editor_warning
                                    } else {
                                        theme.muted_foreground
                                    },
                                );
                            });
                    }
                });

            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(16)),
                        row_gap: px(12),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.34)),
                    BorderColor::all(theme.primary.with_alpha(0.38)),
                ))
                .with_children(|inspector| {
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(5),
                            ..default()
                        })
                        .with_children(|header| {
                            spawn_text(
                                header,
                                font.clone(),
                                format!("STEP {:02} · {}", selected_stage_index + 1, selected_label.to_ascii_uppercase()),
                                9.0,
                                theme.primary,
                            );
                            header.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                header,
                                font.clone(),
                                format!("{selected_status} · {selected_progress}%"),
                                9.0,
                                if selected_status == "WAITING" {
                                    theme.muted_foreground
                                } else {
                                    theme.pitch_contour
                                },
                            );
                        });
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_purpose,
                        10.0,
                        theme.muted_foreground,
                    );
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_operation,
                        13.0,
                        theme.foreground,
                    );
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(9),
                            row_gap: px(9),
                            ..default()
                        })
                        .with_children(|facts| {
                            for (label, value) in [
                                ("IMPLEMENTATION", selected_implementation),
                                ("MODEL / ALGORITHM", selected_model),
                                ("REQUESTED DEVICE", selected_requested_device),
                                ("ACTUAL DEVICE", selected_actual_device),
                                ("INPUT", selected_input),
                                ("OUTPUT", selected_output),
                            ] {
                                facts
                                    .spawn((
                                        Node {
                                            min_width: px(205),
                                            flex_basis: px(240),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::all(px(10)),
                                            row_gap: px(3),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.34)),
                                        BorderColor::all(theme.border.with_alpha(0.4)),
                                    ))
                                    .with_children(|fact| {
                                        spawn_text(
                                            fact,
                                            font.clone(),
                                            label,
                                            7.0,
                                            theme.muted_foreground,
                                        );
                                        spawn_wrapped_text(
                                            fact,
                                            font.clone(),
                                            value,
                                            9.0,
                                            theme.foreground,
                                        );
                                    });
                            }
                        });
                    for (label, from, to, reason) in selected_device_fallback
                        .map(|(from, reason)| {
                            ("COMPUTE FALLBACK", from, selected_actual_device, reason)
                        })
                        .into_iter()
                        .chain(selected_backend_fallback.map(|(from, reason)| {
                            ("MODEL FALLBACK", from, selected_implementation, reason)
                        }))
                    {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!("{label} · {} > {} · {reason}", from.to_ascii_uppercase(), to.to_ascii_uppercase()),
                            9.0,
                            theme.editor_warning,
                        );
                    }
                    if let Some(error) = history_error {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!("SESSION ERROR · {error}"),
                            9.0,
                            theme.destructive,
                        );
                    }
                });

            if let Some(live) = task.live.as_ref() {
                session_card
                    .spawn(Node {
                        width: percent(100),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(10),
                        row_gap: px(10),
                        ..default()
                    })
                    .with_children(|details| {
                        let device_route = live
                            .fallback_from
                            .as_ref()
                            .map(|from| {
                                format!(
                                    "{} > {}",
                                    from.to_ascii_uppercase(),
                                    live.device.to_ascii_uppercase()
                                )
                            })
                            .unwrap_or_else(|| live.device.to_ascii_uppercase());
                        for (label, value) in [
                            ("IMPLEMENTATION", live.implementation.clone()),
                            ("MODEL / ALGORITHM", live.model.clone()),
                            ("ACTUAL COMPUTE ROUTE", device_route),
                        ] {
                            details
                                .spawn((
                                    Node {
                                        min_width: px(230),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(px(12)),
                                        row_gap: px(3),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.26)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                ))
                                .with_children(|item| {
                                    spawn_text(
                                        item,
                                        font.clone(),
                                        label,
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                    spawn_wrapped_text(
                                        item,
                                        font.clone(),
                                        value,
                                        10.0,
                                        theme.foreground,
                                    );
                                });
                        }
                    });
            }
        });
}

fn spawn_analysis_history_list(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    if session.analysis_history.is_empty() {
        return;
    }
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(30), px(18)),
            row_gap: px(8),
            border: UiRect::bottom(px(1)),
            ..default()
        })
        .with_children(|history_list| {
            history_list
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    spawn_text(
                        header,
                        font.clone(),
                        format!("ANALYSIS HISTORY · {}", session.analysis_history.len()),
                        9.0,
                        theme.muted_foreground,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    if !session.pending_analysis_history_clear {
                        spawn_text_button(
                            header,
                            font.clone(),
                            theme,
                            "Clear history…",
                            8.0,
                            UiAction::RequestClearAnalysisHistory,
                        );
                    }
                });
            if session.pending_analysis_history_clear {
                history_list
                    .spawn((
                        Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            padding: UiRect::all(px(11)),
                            column_gap: px(9),
                            row_gap: px(7),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(theme.destructive.with_alpha(0.06)),
                        BorderColor::all(theme.destructive.with_alpha(0.46)),
                    ))
                    .with_children(|confirmation| {
                        spawn_wrapped_text(
                            confirmation,
                            font.clone(),
                            "Delete every saved analysis session? Songs, charts, models, generated assets, and the active queue are not affected.",
                            9.0,
                            theme.muted_foreground,
                        );
                        confirmation.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_text_button(
                            confirmation,
                            font.clone(),
                            theme,
                            "Cancel",
                            8.0,
                            UiAction::CancelClearAnalysisHistory,
                        );
                        spawn_text_button(
                            confirmation,
                            font.clone(),
                            theme,
                            "Delete history",
                            8.0,
                            UiAction::ConfirmClearAnalysisHistory,
                        );
                    });
            }
            for history in session.analysis_history.iter().take(20) {
                let selected = session.selected_analysis_history == Some(history.id);
                let duration_seconds =
                    ((history.finished_at_ms - history.started_at_ms).max(0) / 1000) as u64;
                history_list
                    .spawn((
                        Button,
                        UiAction::SelectAnalysisHistory(Some(history.id)),
                        Node {
                            width: percent(100),
                            min_height: px(48),
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(px(13), px(9)),
                            column_gap: px(12),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme.primary.with_alpha(0.10)
                        } else {
                            theme.background.with_alpha(0.24)
                        }),
                        BorderColor::all(if selected {
                            theme.primary.with_alpha(0.58)
                        } else {
                            theme.border.with_alpha(0.42)
                        }),
                    ))
                    .with_children(|row| {
                        row.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                history.title.clone(),
                                10.0,
                                theme.foreground,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                history.artist.clone(),
                                8.0,
                                theme.muted_foreground,
                            );
                        });
                        spawn_text(
                            row,
                            font.clone(),
                            format!("{}:{:02}", duration_seconds / 60, duration_seconds % 60),
                            8.0,
                            theme.muted_foreground,
                        );
                        spawn_text(
                            row,
                            font.clone(),
                            history.status.to_ascii_uppercase(),
                            8.0,
                            if history.status == "completed" {
                                theme.pitch_contour
                            } else {
                                theme.destructive
                            },
                        );
                    });
            }
        });
}

fn spawn_about_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseAbout,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(110),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(520),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            UiTransform::from_xy(percent(-50), percent(-50)),
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            ZIndex(111),
        ))
        .with_children(|dialog| {
            dialog
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|identity| {
                    identity.spawn((
                        Node {
                            width: px(72),
                            height: px(72),
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::all(px(16)),
                            ..default()
                        },
                        ImageNode::new(asset_server.load(LOGO_PATH)),
                    ));
                    identity
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(copy, font.clone(), "Uta Studio", 24.0, theme.foreground);
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "AI generation · precise chart editing · interoperable export",
                                10.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_text_button(
                        identity,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::CloseAbout,
                    );
                });
            spawn_text(
                dialog,
                font.clone(),
                format!("Version {}", env!("CARGO_PKG_VERSION")),
                11.0,
                theme.foreground,
            );
            spawn_text(
                dialog,
                font.clone(),
                "License · GPL-3.0-or-later",
                10.0,
                theme.muted_foreground,
            );
            dialog.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.7)),
            ));
            spawn_text(dialog, font.clone(), "ATTRIBUTIONS", 9.0, theme.primary);
            for attribution in [
                "Lyrics data · LRCLIB",
                "Stem separation · UVR / Demucs",
                "Speech recognition · WhisperX / OpenAI Whisper / NVIDIA Parakeet",
                "Forced alignment · WhisperX / torchaudio / Qwen3-ForcedAligner / FA-Kara (MIT)",
                "Optional Japanese model · NextFire MMS Karaoke (AGPL-3.0)",
                "CJK romanization · fugashi / pypinyin / hangul-romanize / ToJyutping",
            ] {
                spawn_wrapped_text(
                    dialog,
                    font.clone(),
                    attribution,
                    9.0,
                    theme.muted_foreground,
                );
            }
        });
}

fn spawn_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let Some(editor) = session.editor.as_ref() else {
        parent
            .spawn((
                Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(theme.background),
            ))
            .with_children(|page| {
                page.spawn((
                    Node {
                        width: percent(100),
                        height: px(58),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.58)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons,
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::Back,
                        false,
                        false,
                        34.0,
                    );
                    spawn_text(toolbar, font.clone(), "Chart editor", 12.0, theme.foreground);
                });
                page.spawn(Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|empty| {
                    empty
                        .spawn(Node {
                            width: px(460),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|message| {
                            let loading = session.editor_load_job.receiver.is_some();
                            spawn_text(
                                message,
                                font.clone(),
                                if loading {
                                    "Preparing chart editor…"
                                } else {
                                    "Chart needs attention"
                                },
                                18.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                message,
                                font.clone(),
                                session.notice.as_deref().unwrap_or(
                                    "The chart editor could not be loaded. Return to the song and review its analysis status.",
                                ),
                                11.0,
                                theme.muted_foreground,
                            );
                        });
                });
            });
        return;
    };
    let song = session.selected_song();
    let notes = chart_notes(&editor.chart);
    let lyrics = chart_lyrics(&editor.chart, &notes);

    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|editor_root| {
            editor_root
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(58),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect {
                            left: px(12),
                            right: px(44),
                            ..default()
                        },
                        column_gap: px(8),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.58)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::Back,
                        false,
                        false,
                        34.0,
                    );
                    toolbar
                        .spawn(Node {
                            min_width: px(0),
                            width: px(280),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|identity| {
                            spawn_text(
                                identity,
                                font.clone(),
                                song.as_ref()
                                    .map(|song| song.title.as_str())
                                    .unwrap_or("Chart editor"),
                                12.0,
                                theme.foreground,
                            );
                            spawn_text(
                                identity,
                                font.clone(),
                                song.as_ref()
                                    .map(|song| song.artist.as_str())
                                    .unwrap_or("Uta Studio"),
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    if editor.dirty {
                        toolbar
                            .spawn((
                                Node {
                                    padding: UiRect::axes(px(7), px(3)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme.primary.with_alpha(0.12)),
                            ))
                            .with_children(|badge| {
                                spawn_text(badge, font.clone(), "UNSAVED", 8.0, theme.primary);
                            });
                    }
                    toolbar.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Undo,
                        UiAction::EditorUndo,
                        false,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Redo,
                        UiAction::EditorRedo,
                        false,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::PanelBottom,
                        UiAction::ToggleLyrics,
                        !editor.lyrics_hidden,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::PanelRight,
                        UiAction::ToggleInspector,
                        editor.inspector_open,
                        false,
                        34.0,
                    );
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::Save,
                        if editor.dirty { "Save *" } else { "Save" },
                        UiAction::SaveEditor,
                        false,
                    );
                });

            editor_root
                .spawn(Node {
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|workspace| {
                    workspace
                        .spawn(Node {
                            min_width: px(0),
                            min_height: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|timeline_column| {
                            spawn_editor_timeline(
                                timeline_column,
                                font.clone(),
                                editor,
                                &notes,
                                theme,
                            );
                            if !editor.lyrics_hidden {
                                spawn_editor_lyrics(
                                    timeline_column,
                                    font.clone(),
                                    editor,
                                    &lyrics,
                                    theme,
                                );
                            }
                            spawn_editor_dock(
                                timeline_column,
                                font.clone(),
                                icons.clone(),
                                editor,
                                session.open_editor_select,
                                theme,
                            );
                            timeline_column
                                .spawn((
                                    Node {
                                        width: percent(100),
                                        height: px(28),
                                        flex_shrink: 0.0,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::horizontal(px(16)),
                                        border: UiRect::top(px(1)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.card.with_alpha(0.42)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                ))
                                .with_children(|status| {
                                    let selected_notes = editor.selected_note_indices().len();
                                    let selected_lyrics = editor.selected_word_indices().len();
                                    let selection = if selected_notes > 0 {
                                        format!("{selected_notes} note(s) selected")
                                    } else if selected_lyrics > 0 {
                                        format!("{selected_lyrics} lyric item(s) selected")
                                    } else {
                                        "No selection".to_string()
                                    };
                                    spawn_text(
                                        status,
                                        font.clone(),
                                        format!(
                                            "{selection} · {:.1}–{:.1}s · {} notes",
                                            editor.viewport_start,
                                            editor.viewport_end(),
                                            notes.len()
                                        ),
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                    status.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    spawn_text(
                                        status,
                                        font.clone(),
                                        "Double-click lyric to edit · drag edges to resize · wheel / Shift / Ctrl / Alt to navigate",
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                });
                        });
                    if editor.inspector_open {
                        spawn_editor_inspector(workspace, font.clone(), editor, &notes, theme);
                    }
                });

            if let Some(notice) = session.notice.as_deref() {
                editor_root
                    .spawn((
                        Node {
                            width: percent(100),
                            min_height: px(28),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(16)),
                            ..default()
                        },
                        BackgroundColor(theme.muted.with_alpha(0.5)),
                        children![(
                            Text::new(notice),
                            ui_text_font(font, 9.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::default(),
                        )],
                    ));
            }
        });
}

fn spawn_editor_dock(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    editor: &NativeEditor,
    open_select: Option<EditorDockSelectKind>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(48),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12)),
                column_gap: px(6),
                border: UiRect::top(px(1)),
                overflow: Overflow::scroll_x(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.7)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|dock| {
            let audio_available = editor.audio_status.loaded && editor.audio_status.error.is_none();
            if audio_available {
                spawn_icon_button(
                    dock,
                    icons.clone(),
                    theme,
                    if editor.audio_status.playing {
                        UiIcon::Pause
                    } else {
                        UiIcon::Play
                    },
                    UiAction::TogglePlayback,
                    true,
                    false,
                    36.0,
                );
                spawn_icon_button(
                    dock,
                    icons.clone(),
                    theme,
                    UiIcon::ArrowLeft,
                    UiAction::SeekEditorStart,
                    false,
                    false,
                    30.0,
                );
                dock.spawn((
                    EditorClockText,
                    Text::new(format_editor_clock(
                        editor.visible_position,
                        editor.audio_status.duration_secs,
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.foreground),
                    TextLayout::no_wrap(),
                ));
                let mut audio_options = vec![("instrumental", "Instrumental")];
                if editor.chart.audio.vocals.is_some() {
                    audio_options.insert(0, ("vocals", "Vocals"));
                }
                audio_options.push(("original", "Original"));
                spawn_editor_select(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    EditorDockSelectKind::AudioSource,
                    UiIcon::Music,
                    match editor.audio_source.as_str() {
                        "vocals" => "Vocals",
                        "original" => "Original",
                        _ => "Instrumental",
                    },
                    editor.audio_source.as_str(),
                    &audio_options,
                    open_select == Some(EditorDockSelectKind::AudioSource),
                );
            } else {
                spawn_icon(
                    dock,
                    icons.clone(),
                    UiIcon::Play,
                    16.0,
                    theme.muted_foreground.with_alpha(0.35),
                );
                spawn_text(
                    dock,
                    font.clone(),
                    "Audio unavailable · chart editing remains enabled",
                    9.0,
                    theme.muted_foreground,
                );
            }
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            let selected_notes = editor.selected_note_indices().len();
            let selected_lyrics = editor.selected_word_indices().len();
            let lyric_context = selected_notes == 0 && selected_lyrics > 0;
            let mut tools = Vec::new();
            let context_label = if lyric_context {
                tools.push((UiIcon::Add, "Add", UiAction::AddEditorWord, false));
                tools.push((UiIcon::Scissors, "Split", UiAction::SplitEditorNote, false));
                if selected_lyrics > 1 {
                    tools.push((UiIcon::Combine, "Merge", UiAction::MergeEditorNotes, false));
                }
                tools.push((UiIcon::Trash, "Delete", UiAction::DeleteEditorNote, true));
                format!("LYRICS · {selected_lyrics}")
            } else if selected_notes > 0 {
                tools.push((UiIcon::Scissors, "Split", UiAction::SplitEditorNote, false));
                if selected_notes > 1 {
                    tools.push((UiIcon::Combine, "Merge", UiAction::MergeEditorNotes, false));
                }
                tools.extend([
                    (UiIcon::Copy, "Copy", UiAction::CopyEditorNote, false),
                    (
                        UiIcon::Copy,
                        "Duplicate",
                        UiAction::DuplicateEditorNotes,
                        false,
                    ),
                    (
                        UiIcon::Sparkles,
                        "Type",
                        UiAction::CycleEditorNoteKind,
                        false,
                    ),
                    (
                        UiIcon::Grid,
                        "Quantize",
                        UiAction::QuantizeEditorNotes,
                        false,
                    ),
                    (UiIcon::Trash, "Delete", UiAction::DeleteEditorNote, true),
                ]);
                format!("NOTES · {selected_notes}")
            } else {
                tools.extend([
                    (UiIcon::Add, "Note", UiAction::AddEditorNote, false),
                    (UiIcon::Add, "Lyric", UiAction::AddEditorWord, false),
                ]);
                if !editor.clipboard_notes.is_empty() {
                    tools.push((UiIcon::Clipboard, "Paste", UiAction::PasteEditorNote, false));
                }
                tools.push((UiIcon::Repair, "Repair", UiAction::RepairEditorChart, false));
                "CREATE".to_string()
            };
            spawn_text(
                dock,
                font.clone(),
                context_label,
                8.0,
                theme.muted_foreground,
            );
            for (icon, label, action, destructive) in tools {
                spawn_toolbar_button(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    icon,
                    label,
                    action,
                    destructive,
                );
            }
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            spawn_editor_select(
                dock,
                font.clone(),
                icons.clone(),
                theme,
                EditorDockSelectKind::SnapGrid,
                UiIcon::Grid,
                format!("Grid {}", format_snap_grid(editor.snap_seconds)),
                &editor.snap_seconds.to_string(),
                &[
                    ("0", "Grid off"),
                    ("0.01", "10 ms"),
                    ("0.025", "25 ms"),
                    ("0.05", "50 ms"),
                    ("0.1", "100 ms"),
                    ("0.25", "250 ms"),
                ],
                open_select == Some(EditorDockSelectKind::SnapGrid),
            );
            dock.spawn(Node {
                min_width: px(10),
                flex_grow: 1.0,
                ..default()
            });
            spawn_icon_button(
                dock,
                icons.clone(),
                theme,
                UiIcon::ZoomOut,
                UiAction::AdjustEditorTimeZoom(-1),
                false,
                false,
                32.0,
            );
            spawn_text(
                dock,
                font.clone(),
                format!("{:.0}s", editor.viewport_duration),
                9.0,
                theme.muted_foreground,
            );
            spawn_icon_button(
                dock,
                icons,
                theme,
                UiIcon::ZoomIn,
                UiAction::AdjustEditorTimeZoom(1),
                false,
                false,
                32.0,
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Pitch ↓",
                9.0,
                UiAction::PanEditorPitch(-1),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Pitch ↑",
                9.0,
                UiAction::PanEditorPitch(1),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Range +",
                9.0,
                UiAction::AdjustEditorPitchZoom(-1),
            );
            spawn_text_button(
                dock,
                font,
                theme,
                "Range −",
                9.0,
                UiAction::AdjustEditorPitchZoom(1),
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_editor_select(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    kind: EditorDockSelectKind,
    icon: UiIcon,
    label: impl Into<String>,
    current_value: &str,
    options: &[(&str, &str)],
    open: bool,
) {
    let label = label.into();
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                height: px(32),
                flex_shrink: 0.0,
                ..default()
            },
            ZIndex(if open { 70 } else { 0 }),
        ))
        .with_children(|control| {
            control
                .spawn((
                    Button,
                    UiAction::OpenEditorSelect(kind),
                    Node {
                        height: px(32),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(9)),
                        column_gap: px(6),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(if open { 0.72 } else { 0.38 })),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.72)
                    } else {
                        theme.border.with_alpha(0.44)
                    }),
                ))
                .with_children(|button| {
                    spawn_icon(button, icons.clone(), icon, 14.0, theme.foreground);
                    spawn_text(button, font.clone(), label, 9.0, theme.foreground);
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::ChevronDown,
                        12.0,
                        theme.muted_foreground,
                    );
                });
            if open {
                control
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            bottom: px(36),
                            min_width: px(150),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(5)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(7)),
                            ..default()
                        },
                        BackgroundColor(theme.card),
                        BorderColor::all(theme.border.with_alpha(0.9)),
                        ZIndex(70),
                    ))
                    .with_children(|menu| {
                        for (value, option_label) in options {
                            let selected = *value == current_value;
                            menu.spawn((
                                Button,
                                UiAction::SelectEditorValue(kind, (*value).to_string()),
                                Node {
                                    width: percent(100),
                                    min_height: px(30),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(9)),
                                    column_gap: px(8),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.primary.with_alpha(0.12)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|option| {
                                spawn_text(
                                    option,
                                    font.clone(),
                                    *option_label,
                                    9.0,
                                    if selected {
                                        theme.primary
                                    } else {
                                        theme.foreground
                                    },
                                );
                                option.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                if selected {
                                    spawn_icon(
                                        option,
                                        icons.clone(),
                                        UiIcon::Check,
                                        13.0,
                                        theme.primary,
                                    );
                                }
                            });
                        }
                    });
            }
        });
}

fn spawn_editor_timeline(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    notes: &[ChartNoteView],
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(240),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    position_type: PositionType::Relative,
                    width: px(58),
                    height: percent(100),
                    flex_shrink: 0.0,
                    border: UiRect::right(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme.card.with_alpha(0.44)),
                BorderColor::all(theme.border.with_alpha(0.45)),
            ))
            .with_children(|gutter| {
                gutter.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(7),
                        top: px(6),
                        ..default()
                    },
                    Text::new("AUDIO"),
                    ui_text_font(font.clone(), 7.0),
                    TextColor(theme.muted_foreground.with_alpha(0.65)),
                ));
                let pitch_span = (editor.pitch_max - editor.pitch_min).max(1.0);
                let pitch_step = (pitch_span / 42.0).ceil().max(1.0) as usize;
                for midi in ((editor.pitch_min.floor() as i32).clamp(0, 127)
                    ..=(editor.pitch_max.ceil() as i32).clamp(0, 127))
                    .step_by(pitch_step)
                {
                    let top = pitch_percent(f64::from(midi) + 0.5, editor);
                    let bottom = pitch_percent(f64::from(midi) - 0.5, editor);
                    let black_key = matches!(midi.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
                    gutter
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(0),
                                top: percent(top),
                                width: if black_key { px(38) } else { percent(100) },
                                height: percent((bottom - top).max(0.1)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::FlexEnd,
                                padding: UiRect::right(px(5)),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BackgroundColor(if black_key {
                                theme.background.with_alpha(0.96)
                            } else {
                                theme.foreground.with_alpha(0.055)
                            }),
                            BorderColor::all(theme.border.with_alpha(if black_key {
                                0.5
                            } else {
                                0.32
                            })),
                        ))
                        .with_children(|key| {
                            if midi.rem_euclid(12) == 0 {
                                key.spawn((
                                    Text::new(midi_note_name(f64::from(midi))),
                                    ui_text_font(font.clone(), 7.0),
                                    TextColor(theme.muted_foreground.with_alpha(0.82)),
                                    TextLayout::no_wrap(),
                                ));
                            }
                        });
                }
            });
            row.spawn((
                Button,
                EditorTimelineSurface,
                Node {
                    position_type: PositionType::Relative,
                    min_width: px(0),
                    height: percent(100),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.96)),
            ))
            .with_children(|canvas| {
                canvas.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        right: px(0),
                        top: px(0),
                        height: percent(EDITOR_PITCH_TOP_PERCENT),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.56)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                    Pickable::IGNORE,
                ));
                let pitch_step = ((editor.pitch_max - editor.pitch_min) / 30.0)
                    .ceil()
                    .max(1.0) as usize;
                for midi in ((editor.pitch_min.floor() as i32).clamp(0, 127)
                    ..=(editor.pitch_max.ceil() as i32).clamp(0, 127))
                    .step_by(pitch_step)
                {
                    let top = pitch_percent(f64::from(midi) + 0.5, editor);
                    let bottom = pitch_percent(f64::from(midi) - 0.5, editor);
                    let black_key = matches!(midi.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
                    if black_key {
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(0),
                                right: px(0),
                                top: percent(top),
                                height: percent((bottom - top).max(0.1)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.24)),
                            Pickable::IGNORE,
                        ));
                    }
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: percent(pitch_percent(f64::from(midi), editor)),
                            height: px(1),
                            ..default()
                        },
                        BackgroundColor(theme.border.with_alpha(if midi.rem_euclid(12) == 0 {
                            0.54
                        } else {
                            0.22
                        })),
                        Pickable::IGNORE,
                    ));
                }
                for step in 0..=12 {
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: px(0),
                            bottom: px(0),
                            left: percent(step as f32 / 12.0 * 100.0),
                            width: px(1),
                            ..default()
                        },
                        BackgroundColor(theme.border.with_alpha(if step % 3 == 0 {
                            0.38
                        } else {
                            0.14
                        })),
                        Pickable::IGNORE,
                    ));
                    if step % 2 == 0 && step < 12 {
                        let time = editor.viewport_start
                            + editor.viewport_duration * f64::from(step) / 12.0;
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(step as f32 / 12.0 * 100.0),
                                top: px(4),
                                padding: UiRect::left(px(4)),
                                ..default()
                            },
                            Text::new(format!("{time:.1}s")),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(theme.muted_foreground),
                            Pickable::IGNORE,
                        ));
                    }
                }
                let peak_count = editor.waveform.peaks.len();
                if peak_count > 0 && editor.waveform.duration_secs > 0.0 {
                    let visible_peaks = editor
                        .waveform
                        .peaks
                        .iter()
                        .enumerate()
                        .filter_map(|(index, peak)| {
                            let time =
                                index as f64 / peak_count as f64 * editor.waveform.duration_secs;
                            (time >= editor.viewport_start && time <= editor.viewport_end())
                                .then_some((time, *peak))
                        })
                        .collect::<Vec<_>>();
                    let stride = visible_peaks.len().div_ceil(360).max(1);
                    for (time, (minimum, maximum)) in visible_peaks.into_iter().step_by(stride) {
                        let amplitude = (maximum - minimum).abs().clamp(0.01, 2.0);
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(time_percent(time, editor)),
                                top: percent(13.0 - amplitude * 3.0),
                                width: px(1),
                                height: percent(amplitude * 6.0),
                                ..default()
                            },
                            BackgroundColor(theme.waveform.with_alpha(0.32)),
                            Pickable::IGNORE,
                        ));
                    }
                }
                let pitch_frames = chart_pitch_frames(&editor.chart);
                for note in notes.iter().filter(|note| {
                    note.end >= editor.viewport_start && note.start <= editor.viewport_end()
                }) {
                    let left = time_percent(note.start, editor);
                    let right = time_percent(note.end, editor);
                    let width = (right - left).max(0.4);
                    let top = pitch_percent(note.midi, editor);
                    let selected = editor.selected_notes.contains(&note.index)
                        || editor.selected_note == Some(note.index);
                    let active =
                        editor.visible_position >= note.start && editor.visible_position < note.end;
                    let note_frames = pitch_frames
                        .iter()
                        .filter(|frame| {
                            frame.time >= note.start
                                && frame.time <= note.end
                                && (frame.midi - note.midi).abs() <= 4.0
                                && frame.confidence >= 0.18
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let max_points =
                        (((note.end - note.start) * 16.0).ceil() as usize).clamp(2, 14);
                    let contour = abstract_pitch_contour(&note_frames, max_points);
                    let note_color = editor_note_color(&note.kind, theme);
                    canvas
                        .spawn((
                            Button,
                            EditorNoteNode(note.index),
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(left),
                                top: percent(top),
                                width: percent(width),
                                min_width: px(6),
                                height: px(18),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(6)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(if selected {
                                theme.editor_selection.with_alpha(0.9)
                            } else if active {
                                theme.primary.with_alpha(0.86)
                            } else {
                                note_color.with_alpha(
                                    (0.9 + note.confidence.clamp(0.0, 1.0) * 0.08) as f32,
                                )
                            }),
                            BorderColor::all(if selected {
                                theme.editor_selection.with_alpha(1.0)
                            } else if active {
                                theme.primary.with_alpha(1.0)
                            } else {
                                note_color.with_alpha(1.0)
                            }),
                            BoxShadow::new(
                                if selected || active {
                                    Color::srgba(0.0, 0.0, 0.0, 0.28)
                                } else {
                                    Color::NONE
                                },
                                px(0),
                                px(3),
                                px(7),
                                px(-2),
                            ),
                            UiTransform::from_xy(px(0), px(-9)),
                            ZIndex(if selected || active { 2 } else { 1 }),
                        ))
                        .with_children(|note_node| {
                            if width >= 2.6 {
                                note_node.spawn((
                                    Text::new(midi_note_name(note.midi)),
                                    ui_text_font(font.clone(), 8.0),
                                    TextColor(if selected {
                                        theme.background
                                    } else if active {
                                        theme.primary_foreground
                                    } else if theme.dark {
                                        theme.foreground.with_alpha(0.96)
                                    } else {
                                        theme.primary_foreground.with_alpha(0.96)
                                    }),
                                    TextLayout::no_wrap(),
                                    ZIndex(2),
                                    Pickable::IGNORE,
                                ));
                            }
                            let duration = (note.end - note.start).max(0.001);
                            if width >= 2.2 && contour.len() > 1 {
                                for (index, point) in contour.iter().enumerate() {
                                    let point_left = (((point.time - note.start) / duration)
                                        * 100.0)
                                        .clamp(0.0, 100.0)
                                        as f32;
                                    let point_right = contour
                                        .get(index + 1)
                                        .map(|next| {
                                            (((next.time - note.start) / duration) * 100.0)
                                                .clamp(0.0, 100.0)
                                                as f32
                                        })
                                        .unwrap_or(100.0);
                                    let point_top = (50.0 - (point.midi - note.midi) as f32 * 10.0)
                                        .clamp(12.0, 88.0);
                                    let color = if selected {
                                        theme.background.with_alpha(0.72)
                                    } else if active {
                                        theme.primary_foreground.with_alpha(0.8)
                                    } else {
                                        theme
                                            .pitch_contour
                                            .with_alpha((0.46 + point.confidence * 0.34) as f32)
                                    };
                                    note_node.spawn((
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: percent(point_left),
                                            top: percent(point_top),
                                            width: percent((point_right - point_left).max(0.8)),
                                            height: px(1.2),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(color),
                                        ZIndex(1),
                                        Pickable::IGNORE,
                                    ));
                                }
                            }
                            for (edge, left, right) in [
                                (NoteEdge::Start, Some(px(-3)), None),
                                (NoteEdge::End, None, Some(px(-3))),
                            ] {
                                note_node.spawn((
                                    Button,
                                    EditorNoteResizeHandle {
                                        index: note.index,
                                        edge,
                                    },
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: left.unwrap_or_default(),
                                        right: right.unwrap_or_default(),
                                        top: px(-2),
                                        bottom: px(-2),
                                        width: px(8),
                                        border_radius: BorderRadius::all(px(3)),
                                        ..default()
                                    },
                                    BackgroundColor(if selected {
                                        theme.background.with_alpha(0.62)
                                    } else {
                                        Color::NONE
                                    }),
                                ));
                            }
                        });
                }
                let playhead = time_percent(editor.visible_position, editor);
                canvas.spawn((
                    EditorPlayhead,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(playhead),
                        top: px(0),
                        bottom: px(0),
                        width: px(1.5),
                        ..default()
                    },
                    BackgroundColor(theme.primary.with_alpha(0.94)),
                    ZIndex(3),
                    Pickable::IGNORE,
                ));
            });
        });
}

fn spawn_editor_lyrics(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    lyrics: &[ChartLyricView],
    theme: &StudioTheme,
) {
    let visible_lane_count = lyrics
        .iter()
        .filter(|lyric| lyric.end >= editor.viewport_start && lyric.start <= editor.viewport_end())
        .map(|lyric| lyric.lane + 1)
        .max()
        .unwrap_or(1);
    let lane_height = (14.0 + visible_lane_count as f32 * 26.0).clamp(46.0, 92.0);
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(lane_height),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.58)),
            BorderColor::all(theme.border.with_alpha(0.45)),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(58),
                    height: percent(100),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::right(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.45)),
                children![(
                    Text::new("LYRICS"),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(theme.muted_foreground),
                )],
            ));
            row.spawn((
                EditorLyricsSurface,
                Node {
                    position_type: PositionType::Relative,
                    min_width: px(0),
                    height: percent(100),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|lane| {
                for lyric in lyrics.iter().filter(|lyric| {
                    lyric.end >= editor.viewport_start && lyric.start <= editor.viewport_end()
                }) {
                    let left = time_percent(lyric.start, editor);
                    let right = time_percent(lyric.end, editor);
                    let selection = WordSelection {
                        segment: lyric.segment,
                        word: lyric.word,
                    };
                    let selected = editor.selected_words.contains(&selection)
                        || editor.selected_word
                            == Some(WordSelection {
                                segment: lyric.segment,
                                word: lyric.word,
                            });
                    let active = editor.visible_position >= lyric.start
                        && editor.visible_position < lyric.end;
                    lane.spawn((
                        Button,
                        UiAction::SelectEditorWord(
                            lyric.segment,
                            lyric.word,
                            (lyric.start.max(0.0) * 1000.0).round() as u64,
                        ),
                        EditorLyricNode { selection },
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            top: px(6.0 + lyric.lane as f32 * 26.0),
                            width: percent((right - left).max(1.5)),
                            min_width: px(26),
                            height: px(22),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::horizontal(px(7)),
                            margin: UiRect::horizontal(px(1)),
                            overflow: Overflow::clip(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme.editor_selection.with_alpha(0.28)
                        } else if active {
                            theme.primary.with_alpha(0.22)
                        } else if lyric.guided {
                            theme.muted.with_alpha(if theme.dark { 0.34 } else { 0.74 })
                        } else {
                            theme
                                .editor_warning
                                .with_alpha(if theme.dark { 0.07 } else { 0.045 })
                        }),
                        BorderColor::all(if selected {
                            theme.editor_selection.with_alpha(0.94)
                        } else if active {
                            theme.primary.with_alpha(0.9)
                        } else if lyric.guided {
                            theme
                                .border
                                .with_alpha(if theme.dark { 0.86 } else { 0.68 })
                        } else {
                            theme.border.with_alpha(0.7)
                        }),
                    ))
                    .with_children(|lyric_node| {
                        if !lyric.guided {
                            lyric_node.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(2),
                                    top: px(4),
                                    bottom: px(4),
                                    width: px(2),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme.editor_warning.with_alpha(0.9)),
                                Pickable::IGNORE,
                            ));
                        }
                        if editor.word_edit_focus == Some(selection) {
                            lyric_node.spawn((
                                InlineEditorWordInput,
                                EditorWordInput(selection),
                                EditableText {
                                    max_characters: Some(160),
                                    visible_width: Some(18.0),
                                    ..EditableText::new(&lyric.text)
                                },
                                Node {
                                    width: percent(100),
                                    min_width: px(0),
                                    height: percent(100),
                                    align_items: AlignItems::Center,
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                ui_text_font(font.clone(), 9.0),
                                TextColor(theme.foreground),
                                TextCursorStyle {
                                    color: theme.editor_selection,
                                    selected_text_color: Some(theme.primary_foreground),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.72)),
                                TabIndex(0),
                                AutoFocus,
                            ));
                        } else {
                            lyric_node.spawn((
                                Text::new(lyric.text.clone()),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(if selected || active || lyric.guided {
                                    theme.foreground
                                } else {
                                    theme.foreground.with_alpha(0.84)
                                }),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        }
                        if selected {
                            for (edge, left, right) in [
                                (NoteEdge::Start, Some(px(0)), None),
                                (NoteEdge::End, None, Some(px(0))),
                            ] {
                                lyric_node.spawn((
                                    Button,
                                    EditorLyricResizeHandle { selection, edge },
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: left.unwrap_or_default(),
                                        right: right.unwrap_or_default(),
                                        top: px(1),
                                        bottom: px(1),
                                        width: px(7),
                                        border_radius: BorderRadius::all(px(2)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.editor_selection.with_alpha(0.78)),
                                ));
                            }
                        }
                    });
                }
            });
        });
}

fn spawn_editor_inspector(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    notes: &[ChartNoteView],
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: px(260),
                height: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(8),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.5)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|inspector| {
            spawn_text(
                inspector,
                font.clone(),
                "CHART INSPECTOR",
                8.0,
                theme.primary,
            );
            let selected = editor.selected_note_indices();
            if selected.len() > 1 {
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("{} pitch notes", selected.len()),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Drag any selected bar to move and transpose the group. Shift-click adds or removes notes; Shift-drag draws a selection rectangle.",
                    10.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Cycle note type", UiAction::CycleEditorNoteKind),
                    ("Split selection", UiAction::SplitEditorNote),
                    ("Merge selection", UiAction::MergeEditorNotes),
                    ("Quantize selection", UiAction::QuantizeEditorNotes),
                    ("Duplicate selection", UiAction::DuplicateEditorNotes),
                    ("Copy selection", UiAction::CopyEditorNote),
                    ("Delete selection", UiAction::DeleteEditorNote),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
            } else if let Some(note) = editor
                .selected_note
                .and_then(|index| notes.iter().find(|note| note.index == index))
            {
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("MIDI {:.0}", note.midi),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    format!(
                        "{:.3}s – {:.3}s\nType: {}\nDrag to change time and pitch.",
                        note.start, note.end, note.kind
                    ),
                    10.0,
                    theme.muted_foreground,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Cycle note type",
                    UiAction::CycleEditorNoteKind,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Split at playhead",
                    UiAction::SplitEditorNote,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Quantize note",
                    UiAction::QuantizeEditorNotes,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Duplicate note",
                    UiAction::DuplicateEditorNotes,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Copy note",
                    UiAction::CopyEditorNote,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Delete note",
                    UiAction::DeleteEditorNote,
                );
                if !editor.clipboard_notes.is_empty() {
                    spawn_action_button(
                        inspector,
                        font.clone(),
                        theme,
                        "Paste at playhead",
                        UiAction::PasteEditorNote,
                    );
                }
            } else if editor.selected_word_indices().len() > 1 {
                let count = editor.selected_word_indices().len();
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("{count} lyric words"),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Shift/Ctrl-click toggles words. Timing moves apply to the whole selection; merge requires words from one phrase.",
                    9.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Move selection −10 ms", UiAction::ShiftEditorWord(-1)),
                    ("Move selection +10 ms", UiAction::ShiftEditorWord(1)),
                    ("Split selected words", UiAction::SplitEditorWord),
                    ("Merge selected words", UiAction::MergeEditorWord),
                    ("Delete selected words", UiAction::DeleteEditorWord),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
            } else if let Some(selection) = editor.selected_word
                && let Some((text, start, end)) = selected_editor_word(&editor.chart, selection)
            {
                spawn_text(
                    inspector,
                    font.clone(),
                    "Lyric word",
                    17.0,
                    theme.foreground,
                );
                let mut input = inspector.spawn((
                    EditorWordInput(selection),
                    EditableText {
                        max_characters: Some(160),
                        visible_width: Some(22.0),
                        ..EditableText::new(text)
                    },
                    Node {
                        width: percent(100),
                        min_height: px(36),
                        padding: UiRect::axes(px(9), px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    ui_text_font(font.clone(), 11.0),
                    TextColor(theme.foreground),
                    TextCursorStyle {
                        color: theme.primary,
                        selected_text_color: Some(theme.primary_foreground),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.4)),
                    BorderColor::all(theme.border.with_alpha(0.6)),
                    TabIndex(0),
                ));
                if editor.word_edit_focus == Some(selection) {
                    input.insert(AutoFocus);
                }
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    format!(
                        "{start:.3}s – {end:.3}s · whole-word and boundary controls use 10 ms steps."
                    ),
                    9.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Add word at playhead", UiAction::AddEditorWord),
                    ("Move word −10 ms", UiAction::ShiftEditorWord(-1)),
                    ("Move word +10 ms", UiAction::ShiftEditorWord(1)),
                    ("Start −10 ms", UiAction::AdjustEditorWordStart(-1)),
                    ("Start +10 ms", UiAction::AdjustEditorWordStart(1)),
                    ("End −10 ms", UiAction::AdjustEditorWordEnd(-1)),
                    ("End +10 ms", UiAction::AdjustEditorWordEnd(1)),
                    ("Split word", UiAction::SplitEditorWord),
                    ("Merge next word", UiAction::MergeEditorWord),
                    ("New phrase here", UiAction::SplitEditorPhrase),
                    ("Join next phrase", UiAction::MergeEditorPhrase),
                    ("Delete word", UiAction::DeleteEditorWord),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
            } else {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Select a note or lyric word. Shift-click/drag selects multiple notes.",
                    10.0,
                    theme.muted_foreground,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Add lyric at playhead",
                    UiAction::AddEditorWord,
                );
            }

            let issues = analyze_chart_issues(&editor.chart);
            inspector.spawn(Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(6)),
                ..default()
            });
            spawn_text(
                inspector,
                font.clone(),
                "CHART CHECKS",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                inspector,
                font.clone(),
                if issues.total() == 0 {
                    "No timing or lyric coverage issues found.".to_string()
                } else {
                    format!(
                        "{} errors · {} warnings · {} total",
                        issues.errors,
                        issues.warnings,
                        issues.total()
                    )
                },
                9.0,
                if issues.errors > 0 {
                    theme.destructive
                } else {
                    theme.muted_foreground
                },
            );
            if issues.auto_fixable {
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Apply safe repairs",
                    UiAction::RepairEditorChart,
                );
            }
            spawn_text(
                inspector,
                font.clone(),
                "GLOBAL TIMING",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                inspector,
                font.clone(),
                "Shift lyrics and pitch together when the whole chart is early or late.",
                9.0,
                theme.muted_foreground,
            );
            spawn_action_button(
                inspector,
                font.clone(),
                theme,
                "Shift all −10 ms",
                UiAction::ShiftWholeChart(-1),
            );
            spawn_action_button(
                inspector,
                font,
                theme,
                "Shift all +10 ms",
                UiAction::ShiftWholeChart(1),
            );
        });
}

fn selected_editor_word(
    chart: &app_core::ChartDocument,
    selection: WordSelection,
) -> Option<(String, f64, f64)> {
    let word = chart
        .transcript
        .get("segments")?
        .as_array()?
        .get(selection.segment)?
        .get("words")?
        .as_array()?
        .get(selection.word)?;
    Some((
        word.get("word")?.as_str()?.to_string(),
        note_number(word, "start", 0.0),
        note_number(word, "end", 0.02),
    ))
}

fn all_editor_word_selections(transcript: &serde_json::Value) -> BTreeSet<WordSelection> {
    transcript
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(segment, value)| {
            value
                .get("words")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .map(move |(word, _)| WordSelection { segment, word })
        })
        .collect()
}

fn chart_notes(chart: &app_core::ChartDocument) -> Vec<ChartNoteView> {
    chart
        .pitch_notes
        .get("notes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, note)| {
            Some(ChartNoteView {
                index,
                start: note.get("start")?.as_f64()?,
                end: note.get("end")?.as_f64()?,
                midi: note.get("midi")?.as_f64()?,
                confidence: note
                    .get("confidence")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(1.0),
                kind: note
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("normal")
                    .to_string(),
            })
        })
        .collect()
}

fn chart_pitch_frames(chart: &app_core::ChartDocument) -> Vec<ChartPitchFrame> {
    chart
        .pitch_track
        .get("frames")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|frame| {
            let hz = frame.get("hz")?.as_f64()?;
            let time = frame.get("time")?.as_f64()?;
            (hz.is_finite() && hz > 0.0).then(|| ChartPitchFrame {
                time,
                midi: 69.0 + 12.0 * (hz / 440.0).log2(),
                confidence: frame
                    .get("confidence")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
            })
        })
        .collect()
}

fn abstract_pitch_contour(frames: &[ChartPitchFrame], max_points: usize) -> Vec<ChartPitchFrame> {
    if frames.is_empty() || max_points == 0 {
        return Vec::new();
    }
    let stride = frames.len().div_ceil(max_points).max(1);
    frames
        .chunks(stride)
        .filter_map(|chunk| {
            let weight = chunk
                .iter()
                .map(|frame| frame.confidence.clamp(0.05, 1.0))
                .sum::<f64>();
            (weight > 0.0).then(|| ChartPitchFrame {
                time: chunk
                    .iter()
                    .map(|frame| frame.time * frame.confidence.clamp(0.05, 1.0))
                    .sum::<f64>()
                    / weight,
                midi: chunk
                    .iter()
                    .map(|frame| frame.midi * frame.confidence.clamp(0.05, 1.0))
                    .sum::<f64>()
                    / weight,
                confidence: chunk.iter().map(|frame| frame.confidence).sum::<f64>()
                    / chunk.len() as f64,
            })
        })
        .collect()
}

fn chart_lyrics(chart: &app_core::ChartDocument, notes: &[ChartNoteView]) -> Vec<ChartLyricView> {
    let mut lyrics = chart
        .transcript
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(segment_index, segment)| {
            let segment_start = segment
                .get("start")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let segment_end = segment
                .get("end")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(segment_start + 0.04);
            let words = segment
                .get("words")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            if words.is_empty() {
                vec![(
                    segment_index,
                    0,
                    segment_start,
                    segment_end,
                    segment
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )]
            } else {
                words
                    .into_iter()
                    .enumerate()
                    .filter_map(|(word_index, word)| {
                        Some((
                            segment_index,
                            word_index,
                            word.get("start")?.as_f64()?,
                            word.get("end")?.as_f64()?,
                            word.get("word")?.as_str()?.to_string(),
                        ))
                    })
                    .collect()
            }
        })
        .filter(|(_, _, _, _, text)| !text.trim().is_empty())
        .collect::<Vec<_>>();
    lyrics.sort_by(|left, right| left.2.total_cmp(&right.2));
    let mut lane_ends = [f64::NEG_INFINITY; 3];
    lyrics
        .into_iter()
        .map(|(segment, word, start, end, text)| {
            let lane = lane_ends
                .iter()
                .position(|lane_end| *lane_end <= start)
                .unwrap_or_else(|| {
                    lane_ends
                        .iter()
                        .enumerate()
                        .min_by(|left, right| left.1.total_cmp(right.1))
                        .map(|(index, _)| index)
                        .unwrap_or(0)
                });
            lane_ends[lane] = end.max(start + 0.04);
            let guided = notes
                .iter()
                .any(|note| note.start < end && note.end > start);
            ChartLyricView {
                segment,
                word,
                start,
                end,
                text,
                lane,
                guided,
            }
        })
        .collect()
}

fn time_percent(time: f64, editor: &NativeEditor) -> f32 {
    (((time - editor.viewport_start) / editor.viewport_duration) * 100.0).clamp(0.0, 100.0) as f32
}

fn pitch_percent(midi: f64, editor: &NativeEditor) -> f32 {
    let span = (editor.pitch_max - editor.pitch_min).max(1.0);
    (EDITOR_PITCH_TOP_PERCENT
        + (((editor.pitch_max - midi) / span) as f32 * EDITOR_PITCH_HEIGHT_PERCENT))
        .clamp(
            EDITOR_PITCH_TOP_PERCENT,
            EDITOR_PITCH_TOP_PERCENT + EDITOR_PITCH_HEIGHT_PERCENT,
        )
}

fn midi_note_name(midi: f64) -> String {
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let midi = midi.round().clamp(0.0, 127.0) as i32;
    format!("{}{}", NAMES[midi.rem_euclid(12) as usize], midi / 12 - 1)
}

fn editor_note_color(kind: &str, theme: &StudioTheme) -> Color {
    match kind {
        "golden" => Color::srgb(0.94, 0.67, 0.2),
        "golden_rap" => Color::srgb(0.94, 0.45, 0.18),
        "rap" => Color::srgb(0.71, 0.43, 0.92),
        "freestyle" => theme.muted_foreground.with_alpha(0.48),
        _ => theme.note_normal,
    }
}

fn set_editor_pitch_span(editor: &mut NativeEditor, span: f64) {
    let span = span.clamp(8.0, 127.0);
    let center = (editor.pitch_min + editor.pitch_max) / 2.0;
    editor.pitch_min = (center - span / 2.0).clamp(0.0, 127.0 - span);
    editor.pitch_max = editor.pitch_min + span;
}

fn format_editor_clock(position: f64, duration: f64) -> String {
    format!(
        "{} / {}",
        format_duration(position),
        format_duration(duration)
    )
}

fn format_snap_grid(seconds: f64) -> String {
    if seconds <= 0.0 {
        "off".to_string()
    } else {
        format!("{}ms", (seconds * 1000.0).round() as u32)
    }
}

fn lyrics_text(file_hash: &str, mode: LyricsInputMode) -> String {
    if mode == LyricsInputMode::Plain
        && let Some(file) = app_core::load_lyrics_file(file_hash)
    {
        return file.lines.join("\n");
    }
    let Ok(chart) = app_core::load_chart(file_hash) else {
        return String::new();
    };
    chart
        .transcript
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|segment| {
            let text = segment
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            if text.is_empty() {
                return None;
            }
            if mode == LyricsInputMode::TimedLrc {
                let start = segment
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                Some(format!("[{}]{text}", format_lrc_timestamp(start)))
            } else {
                Some(text.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_lrc_timestamp(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{:02}:{:02}.{:02}",
        centiseconds / 6000,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}

fn spawn_song_detail(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let Some(song) = session.selected_song() else {
        parent
            .spawn(Node {
                min_height: px(0),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            })
            .with_children(|empty| {
                spawn_text(
                    empty,
                    font.clone(),
                    "Choose a song first",
                    22.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    empty,
                    font.clone(),
                    "Open a track from the library to see its production page.",
                    11.0,
                    theme.muted_foreground,
                );
                spawn_action_button(empty, font, theme, "Back to library", UiAction::Home);
            });
        return;
    };

    let cover = album_art_handle(&song, asset_server, images, local_images);
    let ambient_cover = ambient_album_art_handle(&song, asset_server, images, local_images);
    parent
        .spawn((
            SongDetailContent,
            ScrollPosition::default(),
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|detail| {
            detail
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(310),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::FlexEnd,
                        padding: UiRect::axes(px(40), px(30)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.72)),
                    BorderColor::all(theme.border.with_alpha(0.45)),
                ))
                .with_children(|hero| {
                    hero.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        ImageNode::new(ambient_cover)
                            .with_color(Color::srgba(1.0, 1.0, 1.0, 0.68))
                            .with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                    ));
                    hero.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.012, 0.014, 0.022, 0.54)),
                    ));
                    hero.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            bottom: px(0),
                            height: percent(52),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.006, 0.008, 0.014, 0.34)),
                    ));
                    hero.spawn((
                        Node {
                            align_items: AlignItems::FlexEnd,
                            column_gap: px(22),
                            ..default()
                        },
                        ZIndex(1),
                    ))
                    .with_children(|identity| {
                        identity.spawn((
                            Node {
                                width: px(150),
                                height: px(150),
                                flex_shrink: 0.0,
                                overflow: Overflow::clip(),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            ImageNode::new(cover),
                            BorderColor::all(theme.border.with_alpha(0.75)),
                        ));
                        identity
                            .spawn(Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                ..default()
                            })
                            .with_children(|copy| {
                                spawn_text(
                                    copy,
                                    font.clone(),
                                    "PRODUCTION MASTER",
                                    9.0,
                                    Color::srgb(0.72, 0.68, 1.0),
                                );
                                spawn_wrapped_text(
                                    copy,
                                    font.clone(),
                                    song.title.clone(),
                                    34.0,
                                    Color::srgb(0.97, 0.97, 0.99),
                                );
                                spawn_text(
                                    copy,
                                    font.clone(),
                                    format!(
                                        "{}{}",
                                        song.artist,
                                        if song.album.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {}", song.album)
                                        }
                                    ),
                                    13.0,
                                    Color::srgba(0.96, 0.96, 0.98, 0.76),
                                );
                            });
                    });
                });

            detail
                .spawn(Node {
                    width: percent(100),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(40), px(22)),
                    row_gap: px(16),
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: percent(100),
                            min_height: px(54),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(18),
                            row_gap: px(8),
                            padding: UiRect::bottom(px(16)),
                            border: UiRect::bottom(px(1)),
                            ..default()
                        },
                        BorderColor::all(theme.border.with_alpha(0.55)),
                    ))
                    .with_children(|summary| {
                        summary
                            .spawn(Node {
                                min_width: px(220),
                                flex_grow: 1.0,
                                align_items: AlignItems::Center,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(18),
                                row_gap: px(5),
                                ..default()
                            })
                            .with_children(|metadata| {
                                spawn_text(
                                    metadata,
                                    font.clone(),
                                    format_duration(song.duration_secs),
                                    10.0,
                                    theme.muted_foreground,
                                );
                                spawn_text(
                                    metadata,
                                    font.clone(),
                                    song.language.as_deref().unwrap_or("Language unknown"),
                                    10.0,
                                    theme.muted_foreground,
                                );
                                if let Some(key) = song.override_key.as_ref().or(song.key.as_ref()) {
                                    spawn_text(
                                        metadata,
                                        font.clone(),
                                        format!("Key {key}"),
                                        10.0,
                                        theme.muted_foreground,
                                    );
                                }
                                spawn_text(
                                    metadata,
                                    font.clone(),
                                    format!("{:.1}× tempo", song.tempo),
                                    10.0,
                                    theme.muted_foreground,
                                );
                            });
                        summary
                            .spawn(Node {
                                min_width: px(0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::FlexEnd,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(8),
                                row_gap: px(6),
                                ..default()
                            })
                            .with_children(|actions| {
                                let current = session.library_playback.file_hash.as_deref()
                                    == Some(song.file_hash.as_str())
                                    && session.library_playback.status.loaded;
                                spawn_compact_action_button(
                                    actions,
                                    font.clone(),
                                    theme,
                                    if current && session.library_playback.status.playing {
                                        "Pause"
                                    } else if current {
                                        "Resume"
                                    } else {
                                        "Play original"
                                    },
                                    if current {
                                        UiAction::ToggleLibraryPlayback
                                    } else {
                                        UiAction::PlayLibrarySong(song.file_hash.clone())
                                    },
                                );
                                spawn_song_primary_actions(
                                    actions,
                                    font.clone(),
                                    &song,
                                    session,
                                    theme,
                                );
                            });
                    });

                    body.spawn(Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(18),
                        row_gap: px(18),
                        ..default()
                    })
                    .with_children(|columns| {
                        columns
                            .spawn((
                                Node {
                                    min_width: px(540),
                                    flex_basis: px(620),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.card.with_alpha(0.32)),
                                BorderColor::all(theme.border.with_alpha(0.55)),
                            ))
                            .with_children(|production| {
                                spawn_detail_heading(
                                    production,
                                    font.clone(),
                                    theme,
                                    "AUTHORING",
                                    "Production controls",
                                );
                                if song.is_analyzed
                                    && !matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    )
                                {
                                    spawn_shift_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Key",
                                        song.key
                                            .as_ref()
                                            .map(|key| format!("Original key: {key}"))
                                            .unwrap_or_else(|| {
                                                "Analyze again to detect the key.".to_string()
                                            }),
                                        song.override_key
                                            .as_ref()
                                            .or(song.key.as_ref())
                                            .cloned()
                                            .unwrap_or_else(|| "—".to_string()),
                                        UiAction::ShiftSongKey(song.file_hash.clone(), -1),
                                        UiAction::ShiftSongKey(song.file_hash.clone(), 1),
                                    );
                                    spawn_shift_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Tempo",
                                        "Create an export-speed variant in 0.1× steps.",
                                        format!("{:.1}×", song.tempo),
                                        UiAction::ShiftSongTempo(song.file_hash.clone(), -1),
                                        UiAction::ShiftSongTempo(song.file_hash.clone(), 1),
                                    );
                                } else {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Key & tempo",
                                        "Controls become available after compatible analysis.",
                                        None::<(&'static str, UiAction)>,
                                    );
                                }
                                spawn_setting_row(
                                    production,
                                    font.clone(),
                                    theme,
                                    "Lyrics",
                                    "Paste plain lyrics to realign, or provide timed LRC without replacing source media.",
                                    if matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    ) {
                                        None
                                    } else {
                                        Some((
                                            "Edit lyrics…".to_string(),
                                            UiAction::OpenLyricsEditor(song.file_hash.clone()),
                                        ))
                                    },
                                );
                                if !matches!(
                                    song.transcript_source,
                                    Some(app_core::TranscriptSource::Usdx)
                                ) {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Language",
                                        format!(
                                            "Current analysis language: {}. Choose whether to realign current lyrics or transcribe again.",
                                            song.language.as_deref().unwrap_or("automatic")
                                        ),
                                        Some((
                                            "Change language…",
                                            UiAction::OpenLanguageEditor(song.file_hash.clone()),
                                        )),
                                    );
                                }
                                spawn_setting_row(
                                    production,
                                    font.clone(),
                                    theme,
                                    "Analysis defaults",
                                    "Tune separator, transcription, alignment, pitch, batching, and sensitivity. Existing chart data changes only after re-analysis.",
                                    Some(("Open analysis settings", UiAction::SettingsTab(SettingsTab::Analysis))),
                                );
                                if song.is_analyzed
                                    && !matches!(
                                        song.transcript_source,
                                        Some(app_core::TranscriptSource::Usdx)
                                    )
                                {
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Word timing",
                                        "Rebuild timings from current lyrics using the selected alignment backend.",
                                        Some((
                                            "Realign",
                                            UiAction::RealignSong(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Lyrics source",
                                        "Refetch lyrics and align, or force a fresh transcription from the vocals.",
                                        Some((
                                            "Refetch & align",
                                            UiAction::ReanalyzeTranscript(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Transcription",
                                        "Ignore online lyrics and transcribe the vocals again.",
                                        Some((
                                            "Force transcribe",
                                            UiAction::ForceTranscribe(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Frequency analysis",
                                        "Generate or repair the editable pitch guide.",
                                        Some((
                                            "Analyze pitch",
                                            UiAction::ReanalyzePitch(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Full reanalysis",
                                        "Recreate stems, lyrics, timing, key, tempo, and pitch assets.",
                                        Some((
                                            "Reanalyze all",
                                            UiAction::ReanalyzeFull(song.file_hash.clone()),
                                        )),
                                    );
                                    spawn_setting_row(
                                        production,
                                        font.clone(),
                                        theme,
                                        "Generated song data",
                                        "Delete generated cache for this song. Source media is never changed.",
                                        Some((
                                            "Delete cache…",
                                            UiAction::RequestDeleteSongCache(song.file_hash.clone()),
                                        )),
                                    );
                                }
                            });

                        columns
                            .spawn((
                                Node {
                                    width: px(360),
                                    min_width: px(360),
                                    flex_grow: 1.0,
                                    flex_shrink: 0.0,
                                    flex_direction: FlexDirection::Column,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.card.with_alpha(0.28)),
                                BorderColor::all(theme.border.with_alpha(0.55)),
                            ))
                            .with_children(|overview| {
                                spawn_detail_heading(
                                    overview,
                                    font.clone(),
                                    theme,
                                    "PRODUCTION OVERVIEW",
                                    "Track information",
                                );
                                for (label, value) in song_overview_rows(&song) {
                                    spawn_detail_value(
                                        overview,
                                        font.clone(),
                                        theme,
                                        label,
                                        value,
                                    );
                                }
                                spawn_source_file_row(
                                    overview,
                                    font.clone(),
                                    theme,
                                    &song.path,
                                );
                            });
                    });

                    if let Some(notice) = session.notice.as_deref() {
                        spawn_wrapped_text(
                            body,
                            font.clone(),
                            notice,
                            10.0,
                            theme.muted_foreground,
                        );
                    }
                });
            if let Some(editor) = session.lyrics_editor.as_ref() {
                spawn_lyrics_editor(
                    detail,
                    font.clone(),
                    theme,
                    editor,
                    session.notice.as_deref(),
                );
            }
            if let Some(file_hash) = session.pending_cache_delete.as_deref() {
                spawn_cache_delete_confirmation(detail, font.clone(), theme, file_hash);
            }
            if let Some(editor) = session.language_editor.as_ref() {
                spawn_language_editor(
                    detail,
                    font.clone(),
                    theme,
                    editor,
                    session.notice.as_deref(),
                );
            }
        });
}

fn spawn_lyrics_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(80),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: percent(72),
                        max_width: px(760),
                        height: percent(78),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(22)),
                        row_gap: px(10),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "EDIT LYRICS", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        if editor.mode == LyricsInputMode::TimedLrc {
                            "Timed LRC"
                        } else {
                            "Plain lyrics"
                        },
                        18.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        if editor.mode == LyricsInputMode::TimedLrc {
                            "Paste line-level or enhanced LRC. Existing analyzed songs keep their stems; new songs can author over the original mix or explicitly queue separation."
                        } else if app_core::AppConfig::load().align_backend() == "mms_karaoke" {
                            "Enter one lyric phrase per line. MMS Karaoke accepts optional pronunciation overrides such as {漢字|かな} or [display|romaji]. Saving queues alignment and never modifies the source song."
                        } else {
                            "Enter one lyric phrase per line. Saving queues alignment and never modifies the source song."
                        },
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(8),
                            ..default()
                        })
                        .with_children(|options| {
                            spawn_text_button(
                                options,
                                font.clone(),
                                theme,
                                if editor.mode == LyricsInputMode::TimedLrc {
                                    "Use plain lyrics"
                                } else {
                                    "Use timed LRC"
                                },
                                10.0,
                                UiAction::ToggleLyricsInputMode,
                            );
                            if editor.mode == LyricsInputMode::TimedLrc {
                                spawn_text_button(
                                    options,
                                    font.clone(),
                                    theme,
                                    if editor.separate_stems {
                                        "Separate stems: on"
                                    } else {
                                        "Author on original mix"
                                    },
                                    10.0,
                                    UiAction::ToggleLyricsSeparateStems,
                                );
                            }
                            spawn_text_button(
                                options,
                                font.clone(),
                                theme,
                                if editor.searching {
                                    "Searching LRCLIB…"
                                } else if editor.candidates.is_empty() {
                                    "Find on LRCLIB"
                                } else {
                                    "Search LRCLIB again"
                                },
                                10.0,
                                UiAction::SearchLrclibLyrics,
                            );
                        });
                    if let Some(candidate) = editor.candidates.get(editor.candidate_index) {
                        dialog
                            .spawn((
                                Node {
                                    width: percent(100),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(11)),
                                    row_gap: px(6),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.primary.with_alpha(0.06)),
                                BorderColor::all(theme.primary.with_alpha(0.32)),
                            ))
                            .with_children(|match_card| {
                                match_card
                                    .spawn(Node {
                                        width: percent(100),
                                        align_items: AlignItems::Center,
                                        column_gap: px(8),
                                        ..default()
                                    })
                                    .with_children(|header| {
                                        spawn_text(
                                            header,
                                            font.clone(),
                                            format!(
                                                "LRCLIB MATCH  {} / {}",
                                                editor.candidate_index + 1,
                                                editor.candidates.len()
                                            ),
                                            8.0,
                                            theme.primary,
                                        );
                                        header.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        if editor.candidates.len() > 1 {
                                            spawn_text_button(
                                                header,
                                                font.clone(),
                                                theme,
                                                "Previous",
                                                9.0,
                                                UiAction::PreviousLrclibCandidate,
                                            );
                                            spawn_text_button(
                                                header,
                                                font.clone(),
                                                theme,
                                                "Next",
                                                9.0,
                                                UiAction::NextLrclibCandidate,
                                            );
                                        }
                                    });
                                spawn_text(
                                    match_card,
                                    font.clone(),
                                    candidate.track_name.clone(),
                                    11.0,
                                    theme.foreground,
                                );
                                spawn_wrapped_text(
                                    match_card,
                                    font.clone(),
                                    format!(
                                        "{}{} · {} lines · {}",
                                        candidate.artist_name,
                                        if candidate.album_name.trim().is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {}", candidate.album_name)
                                        },
                                        candidate.lines.len(),
                                        format_duration(candidate.duration_secs)
                                    ),
                                    9.0,
                                    theme.muted_foreground,
                                );
                                match_card
                                    .spawn(Node {
                                        width: percent(100),
                                        align_items: AlignItems::Center,
                                        column_gap: px(8),
                                        ..default()
                                    })
                                    .with_children(|actions| {
                                        if candidate.synced_lyrics.is_some() {
                                            spawn_action_button(
                                                actions,
                                                font.clone(),
                                                theme,
                                                "Use timed LRC",
                                                UiAction::UseLrclibTimed,
                                            );
                                        }
                                        if !candidate.lines.is_empty() {
                                            spawn_text_button(
                                                actions,
                                                font.clone(),
                                                theme,
                                                "Use as plain lyrics",
                                                9.0,
                                                UiAction::UseLrclibPlain,
                                            );
                                        }
                                    });
                            });
                    }
                    dialog.spawn((
                        LyricsEditorInput,
                        EditableText {
                            visible_lines: Some(16.0),
                            visible_width: Some(72.0),
                            allow_newlines: true,
                            max_characters: Some(100_000),
                            ..EditableText::new(&editor.initial_text)
                        },
                        Node {
                            width: percent(100),
                            min_height: px(0),
                            flex_grow: 1.0,
                            padding: UiRect::all(px(10)),
                            overflow: Overflow::scroll(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        ui_text_font(font.clone(), 11.0),
                        TextColor(theme.foreground),
                        TextLayout {
                            linebreak: bevy::text::LineBreak::WordOrCharacter,
                            ..default()
                        },
                        TextCursorStyle {
                            color: theme.primary,
                            selected_text_color: Some(theme.primary_foreground),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.65)),
                        BorderColor::all(theme.border.with_alpha(0.72)),
                        TabIndex(0),
                        AutoFocus,
                    ));
                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            9.0,
                            theme.destructive,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CloseLyricsEditor,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save lyrics",
                                UiAction::SaveLyricsEditor,
                            );
                        });
                });
        });
}

fn spawn_cache_delete_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
) {
    let title = app_core::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .map(|song| song.title)
        .unwrap_or_else(|| "this song".to_string());
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(90),
        children![(
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Delete generated song data?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "Generated stems, transcripts, pitch data, and derived variants for “{title}” will be removed. The source song remains untouched."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Delete generated data"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn spawn_language_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLanguageEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(92),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(470),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "LANGUAGE", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Change analysis language",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "Choose a supported language or let the analyzer detect it. The chosen action runs only after saving.",
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog
                        .spawn((
                            Button,
                            UiAction::ToggleLanguagePicker,
                            Node {
                                width: percent(100),
                                height: px(40),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(11)),
                                column_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.65)),
                            BorderColor::all(if editor.picker_open {
                                theme.primary.with_alpha(0.64)
                            } else {
                                theme.border.with_alpha(0.72)
                            }),
                        ))
                        .with_children(|selector| {
                            spawn_text(
                                selector,
                                font.clone(),
                                analysis_language_label(&editor.initial_language),
                                11.0,
                                theme.foreground,
                            );
                            selector.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                selector,
                                font.clone(),
                                editor.initial_language.to_ascii_uppercase(),
                                8.0,
                                theme.muted_foreground,
                            );
                            spawn_text(
                                selector,
                                font.clone(),
                                if editor.picker_open { "^" } else { "v" },
                                9.0,
                                theme.primary,
                            );
                        });
                    if editor.picker_open {
                        dialog
                            .spawn((
                                ScrollPosition::default(),
                                Node {
                                    width: percent(100),
                                    max_height: px(238),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(5)),
                                    row_gap: px(2),
                                    overflow: Overflow::scroll_y(),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.82)),
                                BorderColor::all(theme.border.with_alpha(0.72)),
                            ))
                            .with_children(|options| {
                                for (code, label) in ANALYSIS_LANGUAGE_OPTIONS {
                                    let selected = editor.initial_language == *code;
                                    options
                                        .spawn((
                                            Button,
                                            UiAction::SelectAnalysisLanguage((*code).into()),
                                            Node {
                                                width: percent(100),
                                                min_height: px(30),
                                                align_items: AlignItems::Center,
                                                padding: UiRect::horizontal(px(9)),
                                                column_gap: px(8),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(if selected {
                                                theme.primary.with_alpha(0.13)
                                            } else {
                                                Color::NONE
                                            }),
                                        ))
                                        .with_children(|option| {
                                            spawn_text(
                                                option,
                                                font.clone(),
                                                *label,
                                                9.0,
                                                if selected {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                            option.spawn(Node {
                                                flex_grow: 1.0,
                                                ..default()
                                            });
                                            spawn_text(
                                                option,
                                                font.clone(),
                                                code.to_ascii_uppercase(),
                                                8.0,
                                                if selected {
                                                    theme.primary
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                        });
                                }
                            });
                    }
                    spawn_text_button(
                        dialog,
                        font.clone(),
                        theme,
                        if editor.force_transcribe {
                            "Action: transcribe vocals again"
                        } else {
                            "Action: realign current lyrics"
                        },
                        10.0,
                        UiAction::ToggleLanguageReprocess,
                    );
                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            9.0,
                            theme.destructive,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CloseLanguageEditor,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save & reprocess",
                                UiAction::SaveLanguageEditor,
                            );
                        });
                });
        });
}

fn spawn_song_primary_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    song: &Song,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    if let Some(task) = session
        .analysis_tasks
        .iter()
        .find(|task| task.file_hash == song.file_hash)
        && matches!(
            task.status,
            app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
        )
    {
        let label = match task.status {
            app_core::QueuedStatus::Queued => "Queued for analysis".to_string(),
            app_core::QueuedStatus::Analyzing(progress) => {
                format!("Analyzing · {progress}%")
            }
            app_core::QueuedStatus::Failed(_) => unreachable!(),
        };
        spawn_action_button(parent, font, theme, label, UiAction::ToggleActivity);
        return;
    }

    if !song.is_analyzed {
        if app_core::analysis_runtime_status().ready {
            spawn_action_button(
                parent,
                font,
                theme,
                "Analyze song",
                UiAction::AnalyzeSong(song.file_hash.clone()),
            );
        } else {
            spawn_action_button(
                parent,
                font,
                theme,
                "Set up analysis",
                UiAction::SettingsTab(SettingsTab::Models),
            );
        }
        return;
    }

    if song.authoring_ready {
        spawn_action_button(
            parent,
            font.clone(),
            theme,
            "Export UTZ",
            UiAction::ExportUtz(song.file_hash.clone()),
        );
        spawn_action_button(
            parent,
            font.clone(),
            theme,
            "Export UltraStar",
            UiAction::ExportUltraStar(song.file_hash.clone()),
        );
    }
    spawn_action_button(
        parent,
        font,
        theme,
        if song.editor_ready {
            "Edit chart"
        } else {
            "Prepare & edit"
        },
        UiAction::OpenEditor(song.file_hash.clone()),
    );
}

fn spawn_detail_heading(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                padding: UiRect::axes(px(16), px(13)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|header| {
            spawn_text(header, font.clone(), eyebrow, 8.0, theme.primary);
            spawn_text(header, font, title, 13.0, theme.foreground);
        });
}

fn spawn_detail_value(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    value: String,
) {
    parent
        .spawn((
            Node {
                min_height: px(48),
                padding: UiRect::axes(px(14), px(10)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.3)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), label, 9.0, theme.muted_foreground);
            spawn_wrapped_text(row, font, value, 11.0, theme.foreground);
        });
}

fn song_overview_rows(song: &Song) -> Vec<(&'static str, String)> {
    let media = song
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("media")
        .to_ascii_uppercase();
    let transcript = song
        .transcript_source
        .as_ref()
        .map(|source| format!("{source:?}"))
        .unwrap_or_else(|| "Not generated".to_string());
    vec![
        (
            "Media",
            format!(
                "{media} · {}",
                if song.is_video { "Video" } else { "Audio" }
            ),
        ),
        (
            "Analysis",
            if song.is_analyzed {
                "Analyzed"
            } else {
                "Not analyzed"
            }
            .to_string(),
        ),
        ("Lyrics source", transcript),
        (
            "Stems",
            if song.no_stems {
                "Original mix"
            } else if song.is_analyzed {
                "Separated"
            } else {
                "Pending"
            }
            .to_string(),
        ),
        (
            "Chart assets",
            if song.authoring_ready {
                "Complete".to_string()
            } else if song.authoring_missing.is_empty() {
                "Waiting for chart".to_string()
            } else {
                song.authoring_missing.join(" · ").replace('_', " ")
            },
        ),
        (
            "Export",
            if song.authoring_ready {
                "UTZ · UltraStar"
            } else {
                "Waiting for chart"
            }
            .to_string(),
        ),
    ]
}

fn album_art_handle(
    song: &Song,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
) -> Handle<Image> {
    let Some(path) = song.album_art_path.as_ref() else {
        return asset_server.load(LOGO_PATH);
    };
    if let Some(handle) = local_images.covers.get(path) {
        return handle.clone();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return asset_server.load(LOGO_PATH);
    };
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else {
        "jpg"
    };
    let Ok(decoded) = Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    ) else {
        return asset_server.load(LOGO_PATH);
    };
    let Ok(dynamic) = decoded.try_into_dynamic() else {
        return asset_server.load(LOGO_PATH);
    };
    // Library artwork can be several thousand pixels wide while its largest
    // presentation in the desktop UI is a small cover. Bounding retained
    // textures prevents a route change from uploading another full-resolution
    // image while the analyzer has recently held several gigabytes of models.
    let bounded = dynamic.thumbnail(512, 512);
    let image = Image::from_dynamic(bounded, true, RenderAssetUsages::default());
    let handle = images.add(image);
    local_images.covers.insert(path.clone(), handle.clone());
    handle
}

fn ambient_album_art_handle(
    song: &Song,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
) -> Handle<Image> {
    let Some(path) = song.album_art_path.as_ref() else {
        return asset_server.load(LOGO_PATH);
    };
    if let Some(handle) = local_images.ambient_covers.get(path) {
        return handle.clone();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return asset_server.load(LOGO_PATH);
    };
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else {
        "jpg"
    };
    let Ok(decoded) = Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    ) else {
        return asset_server.load(LOGO_PATH);
    };
    let Ok(dynamic) = decoded.try_into_dynamic() else {
        return asset_server.load(LOGO_PATH);
    };
    // Prepare a wide ambient crop before the UI stretches it across the hero.
    // Stretching a square blurred cover into a banner creates visible horizontal
    // bands; a centered 5:1 crop keeps the colour field natural and restrained.
    let softened = dynamic.thumbnail(1200, 1200).fast_blur(12.0);
    let source_width = softened.width();
    let source_height = softened.height();
    let (crop_width, crop_height) = if source_width >= source_height.saturating_mul(5) {
        (source_height.saturating_mul(5), source_height)
    } else {
        (source_width, (source_width / 5).max(1))
    };
    let cropped = softened.crop_imm(
        (source_width - crop_width) / 2,
        (source_height - crop_height) / 2,
        crop_width,
        crop_height,
    );
    let mut ambient = Image::from_dynamic(cropped, true, RenderAssetUsages::default());
    ambient.sampler = ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor::linear());
    let handle = images.add(ambient);
    local_images
        .ambient_covers
        .insert(path.clone(), handle.clone());
    handle
}

fn spawn_folders(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(34), px(26)),
            row_gap: px(18),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|page| {
            page.spawn((
                Node {
                    width: percent(100),
                    min_height: px(70),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexEnd,
                    padding: UiRect::bottom(px(16)),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.45)),
            ))
            .with_children(|header| {
                header
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), "MY LIBRARY", 8.0, theme.primary);
                        spawn_text(copy, font.clone(), "Folders", 24.0, theme.foreground);
                        spawn_wrapped_text(
                            copy,
                            font.clone(),
                            "Browse watched source locations and open the configured output folder. Uta Studio never moves or deletes source media.",
                            10.0,
                            theme.muted_foreground,
                        );
                    });
                spawn_toolbar_button(
                    header,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::Repeat,
                    "Rescan all",
                    UiAction::RescanLibrary,
                    false,
                );
                spawn_toolbar_button(
                    header,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::Add,
                    "Add folder",
                    UiAction::ChooseFolder,
                    false,
                );
            });

            page.spawn(Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                column_gap: px(16),
                ..default()
            })
            .with_children(|body| {
                body.spawn((
                    Node {
                        width: px(240),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(10)),
                        row_gap: px(3),
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.44)),
                    BorderColor::all(theme.border.with_alpha(0.46)),
                ))
                .with_children(|roots| {
                    spawn_text(
                        roots,
                        font.clone(),
                        format!(
                            "WATCHED LOCATIONS · {}",
                            session.config.library_paths().len()
                        ),
                        8.0,
                        theme.muted_foreground,
                    );
                    roots.spawn(Node {
                        height: px(5),
                        ..default()
                    });
                    for root in session.config.library_paths() {
                        let selected = session.folder_browser.root.as_ref() == Some(&root);
                        roots
                            .spawn((
                                Node {
                                    width: percent(100),
                                    min_height: px(46),
                                    align_items: AlignItems::Center,
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.foreground.with_alpha(0.07)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Button,
                                    UiAction::SelectFolderRoot(root.clone()),
                                    Node {
                                        min_width: px(0),
                                        height: percent(100),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Center,
                                        padding: UiRect::horizontal(px(8)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    children![
                                        (
                                            Text::new(folder_name(&root)),
                                            ui_text_font(font.clone(), 11.0),
                                            TextColor(theme.foreground),
                                            TextLayout::no_wrap(),
                                        ),
                                        (
                                            Text::new(root.to_string_lossy().into_owned()),
                                            ui_text_font(font.clone(), 8.0),
                                            TextColor(theme.muted_foreground.with_alpha(0.64)),
                                            TextLayout::no_wrap(),
                                        )
                                    ],
                                ));
                                spawn_text_button(
                                    row,
                                    font.clone(),
                                    theme,
                                    "×",
                                    13.0,
                                    UiAction::RequestRemoveFolder(root),
                                );
                            });
                    }
                    if session.config.library_paths().is_empty() {
                        spawn_wrapped_text(
                            roots,
                            font.clone(),
                            "No folders added yet.",
                            10.0,
                            theme.muted_foreground,
                        );
                    }
                    roots.spawn((
                        Node {
                            width: percent(100),
                            margin: UiRect::top(px(12)),
                            padding: UiRect::top(px(12)),
                            border: UiRect::top(px(1)),
                            ..default()
                        },
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ));
                    spawn_text(
                        roots,
                        font.clone(),
                        "OUTPUT FOLDER",
                        8.0,
                        theme.muted_foreground,
                    );
                    roots.spawn(Node {
                        height: px(5),
                        ..default()
                    });
                    if let Some(path) = session.config.export_path.as_ref() {
                        let selected = session.folder_browser.root.as_ref() == Some(path);
                        roots
                            .spawn((
                                Node {
                                    width: percent(100),
                                    min_height: px(52),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.foreground.with_alpha(0.07)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Button,
                                    UiAction::SelectFolderRoot(path.clone()),
                                    Node {
                                        width: percent(100),
                                        min_height: px(52),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::FlexStart,
                                        padding: UiRect::horizontal(px(8)),
                                        row_gap: px(2),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|output| {
                                    spawn_text(
                                        output,
                                        font.clone(),
                                        folder_name(path),
                                        11.0,
                                        theme.foreground,
                                    );
                                    output
                                        .spawn(Node {
                                            width: percent(100),
                                            overflow: Overflow::clip(),
                                            ..default()
                                        })
                                        .with_children(|path_copy| {
                                            path_copy.spawn((
                                                Text::new(path.to_string_lossy().into_owned()),
                                                ui_text_font(font.clone(), 8.0),
                                                TextColor(
                                                    theme.muted_foreground.with_alpha(0.64),
                                                ),
                                                TextLayout::no_wrap(),
                                            ));
                                        });
                                });
                            });
                    } else {
                        roots
                            .spawn((
                                Button,
                                UiAction::ChooseExportFolder,
                                Node {
                                    width: percent(100),
                                    min_height: px(52),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::horizontal(px(8)),
                                    row_gap: px(2),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(theme.foreground.with_alpha(0.035)),
                            ))
                            .with_children(|output| {
                                spawn_text(
                                    output,
                                    font.clone(),
                                    "System default",
                                    11.0,
                                    theme.foreground,
                                );
                                spawn_wrapped_text(
                                    output,
                                    font.clone(),
                                    "Choose an output folder",
                                    8.0,
                                    theme.muted_foreground,
                                );
                            });
                    }
                });

                body.spawn((
                    Node {
                        min_width: px(0),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.42)),
                    BorderColor::all(theme.border.with_alpha(0.46)),
                ))
                .with_children(|browser| {
                    browser
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(48),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(12)),
                                column_gap: px(8),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BorderColor::all(theme.border.with_alpha(0.45)),
                        ))
                        .with_children(|path_bar| {
                            spawn_icon_button(
                                path_bar,
                                icons.clone(),
                                theme,
                                UiIcon::ArrowLeft,
                                UiAction::FolderUp,
                                false,
                                false,
                                32.0,
                            );
                            spawn_icon(
                                path_bar,
                                icons.clone(),
                                UiIcon::Folder,
                                15.0,
                                theme.primary,
                            );
                            spawn_text(
                                path_bar,
                                font.clone(),
                                session
                                    .folder_browser
                                    .current
                                    .as_ref()
                                    .map(|path| path.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "Choose a watched folder".to_string()),
                                10.0,
                                theme.muted_foreground,
                            );
                        });

                    browser
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(30),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(14)),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BorderColor::all(theme.border.with_alpha(0.32)),
                        ))
                        .with_children(|columns| {
                            spawn_text(columns, font.clone(), "Name", 9.0, theme.muted_foreground);
                            columns.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(columns, font.clone(), "Kind", 9.0, theme.muted_foreground);
                            columns.spawn(Node {
                                width: px(50),
                                ..default()
                            });
                            spawn_text(columns, font.clone(), "Size", 9.0, theme.muted_foreground);
                        });

                    browser
                        .spawn((
                            FolderEntryList,
                            ScrollPosition::default(),
                            Node {
                                min_width: px(0),
                                min_height: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|list| {
                            if let Some(error) = session.folder_browser.error.as_deref() {
                                spawn_wrapped_text(
                                    list,
                                    font.clone(),
                                    format!("Could not read this folder: {error}"),
                                    10.0,
                                    theme.destructive,
                                );
                            } else if session.folder_browser.current.is_none() {
                                spawn_wrapped_text(
                                    list,
                                    font.clone(),
                                    "Add a music folder to begin.",
                                    11.0,
                                    theme.muted_foreground,
                                );
                            } else {
                                for entry in &session.folder_browser.entries {
                                    spawn_folder_entry(
                                        list,
                                        font.clone(),
                                        icons.clone(),
                                        theme,
                                        entry,
                                    );
                                }
                                if session.folder_browser.entries.is_empty() {
                                    spawn_wrapped_text(
                                        list,
                                        font.clone(),
                                        "This folder is empty.",
                                        10.0,
                                        theme.muted_foreground,
                                    );
                                }
                            }
                        });
                });
            });

            if let Some(context) = session.folder_browser.context_menu.as_ref() {
                spawn_folder_context_menu(page, font.clone(), theme, context);
            }
            if let Some(path) = session.folder_browser.pending_remove.as_ref() {
                spawn_remove_folder_confirmation(page, font.clone(), theme, path);
            }
            if let Some(notice) = session.notice.as_deref() {
                page.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(14),
                        right: px(14),
                        bottom: px(10),
                        min_height: px(30),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.88)),
                    ZIndex(60),
                    children![(
                        Text::new(notice),
                        ui_text_font(font, 9.0),
                        TextColor(theme.muted_foreground),
                    )],
                ));
            }
        });
}

fn spawn_folder_entry(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    entry: &LibraryFolderEntry,
) {
    let context_entry = entry.clone();
    parent
        .spawn((
            Button,
            Node {
                width: percent(100),
                min_height: px(38),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(14)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            })
            .with_children(|name| {
                spawn_icon(
                    name,
                    icons,
                    folder_entry_icon(&entry.kind),
                    14.0,
                    folder_entry_color(&entry.kind, theme),
                );
                spawn_text(
                    name,
                    font.clone(),
                    entry.name.clone(),
                    10.0,
                    theme.foreground,
                );
            });
            row.spawn(Node {
                width: px(82),
                ..default()
            })
            .with_children(|kind| {
                spawn_text(
                    kind,
                    font.clone(),
                    entry.kind.clone(),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(68),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|size| {
                spawn_text(
                    size,
                    font,
                    if entry.size_bytes == 0 {
                        "—".to_string()
                    } else {
                        format_bytes(entry.size_bytes)
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                match event.button {
                    PointerButton::Primary => {
                        let path = PathBuf::from(&context_entry.path);
                        if context_entry.kind == "folder" {
                            session.folder_browser.current = Some(path);
                            session.folder_browser.context_menu = None;
                            session.folder_browser.refresh();
                            session.notice = None;
                        } else if event.count >= 2 {
                            session.notice = Some(open_library_entry(&path, &session.config));
                        }
                        invalidated.0 = true;
                    }
                    PointerButton::Secondary => {
                        session.folder_browser.context_menu = Some(FolderContextMenu {
                            entry: context_entry.clone(),
                            position: event.pointer_location.position,
                        });
                        invalidated.0 = true;
                    }
                    PointerButton::Middle => {}
                }
            },
        );
}

fn spawn_folder_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &FolderContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::DismissFolderContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let left = (context.position.x - SIDEBAR_WIDTH - 12.0).max(8.0);
    let top = (context.position.y - 58.0).max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(220),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.entry.name.clone(),
                9.0,
                theme.muted_foreground,
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                if context.entry.kind == "folder" {
                    "Open folder"
                } else {
                    "Open"
                },
                11.0,
                UiAction::OpenFolderEntry(PathBuf::from(&context.entry.path)),
            );
            spawn_text_button(
                menu,
                font,
                theme,
                "Reveal in folder",
                11.0,
                UiAction::RevealFolderEntry(PathBuf::from(&context.entry.path)),
            );
        });
}

fn spawn_remove_folder_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    path: &std::path::Path,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.72)),
        ZIndex(70),
        children![(
            Node {
                width: px(430),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(22)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Stop watching this folder?"),
                    ui_text_font(font.clone(), 16.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "{}\n\nUta Studio will update its library index but will not move or delete any source media.",
                        path.display()
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelRemoveFolder,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmRemoveFolder,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Stop watching"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn folder_entry_icon(kind: &str) -> UiIcon {
    match kind {
        "folder" => UiIcon::Folder,
        "video" => UiIcon::Video,
        "playlist" => UiIcon::List,
        "chart" => UiIcon::Queue,
        _ => UiIcon::Music,
    }
}

fn folder_entry_color(kind: &str, theme: &StudioTheme) -> Color {
    match kind {
        "folder" => Color::srgb(0.82, 0.59, 0.22),
        "video" => Color::srgb(0.24, 0.7, 0.75),
        "playlist" => Color::srgb(0.24, 0.68, 0.48),
        "chart" => Color::srgb(0.58, 0.42, 0.78),
        _ => theme.primary,
    }
}

fn spawn_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|settings| {
            settings
                .spawn((
                    Node {
                        width: px(224),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(24), px(28)),
                        row_gap: px(4),
                        border: UiRect::right(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.38)),
                    BorderColor::all(theme.border.with_alpha(0.26)),
                ))
                .with_children(|nav| {
                    spawn_text(nav, font.clone(), "UTA STUDIO", 8.0, theme.primary);
                    spawn_text(nav, font.clone(), "Settings", 20.0, theme.foreground);
                    spawn_wrapped_text(
                        nav,
                        font.clone(),
                        "Workspace, library, and generation.",
                        10.0,
                        theme.muted_foreground,
                    );
                    nav.spawn(Node {
                        height: px(18),
                        ..default()
                    });
                    for (tab, icon, label) in [
                        (SettingsTab::General, UiIcon::Monitor, "General"),
                        (SettingsTab::Storage, UiIcon::Database, "Storage"),
                        (SettingsTab::Models, UiIcon::Box, "Models & runtime"),
                        (SettingsTab::Analysis, UiIcon::Sparkles, "Analysis"),
                    ] {
                        spawn_settings_tab(
                            nav,
                            font.clone(),
                            icons.clone(),
                            theme,
                            tab,
                            icon,
                            label,
                            session.settings_tab == tab,
                        );
                    }
                });

            settings
                .spawn((
                    SettingsContent,
                    ScrollPosition(Vec2::new(
                        0.0,
                        session.settings_scroll_offsets[session.settings_tab.index()],
                    )),
                    Node {
                        min_width: px(0),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(40), px(34)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.54)),
                ))
                .with_children(|content| {
                    // A scrollable flex column otherwise shrinks its direct
                    // children to the viewport height before measuring overflow.
                    // Keep one intrinsic-height page so setting rows retain their
                    // intended height and the content scrolls instead of stacking.
                    content
                        .spawn(Node {
                            width: percent(100),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|page| {
                            match session.settings_tab {
                                SettingsTab::General => {
                                    spawn_general_settings(page, font.clone(), session, theme)
                                }
                                SettingsTab::Storage => spawn_storage_settings(
                                    page,
                                    font.clone(),
                                    session,
                                    &cache_stats,
                                    theme,
                                ),
                                SettingsTab::Models => spawn_model_settings(
                                    page,
                                    font.clone(),
                                    icons.clone(),
                                    session,
                                    native_setup,
                                    theme,
                                ),
                                SettingsTab::Analysis => spawn_analysis_settings(
                                    page,
                                    font.clone(),
                                    icons.clone(),
                                    session,
                                    theme,
                                ),
                            }
                            if let Some(notice) = session.notice.as_deref() {
                                page.spawn(Node {
                                    height: px(14),
                                    ..default()
                                });
                                spawn_wrapped_text(
                                    page,
                                    font.clone(),
                                    notice,
                                    10.0,
                                    theme.muted_foreground,
                                );
                            }
                        });
                });
            if let Some(request) = session.pending_setup {
                spawn_setup_confirmation(settings, font.clone(), theme, request);
            }
            if let Some(scope) = session.pending_cache_clear {
                spawn_global_cache_confirmation(settings, font.clone(), theme, scope);
            }
            if let Some(path) = session.folder_browser.pending_remove.as_deref() {
                spawn_remove_folder_confirmation(settings, font, theme, path);
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_settings_tab(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    tab: SettingsTab,
    icon: UiIcon,
    label: &'static str,
    active: bool,
) {
    parent
        .spawn((
            Button,
            UiAction::SettingsTab(tab),
            Node {
                width: percent(100),
                height: px(36),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            let color = if active {
                theme.primary
            } else {
                theme.muted_foreground
            };
            spawn_icon(row, icons, icon, 15.0, color);
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(
                row,
                font,
                label,
                11.0,
                if active {
                    theme.foreground.with_alpha(0.78)
                } else {
                    theme.muted_foreground
                },
            );
        });
}

fn spawn_settings_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
    description: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::bottom(px(20)),
                margin: UiRect::bottom(px(6)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|header| {
            spawn_text(header, font.clone(), eyebrow, 8.0, theme.primary);
            spawn_text(header, font.clone(), title, 20.0, theme.foreground);
            spawn_wrapped_text(header, font, description, 10.0, theme.muted_foreground);
        });
}

fn spawn_general_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "WORKSPACE",
        "General",
        "Window behavior and diagnostic tools.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Dark mode",
        "Enable a dark palette across the application.",
        session.config.dark_mode.unwrap_or(false),
        UiAction::ToggleTheme,
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Fullscreen workspace",
        if session.config.fullscreen.unwrap_or(false) {
            "The editor fills this display."
        } else {
            "The app uses a standard window."
        },
        session.config.fullscreen.unwrap_or(false),
        UiAction::ToggleFullscreen,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Application log",
        "Review recent events when analysis, editing, or export needs troubleshooting.",
        Some(("View log", UiAction::OpenLog)),
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Feature API diagnostics",
        "Verify local APIs, native audio, and real UTZ/UltraStar exports in a unique temporary folder that is always removed.",
        Some(("Run checks", UiAction::RunDiagnostics)),
    );
    spawn_shift_setting_row(
        parent,
        font.clone(),
        theme,
        "Font size",
        "Set the base UI font size. The interface is scaled using this size (10px–18px), which maps to 80%–140%.",
        format!(
            "{}px",
            ui_font_size_percent_to_points(session.config.font_scale_percent())
        ),
        UiAction::AdjustUiFontScale(-1),
        UiAction::AdjustUiFontScale(1),
    );
    if let Some(report) = session.diagnostic_report.as_ref() {
        spawn_diagnostics_report(parent, font.clone(), theme, report);
    }
}

fn spawn_diagnostics_report(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    report: &uta_studio_diagnostics::DiagnosticReport,
) {
    let status_text = if report.ok {
        "Passed"
    } else {
        "Needs attention"
    };
    let status_color = if report.ok {
        theme.primary
    } else {
        theme.destructive
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(132),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(14)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.42)),
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|summary| {
                    spawn_text(
                        summary,
                        font.clone(),
                        "Diagnostic results",
                        10.0,
                        theme.muted_foreground,
                    );
                    summary.spawn((
                        Node {
                            padding: UiRect::axes(px(9), px(3)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(status_color.with_alpha(0.16)),
                        BorderColor::all(status_color.with_alpha(0.45)),
                        children![(
                            Text::new(status_text),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(status_color),
                        )],
                    ));
                });
            spawn_text(
                panel,
                font.clone(),
                format!(
                    "{} passed · {} failed · {} skipped · {} APIs",
                    report.passed, report.failed, report.skipped, report.capabilities
                ),
                10.0,
                theme.foreground,
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|checks| {
                    for check in &report.checks {
                        spawn_diagnostic_check_row(checks, font.clone(), theme, check);
                    }
                });
        });
}

fn spawn_diagnostic_check_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    check: &uta_studio_diagnostics::DiagnosticCheck,
) {
    let status_color = diagnostic_status_color(check.status, theme);
    let status_label = diagnostic_status_label(check.status);
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(7),
            padding: UiRect::bottom(px(10)),
            border: UiRect::bottom(px(1)),
            margin: UiRect::bottom(px(4)),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: percent(100),
                align_items: AlignItems::FlexStart,
                justify_content: JustifyContent::SpaceBetween,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(10),
                ..default()
            })
            .with_children(|heading| {
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|labels| {
                        labels
                            .spawn(Node {
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|id_line| {
                                spawn_text(id_line, font.clone(), check.id, 9.0, theme.foreground);
                                id_line.spawn((
                                    Node {
                                        padding: UiRect::axes(px(6), px(2)),
                                        border_radius: BorderRadius::all(px(999.0)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted.with_alpha(0.16)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                    children![(
                                        Text::new(status_label),
                                        ui_text_font(font.clone(), 8.0),
                                        TextColor(status_color),
                                    )],
                                ));
                            });
                        spawn_text(
                            labels,
                            font.clone(),
                            format!("{}ms", check.elapsed_ms),
                            8.0,
                            theme.muted_foreground,
                        );
                    });
                spawn_text(heading, font.clone(), check.status, 8.0, status_color);
            });
            row.spawn(Node {
                width: percent(100),
                min_width: px(0),
                ..default()
            })
            .with_children(|details| {
                spawn_wrapped_text(
                    details,
                    font.clone(),
                    format!("{} • {}", status_label, check.detail),
                    8.8,
                    theme.muted_foreground,
                );
            });
        });
}

fn diagnostic_status_color<'a>(status: &str, theme: &'a StudioTheme) -> Color {
    match status {
        "passed" => theme.primary,
        "failed" => theme.destructive,
        _ => theme.muted_foreground,
    }
}

fn diagnostic_status_label(status: &str) -> &'static str {
    match status {
        "passed" => "OK",
        "failed" => "FAIL",
        _ => "SKIP",
    }
}

fn spawn_storage_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "LIBRARY",
        "Storage",
        "Manage watched folders and generated data. Your source media is never moved or deleted.",
    );
    spawn_watched_folders_setting(parent, font.clone(), session, theme);
    let export_path = session
        .config
        .export_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Use the last folder chosen by the system dialog".to_string());
    spawn_setting_row_with_actions(
        parent,
        font.clone(),
        theme,
        "Default export folder",
        format!(
            "Every format opens Save As here first. You can still choose another folder for each export.\n\n{export_path}"
        ),
        vec![
            ("Choose…".to_string(), UiAction::ChooseExportFolder),
            (
                "Use system default".to_string(),
                UiAction::ClearExportFolder,
            ),
        ],
    );
    spawn_storage_usage_row(parent, font.clone(), theme, cache_stats);
}

fn spawn_storage_usage_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    cache_stats: &CacheStatsJob,
) {
    let (status, status_color, status_summary) =
        match (cache_stats.current.as_ref(), cache_stats.receiver.is_some()) {
            (Some(stats), false) => {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                (
                    "Current",
                    theme.foreground,
                    format!("Latest scan: {}", format_bytes(total)),
                )
            }
            (Some(stats), true) => {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                (
                    "Recalculating",
                    theme.primary,
                    format!(
                        "Recalculating in background. Latest scan: {}",
                        format_bytes(total)
                    ),
                )
            }
            (None, true) => (
                "Calculating",
                theme.primary,
                "Calculating generated storage usage. This may scan configured cache folders."
                    .to_string(),
            ),
            (None, false) => (
                "Not calculated",
                theme.muted_foreground,
                "Open Storage again or clear one cache entry to start a scan.".to_string(),
            ),
        };
    let mut status_description = status_summary;
    if let Some(error) = cache_stats.error.as_deref() {
        status_description = format!("Cache stats failed to calculate: {error}");
    }
    let status_text_color = if cache_stats.error.is_some() {
        theme.destructive
    } else {
        theme.muted_foreground
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(224),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(20), px(16)),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(32),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                "Generated storage",
                                12.0,
                                theme.foreground,
                            );
                            copy.spawn(Node {
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                flex_wrap: FlexWrap::Wrap,
                                ..default()
                            })
                            .with_children(|status_row| {
                                spawn_text(
                                    status_row,
                                    font.clone(),
                                    "Usage",
                                    9.0,
                                    theme.muted_foreground,
                                );
                                status_row.spawn((
                                    Node {
                                        padding: UiRect::axes(px(8), px(3)),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(status_color.with_alpha(0.16)),
                                    BorderColor::all(status_color.with_alpha(0.45)),
                                    children![(
                                        Text::new(status),
                                        ui_text_font(font.clone(), 9.0),
                                        TextColor(status_color),
                                    )],
                                ));
                            });
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "Cached stems, charts, previews, models, and temporary authoring files.",
                                10.0,
                                theme.muted_foreground,
                            );
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                status_description,
                                10.0,
                                status_text_color,
                            );
                        });
                    spawn_setting_actions(
                        header,
                        font.clone(),
                        theme,
                        vec![
                            (
                                "Clear generated cache".to_string(),
                                UiAction::RequestClearCache(CacheClearScope::Generated),
                            ),
                            (
                                "Clear models".to_string(),
                                UiAction::RequestClearCache(CacheClearScope::Models),
                            ),
                        ],
                    );
                });

            if let Some(stats) = cache_stats.current.as_ref() {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(12),
                            padding: UiRect::all(px(12)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(7)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.24)),
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ))
                    .with_children(|bars| {
                        spawn_text(
                            bars,
                            font.clone(),
                            "Storage breakdown",
                            8.0,
                            theme.muted_foreground,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Songs",
                            stats.songs_bytes,
                            cache_category_share(stats.songs_bytes, total),
                            theme.primary,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Models",
                            stats.models_bytes,
                            cache_category_share(stats.models_bytes, total),
                            theme.editor_selection,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Other",
                            stats.other_bytes,
                            cache_category_share(stats.other_bytes, total),
                            theme.waveform,
                        );
                    });
            }
        });
}

fn cache_category_share(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) as f32
    }
}

fn spawn_storage_usage_category(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    bytes: u64,
    share: f32,
    color: Color,
) {
    let share = (share * 100.0).clamp(0.0, 100.0);
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        })
        .with_children(|entry| {
            entry
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    spawn_text(row, font.clone(), label, 8.0, theme.muted_foreground);
                    spawn_text(
                        row,
                        font.clone(),
                        format!("{} · {:.0}%", format_bytes(bytes), share),
                        9.0,
                        theme.foreground,
                    );
                });
            entry
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(7),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.36)),
                    BorderColor::all(theme.border.with_alpha(0.45)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: percent(share),
                            height: px(7),
                            border_radius: BorderRadius::all(px(999.0)),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                });
        });
}

fn spawn_watched_folders_setting(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let paths = session.config.library_paths();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(104),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(20), px(16)),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(32),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(5),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                "Watched folders",
                                12.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "Add as many music locations as you need. Folder changes are merged into one library.",
                                10.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_setting_actions(
                        header,
                        font.clone(),
                        theme,
                        vec![
                            ("Add folder…".to_string(), UiAction::ChooseFolder),
                            ("Rescan all".to_string(), UiAction::RescanLibrary),
                        ],
                    );
                });

            if paths.is_empty() {
                panel
                    .spawn(Node {
                        width: percent(100),
                        min_height: px(34),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(9)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    })
                    .with_children(|empty| {
                        spawn_wrapped_text(
                            empty,
                            font.clone(),
                            "No local folders connected.",
                            9.0,
                            theme.muted_foreground,
                        );
                    });
            } else {
                for path in &paths {
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(38),
                                align_items: AlignItems::Center,
                                padding: UiRect::vertical(px(2)),
                                column_gap: px(32),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.32)),
                        ))
                        .with_children(|path_row| {
                            path_row
                                .spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    padding: UiRect::horizontal(px(9)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                })
                                .with_children(|path_copy| {
                                    path_copy.spawn((
                                        Text::new(path.to_string_lossy().into_owned()),
                                        ui_text_font(font.clone(), 9.0),
                                        TextColor(theme.muted_foreground),
                                        TextLayout::no_wrap(),
                                    ));
                                });
                            path_row
                                .spawn(Node {
                                    width: px(SETTINGS_CONTROL_WIDTH),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::FlexEnd,
                                    ..default()
                                })
                                .with_children(|actions| {
                                    spawn_compact_action_button(
                                        actions,
                                        font.clone(),
                                        theme,
                                        "Remove",
                                        UiAction::RequestRemoveFolder(path.clone()),
                                    );
                                });
                        });
                }
            }
        });
}

fn spawn_settings_section(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect {
                left: px(20),
                right: px(20),
                top: px(20),
                bottom: px(7),
            },
            row_gap: px(3),
            ..default()
        })
        .with_children(|section| {
            spawn_text(section, font.clone(), label, 8.0, theme.primary);
            spawn_wrapped_text(section, font, description, 9.0, theme.muted_foreground);
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_settings_stage_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
    current: impl Into<String>,
    status: Option<(String, bool)>,
    action: Option<(String, UiAction)>,
) {
    let eyebrow = eyebrow.into();
    let title = title.into();
    let description = description.into();
    let current = current.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(92),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                margin: UiRect::top(px(16)),
                column_gap: px(32),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|copy| {
                    spawn_text(copy, font.clone(), eyebrow, 8.0, theme.primary);
                    spawn_text(copy, font.clone(), title, 14.0, theme.foreground);
                    spawn_wrapped_text(
                        copy,
                        font.clone(),
                        description,
                        9.0,
                        theme.muted_foreground,
                    );
                });
            header
                .spawn(Node {
                    width: px(SETTINGS_CONTROL_WIDTH),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexEnd,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|summary| {
                    spawn_text(summary, font.clone(), current, 10.0, theme.foreground);
                    if let Some((label, available)) = status {
                        spawn_settings_badge(
                            summary,
                            font.clone(),
                            label,
                            if available {
                                theme.primary
                            } else {
                                theme.destructive
                            },
                        );
                    }
                    if let Some((label, action)) = action {
                        spawn_compact_action_button(summary, font, theme, label, action);
                    }
                });
        });
}

fn spawn_settings_badge(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    label: impl Into<String>,
    color: Color,
) {
    parent.spawn((
        Node {
            padding: UiRect::axes(px(8), px(3)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color.with_alpha(0.12)),
        BorderColor::all(color.with_alpha(0.38)),
        children![(Text::new(label), ui_text_font(font, 8.0), TextColor(color),)],
    ));
}

fn model_available(
    status: &app_core::AnalysisRuntimeStatus,
    target: app_core::ModelDownloadTarget,
) -> Option<bool> {
    status
        .models
        .iter()
        .find(|model| model.target == target)
        .map(|model| model.available)
}

fn spawn_model_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    native_setup: &NativeSetup,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "LOCAL INTELLIGENCE",
        "Models & runtime",
        "Checks are read-only; downloads start only after an explicit setup confirmation.",
    );
    if native_setup.receiver.is_some() || native_setup.progress.is_some() {
        spawn_setup_progress_panel(parent, font.clone(), icons.clone(), native_setup, theme);
    }
    let status = app_core::analysis_runtime_status();
    spawn_model_runtime_status_row(parent, font.clone(), theme, &status);
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Acceleration",
        "Choose the hardware target before installing the analysis environment.",
        SettingsSelectKind::ComputeBackend,
        session,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Shared analysis runtime",
        "Setup reuses compatible host ffmpeg, uv, Python, and existing model files. Nothing downloads until you confirm.",
        Some((
            if status.managed_runtime_available {
                "Reconfigure…"
            } else {
                "Set up…"
            },
            UiAction::RequestSetup(None),
        )),
    );
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "MODEL FILES BY ANALYSIS STAGE",
        "This page only manages local files. Choose which engine is active in Analysis; every download still requires confirmation.",
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "01 · VOCAL SEPARATION",
        "Vocal separation",
        "Creates vocal and instrumental stems before recognition.",
        separator_label(session.config.separator()),
        &[app_core::ModelDownloadTarget::Separator],
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes lyrics. Compatibility and language-detection models are identified separately from the selected engine.",
        transcription_summary(&session.config),
        &[
            app_core::ModelDownloadTarget::OpenVinoWhisper,
            app_core::ModelDownloadTarget::Parakeet,
            app_core::ModelDownloadTarget::WhisperLanguageDetection,
            app_core::ModelDownloadTarget::Whisper,
        ],
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "03 · WORD TIMING",
        "Word timing & alignment",
        "Refines recognized or supplied lyrics into editable word timings.",
        align_backend_label(session.config.align_backend()),
        &[
            app_core::ModelDownloadTarget::Alignment,
            app_core::ModelDownloadTarget::MmsKaraokeAlignment,
        ],
    );
    spawn_model_stage(
        parent,
        font,
        theme,
        session,
        &status.models,
        "04 · MELODY",
        "Melody & pitch",
        "Detects the sung fundamental frequency and creates note pitches.",
        pitch_model_label(session.config.pitch_model()),
        &[app_core::ModelDownloadTarget::Pitch],
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_model_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    models: &[app_core::ModelInstallStatus],
    eyebrow: &'static str,
    title: &'static str,
    description: &'static str,
    current: impl Into<String>,
    targets: &[app_core::ModelDownloadTarget],
) {
    if !models.iter().any(|model| targets.contains(&model.target)) {
        return;
    }
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        eyebrow,
        title,
        description,
        current,
        None,
        Some((
            "Configure in Analysis…".to_string(),
            UiAction::SettingsTab(SettingsTab::Analysis),
        )),
    );
    for model in models
        .iter()
        .filter(|model| targets.contains(&model.target))
    {
        spawn_model_install_row(parent, font.clone(), theme, session, model, title);
    }
}

fn model_install_role(config: &AppConfig, target: app_core::ModelDownloadTarget) -> &'static str {
    use app_core::ModelDownloadTarget;
    match target {
        ModelDownloadTarget::Whisper
            if config.asr_engine() == "parakeet"
                || config.compute_backend.as_deref() == Some("intel") =>
        {
            "Fallback"
        }
        ModelDownloadTarget::WhisperLanguageDetection => "Support",
        ModelDownloadTarget::MmsKaraokeAlignment if config.align_backend() != "mms_karaoke" => {
            "Optional"
        }
        _ => "Selected",
    }
}

fn spawn_model_install_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    model: &app_core::ModelInstallStatus,
    stage: &'static str,
) {
    let role = model_install_role(&session.config, model.target);
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(86),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(15)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(Node {
                    align_items: AlignItems::Center,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(5),
                    column_gap: px(7),
                    ..default()
                })
                .with_children(|title| {
                    spawn_text(
                        title,
                        font.clone(),
                        model.label.clone(),
                        12.0,
                        theme.foreground,
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        role,
                        if role == "Optional" {
                            theme.muted_foreground
                        } else {
                            theme.primary
                        },
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        if model.available {
                            "Installed"
                        } else {
                            "Missing"
                        },
                        if model.available {
                            theme.primary
                        } else {
                            theme.destructive
                        },
                    );
                });
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!("{} Used by Analysis > {stage}.", model.description),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|actions| {
                spawn_compact_action_button(
                    actions,
                    font,
                    theme,
                    if model.available {
                        "Reinstall…"
                    } else {
                        "Download…"
                    },
                    UiAction::RequestSetup(Some(model.target)),
                );
            });
        });
}

fn spawn_model_runtime_status_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    status: &app_core::AnalysisRuntimeStatus,
) {
    let (headline, status_color, status_hint) = if status.ready {
        (
            "Ready to analyze",
            theme.primary,
            "The selected runtime and every required model are available locally.",
        )
    } else {
        (
            "Setup required",
            theme.destructive,
            "Some required components are missing. Open setup to install or repair.",
        )
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(168),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|status_row| {
                    status_row
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|status_copy| {
                            status_copy
                                .spawn(Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|headline_row| {
                                    spawn_text(
                                        headline_row,
                                        font.clone(),
                                        "Runtime status",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                    spawn_text(
                                        headline_row,
                                        font.clone(),
                                        headline,
                                        12.0,
                                        theme.foreground,
                                    );
                                    headline_row.spawn((
                                        Node {
                                            padding: UiRect::axes(px(8), px(3)),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(status_color.with_alpha(0.16)),
                                        BorderColor::all(status_color.with_alpha(0.45)),
                                        children![(
                                            Text::new(if status.ready { "OK" } else { "MISSING" }),
                                            ui_text_font(font.clone(), 8.0),
                                            TextColor(status_color),
                                        )],
                                    ));
                                });
                            spawn_wrapped_text(
                                status_copy,
                                font.clone(),
                                status_hint.to_string(),
                                9.0,
                                theme.muted_foreground,
                            );
                            if !status.ready && !status.missing.is_empty() {
                                spawn_wrapped_text(
                                    status_copy,
                                    font.clone(),
                                    format!("Missing components: {}", status.missing.join(" · ")),
                                    8.5,
                                    theme.destructive,
                                );
                            }
                        });
                    spawn_setting_actions(
                        status_row,
                        font.clone(),
                        theme,
                        vec![("Check again".to_string(), UiAction::RefreshRuntimeStatus)],
                    );
                });
            panel
                .spawn(Node {
                    width: percent(100),
                    max_width: px(760),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|stack| {
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "ffmpeg",
                        status.ffmpeg_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "uv",
                        status.uv_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Python",
                        status.system_python_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Analyzer",
                        status.analyzer_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Pitch model",
                        status.pitch_model_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Selected models",
                        status.selected_models_available,
                    );
                });
        });
}

fn spawn_runtime_component_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    available: bool,
) {
    let color = if available {
        theme.primary
    } else {
        theme.destructive
    };
    let badge_label = if available {
        availability(true)
    } else {
        availability(false)
    };
    let badge_background = if available {
        theme.primary.with_alpha(0.16)
    } else {
        theme.destructive.with_alpha(0.16)
    };
    let badge_border = if available {
        theme.primary.with_alpha(0.45)
    } else {
        theme.destructive.with_alpha(0.45)
    };

    parent
        .spawn((
            Node {
                min_width: px(180),
                min_height: px(32),
                flex_basis: px(220),
                flex_grow: 1.0,
                max_width: px(250),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(px(9), px(5)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), label, 9.0, theme.foreground);
            row.spawn((
                Node {
                    padding: UiRect::axes(px(8), px(3)),
                    border_radius: BorderRadius::all(px(999.0)),
                    ..default()
                },
                BackgroundColor(badge_background),
                BorderColor::all(badge_border),
                children![(
                    Text::new(badge_label),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(color),
                )],
            ));
        });
}

fn transcription_summary(config: &AppConfig) -> String {
    if config.asr_engine() == "parakeet" {
        "Parakeet v3".to_string()
    } else if config.compute_backend.as_deref() == Some("intel") {
        "OpenVINO Whisper large-v3-turbo".to_string()
    } else {
        format!(
            "Whisper {}",
            settings_select_label(SettingsSelectKind::WhisperModel, config.whisper_model(),)
        )
    }
}

fn transcription_model_target(config: &AppConfig) -> app_core::ModelDownloadTarget {
    if config.asr_engine() == "parakeet" {
        app_core::ModelDownloadTarget::Parakeet
    } else if config.compute_backend.as_deref() == Some("intel") {
        app_core::ModelDownloadTarget::OpenVinoWhisper
    } else {
        app_core::ModelDownloadTarget::Whisper
    }
}

fn alignment_model_target(config: &AppConfig) -> Option<app_core::ModelDownloadTarget> {
    match config.align_backend() {
        "qwen" => Some(app_core::ModelDownloadTarget::Alignment),
        "mms_karaoke" => Some(app_core::ModelDownloadTarget::MmsKaraokeAlignment),
        _ => None,
    }
}

fn analysis_stage_status(
    status: &app_core::AnalysisRuntimeStatus,
    target: Option<app_core::ModelDownloadTarget>,
) -> (String, bool) {
    match target.and_then(|target| model_available(status, target)) {
        Some(true) => ("Installed".to_string(), true),
        Some(false) => ("Model missing".to_string(), false),
        None if status.analyzer_available => ("Runtime managed".to_string(), true),
        None => ("Runtime missing".to_string(), false),
    }
}

fn spawn_analysis_pipeline(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    status: &app_core::AnalysisRuntimeStatus,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.3)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|panel| {
            spawn_text(
                panel,
                font.clone(),
                "CURRENT ANALYSIS PIPELINE",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                panel,
                font.clone(),
                "The same four stages and names are used on Models & runtime.",
                9.0,
                theme.muted_foreground,
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|pipeline| {
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "01 · Vocals",
                        separator_label(session.config.separator()),
                        analysis_stage_status(
                            status,
                            Some(app_core::ModelDownloadTarget::Separator),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "02 · Lyrics",
                        transcription_summary(&session.config),
                        analysis_stage_status(
                            status,
                            Some(transcription_model_target(&session.config)),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "03 · Timing",
                        align_backend_label(session.config.align_backend()),
                        analysis_stage_status(status, alignment_model_target(&session.config)),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "04 · Pitch",
                        pitch_model_label(session.config.pitch_model()),
                        analysis_stage_status(status, Some(app_core::ModelDownloadTarget::Pitch)),
                    );
                });
        });
}

fn spawn_analysis_pipeline_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stage: &'static str,
    selected: impl Into<String>,
    status: (String, bool),
) {
    parent
        .spawn((
            Node {
                min_width: px(190),
                min_height: px(70),
                flex_basis: px(220),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(11)),
                row_gap: px(5),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.48)),
            BorderColor::all(theme.border.with_alpha(0.46)),
        ))
        .with_children(|card| {
            spawn_text(card, font.clone(), stage, 8.0, theme.muted_foreground);
            spawn_text(card, font.clone(), selected, 10.0, theme.foreground);
            spawn_settings_badge(
                card,
                font,
                status.0,
                if status.1 {
                    theme.primary
                } else {
                    theme.destructive
                },
            );
        });
}

fn spawn_analysis_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "GENERATION",
        "Analysis",
        "Configure each stage of newly generated stems, lyrics, timing, and pitch. Existing charts change only after re-analysis.",
    );
    let status = app_core::analysis_runtime_status();
    spawn_analysis_pipeline(parent, font.clone(), theme, session, &status);

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · VOCAL SEPARATION",
        "Vocal separation",
        "Creates a clean vocal source before lyrics and pitch are analyzed.",
        separator_label(session.config.separator()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Separator),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Separation engine",
        "Choose the model family that creates vocal and instrumental stems.",
        SettingsSelectKind::Separator,
        session,
    );
    let separation_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Separation);
    if session.config.separator() != "openvino_demucs" {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "Separation profile",
            if session.config.separator() == "karaoke" {
                "Balanced is recommended. Memory saver uses shorter RoFormer segments; Quality increases segment context and overlap."
            } else {
                "Balanced is recommended. Quality adds shifts and overlap, increasing processing time substantially."
            },
            SettingsSelectKind::SeparatorPreset,
            session,
        );
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            "Advanced separation tuning",
            "Model-specific memory, quality, and overlap controls. Existing stems change only after re-analysis.",
            Some((
                if separation_advanced {
                    "Hide advanced"
                } else {
                    "Show advanced"
                },
                UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Separation),
            )),
        );
    } else {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "FIXED OPENVINO PROFILE",
            "Segment dimensions and overlap are compiled into the installed OpenVINO Demucs graph. Select UVR Karaoke or Demucs to use adjustable separation profiles.",
        );
    }
    if separation_advanced && session.config.separator() == "karaoke" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer segment size",
            "Model default is used until edited. Smaller values reduce memory; larger values may improve continuity. Range: 64–1024.",
            session.config.separator_segment_size(),
            NumericSetting::SeparatorSegmentSize,
            UiAction::AdjustSeparatorSegmentSize(-32),
            UiAction::AdjustSeparatorSegmentSize(32),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer overlap",
            "More overlap can reduce chunk seams at the cost of additional processing. Range: 2–32.",
            session.config.separator_overlap(),
            NumericSetting::SeparatorOverlap,
            UiAction::AdjustSeparatorOverlap(-1),
            UiAction::AdjustSeparatorOverlap(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer batch size",
            "Lower this first if separation runs out of system or accelerator memory. Range: 1–8.",
            session.config.separator_batch_size(),
            NumericSetting::SeparatorBatchSize,
            UiAction::AdjustSeparatorBatchSize(-1),
            UiAction::AdjustSeparatorBatchSize(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Output normalization",
            "Peak normalization applied by the separator before stems enter the lossless cache. Range: 1–100%.",
            session.config.separator_normalization_pct(),
            NumericSetting::SeparatorNormalization,
            UiAction::AdjustSeparatorNormalization(-1),
            UiAction::AdjustSeparatorNormalization(1),
        );
    } else if separation_advanced && session.config.separator() == "demucs" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs shifts",
            "More random shifts can improve separation quality but multiply inference cost. Range: 1–8.",
            session.config.demucs_shifts(),
            NumericSetting::DemucsShifts,
            UiAction::AdjustDemucsShifts(-1),
            UiAction::AdjustDemucsShifts(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs overlap",
            "Overlap between inference windows. Range: 1–95%.",
            session.config.demucs_overlap_pct(),
            NumericSetting::DemucsOverlap,
            UiAction::AdjustDemucsOverlap(-1),
            UiAction::AdjustDemucsOverlap(1),
        );
    }

    let parakeet = session.config.asr_engine() == "parakeet";
    let intel_whisper = !parakeet && session.config.compute_backend.as_deref() == Some("intel");
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes sung words. Fallback settings appear separately when the primary engine needs them.",
        transcription_summary(&session.config),
        Some(analysis_stage_status(
            &status,
            Some(transcription_model_target(&session.config)),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Primary transcription engine",
        "Whisper is broadly compatible; Parakeet is faster for its supported languages.",
        SettingsSelectKind::AsrEngine,
        session,
    );
    if parakeet || intel_whisper {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "COMPATIBILITY FALLBACK",
            if parakeet {
                "Whisper is used only for unsupported languages or when Parakeet returns no usable words."
            } else {
                "Standard Whisper is retained for cases the Intel OpenVINO path cannot process."
            },
        );
    }
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        if parakeet || intel_whisper {
            "Whisper fallback model"
        } else {
            "Whisper model"
        },
        if parakeet || intel_whisper {
            "This does not replace the primary engine; it is loaded only when compatibility fallback is needed."
        } else {
            "Turbo is the balanced default; larger models trade speed for detail."
        },
        SettingsSelectKind::WhisperModel,
        session,
    );
    let transcription_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Transcription);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced transcription tuning",
        "Memory and search controls for this transcription stage.",
        Some((
            if transcription_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Transcription),
        )),
    );
    if transcription_advanced {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            if parakeet || intel_whisper {
                "Whisper fallback precision"
            } else {
                "Recognition precision"
            },
            "Whisper search breadth. Values are clamped between 1 and 16.",
            session.config.beam_size(),
            NumericSetting::BeamSize,
            UiAction::AdjustBeamSize(-1),
            UiAction::AdjustBeamSize(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            if parakeet {
                "Parakeet batch size"
            } else {
                "Whisper batch size"
            },
            "Lower this if this transcription engine runs out of GPU or system memory.",
            session.config.batch_size(),
            NumericSetting::BatchSize,
            UiAction::AdjustBatchSize(-1),
            UiAction::AdjustBatchSize(1),
        );
    }

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "03 · WORD TIMING",
        "Word timing & alignment",
        "Refines recognized or supplied lyrics into editable word timings.",
        align_backend_label(session.config.align_backend()),
        Some(analysis_stage_status(
            &status,
            alignment_model_target(&session.config),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Alignment engine",
        if session.config.align_backend() == "mms_karaoke" {
            "MMS Karaoke targets known Japanese lyrics. Automatic transcription retains its compatible timing path."
        } else if parakeet {
            "Used for compatibility fallback and supplied lyrics; Parakeet's direct timestamps can skip this stage."
        } else {
            "Choose how recognized or supplied lyrics are refined into word timings."
        },
        SettingsSelectKind::AlignBackend,
        session,
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "04 · MELODY",
        "Melody & pitch",
        "Detects sung pitch after vocal separation and creates editable notes.",
        pitch_model_label(session.config.pitch_model()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Pitch),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons,
        theme,
        "Pitch detection model",
        "Detects the sung fundamental frequency used to create note pitches.",
        SettingsSelectKind::PitchModel,
        session,
    );
    let pitch_advanced = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Pitch);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced pitch tuning",
        "Controls how strongly detected vocals are filtered before notes are created.",
        Some((
            if pitch_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Pitch),
        )),
    );
    if pitch_advanced {
        let threshold = (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32;
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Vocal detection sensitivity",
            "Lower for soft singing; raise to remove more silence. Range: 0–60%.",
            threshold,
            NumericSetting::VocalThreshold,
            UiAction::AdjustVocalThreshold(-1),
            UiAction::AdjustVocalThreshold(1),
        );
    }

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "AUTOMATION",
        "Controls when the four-stage pipeline starts; these are not model settings.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Auto-analyze",
        if session.config.auto_analyze() {
            "On · Unanalyzed songs are queued after a library scan."
        } else {
            "Off · New songs wait for an explicit analysis action."
        },
        session.config.auto_analyze(),
        UiAction::ToggleAutoAnalyze,
    );
    spawn_setting_row(
        parent,
        font,
        theme,
        "Analysis defaults",
        "Restore every stage and its advanced controls to the recommended starting values.",
        Some(("Restore defaults", UiAction::RestoreAnalysisDefaults)),
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_number_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: u32,
    setting: NumericSetting,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control.spawn((
                            EditableText {
                                max_characters: Some(2),
                                ..EditableText::new(value.to_string())
                            },
                            setting,
                            Node {
                                min_width: px(56),
                                height: px(20),
                                flex_grow: 1.0,
                                align_self: AlignSelf::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            ui_text_font(font.clone(), 11.0),
                            TextColor(theme.foreground),
                            TextLayout::justify(Justify::Center),
                            TextCursorStyle {
                                color: theme.primary,
                                selected_text_color: Some(theme.primary_foreground),
                                ..default()
                            },
                            TabIndex(0),
                        ));
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

fn spawn_switch_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    enabled: bool,
    action: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Button,
                        action,
                        Node {
                            width: px(42),
                            height: px(24),
                            align_items: AlignItems::Center,
                            justify_content: if enabled {
                                JustifyContent::FlexEnd
                            } else {
                                JustifyContent::FlexStart
                            },
                            padding: UiRect::horizontal(px(3)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(if enabled {
                            theme.primary.with_alpha(0.86)
                        } else {
                            theme.background.with_alpha(0.7)
                        }),
                        BorderColor::all(if enabled {
                            theme.primary.with_alpha(0.9)
                        } else {
                            theme.border.with_alpha(0.75)
                        }),
                    ))
                    .with_children(|switch| {
                        switch.spawn((
                            Node {
                                width: px(16),
                                height: px(16),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(if enabled {
                                theme.primary_foreground
                            } else {
                                theme.muted_foreground.with_alpha(0.8)
                            }),
                        ));
                    });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_shift_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: impl Into<String>,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    let value = value.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(68),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(13)),
                column_gap: px(22),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(copy, font.clone(), description, 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control
                            .spawn(Node {
                                min_width: px(68),
                                flex_grow: 1.0,
                                height: percent(100),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            })
                            .with_children(|value_node| {
                                spawn_text(value_node, font.clone(), value, 10.0, theme.foreground);
                            });
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

fn spawn_setup_progress_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    setup: &NativeSetup,
    theme: &StudioTheme,
) {
    let progress_percent = setup
        .progress
        .as_ref()
        .map_or(0, |progress| progress.percent);
    let action = setup
        .progress
        .as_ref()
        .map(|progress| progress.action.as_str())
        .unwrap_or("Starting setup…");
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(px(18)),
                padding: UiRect::all(px(16)),
                row_gap: px(9),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.44)),
            BorderColor::all(theme.primary.with_alpha(0.34)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(9),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons.clone(), UiIcon::Repair, 17.0, theme.primary);
                    spawn_text(
                        header,
                        font.clone(),
                        "Setting up models & runtime",
                        12.0,
                        theme.foreground,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{progress_percent}%"),
                        10.0,
                        theme.primary,
                    );
                });
            spawn_wrapped_text(panel, font.clone(), action, 10.0, theme.muted_foreground);
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(4),
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.border.with_alpha(0.38)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: percent(progress_percent as f32),
                            height: percent(100),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                    ));
                });
            if let Some(progress) = setup.progress.as_ref() {
                for task in &progress.tasks {
                    let (icon, color) = match task.state {
                        app_core::SetupTaskState::Done => (UiIcon::Check, theme.primary),
                        app_core::SetupTaskState::Running => (UiIcon::Repair, theme.foreground),
                        app_core::SetupTaskState::Pending => {
                            (UiIcon::CircleCheck, theme.muted_foreground.with_alpha(0.45))
                        }
                    };
                    panel
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|task_row| {
                            spawn_icon(task_row, icons.clone(), icon, 13.0, color);
                            spawn_text(task_row, font.clone(), task.label.clone(), 9.0, color);
                            if let Some(bytes) = task.downloaded_bytes {
                                task_row.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                spawn_text(
                                    task_row,
                                    font.clone(),
                                    match task.total_bytes {
                                        Some(total) => format!(
                                            "{} / {}",
                                            format_bytes(bytes),
                                            format_bytes(total)
                                        ),
                                        None => format_bytes(bytes),
                                    },
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                        });
                }
            }
            for line in setup.logs.iter().rev().take(4).rev() {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    line,
                    8.0,
                    theme.muted_foreground.with_alpha(0.76),
                );
            }
        });
}

fn spawn_setup_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    request: SetupRequest,
) {
    let mms_karaoke_selected = app_core::AppConfig::load().align_backend() == "mms_karaoke";
    let mms_karaoke_download = matches!(
        request.target,
        Some(app_core::ModelDownloadTarget::MmsKaraokeAlignment)
    ) || (mms_karaoke_selected
        && matches!(
            request.target,
            None | Some(app_core::ModelDownloadTarget::Alignment)
        ));
    let (title, description) = if mms_karaoke_download {
        if request.target.is_some() {
            (
                "Download MMS Karaoke model?",
                "Uta Studio will download the optional 1.26 GB Japanese alignment model from NextFire. The model is currently published under AGPL-3.0; confirming means you choose to install and use that separately licensed artifact.",
            )
        } else {
            (
                "Set up runtime and MMS Karaoke?",
                "Uta Studio will prepare the analysis runtime and download the selected optional 1.26 GB Japanese alignment model. The NextFire model is currently published under AGPL-3.0; confirming means you choose to install and use that separately licensed artifact.",
            )
        }
    } else if request.target.is_some() {
        (
            "Download selected model?",
            "Uta Studio will use the configured host tools and download only the selected artifact after you confirm.",
        )
    } else {
        (
            "Set up analysis runtime?",
            "Uta Studio will reuse compatible host tools and existing artifacts, then install only missing runtime packages and models.",
        )
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.74)),
        ZIndex(80),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "{description}\n\nDownloads never start merely because Settings was opened. You can cancel now without changing any runtime or model data."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelSetup,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmSetup,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.primary),
                            children![(
                                Text::new(if request.target.is_some() {
                                    "Download"
                                } else {
                                    "Set up"
                                }),
                                ui_text_font(font, 10.0),
                                TextColor(theme.primary_foreground),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn spawn_global_cache_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    scope: CacheClearScope,
) {
    let (title, description) = match scope {
        CacheClearScope::Generated => (
            "Clear generated cache?",
            "Generated stems, charts, previews, and authoring variants will be removed. Indexed source songs remain untouched.",
        ),
        CacheClearScope::Models => (
            "Clear downloaded models?",
            "Downloaded model artifacts will be removed. Existing configured directories remain in place, and analysis stays disabled until an explicit download.",
        ),
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(90),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(description),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelClearCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmClearCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Clear now"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn compute_backend_label(value: &str) -> &'static str {
    match value {
        "cuda" => "NVIDIA CUDA",
        "intel" => "Intel Arc",
        _ => "CPU",
    }
}

fn separator_label(value: &str) -> &'static str {
    match value {
        "demucs" => "Demucs",
        "openvino_demucs" => "OpenVINO Demucs v4 (Intel GPU)",
        _ => "UVR Karaoke",
    }
}

fn asr_engine_label(value: &str) -> &'static str {
    if value == "parakeet" {
        "Parakeet v3 (Experimental)"
    } else {
        "Whisper"
    }
}

fn align_backend_label(value: &str) -> &'static str {
    match value {
        "ctc" => "CTC Forced Alignment",
        "qwen" => "Qwen Forced Alignment",
        "mms_karaoke" => "MMS Karaoke (Japanese)",
        _ => "WhisperX",
    }
}

fn pitch_model_label(value: &str) -> &'static str {
    match value {
        "rmvpe" => "RMVPE",
        _ => "RMVPE",
    }
}

fn settings_select_value(kind: SettingsSelectKind, config: &AppConfig) -> &str {
    match kind {
        SettingsSelectKind::ComputeBackend => config.compute_backend.as_deref().unwrap_or("cpu"),
        SettingsSelectKind::Separator => config.separator(),
        SettingsSelectKind::SeparatorPreset => separator_preset(config),
        SettingsSelectKind::AsrEngine => config.asr_engine(),
        SettingsSelectKind::WhisperModel => config.whisper_model(),
        SettingsSelectKind::AlignBackend => config.align_backend(),
        SettingsSelectKind::PitchModel => config.pitch_model(),
    }
}

fn settings_select_label(kind: SettingsSelectKind, value: &str) -> &'static str {
    match kind {
        SettingsSelectKind::ComputeBackend => compute_backend_label(value),
        SettingsSelectKind::Separator => separator_label(value),
        SettingsSelectKind::SeparatorPreset => match value {
            "memory" => "Memory saver",
            "quality" => "Quality",
            "custom" => "Custom",
            _ => "Balanced",
        },
        SettingsSelectKind::AsrEngine => asr_engine_label(value),
        SettingsSelectKind::WhisperModel => match value {
            "large-v3" => "Large v3",
            "large-v3-turbo" => "Large v3 Turbo",
            "medium" => "Medium",
            "small" => "Small",
            "base" => "Base",
            "tiny" => "Tiny",
            _ => "Large v3",
        },
        SettingsSelectKind::AlignBackend => align_backend_label(value),
        SettingsSelectKind::PitchModel => pitch_model_label(value),
    }
}

fn settings_select_options(
    kind: SettingsSelectKind,
    intel_backend: bool,
) -> &'static [(&'static str, &'static str)] {
    match kind {
        SettingsSelectKind::ComputeBackend => &[
            ("cpu", "CPU"),
            ("cuda", "NVIDIA CUDA"),
            ("intel", "Intel Arc"),
        ],
        SettingsSelectKind::Separator if intel_backend => &[
            ("karaoke", "UVR Karaoke"),
            ("demucs", "Demucs"),
            ("openvino_demucs", "OpenVINO Demucs v4"),
        ],
        SettingsSelectKind::Separator => &[("karaoke", "UVR Karaoke"), ("demucs", "Demucs")],
        SettingsSelectKind::SeparatorPreset => &[
            ("balanced", "Balanced · recommended"),
            ("memory", "Memory saver · lower peak usage"),
            ("quality", "Quality · slower, more context"),
        ],
        SettingsSelectKind::AsrEngine => &[
            ("whisper", "Whisper"),
            ("parakeet", "Parakeet v3 (Experimental)"),
        ],
        SettingsSelectKind::WhisperModel => &[
            ("large-v3", "Large v3"),
            ("large-v3-turbo", "Large v3 Turbo"),
            ("medium", "Medium"),
            ("small", "Small"),
            ("base", "Base"),
            ("tiny", "Tiny"),
        ],
        SettingsSelectKind::AlignBackend => &[
            ("whisperx", "WhisperX"),
            ("ctc", "CTC Forced Alignment"),
            ("qwen", "Qwen Forced Alignment"),
            ("mms_karaoke", "MMS Karaoke (Japanese)"),
        ],
        SettingsSelectKind::PitchModel => &[("rmvpe", "RMVPE")],
    }
}

fn separator_preset(config: &AppConfig) -> &'static str {
    match config.separator() {
        "karaoke"
            if config.separator_segment_size.is_none()
                && config.separator_overlap() == 8
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "balanced"
        }
        "karaoke"
            if config.separator_segment_size == Some(128)
                && config.separator_overlap() == 4
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "memory"
        }
        "karaoke"
            if config.separator_segment_size == Some(512)
                && config.separator_overlap() == 16
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 95 =>
        {
            "quality"
        }
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 25 => {
            "balanced"
        }
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 15 => {
            "memory"
        }
        "demucs" if config.demucs_shifts() == 2 && config.demucs_overlap_pct() == 50 => {
            "quality"
        }
        "openvino_demucs" => "balanced",
        _ => "custom",
    }
}

fn apply_separator_preset(config: &mut AppConfig, preset: &str) {
    match (config.separator(), preset) {
        ("karaoke", "balanced") => {
            config.separator_segment_size = None;
            config.separator_overlap = None;
            config.separator_batch_size = None;
            config.separator_normalization_pct = None;
        }
        ("karaoke", "memory") => {
            config.separator_segment_size = Some(128);
            config.separator_overlap = Some(4);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(90);
        }
        ("karaoke", "quality") => {
            config.separator_segment_size = Some(512);
            config.separator_overlap = Some(16);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(95);
        }
        ("demucs", "balanced") => {
            config.demucs_shifts = None;
            config.demucs_overlap_pct = None;
        }
        ("demucs", "memory") => {
            config.demucs_shifts = Some(1);
            config.demucs_overlap_pct = Some(15);
        }
        ("demucs", "quality") => {
            config.demucs_shifts = Some(2);
            config.demucs_overlap_pct = Some(50);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_select_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    kind: SettingsSelectKind,
    session: &StudioSession,
) {
    let label = label.into();
    let description = description.into();
    let current = settings_select_value(kind, &session.config);
    let open = session.open_settings_select == Some(kind);
    let options = settings_select_options(
        kind,
        session.config.compute_backend.as_deref() == Some("intel"),
    );
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                min_height: px(76),
                align_items: if open {
                    AlignItems::FlexStart
                } else {
                    AlignItems::Center
                },
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
            ZIndex(if open { 60 } else { 0 }),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                position_type: PositionType::Relative,
                width: px(SETTINGS_CONTROL_WIDTH),
                height: if open { Val::Auto } else { px(36) },
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|control| {
                control
                    .spawn((
                        Button,
                        UiAction::OpenSettingsSelect(kind),
                        Node {
                            width: percent(100),
                            height: px(36),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(12)),
                            column_gap: px(8),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(if open { 0.76 } else { 0.5 })),
                        BorderColor::all(if open {
                            theme.primary.with_alpha(0.72)
                        } else {
                            theme.border.with_alpha(0.66)
                        }),
                    ))
                    .with_children(|button| {
                        spawn_text(
                            button,
                            font.clone(),
                            settings_select_label(kind, current),
                            10.0,
                            theme.foreground,
                        );
                        button.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_icon(
                            button,
                            icons.clone(),
                            UiIcon::ChevronDown,
                            14.0,
                            theme.muted_foreground,
                        );
                    });
                if open {
                    control
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(5)),
                                row_gap: px(2),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(7)),
                                ..default()
                            },
                            BackgroundColor(theme.card),
                            BorderColor::all(theme.border.with_alpha(0.9)),
                            ZIndex(60),
                        ))
                        .with_children(|menu| {
                            for (value, option_label) in options {
                                let selected = *value == current;
                                menu.spawn((
                                    Button,
                                    UiAction::SelectSettingsValue(kind, (*value).to_string()),
                                    Node {
                                        width: percent(100),
                                        min_height: px(31),
                                        align_items: AlignItems::Center,
                                        padding: UiRect::axes(px(9), px(7)),
                                        column_gap: px(8),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(if selected {
                                        theme.primary.with_alpha(0.12)
                                    } else {
                                        Color::NONE
                                    }),
                                ))
                                .with_children(|option| {
                                    spawn_wrapped_text(
                                        option,
                                        font.clone(),
                                        *option_label,
                                        10.0,
                                        if selected {
                                            theme.primary
                                        } else {
                                            theme.foreground
                                        },
                                    );
                                    option.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    if selected {
                                        spawn_icon(
                                            option,
                                            icons.clone(),
                                            UiIcon::Check,
                                            14.0,
                                            theme.primary,
                                        );
                                    }
                                });
                            }
                        });
                }
            });
        });
}

fn next_choice(current: &str, choices: &[&str]) -> String {
    let index = choices
        .iter()
        .position(|choice| *choice == current)
        .unwrap_or(0);
    choices[(index + 1) % choices.len()].to_string()
}

fn spawn_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    action: Option<(impl Into<String>, UiAction)>,
) {
    let label = label.into();
    let description = description.into();
    let action = action.map(|(label, action)| (label.into(), action));
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            if let Some((label, action)) = action {
                row.spawn(Node {
                    width: px(SETTINGS_CONTROL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|control_column| {
                    spawn_action_button(control_column, font, theme, label, action);
                });
            }
        });
}

fn spawn_setting_row_with_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    actions: Vec<(String, UiAction)>,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(92),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            spawn_setting_actions(row, font, theme, actions);
        });
}

fn spawn_setting_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    actions: Vec<(String, UiAction)>,
) {
    parent
        .spawn(Node {
            width: px(SETTINGS_CONTROL_WIDTH),
            flex_shrink: 0.0,
            justify_content: JustifyContent::FlexEnd,
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(8),
            column_gap: px(8),
            ..default()
        })
        .with_children(|controls| {
            for (label, action) in actions {
                spawn_compact_action_button(controls, font.clone(), theme, label, action);
            }
        });
}

fn spawn_compact_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(0),
            height: px(34),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(11)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.52)),
        BorderColor::all(theme.border.with_alpha(0.66)),
        children![(
            Text::new(label),
            ui_text_font(font, 9.0),
            TextColor(theme.foreground),
            TextLayout::no_wrap(),
        )],
    ));
}

fn spawn_source_file_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    path: &std::path::Path,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(82),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), "Source file", 12.0, theme.foreground);
                copy.spawn((
                    Text::new(path.to_string_lossy().into_owned()),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                ));
            });
            row.spawn(Node {
                width: px(112),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|action| {
                spawn_action_button(
                    action,
                    font,
                    theme,
                    "Open",
                    UiAction::OpenSource(path.to_path_buf()),
                );
            });
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(136),
            height: px(34),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(12)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.52)),
        BorderColor::all(theme.border.with_alpha(0.66)),
        children![(
            Text::new(label),
            ui_text_font(font, 10.0),
            TextColor(theme.foreground),
            TextLayout::no_wrap(),
        )],
    ));
}

fn spawn_wrapped_text(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        TextLayout::default(),
    ));
}

fn availability(available: bool) -> &'static str {
    if available { "available" } else { "missing" }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn spawn_empty_library(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    scanning: bool,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::all(px(20)),
            ..default()
        })
        .with_children(|stage| {
            stage
                .spawn((
                    Node {
                        width: percent(100),
                        max_width: px(720),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(40), px(48)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        row_gap: px(14),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.76)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.12),
                        px(0),
                        px(18),
                        px(28),
                        px(-12),
                    ),
                ))
                .with_children(|card| {
                    card.spawn((
                        Node {
                            width: px(48),
                            height: px(48),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(8)),
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.12)),
                        children![(
                            Text::new("♫"),
                            ui_text_font(font.clone(), 24.0),
                            TextColor(theme.primary),
                        )],
                    ));
                    spawn_text(card, font.clone(), "FIRST STEP", 10.0, theme.primary);
                    spawn_text(
                        card,
                        font.clone(),
                        "Choose your song library",
                        24.0,
                        theme.foreground,
                    );
                    card.spawn((
                        Node {
                            max_width: px(570),
                            ..default()
                        },
                        children![(
                            Text::new(
                                "Pick a local folder. Uta Studio will scan it, generate stems and charts with AI, then let you correct every word and note before exporting.",
                            ),
                            ui_text_font(font.clone(), 13.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::justify(Justify::Center),
                        )],
                    ));
                    card.spawn((
                        Button,
                        UiAction::ChooseFolder,
                        Node {
                            height: px(42),
                            padding: UiRect::horizontal(px(18)),
                            margin: UiRect::top(px(8)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(8)),
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                        children![(
                            Text::new(if scanning {
                                "Scanning song folder…"
                            } else {
                                "□  Choose song folder"
                            }),
                            ui_text_font(font, 13.0),
                            TextColor(theme.primary_foreground),
                        )],
                    ));
                });
        });
}

fn library_select_options(kind: LibrarySelectKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        LibrarySelectKind::Status => &[
            ("all", "All statuses"),
            ("not_analyzed", "Not analyzed"),
            ("queued", "Queued"),
            ("analyzing", "Analyzing"),
            ("analyzed", "Analyzed"),
            ("failed", "Failed"),
        ],
        LibrarySelectKind::TranscriptSource => &[
            ("all", "All lyric types"),
            ("generated", "Generated"),
            ("lyrics", "AI aligned"),
            ("lrc", "LRC"),
            ("usdx", "UltraStar"),
        ],
    }
}

fn library_select_value(kind: LibrarySelectKind, session: &StudioSession) -> &str {
    match kind {
        LibrarySelectKind::Status => session.library_status.as_deref().unwrap_or("all"),
        LibrarySelectKind::TranscriptSource => session
            .library_transcript_source
            .as_deref()
            .unwrap_or("all"),
    }
}

fn library_select_label(kind: LibrarySelectKind, value: &str) -> &'static str {
    library_select_options(kind)
        .iter()
        .find_map(|(option, label)| (*option == value).then_some(*label))
        .unwrap_or("All")
}

fn spawn_library_filter_select(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    kind: LibrarySelectKind,
    session: &StudioSession,
) {
    let current = library_select_value(kind, session);
    let open = session.open_library_select == Some(kind);
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(132),
                height: px(34),
                flex_shrink: 0.0,
                ..default()
            },
            ZIndex(if open { 70 } else { 0 }),
        ))
        .with_children(|control| {
            control
                .spawn((
                    Button,
                    UiAction::OpenLibrarySelect(kind),
                    Node {
                        width: percent(100),
                        height: px(34),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(10)),
                        column_gap: px(7),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(if open { 0.82 } else { 0.46 })),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.68)
                    } else {
                        theme.border.with_alpha(0.54)
                    }),
                ))
                .with_children(|button| {
                    spawn_text(
                        button,
                        font.clone(),
                        library_select_label(kind, current),
                        9.0,
                        theme.foreground,
                    );
                    button.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::ChevronDown,
                        12.0,
                        theme.muted_foreground,
                    );
                });
            if open {
                control
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: px(38),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(4)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.card),
                        BorderColor::all(theme.border.with_alpha(0.9)),
                        ZIndex(70),
                    ))
                    .with_children(|menu| {
                        for (value, label) in library_select_options(kind) {
                            let selected = *value == current;
                            menu.spawn((
                                Button,
                                UiAction::SelectLibraryValue(kind, (*value).to_string()),
                                Node {
                                    width: percent(100),
                                    min_height: px(29),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(8)),
                                    column_gap: px(6),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(if selected {
                                    theme.primary.with_alpha(0.12)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|option| {
                                spawn_text(
                                    option,
                                    font.clone(),
                                    *label,
                                    9.0,
                                    if selected {
                                        theme.primary
                                    } else {
                                        theme.foreground
                                    },
                                );
                                option.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                if selected {
                                    spawn_icon(
                                        option,
                                        icons.clone(),
                                        UiIcon::Check,
                                        12.0,
                                        theme.primary,
                                    );
                                }
                            });
                        }
                    });
            }
        });
}

fn spawn_export_all_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSession,
) {
    let open = session.export_all_open;
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(124),
                height: px(34),
                flex_shrink: 0.0,
                ..default()
            },
            ZIndex(if open { 70 } else { 0 }),
        ))
        .with_children(|control| {
            control
                .spawn((
                    Button,
                    UiAction::ToggleExportAllMenu,
                    Node {
                        width: percent(100),
                        height: px(34),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(10)),
                        column_gap: px(7),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(if open { 0.82 } else { 0.46 })),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.68)
                    } else {
                        theme.border.with_alpha(0.54)
                    }),
                ))
                .with_children(|button| {
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::Save,
                        13.0,
                        theme.muted_foreground,
                    );
                    spawn_text(
                        button,
                        font.clone(),
                        if session.export_job.receiver.is_some() {
                            "Exporting…"
                        } else {
                            "Export all"
                        },
                        9.0,
                        theme.foreground,
                    );
                    button.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::ChevronDown,
                        12.0,
                        theme.muted_foreground,
                    );
                });
            if open {
                control
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(0),
                            top: px(38),
                            width: px(220),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(5)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.card),
                        BorderColor::all(theme.border.with_alpha(0.9)),
                        ZIndex(70),
                    ))
                    .with_children(|menu| {
                        for (label, detail, action) in [
                            (
                                "UTZ packages",
                                "One validated .utz per ready chart",
                                UiAction::ExportAllUtz,
                            ),
                            (
                                "UltraStar bundles",
                                "One .txt bundle per ready chart",
                                UiAction::ExportAllUltraStar,
                            ),
                        ] {
                            menu.spawn((
                                Button,
                                action,
                                Node {
                                    width: percent(100),
                                    min_height: px(46),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::horizontal(px(9)),
                                    row_gap: px(2),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                            ))
                            .with_children(|option| {
                                spawn_text(option, font.clone(), label, 10.0, theme.foreground);
                                spawn_text(
                                    option,
                                    font.clone(),
                                    detail,
                                    8.0,
                                    theme.muted_foreground,
                                );
                            });
                        }
                    });
            }
        });
}

fn spawn_library_collection(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let artists = session.library_view == LibraryView::Artists;
    let (title, description, icon, items) = if artists {
        (
            "Artists",
            "Browse artists in the main workspace. The sidebar remains a quiet navigation surface.",
            UiIcon::Artists,
            &session.menu_items.artists,
        )
    } else {
        (
            "Albums",
            "Browse albums in the main workspace. Choose one to see all matching tracks.",
            UiIcon::Albums,
            &session.menu_items.albums,
        )
    };
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|page| {
            page.spawn((
                Node {
                    width: percent(100),
                    padding: UiRect::axes(px(28), px(24)),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.55)),
            ))
            .with_children(|header| {
                spawn_text(header, font.clone(), "MY LIBRARY", 9.0, theme.primary);
                spawn_text(header, font.clone(), title, 34.0, theme.foreground);
                spawn_wrapped_text(
                    header,
                    font.clone(),
                    format!("{description} · {} total", items.len()),
                    11.0,
                    theme.muted_foreground,
                );
            });
            page.spawn((
                LibrarySongList,
                ScrollPosition::default(),
                Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::FlexStart,
                    padding: UiRect::all(px(22)),
                    row_gap: px(12),
                    column_gap: px(12),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                for item in items {
                    let facet = if artists {
                        LibraryFacet::Artist {
                            value: item.value.clone(),
                            label: item.label.clone(),
                        }
                    } else {
                        LibraryFacet::Album {
                            value: item.value.clone(),
                            label: item.label.clone(),
                        }
                    };
                    list.spawn((
                        Button,
                        UiAction::SetLibraryFacet(facet),
                        Node {
                            width: px(230),
                            min_height: px(72),
                            align_items: AlignItems::Center,
                            padding: UiRect::all(px(12)),
                            column_gap: px(11),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.card.with_alpha(0.36)),
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            Node {
                                width: px(38),
                                height: px(38),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(theme.primary.with_alpha(0.09)),
                        ))
                        .with_children(|mark| {
                            spawn_icon(mark, icons.clone(), icon, 17.0, theme.primary);
                        });
                        card.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|copy| {
                            copy.spawn((
                                Text::new(item.label.clone()),
                                ui_text_font(font.clone(), 12.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                            ));
                            spawn_text(
                                copy,
                                font.clone(),
                                format!(
                                    "{} track{}",
                                    item.count,
                                    if item.count == 1 { "" } else { "s" }
                                ),
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    });
                }
                if items.is_empty() {
                    spawn_wrapped_text(
                        list,
                        font,
                        if artists {
                            "No artist metadata is available yet."
                        } else {
                            "No album metadata is available yet."
                        },
                        11.0,
                        theme.muted_foreground,
                    );
                }
            });
        });
}

fn spawn_library(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|library| {
            library
                .spawn((
                    Node {
                        width: percent(100),
                        padding: UiRect::axes(px(28), px(24)),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|header| {
                    spawn_text(
                        header,
                        font.clone(),
                        if session.library_facet.is_some() {
                            "MY LIBRARY"
                        } else {
                            session.library_view.eyebrow()
                        },
                        9.0,
                        theme.muted_foreground,
                    );
                    spawn_text(
                        header,
                        font.clone(),
                        session
                            .library_search
                            .as_deref()
                            .map(|query| format!("Results for “{query}”"))
                            .unwrap_or_else(|| session.library_title().to_string()),
                        34.0,
                        theme.foreground,
                    );
                    spawn_text(
                        header,
                        font.clone(),
                        if session.library_view == LibraryView::Queue {
                            let active = session
                                .analysis_tasks
                                .iter()
                                .filter(|task| {
                                    matches!(
                                        task.status,
                                        app_core::QueuedStatus::Queued
                                            | app_core::QueuedStatus::Analyzing(_)
                                    )
                                })
                                .count();
                            let failed = session
                                .analysis_tasks
                                .iter()
                                .filter(|task| {
                                    matches!(task.status, app_core::QueuedStatus::Failed(_))
                                })
                                .count();
                            format!("{active} active · {failed} failed · live updates")
                        } else {
                            format!(
                                "{} tracks · production workspace{}",
                                session.songs.processed_count,
                                if session.scanning { " · scanning" } else { "" }
                            )
                        },
                        11.0,
                        theme.muted_foreground,
                    );
                    header
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(8),
                            row_gap: px(8),
                            margin: UiRect::top(px(12)),
                            ..default()
                        })
                        .with_children(|tools| {
                            tools.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            if session.library_view == LibraryView::All
                                && session.library_facet.is_none()
                            {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Repeat,
                                    if session.scanning {
                                        "Scanning…"
                                    } else {
                                        "Rescan library"
                                    },
                                    UiAction::RescanLibrary,
                                    false,
                                );
                            }
                            spawn_library_filter_select(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                LibrarySelectKind::Status,
                                session,
                            );
                            spawn_library_filter_select(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                LibrarySelectKind::TranscriptSource,
                                session,
                            );
                            spawn_export_all_menu(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                session,
                            );
                            if app_core::analysis_runtime_status().ready {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Sparkles,
                                    "Analyze all",
                                    UiAction::AnalyzeAll,
                                    false,
                                );
                            } else {
                                spawn_toolbar_button(
                                    tools,
                                    font.clone(),
                                    icons.clone(),
                                    theme,
                                    UiIcon::Repair,
                                    "Set up analysis",
                                    UiAction::SettingsTab(SettingsTab::Models),
                                    false,
                                );
                            }
                            spawn_toolbar_button(
                                tools,
                                font.clone(),
                                icons.clone(),
                                theme,
                                if session.config.song_list_view.as_deref() == Some("grid") {
                                    UiIcon::List
                                } else {
                                    UiIcon::Grid
                                },
                                if session.config.song_list_view.as_deref() == Some("grid") {
                                    "Table view"
                                } else {
                                    "Grid view"
                                },
                                UiAction::ToggleLibraryLayout,
                                false,
                            );
                        });
                });

            library
                .spawn((
                    LibrarySongList,
                    ScrollPosition::default(),
                    Node {
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: if session.config.song_list_view.as_deref() == Some("grid")
                        {
                            FlexDirection::Row
                        } else {
                            FlexDirection::Column
                        },
                        flex_wrap: if session.config.song_list_view.as_deref() == Some("grid") {
                            FlexWrap::Wrap
                        } else {
                            FlexWrap::NoWrap
                        },
                        align_content: AlignContent::FlexStart,
                        padding: if session.config.song_list_view.as_deref() == Some("grid") {
                            UiRect::all(px(22))
                        } else {
                            UiRect::ZERO
                        },
                        row_gap: px(14),
                        column_gap: px(14),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    let grid = session.config.song_list_view.as_deref() == Some("grid");
                    if session.library_view == LibraryView::Queue {
                        spawn_analysis_session_overview(list, font.clone(), session, theme);
                        spawn_analysis_history_list(list, font.clone(), session, theme);
                    }
                    if !grid {
                        spawn_song_header(list, font.clone(), theme);
                    }
                    for song in &session.songs.processed {
                        let cover = album_art_handle(song, asset_server, images, local_images);
                        let analysis_status = session
                            .analysis_tasks
                            .iter()
                            .find(|task| task.file_hash == song.file_hash)
                            .map(|task| &task.status);
                        if grid {
                            spawn_library_song_card(
                                list,
                                font.clone(),
                                theme,
                                song,
                                cover,
                                analysis_status,
                            );
                        } else {
                            spawn_library_song_row(
                                list,
                                font.clone(),
                                theme,
                                song,
                                cover,
                                analysis_status,
                            );
                        }
                    }
                    if session.songs.processed.is_empty() {
                        spawn_text(
                            list,
                            font.clone(),
                            if session.library_view == LibraryView::Queue
                                && session.analysis_tasks.iter().any(|task| {
                                    matches!(task.status, app_core::QueuedStatus::Failed(_))
                                })
                            {
                                "No active jobs. Open Activity to review failed analyses."
                            } else if session.library_view == LibraryView::Queue {
                                if session.analysis_history.is_empty() {
                                    "The analysis queue is empty. Choose an unanalyzed song to start."
                                } else {
                                    "No analysis is running. Select a previous session above."
                                }
                            } else if session.scanning {
                                "Scanning your library…"
                            } else {
                                "This library is empty or still being scanned."
                            },
                            13.0,
                            theme.muted_foreground,
                        );
                    } else if session.songs.processed.len() < session.songs.count {
                        spawn_action_button(
                            list,
                            font.clone(),
                            theme,
                            format!(
                                "Load more · {} of {}",
                                session.songs.processed.len(),
                                session.songs.count
                            ),
                            UiAction::LoadMoreSongs,
                        );
                    }
                });

            if let Some(context) = session.song_context.as_ref() {
                spawn_song_context_menu(library, font.clone(), theme, context);
            }
        });
}

fn spawn_song_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &SongContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::DismissSongContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let left = (context.position.x - SIDEBAR_WIDTH - 14.0).max(8.0);
    let top = (context.position.y - 58.0).max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(270),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.song.title.clone(),
                11.0,
                theme.foreground,
            );
            spawn_text(
                menu,
                font.clone(),
                format!("{} · Track actions", context.song.artist),
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Play original",
                11.0,
                UiAction::PlayLibrarySong(context.song.file_hash.clone()),
            );
            if context.song.editor_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Edit chart",
                    11.0,
                    UiAction::OpenEditor(context.song.file_hash.clone()),
                );
            } else if !context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Analyze song",
                    11.0,
                    UiAction::AnalyzeSong(context.song.file_hash.clone()),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open track page",
                11.0,
                UiAction::OpenSong(context.song.file_hash.clone()),
            );
            if context.song.authoring_ready {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export Uta package (.utz)",
                    11.0,
                    UiAction::ExportUtz(context.song.file_hash.clone()),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Export UltraStar (.txt)",
                    11.0,
                    UiAction::ExportUltraStar(context.song.file_hash.clone()),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Open with default app",
                11.0,
                UiAction::OpenSource(context.song.path.clone()),
            );
            spawn_text_button(
                menu,
                font,
                theme,
                "Show in file manager",
                11.0,
                UiAction::RevealSource(context.song.path.clone()),
            );
        });
}

fn song_status_copy(
    song: &Song,
    analysis_status: Option<&app_core::QueuedStatus>,
    theme: &StudioTheme,
) -> (String, Color) {
    match analysis_status {
        Some(app_core::QueuedStatus::Queued) => ("Queued".to_string(), theme.primary),
        Some(app_core::QueuedStatus::Analyzing(progress)) => {
            (format!("Analyzing · {progress}%"), theme.primary)
        }
        Some(app_core::QueuedStatus::Failed(_)) => ("Failed".to_string(), theme.destructive),
        None if song.authoring_ready => ("Ready to author".to_string(), theme.pitch_contour),
        None if song.is_analyzed => ("Analysis incomplete".to_string(), theme.editor_warning),
        None => ("Not analyzed".to_string(), theme.muted_foreground),
    }
}

fn spawn_library_song_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    song: &Song,
    cover: Handle<Image>,
    analysis_status: Option<&app_core::QueuedStatus>,
) {
    let context_song = song.clone();
    parent
        .spawn((
            Button,
            Node {
                width: percent(100),
                min_height: px(60),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(22), px(8)),
                border: UiRect::bottom(px(1)),
                column_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(theme.border.with_alpha(0.28)),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(44),
                    height: px(44),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                ImageNode::new(cover),
                BorderColor::all(theme.border.with_alpha(0.42)),
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(2),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|metadata| {
                metadata.spawn((
                    Text::new(song.title.clone()),
                    ui_text_font(font.clone(), 11.0),
                    TextColor(theme.foreground),
                    TextLayout::no_wrap(),
                ));
                metadata.spawn((
                    Text::new(song.language.as_deref().unwrap_or("Language unknown")),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                ));
            });
            row.spawn(Node {
                width: px(150),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|artist| {
                spawn_text(
                    artist,
                    font.clone(),
                    if song.artist.trim().is_empty() {
                        "Unknown artist".to_string()
                    } else {
                        song.artist.clone()
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(180),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|album| {
                spawn_text(
                    album,
                    font.clone(),
                    if song.album.trim().is_empty() {
                        "—".to_string()
                    } else {
                        song.album.clone()
                    },
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(64),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|duration| {
                spawn_text(
                    duration,
                    font.clone(),
                    format_duration(song.duration_secs),
                    10.0,
                    theme.muted_foreground,
                );
            });
            let (status_label, status_color) = song_status_copy(song, analysis_status, theme);
            row.spawn(Node {
                width: px(150),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|status| {
                status
                    .spawn((
                        Node {
                            min_height: px(24),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(8)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(status_color.with_alpha(0.08)),
                        BorderColor::all(status_color.with_alpha(0.34)),
                    ))
                    .with_children(|badge| {
                        spawn_text(badge, font.clone(), status_label, 8.0, status_color);
                    });
            });
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_pointer(&event, &context_song, &mut session, &mut invalidated);
            },
        );
}

fn spawn_library_song_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    song: &Song,
    cover: Handle<Image>,
    analysis_status: Option<&app_core::QueuedStatus>,
) {
    let context_song = song.clone();
    let (status_label, status_color) = song_status_copy(song, analysis_status, theme);
    parent
        .spawn((
            Button,
            Node {
                width: px(172),
                min_height: px(226),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.34)),
            BorderColor::all(theme.border.with_alpha(0.38)),
        ))
        .with_children(|card| {
            card.spawn((
                Node {
                    width: px(154),
                    height: px(154),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                ImageNode::new(cover),
                Pickable::IGNORE,
            ));
            for (text, size, color) in [
                (song.title.clone(), 11.0, theme.foreground),
                (song.artist.clone(), 9.0, theme.muted_foreground),
                (status_label, 8.0, status_color),
            ] {
                card.spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    height: px(size + 5.0),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((
                        Text::new(text),
                        ui_text_font(font.clone(), size),
                        TextColor(color),
                        TextLayout::no_wrap(),
                    ));
                });
            }
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_song_from_pointer(&event, &context_song, &mut session, &mut invalidated);
            },
        );
}

fn open_song_from_pointer(
    event: &Pointer<Press>,
    song: &Song,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) {
    match event.button {
        PointerButton::Primary => {
            session.selected_song = Some(song.file_hash.clone());
            session.route = StudioRoute::SongDetail;
            session.song_context = None;
            session.notice = None;
        }
        PointerButton::Secondary => {
            session.song_context = Some(SongContextMenu {
                song: song.clone(),
                position: event.pointer_location.position,
            });
        }
        PointerButton::Middle => return,
    }
    invalidated.0 = true;
}

fn spawn_song_header(parent: &mut ChildSpawnerCommands, font: Handle<Font>, theme: &StudioTheme) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(34),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(22)),
                ..default()
            },
            BackgroundColor(theme.muted.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: px(56),
                flex_shrink: 0.0,
                ..default()
            });
            spawn_text(row, font.clone(), "TRACK", 9.0, theme.muted_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn(Node {
                width: px(150),
                ..default()
            })
            .with_children(|artist| {
                spawn_text(artist, font.clone(), "ARTIST", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(180),
                ..default()
            })
            .with_children(|album| {
                spawn_text(album, font.clone(), "ALBUM", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(64),
                ..default()
            })
            .with_children(|duration| {
                spawn_text(duration, font.clone(), "TIME", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(150),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|status| {
                spawn_text(status, font, "STATUS", 9.0, theme.muted_foreground);
            });
        });
}

fn ui_font_scale() -> f32 {
    f32::from_bits(GLOBAL_UI_FONT_SCALE_BITS.load(Ordering::SeqCst))
}

fn ui_font_size_percent_to_points(scale_percent: u32) -> u32 {
    let size = (scale_percent as f32) * (UI_FONT_BASE_SIZE_PX as f32) / 100.0;
    size.round()
        .clamp(UI_FONT_SIZE_MIN_PX as f32, UI_FONT_SIZE_MAX_PX as f32) as u32
}

fn ui_font_points_to_scale_percent(size_px: u32) -> u32 {
    let clamped = size_px.clamp(UI_FONT_SIZE_MIN_PX, UI_FONT_SIZE_MAX_PX);
    let percent = (clamped as f32) * 100.0 / (UI_FONT_BASE_SIZE_PX as f32);
    percent.round().clamp(
        UI_FONT_SCALE_MIN_PERCENT as f32,
        UI_FONT_SCALE_MAX_PERCENT as f32,
    ) as u32
}

fn set_ui_font_scale(scale: f32) {
    let scale = scale.clamp(0.25, 2.0);
    GLOBAL_UI_FONT_SCALE_BITS.store(scale.to_bits(), Ordering::SeqCst);
}

fn ui_font_size(size: f32) -> f32 {
    size * ui_font_scale()
}

fn ui_text_font(font: Handle<Font>, size: f32) -> TextFont {
    TextFont::from(font).with_font_size(ui_font_size(size))
}

fn spawn_text_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    size: f32,
    action: UiAction,
) {
    let label = label.into();
    parent.spawn((
        Button,
        action,
        Node {
            min_width: px(28),
            height: px(32),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            Text::new(label),
            ui_text_font(font, size),
            TextColor(theme.sidebar_foreground),
        )],
    ));
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        ui_text_font(font, size),
        TextColor(color),
        TextLayout::no_wrap(),
    ));
}

#[allow(clippy::too_many_arguments)]
fn handle_window_close_requests(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut commands: Commands,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    setup: Res<NativeSetup>,
    diagnostics: Res<NativeDiagnostics>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let Some(request) = requests.read().next() else {
        return;
    };
    let has_unsaved_edits = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let background_work = session.scanning
        || session.authoring_busy
        || setup.receiver.is_some()
        || diagnostics.receiver.is_some()
        || session.export_job.receiver.is_some()
        || session.editor_load_job.receiver.is_some()
        || session.lyrics_search_job.receiver.is_some()
        || session.analysis_tasks.iter().any(|task| {
            matches!(
                task.status,
                app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
            )
        });
    if has_unsaved_edits || background_work {
        session.pending_leave = Some(PendingLeave::Exit);
        invalidated.0 = true;
    } else {
        let _ = audio.0.stop();
        let _ = library_audio.0.stop();
        commands.entity(request.window).despawn();
    }
}

// Keeping these as separate Bevy system parameters preserves change detection.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_actions(
    mut commands: Commands,
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    keys: Res<ButtonInput<KeyCode>>,
    lyrics_inputs: Query<&EditableText, With<LyricsEditorInput>>,
    search_inputs: Query<
        &EditableText,
        (
            With<LibrarySearchInput>,
            Without<LyricsEditorInput>,
            Without<LanguageEditorInput>,
        ),
    >,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    mut setup: ResMut<NativeSetup>,
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut authoring: ResMut<NativeAuthoringJob>,
    mut theme: ResMut<StudioTheme>,
    mut clear_color: ResMut<ClearColor>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Ok((window_entity, mut window)) = windows.single_mut() else {
            continue;
        };
        match action {
            UiAction::ToggleGlobalSearch => {
                session.search_open = !session.search_open;
                session.activity_open = false;
                session.about_open = false;
                invalidated.0 = true;
            }
            UiAction::ToggleActivity => {
                session.activity_open = !session.activity_open;
                session.search_open = false;
                session.about_open = false;
                session.analysis_tasks = app_core::load_analysis_tasks();
                invalidated.0 = true;
            }
            UiAction::CloseActivity => {
                session.activity_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectAnalysisHistory(id) => {
                session.selected_analysis_history = *id;
                session.selected_analysis_stage = None;
                session.activity_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectAnalysisStage(stage) => {
                session.selected_analysis_stage = Some(stage.clone());
                invalidated.0 = true;
            }
            UiAction::RequestClearAnalysisHistory => {
                session.pending_analysis_history_clear = true;
                invalidated.0 = true;
            }
            UiAction::CancelClearAnalysisHistory => {
                session.pending_analysis_history_clear = false;
                invalidated.0 = true;
            }
            UiAction::ConfirmClearAnalysisHistory => {
                session.pending_analysis_history_clear = false;
                match app_core::clear_analysis_history() {
                    Ok(()) => {
                        session.analysis_history.clear();
                        session.selected_analysis_history = None;
                        session.selected_analysis_stage = None;
                        session.notice = Some("Analysis history deleted.".into());
                    }
                    Err(error) => {
                        session.notice = Some(format!("Could not delete analysis history: {error}"));
                    }
                }
                invalidated.0 = true;
            }
            UiAction::OpenAbout => {
                session.about_open = true;
                session.activity_open = false;
                session.search_open = false;
                invalidated.0 = true;
            }
            UiAction::CloseAbout => {
                session.about_open = false;
                invalidated.0 = true;
            }
            UiAction::Back => {
                session.open_settings_select = None;
                session.open_editor_select = None;
                if session.route == StudioRoute::Editor {
                    if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                        session.pending_leave = Some(PendingLeave::Back);
                    } else {
                        let _ = audio.0.stop();
                        session.editor = None;
                        session.route = StudioRoute::SongDetail;
                        session.notice = None;
                    }
                    invalidated.0 = true;
                } else if session.route != StudioRoute::Library {
                    session.route = StudioRoute::Library;
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::Home => {
                session.open_settings_select = None;
                session.open_editor_select = None;
                if session.route == StudioRoute::Editor {
                    if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                        session.pending_leave = Some(PendingLeave::Home);
                        invalidated.0 = true;
                        continue;
                    }
                    let _ = audio.0.stop();
                    session.editor = None;
                }
                if session.route != StudioRoute::Library {
                    session.route = StudioRoute::Library;
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::CancelLeave => {
                session.pending_leave = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmLeave => {
                let destination = session.pending_leave.take();
                let _ = audio.0.stop();
                match destination {
                    Some(PendingLeave::Exit) => {
                        let _ = library_audio.0.stop();
                        commands.entity(window_entity).despawn();
                    }
                    Some(PendingLeave::Back) => {
                        session.editor = None;
                        session.route = StudioRoute::SongDetail;
                        session.notice = None;
                        invalidated.0 = true;
                    }
                    Some(PendingLeave::Home) => {
                        session.editor = None;
                        session.route = StudioRoute::Library;
                        session.notice = None;
                        invalidated.0 = true;
                    }
                    None => {}
                }
            }
            UiAction::SetLibraryView(view) => {
                session.library_view = *view;
                session.library_status = None;
                session.library_search = None;
                session.library_facet = None;
                session.route = StudioRoute::Library;
                session.song_context = None;
                session.activity_open = false;
                session.about_open = false;
                session.search_open = false;
                session.notice = None;
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::SetLibraryFacet(facet) => {
                session.library_view = LibraryView::All;
                session.library_search = None;
                session.library_facet = Some(facet.clone());
                session.route = StudioRoute::Library;
                session.song_context = None;
                session.notice = None;
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::LoadMoreSongs => {
                session.load_more_songs();
                invalidated.0 = true;
            }
            UiAction::ApplyLibrarySearch => {
                let value = search_inputs
                    .single()
                    .map(|input| input.value().to_string())
                    .unwrap_or_default();
                let value = value.trim();
                session.library_search = (!value.is_empty()).then(|| value.to_string());
                session.route = StudioRoute::Library;
                session.library_view = LibraryView::All;
                session.library_facet = None;
                session.search_open = false;
                session.refresh_library();
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::ClearLibrarySearch => {
                session.library_search = None;
                session.route = StudioRoute::Library;
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryLayout => {
                session.config.song_list_view = Some(
                    if session.config.song_list_view.as_deref() == Some("grid") {
                        "table"
                    } else {
                        "grid"
                    }
                    .to_string(),
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::ToggleExportAllMenu => {
                session.export_all_open = !session.export_all_open;
                session.open_library_select = None;
                invalidated.0 = true;
            }
            UiAction::ExportAllUtz | UiAction::ExportAllUltraStar => {
                session.export_all_open = false;
                let extension = if matches!(action, UiAction::ExportAllUtz) {
                    "utz"
                } else {
                    "txt"
                };
                if let Some(export_directory) = session.config.export_path.clone() {
                    session.notice = Some(start_export_all_job(
                        extension,
                        export_directory,
                        &mut session.export_job,
                    ));
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Storage;
                    session.request_cache_stats_refresh = true;
                    session.notice =
                        Some("Choose a default export folder before using Export all.".to_string());
                }
                invalidated.0 = true;
            }
            UiAction::OpenLibrarySelect(kind) => {
                session.open_library_select = if session.open_library_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                session.export_all_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectLibraryValue(kind, value) => {
                let value = (value != "all").then(|| value.clone());
                match kind {
                    LibrarySelectKind::Status => session.library_status = value,
                    LibrarySelectKind::TranscriptSource => {
                        session.library_transcript_source = value
                    }
                }
                session.open_library_select = None;
                session.refresh_library();
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::AnalyzeAll => {
                if app_core::analysis_runtime_status().ready {
                    app_core::enqueue_all(&session.library_filters());
                    session.analysis_tasks = app_core::load_analysis_tasks();
                    session.library_view = LibraryView::Queue;
                    session.library_facet = None;
                    session.route = StudioRoute::Library;
                    session.refresh_library();
                    session.notice = Some("Matching unanalyzed songs were queued.".to_string());
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Models;
                    session.notice = Some(
                        "Analysis is disabled until setup is completed in Settings > Models & runtime."
                            .to_string(),
                    );
                }
                invalidated.0 = true;
            }
            UiAction::Folders => {
                session.route = StudioRoute::Folders;
                session.notice = None;
                session.folder_browser.context_menu = None;
                if session.folder_browser.root.is_none()
                    && let Some(path) = session.config.library_paths().into_iter().next()
                {
                    session.folder_browser.select_root(path);
                }
                invalidated.0 = true;
            }
            UiAction::Settings => {
                session.route = StudioRoute::Settings;
                session.notice = None;
                session.open_settings_select = None;
                if session.settings_tab == SettingsTab::Storage {
                    session.request_cache_stats_refresh = true;
                }
                invalidated.0 = true;
            }
            UiAction::SettingsTab(tab) => {
                session.route = StudioRoute::Settings;
                session.settings_tab = *tab;
                session.notice = None;
                session.open_settings_select = None;
                session.request_cache_stats_refresh =
                    matches!(session.settings_tab, SettingsTab::Storage);
                invalidated.0 = true;
            }
            UiAction::ToggleFullscreen => {
                if let Some(error) = toggle_fullscreen(&mut window, &mut session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::OpenLog => {
                let path = app_core::default_uta_studio_dir().join("uta-studio.log");
                session.notice = Some(if path.is_file() {
                    match open::that_detached(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(error) => format!("Could not open {}: {error}", path.display()),
                    }
                } else {
                    format!("No application log exists yet at {}", path.display())
                });
                invalidated.0 = true;
            }
            UiAction::RunDiagnostics => {
                if diagnostics.receiver.is_some() {
                    session.notice = Some("Feature diagnostics are already running.".to_string());
                } else {
                    let (sender, receiver) = mpsc::channel();
                    std::thread::spawn(move || {
                        let report = uta_studio_diagnostics::run_feature_diagnostics(
                            uta_studio_diagnostics::DiagnosticRequest {
                                file_hash: None,
                                include_export_smoke: true,
                            },
                        );
                        let _ = sender.send(report);
                    });
                    diagnostics.receiver = Some(Mutex::new(receiver));
                    session.diagnostic_report = None;
                    session.notice =
                        Some("Running safe diagnostics and temporary export checks…".to_string());
                }
                invalidated.0 = true;
            }
            UiAction::RefreshRuntimeStatus => {
                session.notice = Some("Runtime status refreshed from local files.".to_string());
                invalidated.0 = true;
            }
            UiAction::OpenSettingsSelect(kind) => {
                session.open_settings_select = if session.open_settings_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                invalidated.0 = true;
            }
            UiAction::SelectSettingsValue(kind, value) => {
                match kind {
                    SettingsSelectKind::ComputeBackend => {
                        session.config.compute_backend = Some(value.clone());
                    }
                    SettingsSelectKind::Separator => {
                        session.config.separator = Some(value.clone());
                    }
                    SettingsSelectKind::SeparatorPreset => {
                        apply_separator_preset(&mut session.config, value);
                    }
                    SettingsSelectKind::AsrEngine => {
                        session.config.asr_engine = Some(value.clone());
                    }
                    SettingsSelectKind::WhisperModel => {
                        session.config.whisper_model = Some(value.clone());
                    }
                    SettingsSelectKind::AlignBackend => {
                        session.config.align_backend = Some(value.clone());
                    }
                    SettingsSelectKind::PitchModel => {
                        session.config.pitch_model = Some(value.clone());
                    }
                }
                if session.config.compute_backend.as_deref() != Some("intel")
                    && session.config.separator() == "openvino_demucs"
                {
                    session.config.separator = Some("karaoke".to_string());
                }
                session.open_settings_select = None;
                session.notice = save_config_error(&session.config).or_else(|| {
                    Some(match kind {
                        SettingsSelectKind::ComputeBackend => format!(
                            "Acceleration set to {}. Reconfigure the runtime to apply it.",
                            settings_select_label(*kind, value)
                        ),
                        SettingsSelectKind::SeparatorPreset => format!(
                            "{} separation profile applied. Existing stems change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                        _ => format!(
                            "{} selected. Existing charts change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                    })
                });
                invalidated.0 = true;
            }
            UiAction::ToggleAnalysisAdvanced(section) => {
                session.open_analysis_advanced = if session.open_analysis_advanced == Some(*section)
                {
                    None
                } else {
                    Some(*section)
                };
                session.open_settings_select = None;
                invalidated.0 = true;
            }
            UiAction::RequestSetup(target) => {
                if setup.receiver.is_some() {
                    session.notice = Some("A runtime setup job is already running.".to_string());
                } else {
                    session.pending_setup = Some(SetupRequest { target: *target });
                    session.notice = None;
                }
                invalidated.0 = true;
            }
            UiAction::CancelSetup => {
                session.pending_setup = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmSetup => {
                if let Some(request) = session.pending_setup.take() {
                    start_native_setup(&session.config, request, &mut setup);
                    session.notice = Some("Preparing analysis runtime…".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::RescanLibrary => {
                if session.config.library_paths().is_empty() {
                    session.notice = Some("Add a watched folder before scanning.".to_string());
                } else if session.scanning {
                    session.notice = Some("A library scan is already running.".to_string());
                } else {
                    session.scanning = true;
                    session.notice = Some("Library scan started.".to_string());
                    app_core::start_scan();
                }
                invalidated.0 = true;
            }
            UiAction::ToggleTheme => {
                session.config.dark_mode = Some(!theme.dark);
                session.notice = save_config_error(&session.config);
                *theme = StudioTheme::new(!theme.dark);
                clear_color.0 = theme.background;
                window.window_theme = Some(if theme.dark {
                    WindowTheme::Dark
                } else {
                    WindowTheme::Light
                });
                invalidated.0 = true;
            }
            UiAction::ChooseFolder => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let mut paths = session.config.library_paths();
                    if !paths.contains(&path) {
                        paths.push(path.clone());
                        session.config.library_source = Some(LibrarySource::Folders { paths });
                        if let Some(error) = save_config_error(&session.config) {
                            session.notice = Some(error);
                        } else {
                            session.scanning = true;
                            session.notice =
                                Some("Folder added; library scan started.".to_string());
                            app_core::start_scan();
                            session.refresh_library();
                            if session.route == StudioRoute::Folders {
                                session.folder_browser.select_root(path);
                            }
                        }
                    } else {
                        session.notice = Some("That folder is already watched.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::ChooseExportFolder => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    session.config.export_path = Some(path);
                    session.notice = save_config_error(&session.config)
                        .or_else(|| Some("Default export folder updated.".to_string()));
                    invalidated.0 = true;
                }
            }
            UiAction::ClearExportFolder => {
                session.config.export_path = None;
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some("Export dialogs will use the system default.".to_string()));
                invalidated.0 = true;
            }
            UiAction::SelectFolderRoot(path) => {
                session.folder_browser.select_root(path.clone());
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::FolderUp => {
                if let Some(parent) = session.folder_browser.parent() {
                    session.folder_browser.current = Some(parent);
                    session.folder_browser.context_menu = None;
                    session.folder_browser.refresh();
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::OpenFolderEntry(path) => {
                session.folder_browser.context_menu = None;
                if path.is_dir() {
                    session.folder_browser.current = Some(path.clone());
                    session.folder_browser.refresh();
                    session.notice = None;
                } else {
                    session.notice = Some(open_library_entry(path, &session.config));
                }
                invalidated.0 = true;
            }
            UiAction::RevealFolderEntry(path) => {
                session.folder_browser.context_menu = None;
                session.notice = Some(reveal_library_entry(path, &session.config));
                invalidated.0 = true;
            }
            UiAction::DismissFolderContext => {
                session.folder_browser.context_menu = None;
                invalidated.0 = true;
            }
            UiAction::RequestRemoveFolder(path) => {
                session.folder_browser.context_menu = None;
                session.folder_browser.pending_remove = Some(path.clone());
                invalidated.0 = true;
            }
            UiAction::CancelRemoveFolder => {
                session.folder_browser.pending_remove = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmRemoveFolder => {
                if let Some(path) = session.folder_browser.pending_remove.take() {
                    let mut paths = session.config.library_paths();
                    paths.retain(|entry| entry != &path);
                    session.config.library_source = if paths.is_empty() {
                        None
                    } else {
                        Some(LibrarySource::Folders { paths })
                    };
                    if let Some(error) = save_config_error(&session.config) {
                        session.notice = Some(error);
                    } else {
                        if session.config.library_source.is_some() {
                            session.scanning = true;
                            app_core::start_scan();
                        } else {
                            app_core::clear_library_index();
                            session.scanning = false;
                        }
                        session.notice = Some(format!(
                            "Stopped watching {}. No source media was moved or deleted.",
                            path.display()
                        ));
                    }
                    session.folder_browser = FolderBrowser::new(&session.config);
                    session.refresh_library();
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustBeamSize(delta) => {
                session.config.beam_size = Some(
                    (i64::from(session.config.beam_size()) + i64::from(*delta)).clamp(1, 16) as u32,
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustBatchSize(delta) => {
                session.config.batch_size = Some(
                    (i64::from(session.config.batch_size()) + i64::from(*delta)).clamp(1, 16)
                        as u32,
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorSegmentSize(delta) => {
                session.config.separator_segment_size = Some(
                    (i64::from(session.config.separator_segment_size()) + i64::from(*delta))
                        .clamp(64, 1024) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorOverlap(delta) => {
                session.config.separator_overlap = Some(
                    (i64::from(session.config.separator_overlap()) + i64::from(*delta))
                        .clamp(2, 32) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorBatchSize(delta) => {
                session.config.separator_batch_size = Some(
                    (i64::from(session.config.separator_batch_size()) + i64::from(*delta))
                        .clamp(1, 8) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorNormalization(delta) => {
                session.config.separator_normalization_pct = Some(
                    (i64::from(session.config.separator_normalization_pct()) + i64::from(*delta))
                        .clamp(1, 100) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustDemucsShifts(delta) => {
                session.config.demucs_shifts = Some(
                    (i64::from(session.config.demucs_shifts()) + i64::from(*delta))
                        .clamp(1, 8) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustDemucsOverlap(delta) => {
                session.config.demucs_overlap_pct = Some(
                    (i64::from(session.config.demucs_overlap_pct()) + i64::from(*delta))
                        .clamp(1, 95) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustUiFontScale(delta) => {
                let current = ui_font_size_percent_to_points(session.config.font_scale_percent());
                let next = (i64::from(current)
                    + i64::from(*delta) * i64::from(UI_FONT_SIZE_STEP_PX))
                .clamp(
                    i64::from(UI_FONT_SIZE_MIN_PX),
                    i64::from(UI_FONT_SIZE_MAX_PX),
                );
                let next_percent = ui_font_points_to_scale_percent(next as u32);
                session.config.font_scale_percent = Some(next_percent);
                set_ui_font_scale(next_percent as f32 / 100.0);
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some(format!("Font size: {}px", next)));
                invalidated.0 = true;
            }
            UiAction::ToggleAutoAnalyze => {
                session.config.auto_analyze = Some(!session.config.auto_analyze());
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustVocalThreshold(delta) => {
                let current = (session.config.vocal_detection_threshold_pct() * 100.0).round();
                let value = (current + f64::from(*delta)).clamp(0.0, 60.0) / 100.0;
                session.config.vocal_detection_threshold_pct = Some(value);
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::RestoreAnalysisDefaults => {
                session.config.separator = Some("karaoke".to_string());
                session.config.separator_segment_size = None;
                session.config.separator_overlap = None;
                session.config.separator_batch_size = None;
                session.config.separator_normalization_pct = None;
                session.config.demucs_shifts = None;
                session.config.demucs_overlap_pct = None;
                session.config.asr_engine = Some("whisper".to_string());
                session.config.align_backend = Some("whisperx".to_string());
                session.config.pitch_model = Some("rmvpe".to_string());
                session.config.vocal_detection_threshold_pct = Some(0.15);
                session.config.whisper_model = Some("large-v3".to_string());
                session.config.beam_size = Some(8);
                session.config.batch_size = Some(8);
                session.config.compute_backend = Some("cpu".to_string());
                session.config.auto_analyze = Some(false);
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some("Analysis defaults restored.".to_string()));
                invalidated.0 = true;
            }
            UiAction::RequestClearCache(scope) => {
                session.pending_cache_clear = Some(*scope);
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CancelClearCache => {
                session.pending_cache_clear = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmClearCache => {
                if let Some(scope) = session.pending_cache_clear.take() {
                    match scope {
                        CacheClearScope::Generated => {
                            app_core::CacheDir::new().clear_all();
                            session.refresh_library();
                            session.request_cache_stats_refresh = true;
                            session.notice = Some(
                                "Generated cache cleared. Source media was not changed."
                                    .to_string(),
                            );
                        }
                        CacheClearScope::Models => {
                            app_core::clear_models();
                            session.request_cache_stats_refresh = true;
                            session.notice = Some(
                                "Downloaded models cleared. Runtime setup now reports the missing artifacts."
                                    .to_string(),
                            );
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::OpenSong(file_hash) => {
                session.song_context = None;
                session.selected_song = Some(file_hash.clone());
                session.route = StudioRoute::SongDetail;
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::AnalyzeSong(file_hash) => {
                session.song_context = None;
                if app_core::analysis_runtime_status().ready {
                    app_core::enqueue_one(file_hash);
                    session.analysis_tasks = app_core::load_analysis_tasks();
                    session.library_view = LibraryView::Queue;
                    session.library_facet = None;
                    session.route = StudioRoute::Library;
                    session.refresh_library();
                    session.notice = Some("Song queued for analysis.".to_string());
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Models;
                    session.notice = Some(
                        "Analysis is disabled until the runtime and selected models are installed."
                            .to_string(),
                    );
                }
                invalidated.0 = true;
            }
            UiAction::OpenEditor(file_hash) => {
                session.song_context = None;
                session.selected_song = Some(file_hash.clone());
                session.editor = None;
                session.route = StudioRoute::Editor;
                if session.library_playback.status.playing
                    && let Ok(status) = library_audio.0.pause()
                {
                    session.library_playback.visible_position = status.position_secs;
                    session.library_playback.status = status;
                    session.library_playback.last_audio_sync = Instant::now();
                }
                session.notice = Some(start_editor_load_job(
                    file_hash,
                    Arc::clone(&audio.0),
                    &mut session.editor_load_job,
                ));
                invalidated.0 = true;
            }
            UiAction::ExportUtz(file_hash) => {
                session.song_context = None;
                let export_directory = session.config.export_path.clone();
                session.notice = Some(start_export_job(
                    file_hash,
                    "utz",
                    export_directory,
                    &mut session.export_job,
                ));
                invalidated.0 = true;
            }
            UiAction::ExportUltraStar(file_hash) => {
                session.song_context = None;
                let export_directory = session.config.export_path.clone();
                session.notice = Some(start_export_job(
                    file_hash,
                    "txt",
                    export_directory,
                    &mut session.export_job,
                ));
                invalidated.0 = true;
            }
            UiAction::OpenSource(path) => {
                session.song_context = None;
                session.notice = Some(match validate_source_path(path, &session.config) {
                    Ok(path) => match open::that_detached(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(error) => format!("Could not open {}: {error}", path.display()),
                    },
                    Err(error) => error,
                });
                invalidated.0 = true;
            }
            UiAction::RevealSource(path) => {
                session.song_context = None;
                session.notice = Some(reveal_library_entry(path, &session.config));
                invalidated.0 = true;
            }
            UiAction::DismissSongContext => {
                session.song_context = None;
                invalidated.0 = true;
            }
            UiAction::OpenLyricsEditor(file_hash) => {
                let song = app_core::load_song_by_hash(file_hash).ok().flatten();
                let mode = if song.as_ref().is_some_and(|song| {
                    matches!(
                        song.transcript_source,
                        Some(app_core::TranscriptSource::Lrc)
                    )
                }) {
                    LyricsInputMode::TimedLrc
                } else {
                    LyricsInputMode::Plain
                };
                session.lyrics_editor = Some(NativeLyricsEditor {
                    file_hash: file_hash.clone(),
                    mode,
                    separate_stems: true,
                    initial_text: lyrics_text(file_hash, mode),
                    candidates: Vec::new(),
                    candidate_index: 0,
                    searching: false,
                });
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CloseLyricsEditor => {
                session.lyrics_editor = None;
                invalidated.0 = true;
            }
            UiAction::ToggleLyricsInputMode => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = lyrics_inputs.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.mode = if editor.mode == LyricsInputMode::Plain {
                        LyricsInputMode::TimedLrc
                    } else {
                        LyricsInputMode::Plain
                    };
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleLyricsSeparateStems => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = lyrics_inputs.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.separate_stems = !editor.separate_stems;
                    invalidated.0 = true;
                }
            }
            UiAction::SearchLrclibLyrics => {
                if session.lyrics_search_job.receiver.is_none()
                    && let Some(editor) = session.lyrics_editor.as_mut()
                {
                    if let Ok(input) = lyrics_inputs.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.searching = true;
                    editor.candidates.clear();
                    editor.candidate_index = 0;
                    let file_hash = editor.file_hash.clone();
                    let (sender, receiver) = mpsc::channel();
                    std::thread::spawn(move || {
                        let candidates = app_core::search_lrclib_for_hash(&file_hash);
                        let _ = sender.send(candidates);
                    });
                    session.lyrics_search_job.receiver = Some(Mutex::new(receiver));
                    session.notice = Some("Searching LRCLIB…".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::PreviousLrclibCandidate => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = lyrics_inputs.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.candidate_index = editor.candidate_index.saturating_sub(1);
                    invalidated.0 = true;
                }
            }
            UiAction::NextLrclibCandidate => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = lyrics_inputs.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.candidate_index =
                        (editor.candidate_index + 1).min(editor.candidates.len().saturating_sub(1));
                    invalidated.0 = true;
                }
            }
            UiAction::UseLrclibPlain => {
                if let Some(editor) = session.lyrics_editor.as_mut()
                    && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                {
                    editor.initial_text = candidate.lines.join("\n");
                    editor.mode = LyricsInputMode::Plain;
                    session.notice = Some("LRCLIB plain lyrics loaded for review.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::UseLrclibTimed => {
                if let Some(editor) = session.lyrics_editor.as_mut()
                    && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                    && let Some(lrc) = candidate.synced_lyrics.as_ref()
                {
                    editor.initial_text = lrc.clone();
                    editor.mode = LyricsInputMode::TimedLrc;
                    session.notice = Some("LRCLIB timed lyrics loaded for review.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::SaveLyricsEditor => {
                let value = lyrics_inputs
                    .single()
                    .map(|input| input.value().to_string())
                    .unwrap_or_default();
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    editor.initial_text = value.clone();
                    let song = app_core::load_song_by_hash(&editor.file_hash)
                        .ok()
                        .flatten();
                    let requires_runtime = editor.mode == LyricsInputMode::Plain
                        || (editor.mode == LyricsInputMode::TimedLrc
                            && editor.separate_stems
                            && song.as_ref().is_some_and(|song| !song.is_analyzed));
                    let result = if requires_runtime && !app_core::analysis_runtime_status().ready {
                        Err("Analysis runtime is unavailable. Choose authoring on the original mix for timed LRC, or finish setup in Settings > Models & runtime.".to_string())
                    } else if editor.mode == LyricsInputMode::TimedLrc {
                        if song.as_ref().is_some_and(|song| song.is_analyzed) {
                            app_core::apply_timed_lyrics(&editor.file_hash, &value)
                        } else {
                            app_core::provide_lrc(&editor.file_hash, &value, editor.separate_stems)
                        }
                    } else {
                        app_core::save_lyrics_and_realign(
                            &editor.file_hash,
                            value.lines().map(str::to_string).collect(),
                        )
                    };
                    match result {
                        Ok(()) => {
                            session.lyrics_editor = None;
                            session.refresh_library();
                            session.notice = Some(
                                if requires_runtime {
                                    "Lyrics saved and alignment queued."
                                } else {
                                    "Timed lyrics saved."
                                }
                                .to_string(),
                            );
                        }
                        Err(error) => {
                            session.notice = Some(format!("Could not save lyrics: {error}"))
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::OpenLanguageEditor(file_hash) => {
                let config = AppConfig::load();
                let initial_language = config
                    .language_override(file_hash)
                    .map(canonical_analysis_language)
                    .unwrap_or_else(|| "auto".into());
                session.language_editor = Some(NativeLanguageEditor {
                    file_hash: file_hash.clone(),
                    initial_language,
                    force_transcribe: false,
                    picker_open: false,
                });
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CloseLanguageEditor => {
                session.language_editor = None;
                invalidated.0 = true;
            }
            UiAction::ToggleLanguageReprocess => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.force_transcribe = !editor.force_transcribe;
                    editor.picker_open = false;
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleLanguagePicker => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.picker_open = !editor.picker_open;
                    invalidated.0 = true;
                }
            }
            UiAction::SelectAnalysisLanguage(language) => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.initial_language = canonical_analysis_language(language);
                    editor.picker_open = false;
                    invalidated.0 = true;
                }
            }
            UiAction::SaveLanguageEditor => {
                if let Some(editor) = session.language_editor.as_mut() {
                    let language = editor.initial_language.clone();
                    if !app_core::analysis_runtime_status().ready {
                        session.notice = Some(
                            "Analysis is disabled until setup is completed in Settings > Models & runtime."
                                .to_string(),
                        );
                    } else {
                        if language == "auto" {
                            let mut config = AppConfig::load();
                            config.clear_language_override(&editor.file_hash);
                            if let Err(error) = config.save() {
                                session.notice =
                                    Some(format!("Could not save the language setting: {error}"));
                                invalidated.0 = true;
                                continue;
                            }
                            if editor.force_transcribe {
                                app_core::reanalyze_force_transcribe(&editor.file_hash);
                            } else {
                                let _ = app_core::realign(&editor.file_hash, None);
                            }
                        } else if editor.force_transcribe {
                            let mut config = AppConfig::load();
                            config
                                .set_language_override(editor.file_hash.clone(), language.clone());
                            if let Err(error) = config.save() {
                                session.notice =
                                    Some(format!("Could not save the language setting: {error}"));
                                invalidated.0 = true;
                                continue;
                            }
                            app_core::reanalyze_force_transcribe(&editor.file_hash);
                        } else {
                            let _ = app_core::realign(&editor.file_hash, Some(language.clone()));
                        }
                        session.language_editor = None;
                        session.config = AppConfig::load();
                        session.notice = Some(if language == "auto" {
                            "Automatic language detection enabled; reprocessing queued.".into()
                        } else {
                            format!("Language set to {language}; reprocessing queued.")
                        });
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::RealignSong(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::realign(file_hash, None)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzeTranscript(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_transcript(file_hash, None)
                }));
                invalidated.0 = true;
            }
            UiAction::ForceTranscribe(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_force_transcribe(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzePitch(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_pitch(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzeFull(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_full(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::RequestDeleteSongCache(file_hash) => {
                session.pending_cache_delete = Some(file_hash.clone());
                invalidated.0 = true;
            }
            UiAction::CancelDeleteSongCache => {
                session.pending_cache_delete = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmDeleteSongCache => {
                if let Some(file_hash) = session.pending_cache_delete.take() {
                    app_core::delete_cache(&file_hash);
                    session.refresh_library();
                    session.notice = Some(
                        "Generated song data deleted. Source media was not changed.".to_string(),
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::ShiftSongKey(file_hash, delta) => {
                session.notice = Some(start_key_shift(
                    file_hash,
                    *delta,
                    &mut authoring,
                    &mut session.authoring_busy,
                ));
                invalidated.0 = true;
            }
            UiAction::ShiftSongTempo(file_hash, delta) => {
                session.notice = Some(start_tempo_shift(
                    file_hash,
                    *delta,
                    &mut authoring,
                    &mut session.authoring_busy,
                ));
                invalidated.0 = true;
            }
            UiAction::PlayLibrarySong(file_hash) => {
                let queue = session.songs.processed.clone();
                prepare_library_queue(&queue, file_hash, &mut session.library_playback);
                session.notice =
                    play_library_song(&library_audio.0, file_hash, &mut session.library_playback)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryPlayback => {
                session.notice =
                    toggle_library_playback(&library_audio.0, &mut session.library_playback).err();
                invalidated.0 = true;
            }
            UiAction::SeekLibraryRelative(delta) => {
                session.notice = seek_library_relative(
                    &library_audio.0,
                    &mut session.library_playback,
                    f64::from(*delta),
                )
                .err();
                invalidated.0 = true;
            }
            UiAction::PreviousLibrarySong => {
                session.notice = if library_visible_position(&session.library_playback) > 5.0 {
                    restart_library_song(&library_audio.0, &mut session.library_playback).err()
                } else {
                    let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
                    advance_library_queue(&library_audio.0, &mut session.library_playback, -1, wrap)
                        .err()
                };
                invalidated.0 = true;
            }
            UiAction::NextLibrarySong => {
                let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
                session.notice =
                    advance_library_queue(&library_audio.0, &mut session.library_playback, 1, wrap)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryShuffle => {
                session.library_playback.shuffle = !session.library_playback.shuffle;
                session.notice = Some(if session.library_playback.shuffle {
                    "Shuffle enabled for the playback queue.".to_string()
                } else {
                    "Shuffle disabled.".to_string()
                });
                invalidated.0 = true;
            }
            UiAction::CycleLibraryRepeat => {
                session.library_playback.repeat = session.library_playback.repeat.next();
                session.notice = Some(session.library_playback.repeat.label().to_string());
                invalidated.0 = true;
            }
            UiAction::AdjustLibraryVolume(delta) => {
                let volume = session.library_playback.volume + f64::from(*delta) / 100.0;
                session.notice =
                    set_library_volume(&library_audio.0, &mut session.library_playback, volume)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryMute => {
                let volume = if session.library_playback.volume > 0.0 {
                    session.library_playback.volume_before_mute = session.library_playback.volume;
                    0.0
                } else {
                    session.library_playback.volume_before_mute.max(0.1)
                };
                session.notice =
                    set_library_volume(&library_audio.0, &mut session.library_playback, volume)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryQueue => {
                session.library_playback.queue_open = !session.library_playback.queue_open;
                invalidated.0 = true;
            }
            UiAction::TogglePlayback => {
                session.notice = toggle_editor_playback(&audio.0, session.editor.as_mut()).err();
                invalidated.0 = true;
            }
            UiAction::SeekEditorStart => {
                if let Some(editor) = session.editor.as_mut() {
                    let was_playing = editor.audio_status.playing;
                    match audio.0.seek(0.0) {
                        Ok(mut status) => {
                            if was_playing && let Ok(playing) = audio.0.play() {
                                status = playing;
                            }
                            editor.visible_position = status.position_secs;
                            editor.audio_status = status;
                            editor.last_audio_sync = Instant::now();
                            session.notice = None;
                        }
                        Err(error) => session.notice = Some(error),
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::OpenEditorSelect(kind) => {
                session.open_editor_select = if session.open_editor_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                invalidated.0 = true;
            }
            UiAction::SelectEditorValue(kind, value) => {
                session.open_editor_select = None;
                if let Some(editor) = session.editor.as_mut() {
                    match kind {
                        EditorDockSelectKind::AudioSource => {
                            session.notice =
                                select_editor_audio_source(&audio.0, editor, value).err();
                        }
                        EditorDockSelectKind::SnapGrid => {
                            const GRIDS: [f64; 6] = [0.0, 0.01, 0.025, 0.05, 0.1, 0.25];
                            match value.parse::<f64>() {
                                Ok(value) if GRIDS.contains(&value) => {
                                    editor.snap_seconds = value;
                                    session.notice = None;
                                }
                                _ => {
                                    session.notice =
                                        Some("That timing grid is not supported.".to_string())
                                }
                            }
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleLyrics => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.lyrics_hidden = !editor.lyrics_hidden;
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleInspector => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.inspector_open = !editor.inspector_open;
                    invalidated.0 = true;
                }
            }
            UiAction::SaveEditor => {
                if let Some(editor) = session.editor.as_mut() {
                    session.notice = match app_core::save_chart(
                        &editor.chart.file_hash,
                        editor.chart.transcript.clone(),
                        editor.chart.pitch_notes.clone(),
                    ) {
                        Ok(()) => {
                            editor.dirty = false;
                            Some("Chart saved atomically.".to_string())
                        }
                        Err(error) => Some(format!("Could not save chart: {error}")),
                    };
                    invalidated.0 = true;
                }
            }
            UiAction::EditorUndo => {
                if let Some(editor) = session.editor.as_mut()
                    && editor.undo()
                {
                    session.notice = Some("Undid chart edit.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::EditorRedo => {
                if let Some(editor) = session.editor.as_mut()
                    && editor.redo()
                {
                    session.notice = Some("Redid chart edit.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::AddEditorNote => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.checkpoint();
                    let start = editor.visible_position.max(0.0);
                    let midi = ((editor.pitch_min + editor.pitch_max) / 2.0)
                        .round()
                        .clamp(0.0, 127.0);
                    let selected = insert_chart_note(
                        &mut editor.chart.pitch_notes,
                        serde_json::json!({
                            "start": start,
                            "end": start + 0.5,
                            "midi": midi,
                            "confidence": 1.0,
                            "kind": "normal"
                        }),
                    );
                    if let Some(selected) = selected {
                        editor.select_only_note(selected);
                    }
                    editor.dirty = true;
                    session.notice = Some("Added note at the playhead.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::DeleteEditorNote => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    if selected.is_empty() {
                        let words = editor.selected_word_indices();
                        if !words.is_empty() {
                            editor.checkpoint();
                            let deleted = delete_editor_words(&mut editor.chart.transcript, &words);
                            if deleted > 0 {
                                editor.selected_word = None;
                                editor.selected_words.clear();
                                editor.word_edit_focus = None;
                                editor.dirty = true;
                                session.notice = Some(format!("Deleted {deleted} lyric word(s)."));
                                invalidated.0 = true;
                            } else {
                                editor.undo.pop();
                            }
                        }
                        continue;
                    }
                    editor.checkpoint();
                    let removed = remove_chart_notes(&mut editor.chart.pitch_notes, &selected);
                    if removed > 0 {
                        editor.selected_note = None;
                        editor.selected_notes.clear();
                        editor.dirty = true;
                        session.notice = Some(format!("Deleted {removed} selected note(s)."));
                        invalidated.0 = true;
                    }
                }
            }
            UiAction::SplitEditorNote => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    if selected.is_empty() {
                        let words = editor.selected_word_indices();
                        if !words.is_empty() {
                            editor.checkpoint();
                            let next = split_selected_editor_words(
                                &mut editor.chart.transcript,
                                &words,
                                editor.visible_position,
                            );
                            if !next.is_empty() {
                                editor.selected_word = next.iter().next().copied();
                                editor.selected_words = next;
                                editor.word_edit_focus = None;
                                editor.dirty = true;
                                session.notice = Some("Split selected lyric word(s).".to_string());
                            } else {
                                editor.undo.pop();
                                session.notice = Some(
                                    "The selected lyric words are too short to split.".to_string(),
                                );
                            }
                            invalidated.0 = true;
                        }
                        continue;
                    }
                    editor.checkpoint();
                    let next = split_chart_notes(
                        &mut editor.chart.pitch_notes,
                        &selected,
                        editor.visible_position,
                    );
                    if !next.is_empty() {
                        editor.selected_note = next.iter().next().copied();
                        editor.selected_notes = next;
                        editor.dirty = true;
                        session.notice = Some("Split selected note(s).".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice = Some(
                            "Move the playhead inside the selected note before splitting."
                                .to_string(),
                        );
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::MergeEditorNotes => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    if selected.len() < 2 {
                        let words = editor.selected_word_indices();
                        if !words.is_empty() {
                            editor.checkpoint();
                            let merged = if words.len() == 1 {
                                words.first().copied().filter(|selection| {
                                    merge_editor_word(&mut editor.chart.transcript, *selection)
                                })
                            } else {
                                merge_selected_editor_words(&mut editor.chart.transcript, &words)
                            };
                            if let Some(selection) = merged {
                                editor.select_only_word(selection);
                                editor.dirty = true;
                                session.notice = Some("Merged selected lyric words.".to_string());
                            } else {
                                editor.undo.pop();
                                session.notice =
                                    Some("Select words from the same phrase to merge.".to_string());
                            }
                            invalidated.0 = true;
                            continue;
                        }
                        session.notice = Some("Select at least two notes to merge.".to_string());
                        invalidated.0 = true;
                        continue;
                    }
                    editor.checkpoint();
                    if let Some(index) = merge_chart_notes(
                        &mut editor.chart.pitch_notes,
                        &selected,
                        editor.selected_note,
                    ) {
                        editor.select_only_note(index);
                        editor.dirty = true;
                        session.notice = Some("Merged selected notes.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::QuantizeEditorNotes => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    if selected.is_empty() || editor.snap_seconds <= 0.0 {
                        session.notice =
                            Some("Select notes and enable a timing grid first.".to_string());
                        invalidated.0 = true;
                        continue;
                    }
                    editor.checkpoint();
                    let changed = quantize_chart_notes(
                        &mut editor.chart.pitch_notes,
                        Some(&selected),
                        editor.snap_seconds,
                    );
                    editor.dirty |= changed > 0;
                    session.notice = Some(format!("Quantized {changed} note(s)."));
                    invalidated.0 = true;
                }
            }
            UiAction::DuplicateEditorNotes => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    let clipboard = copy_chart_notes(&editor.chart.pitch_notes, &selected);
                    if clipboard.is_empty() {
                        continue;
                    }
                    let duration = clipboard
                        .iter()
                        .map(|note| note_number(note, "end", 0.03))
                        .reduce(f64::max)
                        .unwrap_or(0.03);
                    editor.checkpoint();
                    let inserted = paste_chart_notes(
                        &mut editor.chart.pitch_notes,
                        &clipboard,
                        editor.visible_position.max(duration + 0.01),
                    );
                    editor.selected_note = inserted.iter().next().copied();
                    editor.selected_notes = inserted;
                    editor.dirty = true;
                    session.notice = Some("Duplicated selected note(s).".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::RepairEditorChart => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.checkpoint();
                    if repair_editor_chart(&mut editor.chart) {
                        editor.selected_note = None;
                        editor.selected_notes.clear();
                        editor.selected_word = None;
                        editor.selected_words.clear();
                        editor.word_edit_focus = None;
                        editor.dirty = true;
                        session.notice = Some("Applied safe timing repairs.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustEditorTimeZoom(direction) => {
                if let Some(editor) = session.editor.as_mut() {
                    let center = editor.viewport_start + editor.viewport_duration / 2.0;
                    let factor = if *direction > 0 { 0.8 } else { 1.25 };
                    editor.viewport_duration =
                        (editor.viewport_duration * factor).clamp(2.0, 180.0);
                    editor.viewport_start = (center - editor.viewport_duration / 2.0).max(0.0);
                    editor.manual_scroll_until = Instant::now() + Duration::from_secs(2);
                    invalidated.0 = true;
                }
            }
            UiAction::PanEditorPitch(direction) => {
                if let Some(editor) = session.editor.as_mut() {
                    let span = editor.pitch_max - editor.pitch_min;
                    let offset = f64::from(*direction) * 4.0;
                    editor.pitch_min = (editor.pitch_min + offset).clamp(0.0, 127.0 - span);
                    editor.pitch_max = editor.pitch_min + span;
                    editor.manual_scroll_until = Instant::now() + Duration::from_secs(2);
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustEditorPitchZoom(direction) => {
                if let Some(editor) = session.editor.as_mut() {
                    let factor = if *direction > 0 { 0.8 } else { 1.25 };
                    let span = (editor.pitch_max - editor.pitch_min) * factor;
                    set_editor_pitch_span(editor, span);
                    editor.manual_scroll_until = Instant::now() + Duration::from_secs(2);
                    invalidated.0 = true;
                }
            }
            UiAction::ShiftWholeChart(direction) => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.checkpoint();
                    let seconds = f64::from(*direction) * 0.01;
                    shift_all_chart_timings(&mut editor.chart, seconds);
                    editor.dirty = true;
                    session.notice = Some(format!(
                        "Shifted the whole chart by {}10 ms.",
                        if *direction < 0 { "−" } else { "+" }
                    ));
                    invalidated.0 = true;
                }
            }
            UiAction::CopyEditorNote => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    editor.clipboard_notes = copy_chart_notes(&editor.chart.pitch_notes, &selected);
                    session.notice = Some(format!(
                        "Copied {} selected note(s).",
                        editor.clipboard_notes.len()
                    ));
                    invalidated.0 = true;
                }
            }
            UiAction::PasteEditorNote => {
                if let Some(editor) = session.editor.as_mut()
                    && !editor.clipboard_notes.is_empty()
                {
                    editor.checkpoint();
                    let inserted = paste_chart_notes(
                        &mut editor.chart.pitch_notes,
                        &editor.clipboard_notes,
                        editor.visible_position,
                    );
                    editor.selected_note = inserted.iter().next().copied();
                    editor.selected_notes = inserted;
                    editor.dirty = true;
                    session.notice = Some("Pasted note(s) at the playhead.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::CycleEditorNoteKind => {
                if let Some(editor) = session.editor.as_mut() {
                    let selected = editor.selected_note_indices();
                    if selected.is_empty() {
                        continue;
                    }
                    editor.checkpoint();
                    let changed = cycle_chart_note_kinds(&mut editor.chart.pitch_notes, &selected);
                    if changed > 0 {
                        editor.dirty = true;
                        session.notice = Some("Changed selected note type(s).".to_string());
                        invalidated.0 = true;
                    }
                }
            }
            UiAction::SelectEditorWord(segment, word, position_ms) => {
                let was_playing = session
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.audio_status.playing);
                match audio.0.seek(*position_ms as f64 / 1000.0) {
                    Ok(mut status) => {
                        if was_playing && let Ok(playing) = audio.0.play() {
                            status = playing;
                        }
                        if let Some(editor) = session.editor.as_mut() {
                            let selection = WordSelection {
                                segment: *segment,
                                word: *word,
                            };
                            editor.visible_position = status.position_secs;
                            editor.audio_status = status;
                            editor.last_audio_sync = Instant::now();
                            let additive = keys.any_pressed([
                                KeyCode::ShiftLeft,
                                KeyCode::ShiftRight,
                                KeyCode::ControlLeft,
                                KeyCode::ControlRight,
                            ]);
                            if additive {
                                if !editor.selected_words.remove(&selection) {
                                    editor.selected_words.insert(selection);
                                    editor.selected_word = Some(selection);
                                } else {
                                    editor.selected_word =
                                        editor.selected_words.iter().next().copied();
                                }
                                editor.word_edit_focus = None;
                                editor.selected_note = None;
                                editor.selected_notes.clear();
                            } else if editor.word_edit_focus == Some(selection) {
                                editor.select_only_word(selection);
                                editor.word_edit_focus = Some(selection);
                            } else if editor.selected_words.len() > 1
                                && editor.selected_words.contains(&selection)
                            {
                                editor.selected_word = Some(selection);
                                editor.selected_note = None;
                                editor.selected_notes.clear();
                            } else {
                                editor.select_only_word(selection);
                            }
                            editor.inspector_open = true;
                        }
                        session.notice = None;
                    }
                    Err(error) => session.notice = Some(error),
                }
                invalidated.0 = true;
            }
            UiAction::AddEditorWord => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.checkpoint();
                    if let Some(selection) = insert_editor_word(
                        &mut editor.chart.transcript,
                        editor.selected_word,
                        editor.visible_position,
                    ) {
                        editor.select_only_word(selection);
                        editor.word_edit_focus = Some(selection);
                        editor.inspector_open = true;
                        editor.dirty = true;
                        session.notice = Some(
                            "Added a lyric word at the playhead. Type in the inspector to replace its text."
                                .to_string(),
                        );
                    } else {
                        editor.undo.pop();
                        session.notice = Some("Could not add a lyric word here.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::DeleteEditorWord => {
                if let Some(editor) = session.editor.as_mut() {
                    let words = editor.selected_word_indices();
                    if words.is_empty() {
                        session.notice = Some("Select lyric words to delete.".to_string());
                        invalidated.0 = true;
                        continue;
                    }
                    editor.checkpoint();
                    let deleted = delete_editor_words(&mut editor.chart.transcript, &words);
                    if deleted > 0 {
                        editor.selected_word = None;
                        editor.selected_words.clear();
                        editor.word_edit_focus = None;
                        editor.dirty = true;
                        session.notice = Some(format!("Deleted {deleted} lyric word(s)."));
                    } else {
                        editor.undo.pop();
                        session.notice = Some("Could not delete the lyric selection.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::ShiftEditorWord(direction) => {
                if let Some(editor) = session.editor.as_mut() {
                    let words = editor.selected_word_indices();
                    if words.is_empty() {
                        continue;
                    }
                    editor.checkpoint();
                    let moved = words
                        .iter()
                        .filter(|selection| {
                            shift_editor_word(
                                &mut editor.chart.transcript,
                                **selection,
                                f64::from(*direction) * 0.01,
                            )
                        })
                        .count();
                    if moved > 0 {
                        editor.dirty = true;
                        session.notice = Some(format!(
                            "Moved {moved} lyric word(s) {} 10 ms.",
                            if *direction < 0 { "earlier" } else { "later" }
                        ));
                    } else {
                        editor.undo.pop();
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustEditorWordStart(direction) => {
                if let Some(editor) = session.editor.as_mut()
                    && let Some(selection) = editor.selected_word
                {
                    editor.checkpoint();
                    if adjust_editor_word_boundary(
                        &mut editor.chart.transcript,
                        selection,
                        f64::from(*direction) * 0.01,
                        0.0,
                    ) {
                        editor.dirty = true;
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustEditorWordEnd(direction) => {
                if let Some(editor) = session.editor.as_mut()
                    && let Some(selection) = editor.selected_word
                {
                    editor.checkpoint();
                    if adjust_editor_word_boundary(
                        &mut editor.chart.transcript,
                        selection,
                        0.0,
                        f64::from(*direction) * 0.01,
                    ) {
                        editor.dirty = true;
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::SplitEditorWord => {
                if let Some(editor) = session.editor.as_mut() {
                    let words = editor.selected_word_indices();
                    if words.is_empty() {
                        continue;
                    }
                    editor.checkpoint();
                    let next = split_selected_editor_words(
                        &mut editor.chart.transcript,
                        &words,
                        editor.visible_position,
                    );
                    if !next.is_empty() {
                        editor.selected_word = next.iter().next().copied();
                        editor.selected_words = next;
                        editor.word_edit_focus = None;
                        editor.dirty = true;
                        session.notice = Some("Split selected lyric word(s).".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice =
                            Some("The selected lyric words are too short to split.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::MergeEditorWord => {
                if let Some(editor) = session.editor.as_mut() {
                    let words = editor.selected_word_indices();
                    editor.checkpoint();
                    let merged = if words.len() == 1 {
                        words.first().copied().filter(|selection| {
                            merge_editor_word(&mut editor.chart.transcript, *selection)
                        })
                    } else {
                        merge_selected_editor_words(&mut editor.chart.transcript, &words)
                    };
                    if let Some(selection) = merged {
                        editor.select_only_word(selection);
                        editor.dirty = true;
                        session.notice = Some("Merged selected lyric words.".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice = Some(
                            "Select at least two words from the same phrase to merge.".to_string(),
                        );
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::SplitEditorPhrase => {
                if let Some(editor) = session.editor.as_mut()
                    && let Some(selection) = editor.selected_word
                {
                    editor.checkpoint();
                    if let Some(next) = split_editor_phrase(&mut editor.chart.transcript, selection)
                    {
                        editor.select_only_word(next);
                        editor.dirty = true;
                        session.notice = Some("Started a new lyric phrase.".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice =
                            Some("Select a word before the end of its phrase.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::MergeEditorPhrase => {
                if let Some(editor) = session.editor.as_mut()
                    && let Some(selection) = editor.selected_word
                {
                    editor.checkpoint();
                    if let Some(next) = merge_editor_phrase(&mut editor.chart.transcript, selection)
                    {
                        editor.select_only_word(next);
                        editor.dirty = true;
                        session.notice = Some("Joined the following lyric phrase.".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice = Some("There is no following phrase to join.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
        }
    }
}

fn update_button_visuals(
    mut commands: Commands,
    theme: Res<StudioTheme>,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &UiAction,
            &mut BackgroundColor,
            Option<&RestingButtonBackground>,
        ),
        Or<(Added<Button>, Changed<Interaction>)>,
    >,
) {
    for (entity, interaction, action, mut background, resting) in &mut buttons {
        let has_recorded_background = resting.is_some();
        let resting = resting.map_or(background.0, |resting| resting.0);
        if !has_recorded_background {
            commands
                .entity(entity)
                .insert(RestingButtonBackground(resting));
        }
        background.0 = button_background(action, *interaction, resting, &theme);
    }
}

fn button_background(
    action: &UiAction,
    interaction: Interaction,
    resting: Color,
    theme: &StudioTheme,
) -> Color {
    // Full-surface dismiss targets are intentionally invisible controls. A
    // hover highlight here reads as if the obscured page itself was selected.
    if matches!(action, UiAction::CloseActivity) {
        return resting;
    }
    match interaction {
        Interaction::None => resting,
        Interaction::Hovered if resting == Color::NONE => theme.sidebar_accent.with_alpha(0.48),
        Interaction::Pressed if resting == Color::NONE => theme.sidebar_accent.with_alpha(0.72),
        Interaction::Hovered => resting.mix(&theme.foreground, 0.06),
        Interaction::Pressed => resting.mix(&theme.foreground, 0.12),
    }
}

fn save_config_error(config: &AppConfig) -> Option<String> {
    config
        .save()
        .err()
        .map(|error| format!("Could not save settings: {error}"))
}

fn sync_numeric_settings(
    mut inputs: Query<(&mut EditableText, &NumericSetting), Changed<EditableText>>,
    mut session: ResMut<StudioSession>,
) {
    for (mut input, setting) in &mut inputs {
        let raw = input.value().to_string();
        let Ok(parsed) = raw.trim().parse::<u32>() else {
            continue;
        };
        let (minimum, maximum) = match setting {
            NumericSetting::BeamSize | NumericSetting::BatchSize => (1, 16),
            NumericSetting::VocalThreshold => (0, 60),
            NumericSetting::SeparatorSegmentSize => (64, 1024),
            NumericSetting::SeparatorOverlap => (2, 32),
            NumericSetting::SeparatorBatchSize | NumericSetting::DemucsShifts => (1, 8),
            NumericSetting::SeparatorNormalization => (1, 100),
            NumericSetting::DemucsOverlap => (1, 95),
        };
        let clamped = parsed.clamp(minimum, maximum);
        if clamped != parsed {
            input.editor_mut().set_text(&clamped.to_string());
        }
        let current = match setting {
            NumericSetting::BeamSize => session.config.beam_size(),
            NumericSetting::BatchSize => session.config.batch_size(),
            NumericSetting::VocalThreshold => {
                (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32
            }
            NumericSetting::SeparatorSegmentSize => session.config.separator_segment_size(),
            NumericSetting::SeparatorOverlap => session.config.separator_overlap(),
            NumericSetting::SeparatorBatchSize => session.config.separator_batch_size(),
            NumericSetting::SeparatorNormalization => {
                session.config.separator_normalization_pct()
            }
            NumericSetting::DemucsShifts => session.config.demucs_shifts(),
            NumericSetting::DemucsOverlap => session.config.demucs_overlap_pct(),
        };
        if clamped == current {
            continue;
        }
        match setting {
            NumericSetting::BeamSize => session.config.beam_size = Some(clamped),
            NumericSetting::BatchSize => session.config.batch_size = Some(clamped),
            NumericSetting::VocalThreshold => {
                session.config.vocal_detection_threshold_pct = Some(f64::from(clamped) / 100.0)
            }
            NumericSetting::SeparatorSegmentSize => {
                session.config.separator_segment_size = Some(clamped)
            }
            NumericSetting::SeparatorOverlap => session.config.separator_overlap = Some(clamped),
            NumericSetting::SeparatorBatchSize => {
                session.config.separator_batch_size = Some(clamped)
            }
            NumericSetting::SeparatorNormalization => {
                session.config.separator_normalization_pct = Some(clamped)
            }
            NumericSetting::DemucsShifts => session.config.demucs_shifts = Some(clamped),
            NumericSetting::DemucsOverlap => session.config.demucs_overlap_pct = Some(clamped),
        }
        if let Some(error) = save_config_error(&session.config) {
            session.notice = Some(error);
        }
    }
}

fn sync_editor_word_input(
    inputs: Query<(&EditableText, &EditorWordInput), Changed<EditableText>>,
    mut session: ResMut<StudioSession>,
) {
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    for (input, marker) in &inputs {
        let text = input.value().to_string();
        let current = selected_editor_word(&editor.chart, marker.0)
            .map(|(text, _, _)| text)
            .unwrap_or_default();
        if text == current {
            continue;
        }
        editor.checkpoint();
        if update_editor_word_text(&mut editor.chart.transcript, marker.0, &text) {
            editor.dirty = true;
        } else {
            editor.undo.pop();
        }
    }
}

fn finish_inline_lyric_edit(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    inline_inputs: Query<(), With<InlineEditorWordInput>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    if !inline_inputs.contains(entity) {
        return;
    }
    focus.clear();
    if let Some(editor) = session.editor.as_mut() {
        editor.word_edit_focus = None;
    }
    invalidated.0 = true;
}

fn handle_library_search_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<&EditableText, With<LibrarySearchInput>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let command = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    if command && keys.just_pressed(KeyCode::KeyK) && session.route != StudioRoute::Editor {
        session.search_open = true;
        session.activity_open = false;
        session.about_open = false;
        invalidated.0 = true;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && session.search_open {
        session.search_open = false;
        invalidated.0 = true;
        return;
    }
    if !session.search_open || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    let Ok(input) = inputs.get(entity) else {
        return;
    };
    let value = input.value().to_string();
    let value = value.trim();
    session.library_search = (!value.is_empty()).then(|| value.to_string());
    session.route = StudioRoute::Library;
    session.library_view = LibraryView::All;
    session.library_facet = None;
    session.search_open = false;
    session.refresh_library();
    invalidated.0 = true;
}

fn handle_fullscreen_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if let Some(error) = toggle_fullscreen(&mut window, &mut session.config) {
        session.notice = Some(error);
    }
    invalidated.0 = true;
}

fn toggle_fullscreen(window: &mut Window, config: &mut AppConfig) -> Option<String> {
    let fullscreen = matches!(window.mode, WindowMode::Windowed);
    window.mode = if fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    config.fullscreen = Some(fullscreen);
    save_config_error(config)
}

fn start_cache_stats_job(cache_stats: &mut CacheStatsJob) {
    if cache_stats.receiver.is_some() {
        return;
    }
    cache_stats.error = None;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(app_core::CacheStats::calculate());
    });
    cache_stats.receiver = Some(Mutex::new(receiver));
}

fn handle_cache_stats_request(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut session: ResMut<StudioSession>,
) {
    if !session.request_cache_stats_refresh {
        return;
    }
    session.request_cache_stats_refresh = false;
    if cache_stats.current.is_none() && cache_stats.receiver.is_none() {
        start_cache_stats_job(&mut cache_stats);
    }
}

fn poll_cache_stats(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = cache_stats
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(stats) => Some(Ok(stats)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Cache stats worker exited unexpectedly.".to_string()))
                }
            },
            Err(_) => Some(Err("Cache stats status channel was poisoned.".to_string())),
        });
    let Some(result) = result else {
        return;
    };
    cache_stats.receiver = None;
    match result {
        Ok(stats) => {
            cache_stats.current = Some(stats);
            cache_stats.error = None;
        }
        Err(error) => cache_stats.error = Some(error),
    }
    invalidated.0 = true;
}

fn start_native_setup(config: &AppConfig, request: SetupRequest, setup: &mut NativeSetup) {
    let (sender, receiver) = mpsc::channel();
    let folders = setup_folders(config, request);
    std::thread::spawn(move || {
        let progress_sender = sender.clone();
        let log_sender = sender.clone();
        let relocation_sender = sender.clone();
        let result = app_core::run_vendor_setup(
            folders,
            move |progress| {
                let _ = progress_sender.send(SetupEvent::Progress(progress));
            },
            move |line| {
                let _ = log_sender.send(SetupEvent::Log(line));
            },
            move |path| {
                relocation_sender
                    .send(SetupEvent::Log(format!(
                        "Application data relocated to {}",
                        path.display()
                    )))
                    .map_err(|error| error.to_string())
            },
        );
        let _ = sender.send(SetupEvent::Complete(result));
    });
    setup.receiver = Some(Mutex::new(receiver));
    setup.progress = None;
    setup.logs.clear();
}

fn setup_folders(config: &AppConfig, request: SetupRequest) -> app_core::SetupFolders {
    app_core::SetupFolders {
        data_path: None,
        cache_paths: config.cache_paths.clone(),
        compute_backend: match config.compute_backend.as_deref() {
            Some("cuda") => app_core::ComputeBackend::Cuda,
            Some("intel") => app_core::ComputeBackend::Intel,
            _ => app_core::ComputeBackend::Cpu,
        },
        model_target: request.target,
    }
}

fn poll_native_setup(
    mut setup: ResMut<NativeSetup>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let mut events = Vec::new();
    let mut channel_poisoned = false;
    {
        let Some(receiver) = setup.receiver.as_ref() else {
            return;
        };
        match receiver.lock() {
            Ok(receiver) => loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        events.push(SetupEvent::Complete(Err(
                            "Analysis setup worker exited unexpectedly.".to_string(),
                        )));
                        break;
                    }
                }
            },
            Err(_) => channel_poisoned = true,
        }
    }
    if channel_poisoned {
        setup.receiver = None;
        setup.progress = None;
        session.notice = Some("Analysis setup status channel was poisoned.".to_string());
        invalidated.0 = true;
        return;
    }
    for event in events {
        match event {
            SetupEvent::Progress(progress) => {
                session.notice = Some(format!("{} · {}%", progress.action, progress.percent));
                setup.progress = Some(progress);
                invalidated.0 = true;
            }
            SetupEvent::Log(line) => {
                setup.logs.push(line);
                if setup.logs.len() > 200 {
                    let excess = setup.logs.len() - 200;
                    setup.logs.drain(..excess);
                }
                invalidated.0 = true;
            }
            SetupEvent::Complete(result) => {
                setup.receiver = None;
                setup.progress = None;
                session.config = AppConfig::load();
                session.notice = Some(match result {
                    Ok(()) => "Analysis runtime setup completed.".to_string(),
                    Err(error) => format!("Analysis runtime setup failed: {error}"),
                });
                invalidated.0 = true;
            }
        }
    }
}

fn poll_native_diagnostics(
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = diagnostics
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(report) => Some(Ok(report)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Feature diagnostics worker exited unexpectedly.".to_string(),
                )),
            },
            Err(_) => Some(Err(
                "Feature diagnostics status channel was poisoned.".to_string()
            )),
        });
    let Some(result) = result else {
        return;
    };
    diagnostics.receiver = None;
    match result {
        Ok(report) => {
            session.notice = Some(format!(
                "Diagnostics {}: {} passed, {} failed, {} skipped.",
                if report.ok { "passed" } else { "completed" },
                report.passed,
                report.failed,
                report.skipped,
            ));
            session.diagnostic_report = Some(report);
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

fn start_key_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let Some(original_key) = song.key.as_deref() else {
        return "Analyze the song again to detect its original key.".to_string();
    };
    let offset = (song.key_offset + i32::from(delta)).clamp(-5, 5);
    if offset == song.key_offset {
        return "Key shift is limited to five semitones in either direction.".to_string();
    }
    let (key, pitch_ratio) = calculate_key_shift(original_key, offset);
    let notice_key = key.clone();
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_key(&file_hash, &key, pitch_ratio, offset)
            .map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "key",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering key variant {notice_key}…")
}

fn start_tempo_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let tempo = ((song.tempo + f64::from(delta) * 0.1) * 10.0).round() / 10.0;
    let tempo = tempo.clamp(0.5, 2.0);
    if (tempo - song.tempo).abs() < f64::EPSILON {
        return "Tempo is limited to 0.5×–2.0×.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_tempo(&file_hash, tempo).map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "tempo",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering {tempo:.1}× tempo variant…")
}

fn poll_authoring_job(
    mut job: ResMut<NativeAuthoringJob>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(event) => Some(Ok(event)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Key/tempo render worker exited unexpectedly.".to_string(),
                )),
            })
    });
    let Some(result) = result else {
        return;
    };
    job.receiver = None;
    session.authoring_busy = false;
    match result {
        Ok(event) => match event.result {
            Ok(rendered) => {
                session.notice = Some(format!(
                    "Song {} shifted successfully · key {} · {:.1}× tempo.",
                    event.kind, rendered.key, rendered.tempo
                ));
                session.refresh_library();
            }
            Err(error) => {
                session.notice = Some(format!("Could not render {} variant: {error}", event.kind))
            }
        },
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

fn start_export_job(
    file_hash: &str,
    extension: &'static str,
    export_directory: Option<PathBuf>,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_song(&file_hash, extension, export_directory.as_deref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Choose where to save the {} export…",
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

fn start_export_all_job(
    extension: &'static str,
    export_directory: PathBuf,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    if !export_directory.is_dir() {
        return format!(
            "The export folder is unavailable: {}. Choose it again in Settings > Storage.",
            export_directory.display()
        );
    }

    let songs = SongsStore::load_all()
        .processed
        .into_iter()
        .filter(|song| song.authoring_ready)
        .collect::<Vec<_>>();
    if songs.is_empty() {
        return "No chart is ready to export. Analyze or import a chart first.".to_string();
    }
    let total = songs.len();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_all_songs(&songs, extension, &export_directory);
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Exporting {total} ready chart{} as {}…",
        if total == 1 { "" } else { "s" },
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

fn poll_export_job(mut session: ResMut<StudioSession>, mut invalidated: ResMut<UiInvalidated>) {
    let result = session.export_job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some("Export worker exited unexpectedly.".to_string())
                }
            })
    });
    let Some(result) = result else {
        return;
    };
    session.export_job.receiver = None;
    session.notice = Some(result);
    invalidated.0 = true;
}

fn start_editor_load_job(
    file_hash: &str,
    audio: Arc<uta_studio_audio::EditorAudioPlayer>,
    job: &mut NativeEditorLoadJob,
) -> String {
    if job.receiver.is_some() {
        return "The chart editor is already loading.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = load_native_editor(&file_hash, audio.as_ref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    "Loading chart, audio, and waveform…".to_string()
}

fn poll_editor_load_job(
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = session
        .editor_load_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
            receiver
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("Chart editor loader exited unexpectedly.".to_string()))
                    }
                })
        });
    let Some(result) = result else {
        return;
    };
    session.editor_load_job.receiver = None;
    match result {
        Ok(editor) => {
            bevy::log::info!("Switching the native UI to the loaded chart editor");
            let audio_notice = editor.audio_status.error.as_ref().map(|error| {
                format!("Chart editing is available, but native audio is unavailable: {error}")
            });
            session.editor = Some(editor);
            session.route = StudioRoute::Editor;
            session.notice = audio_notice;
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

fn poll_lyrics_search_job(
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = session
        .lyrics_search_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
            receiver
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(candidates) => Some(Ok(candidates)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("LRCLIB search worker exited unexpectedly.".to_string()))
                    }
                })
        });
    let Some(result) = result else {
        return;
    };
    session.lyrics_search_job.receiver = None;
    match result {
        Ok(candidates) => {
            let count = candidates.len();
            if let Some(editor) = session.lyrics_editor.as_mut() {
                editor.searching = false;
                editor.candidates = candidates;
                editor.candidate_index = 0;
                session.notice = Some(if count == 0 {
                    "LRCLIB did not return a matching lyric.".to_string()
                } else {
                    format!("Found {count} LRCLIB lyric candidate(s). Review before applying.")
                });
            }
        }
        Err(error) => {
            if let Some(editor) = session.lyrics_editor.as_mut() {
                editor.searching = false;
            }
            session.notice = Some(error);
        }
    }
    invalidated.0 = true;
}

fn calculate_key_shift(original_key: &str, offset: i32) -> (String, f64) {
    const NOTES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let (note, quality) = original_key
        .strip_suffix('m')
        .map(|note| (note, "m"))
        .unwrap_or((original_key, ""));
    let key = NOTES
        .iter()
        .position(|candidate| *candidate == note)
        .map(|index| {
            let shifted = (index as i32 + offset).rem_euclid(NOTES.len() as i32) as usize;
            format!("{}{quality}", NOTES[shifted])
        })
        .unwrap_or_else(|| original_key.to_string());
    (key, 2f64.powf(f64::from(offset) / 12.0))
}

fn run_analysis_action(file_hash: &str, action: impl FnOnce()) -> String {
    if !app_core::analysis_runtime_status().ready {
        return "Analysis is disabled until setup is completed in Settings > Models & runtime."
            .to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    if matches!(
        song.transcript_source,
        Some(app_core::TranscriptSource::Usdx)
    ) {
        return "This action is unavailable for imported USDX charts.".to_string();
    }
    action();
    format!("Queued analysis for “{}”.", song.title)
}

fn load_native_editor(
    file_hash: &str,
    audio: &uta_studio_audio::EditorAudioPlayer,
) -> Result<NativeEditor, String> {
    bevy::log::info!("Loading chart for the native editor");
    let chart = app_core::load_chart(file_hash).map_err(|error| error.to_string())?;
    bevy::log::info!("Decoding the bounded editor waveform while playback is stopped");
    let waveform = app_core::decode_chart_waveform(std::path::Path::new(&chart.audio.instrumental))
        .unwrap_or_default();
    bevy::log::info!("Preparing native editor audio");
    // Authoring does not depend on playback initialization. Keep the native
    // audio error on the editor status so transport can explain the problem,
    // while still allowing the chart and decoded waveform to be edited.
    let status =
        editor_audio_status(audio.load_path(std::path::Path::new(&chart.audio.instrumental)));
    bevy::log::info!("Native editor is ready");
    Ok(NativeEditor::new(chart, status, waveform, "instrumental"))
}

fn editor_audio_status(
    result: Result<uta_studio_audio::EditorAudioStatus, String>,
) -> uta_studio_audio::EditorAudioStatus {
    result.unwrap_or_else(|error| uta_studio_audio::EditorAudioStatus {
        error: Some(error),
        ..default()
    })
}

fn toggle_editor_playback(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: Option<&mut NativeEditor>,
) -> Result<(), String> {
    let editor = editor.ok_or_else(|| "No chart is open".to_string())?;
    let status = if editor.audio_status.playing {
        audio.pause()?
    } else {
        audio.play()?
    };
    editor.visible_position = status.position_secs;
    editor.audio_status = status;
    editor.last_audio_sync = Instant::now();
    Ok(())
}

fn library_visible_position(playback: &LibraryPlayback) -> f64 {
    if playback.status.playing {
        (playback.status.position_secs + playback.last_audio_sync.elapsed().as_secs_f64()).min(
            playback
                .status
                .duration_secs
                .max(playback.status.position_secs),
        )
    } else {
        playback.visible_position
    }
}

fn play_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    file_hash: &str,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    let song = app_core::load_song_by_hash(file_hash)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Song not found: {file_hash}"))?;
    if !song.path.is_file() {
        return Err(format!(
            "Source audio is unavailable: {}",
            song.path.display()
        ));
    }
    audio.load_path(&song.path)?;
    audio.set_volume(playback.volume)?;
    let status = audio.play()?;
    if let Some(error) = status.error.as_ref() {
        return Err(format!("Could not play the original source: {error}"));
    }
    playback.file_hash = Some(file_hash.to_string());
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

fn prepare_library_queue(songs: &[Song], file_hash: &str, playback: &mut LibraryPlayback) {
    playback.queue = songs
        .iter()
        .filter(|song| song.path.is_file())
        .map(|song| song.file_hash.clone())
        .collect();
    if !playback.queue.iter().any(|hash| hash == file_hash) {
        playback.queue.push(file_hash.to_string());
    }
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
}

fn advance_library_queue(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    direction: i8,
    wrap: bool,
) -> Result<(), String> {
    if playback.queue.is_empty() {
        return Err("The playback queue is empty.".to_string());
    }
    let current = playback
        .queue_index
        .or_else(|| {
            playback
                .file_hash
                .as_ref()
                .and_then(|hash| playback.queue.iter().position(|item| item == hash))
        })
        .unwrap_or(0);
    let len = playback.queue.len();
    let next = if playback.shuffle && len > 1 && direction > 0 {
        playback.shuffle_seed = playback
            .shuffle_seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let candidate = (playback.shuffle_seed as usize) % len;
        if candidate == current {
            (candidate + 1) % len
        } else {
            candidate
        }
    } else if direction < 0 {
        if current > 0 {
            current - 1
        } else if wrap {
            len - 1
        } else {
            return Err("This is the start of the queue.".to_string());
        }
    } else if current + 1 < len {
        current + 1
    } else if wrap {
        0
    } else {
        return Err("This is the end of the queue.".to_string());
    };
    let file_hash = playback.queue[next].clone();
    playback.queue_index = Some(next);
    play_library_song(audio, &file_hash, playback)
}

fn restart_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    audio.seek(0.0)?;
    let status = audio.play()?;
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

fn set_library_volume(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    volume: f64,
) -> Result<(), String> {
    playback.volume = volume.clamp(0.0, 1.0);
    if playback.volume > 0.0 {
        playback.volume_before_mute = playback.volume;
    }
    if playback.status.loaded {
        let status = audio.set_volume(playback.volume)?;
        playback.visible_position = status.position_secs;
        playback.status = status;
        playback.last_audio_sync = Instant::now();
    }
    Ok(())
}

fn toggle_library_playback(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before starting playback.".to_string());
    }
    let status = if playback.status.playing {
        audio.pause()?
    } else {
        if playback.status.ended {
            audio.seek(0.0)?;
        }
        audio.play()?
    };
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

fn seek_library_relative(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    delta_secs: f64,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before seeking.".to_string());
    }
    let was_playing = playback.status.playing;
    let target = (library_visible_position(playback) + delta_secs)
        .clamp(0.0, playback.status.duration_secs.max(0.0));
    let mut status = audio.seek(target)?;
    if was_playing {
        status = audio.play()?;
    }
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

fn select_editor_audio_source(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    source: &str,
) -> Result<(), String> {
    if !matches!(source, "vocals" | "instrumental" | "original") {
        return Err("That audition source is not supported.".to_string());
    }
    if source == "vocals" && editor.chart.audio.vocals.is_none() {
        return Err("This chart has no separate vocal source.".to_string());
    }
    let was_playing = editor.audio_status.playing;
    let mut status = audio.load(&editor.chart.file_hash, source)?;
    let path = match source {
        "vocals" => editor.chart.audio.vocals.as_deref(),
        "original" => Some(editor.chart.audio.original.as_str()),
        _ => Some(editor.chart.audio.instrumental.as_str()),
    };
    editor.waveform = path
        .and_then(|path| app_core::decode_chart_waveform(std::path::Path::new(path)).ok())
        .unwrap_or_default();
    if was_playing {
        status = audio.play()?;
    }
    editor.audio_source = source.to_string();
    editor.audio_status = status;
    editor.visible_position = 0.0;
    editor.last_audio_sync = Instant::now();
    Ok(())
}

fn handle_editor_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    editable: Query<(), With<EditableText>>,
    audio: Res<NativeAudio>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        return;
    }
    if focus.get().is_some_and(|entity| editable.contains(entity)) {
        return;
    }
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if control && keys.just_pressed(KeyCode::KeyZ) && !shift {
        if let Some(editor) = session.editor.as_mut()
            && editor.undo()
        {
            session.notice = Some("Undid chart edit.".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if control && (keys.just_pressed(KeyCode::KeyY) || (shift && keys.just_pressed(KeyCode::KeyZ)))
    {
        if let Some(editor) = session.editor.as_mut()
            && editor.redo()
        {
            session.notice = Some("Redid chart edit.".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyA) {
        if let Some(editor) = session.editor.as_mut() {
            if editor.selected_word.is_some() {
                let words = all_editor_word_selections(&editor.chart.transcript);
                editor.selected_word = words.iter().next().copied();
                editor.selected_words = words;
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.word_edit_focus = None;
                session.notice = Some(format!(
                    "Selected {} lyric word(s).",
                    editor.selected_words.len()
                ));
                invalidated.0 = true;
                return;
            }
            let count = chart_notes(&editor.chart).len();
            editor.selected_notes = (0..count).collect();
            editor.selected_note = (count > 0).then_some(0);
            editor.selected_word = None;
            editor.selected_words.clear();
            editor.word_edit_focus = None;
            session.notice = Some(format!("Selected {count} note(s)."));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        if let Some(editor) = session.editor.as_mut() {
            session.notice = match app_core::save_chart(
                &editor.chart.file_hash,
                editor.chart.transcript.clone(),
                editor.chart.pitch_notes.clone(),
            ) {
                Ok(()) => {
                    editor.dirty = false;
                    Some("Chart saved atomically.".to_string())
                }
                Err(error) => Some(format!("Could not save chart: {error}")),
            };
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyC) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            editor.clipboard_notes = copy_chart_notes(&editor.chart.pitch_notes, &selected);
            session.notice = Some(format!("Copied {} note(s).", editor.clipboard_notes.len()));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyX) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                return;
            }
            editor.clipboard_notes = copy_chart_notes(&editor.chart.pitch_notes, &selected);
            editor.checkpoint();
            let removed = remove_chart_notes(&mut editor.chart.pitch_notes, &selected);
            editor.selected_note = None;
            editor.selected_notes.clear();
            editor.dirty |= removed > 0;
            session.notice = Some(format!("Cut {removed} note(s)."));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyV) {
        if let Some(editor) = session.editor.as_mut()
            && !editor.clipboard_notes.is_empty()
        {
            editor.checkpoint();
            let inserted = paste_chart_notes(
                &mut editor.chart.pitch_notes,
                &editor.clipboard_notes,
                editor.visible_position,
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            session.notice = Some("Pasted note(s) at the playhead.".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            let clipboard = copy_chart_notes(&editor.chart.pitch_notes, &selected);
            if clipboard.is_empty() {
                return;
            }
            let selected_end = selected
                .iter()
                .filter_map(|index| chart_notes(&editor.chart).get(*index).map(|note| note.end))
                .reduce(f64::max)
                .unwrap_or(editor.visible_position);
            editor.checkpoint();
            let inserted = paste_chart_notes(
                &mut editor.chart.pitch_notes,
                &clipboard,
                selected_end + editor.snap_seconds.max(0.02),
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            session.notice = Some("Duplicated selected note(s).".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                let words = editor.selected_word_indices();
                if !words.is_empty() {
                    editor.checkpoint();
                    let deleted = delete_editor_words(&mut editor.chart.transcript, &words);
                    if deleted > 0 {
                        editor.selected_word = None;
                        editor.selected_words.clear();
                        editor.word_edit_focus = None;
                        editor.dirty = true;
                        session.notice = Some(format!("Deleted {deleted} lyric word(s)."));
                        invalidated.0 = true;
                    } else {
                        editor.undo.pop();
                    }
                }
                return;
            }
            editor.checkpoint();
            let removed = remove_chart_notes(&mut editor.chart.pitch_notes, &selected);
            if removed > 0 {
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.dirty = true;
                session.notice = Some(format!("Deleted {removed} note(s)."));
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyS) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if !selected.is_empty() {
                editor.checkpoint();
                let next = split_chart_notes(
                    &mut editor.chart.pitch_notes,
                    &selected,
                    editor.visible_position,
                );
                editor.selected_note = next.iter().next().copied();
                editor.selected_notes = next;
                editor.dirty = true;
                session.notice = Some("Split selected note(s).".to_string());
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.len() > 1 {
                editor.checkpoint();
                if let Some(index) = merge_chart_notes(
                    &mut editor.chart.pitch_notes,
                    &selected,
                    editor.selected_note,
                ) {
                    editor.select_only_note(index);
                    editor.dirty = true;
                    session.notice = Some("Merged selected notes.".to_string());
                    invalidated.0 = true;
                }
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if !selected.is_empty() && editor.snap_seconds > 0.0 {
                editor.checkpoint();
                quantize_chart_notes(
                    &mut editor.chart.pitch_notes,
                    Some(&selected),
                    editor.snap_seconds,
                );
                editor.dirty = true;
                session.notice = Some("Quantized selected note(s).".to_string());
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        if let Some(editor) = session.editor.as_mut() {
            let count = chart_notes(&editor.chart).len();
            if count > 0 {
                let next = editor.selected_note.map_or(0, |index| {
                    if shift {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    }
                });
                editor.select_only_note(next);
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if let Some(editor) = session.editor.as_mut()
            && editor.inspector_open
        {
            editor.inspector_open = false;
            invalidated.0 = true;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        session.notice = toggle_editor_playback(&audio.0, session.editor.as_mut()).err();
        invalidated.0 = true;
        return;
    }
    let left = keys.just_pressed(KeyCode::ArrowLeft);
    let right = keys.just_pressed(KeyCode::ArrowRight);
    let up = keys.just_pressed(KeyCode::ArrowUp);
    let down = keys.just_pressed(KeyCode::ArrowDown);
    let delta = if left {
        Some(-2.0)
    } else if right {
        Some(2.0)
    } else {
        None
    };
    if let Some(editor) = session.editor.as_mut() {
        let selected = editor.selected_note_indices();
        if selected.is_empty() && (left || right) && !editor.selected_word_indices().is_empty() {
            let time_step = if editor.snap_seconds > 0.0 {
                editor.snap_seconds
            } else {
                0.01
            };
            editor.checkpoint();
            let words = editor.selected_word_indices();
            let moved = words
                .iter()
                .filter(|selection| {
                    shift_editor_word(
                        &mut editor.chart.transcript,
                        **selection,
                        if left { -time_step } else { time_step },
                    )
                })
                .count();
            if moved > 0 {
                editor.dirty = true;
                session.notice = Some(format!(
                    "Moved {moved} lyric word(s) {} by {}.",
                    if left { "earlier" } else { "later" },
                    format_snap_grid(time_step)
                ));
                invalidated.0 = true;
            } else {
                editor.undo.pop();
            }
            return;
        }
        if !selected.is_empty() && (left || right || up || down) {
            editor.checkpoint();
            let time_step = if editor.snap_seconds > 0.0 {
                editor.snap_seconds
            } else {
                0.01
            };
            let seconds = if left {
                -time_step
            } else if right {
                time_step
            } else {
                0.0
            };
            let semitones = if up {
                if shift { 12.0 } else { 1.0 }
            } else if down {
                if shift { -12.0 } else { -1.0 }
            } else {
                0.0
            };
            shift_chart_notes(
                &mut editor.chart.pitch_notes,
                &selected,
                seconds,
                semitones,
                shift && (left || right),
            );
            editor.dirty = true;
            session.notice = None;
            invalidated.0 = true;
            return;
        }
    }
    let Some(delta) = delta else {
        return;
    };
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    let was_playing = editor.audio_status.playing;
    let target = (editor.visible_position + delta).max(0.0);
    match audio.0.seek(target) {
        Ok(mut status) => {
            if was_playing && let Ok(playing) = audio.0.play() {
                status = playing;
            }
            editor.visible_position = status.position_secs;
            editor.audio_status = status;
            editor.last_audio_sync = Instant::now();
            session.notice = None;
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

fn handle_editor_wheel(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        wheel.clear();
        return;
    }
    let delta = wheel.read().map(|event| event.y + event.x).sum::<f32>();
    if delta.abs() < f32::EPSILON {
        return;
    }
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if control {
        let center = editor.viewport_start + editor.viewport_duration / 2.0;
        editor.viewport_duration =
            (editor.viewport_duration * (1.0 - f64::from(delta) * 0.08)).clamp(2.0, 180.0);
        editor.viewport_start = (center - editor.viewport_duration / 2.0).max(0.0);
    } else if alt {
        let span = ((editor.pitch_max - editor.pitch_min) * (1.0 - f64::from(delta) * 0.08))
            .clamp(8.0, 127.0);
        set_editor_pitch_span(editor, span);
    } else if shift {
        let span = editor.pitch_max - editor.pitch_min;
        let offset = f64::from(delta) * span * 0.05;
        editor.pitch_min = (editor.pitch_min + offset).clamp(0.0, 127.0 - span);
        editor.pitch_max = editor.pitch_min + span;
    } else {
        editor.viewport_start =
            (editor.viewport_start - f64::from(delta) * editor.viewport_duration * 0.08).max(0.0);
    }
    editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
    invalidated.0 = true;
}

// Pointer capture coordinates multiple independent ECS inputs in one frame.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_editor_pointer_capture(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    audio: Res<NativeAudio>,
    mut focus_events: MessageReader<bevy::window::WindowFocused>,
    resize_interactions: Query<(&Interaction, &EditorNoteResizeHandle), Changed<Interaction>>,
    lyric_resize_interactions: Query<
        (&Interaction, &EditorLyricResizeHandle),
        Changed<Interaction>,
    >,
    note_interactions: Query<
        (&Interaction, &EditorNoteNode),
        (Changed<Interaction>, Without<EditorTimelineSurface>),
    >,
    lyric_interactions: Query<
        (&Interaction, &EditorLyricNode),
        (Changed<Interaction>, Without<EditorTimelineSurface>),
    >,
    surface_interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<EditorTimelineSurface>,
            Without<EditorNoteNode>,
        ),
    >,
    timeline: Query<(&ComputedNode, &UiGlobalTransform), With<EditorTimelineSurface>>,
    lyrics_surface: Query<
        (&ComputedNode, &UiGlobalTransform),
        (With<EditorLyricsSurface>, Without<EditorTimelineSurface>),
    >,
    mut capture: ResMut<EditorPointerCapture>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let focus_lost = focus_events.read().any(|event| !event.focused);
    if session.route != StudioRoute::Editor || focus_lost || !mouse.pressed(MouseButton::Left) {
        let finished = capture.drag.take();
        let had_finished = finished.is_some();
        if mouse.just_released(MouseButton::Left)
            && let Some(EditorDrag::Pan { pointer_start, .. }) = finished
            && let Ok(window) = windows.single()
            && let Some(pointer) = window.cursor_position()
            && pointer.distance(pointer_start) <= 5.0
            && let Ok((computed, global_transform)) = timeline.single()
        {
            let size = computed.size() * computed.inverse_scale_factor();
            if size.x > 1.0
                && let Some(editor) = session.editor.as_mut()
            {
                let local = global_transform
                    .affine()
                    .inverse()
                    .transform_point2(pointer);
                let fraction = (local.x / size.x + 0.5).clamp(0.0, 1.0);
                let surface_y = (local.y / size.y + 0.5).clamp(0.0, 1.0);
                let target = editor.viewport_start + f64::from(fraction) * editor.viewport_duration;
                let pitch_surface = surface_y * 100.0 >= EDITOR_PITCH_TOP_PERCENT
                    && surface_y * 100.0 <= EDITOR_PITCH_TOP_PERCENT + EDITOR_PITCH_HEIGHT_PERCENT;
                let double_click = pitch_surface
                    && capture.last_surface_click.is_some_and(|(at, previous)| {
                        at.elapsed() <= Duration::from_millis(360)
                            && previous.distance(pointer) <= 7.0
                    });
                capture.last_surface_click = if double_click {
                    None
                } else {
                    Some((Instant::now(), pointer))
                };
                if double_click {
                    let start = if editor.snap_seconds > 0.0 {
                        (target / editor.snap_seconds).round() * editor.snap_seconds
                    } else {
                        target
                    }
                    .max(0.0);
                    let pitch_fraction = surface_pitch_fraction(surface_y);
                    let midi = (editor.pitch_max
                        - f64::from(pitch_fraction) * (editor.pitch_max - editor.pitch_min))
                        .round()
                        .clamp(0.0, 127.0);
                    editor.checkpoint();
                    if let Some(index) = insert_chart_note(
                        &mut editor.chart.pitch_notes,
                        serde_json::json!({
                            "start": start,
                            "end": start + editor.snap_seconds.max(0.25),
                            "midi": midi,
                            "confidence": 1.0,
                            "kind": "normal"
                        }),
                    ) {
                        editor.select_only_note(index);
                        editor.dirty = true;
                        session.notice = Some("Added note at the pointer.".to_string());
                    }
                    invalidated.0 = true;
                    return;
                }
                let was_playing = editor.audio_status.playing;
                match audio.0.seek(target) {
                    Ok(mut status) => {
                        if was_playing && let Ok(playing) = audio.0.play() {
                            status = playing;
                        }
                        editor.visible_position = status.position_secs;
                        editor.audio_status = status;
                        editor.last_audio_sync = Instant::now();
                        session.notice = None;
                    }
                    Err(error) => session.notice = Some(error),
                }
            }
        }
        if had_finished {
            invalidated.0 = true;
        }
        return;
    }

    let Ok(window) = windows.single() else {
        capture.drag = None;
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        // Keep the logical capture while the pointer is temporarily outside the
        // window. A global release or focus-loss event still clears it above.
        return;
    };
    let Some(editor) = session.editor.as_mut() else {
        capture.drag = None;
        return;
    };

    if capture.drag.is_none() && mouse.just_pressed(MouseButton::Left) {
        let pressed_resize = resize_interactions
            .iter()
            .find_map(|(interaction, handle)| {
                (*interaction == Interaction::Pressed).then_some((handle.index, handle.edge))
            });
        let pressed_lyric_resize =
            lyric_resize_interactions
                .iter()
                .find_map(|(interaction, handle)| {
                    (*interaction == Interaction::Pressed)
                        .then_some((handle.selection, handle.edge))
                });
        let pressed_note = note_interactions.iter().find_map(|(interaction, note)| {
            (*interaction == Interaction::Pressed).then_some(note.0)
        });
        let pressed_lyric = lyric_interactions.iter().find_map(|(interaction, lyric)| {
            (*interaction == Interaction::Pressed).then_some(lyric.selection)
        });
        if let Some((selection, edge)) = pressed_lyric_resize {
            if let Some((_, start, end)) = selected_editor_word(&editor.chart, selection) {
                editor.checkpoint();
                capture.drag = Some(EditorDrag::ResizeLyric {
                    selection,
                    edge,
                    pointer_start: pointer,
                    original_start: start,
                    original_end: end,
                    viewport_duration: editor.viewport_duration,
                });
                editor.select_only_word(selection);
                editor.inspector_open = true;
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if let Some(selection) = pressed_lyric {
            let modifier = keys.any_pressed([
                KeyCode::ShiftLeft,
                KeyCode::ShiftRight,
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
            ]);
            let double_click = !modifier
                && capture.last_lyric_click.is_some_and(|(at, previous)| {
                    at.elapsed() <= Duration::from_millis(360) && previous == selection
                });
            capture.last_lyric_click = if double_click {
                None
            } else {
                Some((Instant::now(), selection))
            };
            if double_click {
                capture.drag = None;
                editor.word_edit_focus = Some(selection);
            } else if !modifier {
                let selected = if editor.selected_words.contains(&selection) {
                    editor.selected_word_indices()
                } else {
                    [selection].into_iter().collect()
                };
                let originals = selected
                    .into_iter()
                    .filter_map(|selection| {
                        selected_editor_word(&editor.chart, selection).map(|(_, start, end)| {
                            EditorWordOriginal {
                                selection,
                                start,
                                end,
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                editor.checkpoint();
                capture.drag = Some(EditorDrag::Lyric {
                    pointer_start: pointer,
                    originals,
                    viewport_duration: editor.viewport_duration,
                });
            }
            editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
            invalidated.0 = true;
        } else if let Some((index, edge)) = pressed_resize {
            if let Some(note) = chart_notes(&editor.chart)
                .into_iter()
                .find(|note| note.index == index)
            {
                editor.checkpoint();
                capture.drag = Some(EditorDrag::ResizeNote {
                    index,
                    edge,
                    pointer_start: pointer,
                    original_start: note.start,
                    original_end: note.end,
                    viewport_duration: editor.viewport_duration,
                });
                editor.select_only_note(index);
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if let Some(index) = pressed_note {
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            if shift {
                if !editor.selected_notes.remove(&index) {
                    editor.selected_notes.insert(index);
                    editor.selected_note = Some(index);
                } else {
                    editor.selected_note = editor.selected_notes.iter().next().copied();
                }
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                invalidated.0 = true;
                return;
            }
            if !editor.selected_notes.contains(&index) {
                editor.select_only_note(index);
            }
            let selected = editor.selected_note_indices();
            let originals = chart_notes(&editor.chart)
                .into_iter()
                .filter(|note| selected.contains(&note.index))
                .map(|note| EditorNoteOriginal {
                    index: note.index,
                    start: note.start,
                    end: note.end,
                    midi: note.midi,
                })
                .collect::<Vec<_>>();
            if !originals.is_empty() {
                editor.checkpoint();
                capture.drag = Some(EditorDrag::Note {
                    pointer_start: pointer,
                    originals,
                    viewport_duration: editor.viewport_duration,
                    pitch_span: editor.pitch_max - editor.pitch_min,
                });
                editor.selected_note = Some(index);
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if surface_interactions
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
        {
            if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
                capture.drag = Some(EditorDrag::Marquee {
                    pointer_start: pointer,
                    base: editor.selected_note_indices(),
                    viewport_start: editor.viewport_start,
                    viewport_duration: editor.viewport_duration,
                    pitch_min: editor.pitch_min,
                    pitch_max: editor.pitch_max,
                });
            } else {
                capture.drag = Some(EditorDrag::Pan {
                    pointer_start: pointer,
                    viewport_start: editor.viewport_start,
                    pitch_min: editor.pitch_min,
                    pitch_max: editor.pitch_max,
                });
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
            }
            editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
        }
    }

    let Some(drag) = capture.drag.clone() else {
        return;
    };
    let surface = if matches!(
        drag,
        EditorDrag::Lyric { .. } | EditorDrag::ResizeLyric { .. }
    ) {
        lyrics_surface.single()
    } else {
        timeline.single()
    };
    let Ok((computed, global_transform)) = surface else {
        return;
    };
    let size = computed.size() * computed.inverse_scale_factor();
    if size.x <= 1.0 || size.y <= 1.0 {
        return;
    }
    let delta = pointer
        - match drag {
            EditorDrag::Note { pointer_start, .. }
            | EditorDrag::ResizeNote { pointer_start, .. }
            | EditorDrag::Lyric { pointer_start, .. }
            | EditorDrag::ResizeLyric { pointer_start, .. }
            | EditorDrag::Pan { pointer_start, .. }
            | EditorDrag::Marquee { pointer_start, .. } => pointer_start,
        };

    match drag {
        EditorDrag::Note {
            originals,
            viewport_duration,
            pitch_span,
            ..
        } => {
            let raw_time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let earliest = originals
                .iter()
                .map(|note| note.start)
                .reduce(f64::min)
                .unwrap_or(0.0);
            let time_delta = raw_time_delta.max(-earliest);
            let pitch_delta =
                -f64::from(delta.y / (size.y * (EDITOR_PITCH_HEIGHT_PERCENT / 100.0))) * pitch_span;
            let mut moved = 0usize;
            for original in &originals {
                let start = (original.start + time_delta).max(0.0);
                let end = start + (original.end - original.start).max(0.03);
                let midi = (original.midi + pitch_delta).round().clamp(0.0, 127.0);
                moved += usize::from(move_chart_note(
                    &mut editor.chart.pitch_notes,
                    original.index,
                    start,
                    end,
                    midi,
                ));
            }
            if moved > 0 {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::ResizeNote {
            index,
            edge,
            original_start,
            original_end,
            viewport_duration,
            ..
        } => {
            let time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let (start, end) = match edge {
                NoteEdge::Start => (
                    (original_start + time_delta).clamp(0.0, original_end - 0.02),
                    original_end,
                ),
                NoteEdge::End => (
                    original_start,
                    (original_end + time_delta).max(original_start + 0.02),
                ),
            };
            if resize_chart_note(&mut editor.chart.pitch_notes, index, start, end) {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::Lyric {
            originals,
            viewport_duration,
            ..
        } => {
            let raw_time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let earliest = originals
                .iter()
                .map(|word| word.start)
                .reduce(f64::min)
                .unwrap_or(0.0);
            let time_delta = raw_time_delta.max(-earliest);
            let moved = originals
                .iter()
                .filter(|word| {
                    set_editor_word_timing(
                        &mut editor.chart.transcript,
                        word.selection,
                        word.start + time_delta,
                        word.end + time_delta,
                    )
                })
                .count();
            if moved > 0 {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::ResizeLyric {
            selection,
            edge,
            original_start,
            original_end,
            viewport_duration,
            ..
        } => {
            let time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let (start, end) = match edge {
                NoteEdge::Start => (
                    (original_start + time_delta).clamp(0.0, original_end - 0.01),
                    original_end,
                ),
                NoteEdge::End => (
                    original_start,
                    (original_end + time_delta).max(original_start + 0.01),
                ),
            };
            if set_editor_word_timing(&mut editor.chart.transcript, selection, start, end) {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::Pan {
            viewport_start,
            pitch_min,
            pitch_max,
            ..
        } => {
            editor.viewport_start =
                (viewport_start - f64::from(delta.x / size.x) * editor.viewport_duration).max(0.0);
            let pitch_span = pitch_max - pitch_min;
            let pitch_offset =
                f64::from(delta.y / (size.y * (EDITOR_PITCH_HEIGHT_PERCENT / 100.0))) * pitch_span;
            editor.pitch_min = (pitch_min + pitch_offset).clamp(0.0, 127.0 - pitch_span);
            editor.pitch_max = editor.pitch_min + pitch_span;
        }
        EditorDrag::Marquee {
            pointer_start,
            base,
            viewport_start,
            viewport_duration,
            pitch_min,
            pitch_max,
        } => {
            let inverse = global_transform.affine().inverse();
            let start = inverse.transform_point2(pointer_start) / size;
            let current = inverse.transform_point2(pointer) / size;
            let left = start.x.min(current.x) + 0.5;
            let right = start.x.max(current.x) + 0.5;
            let top = surface_pitch_fraction(start.y.min(current.y) + 0.5);
            let bottom = surface_pitch_fraction(start.y.max(current.y) + 0.5);
            let time_start = viewport_start + f64::from(left) * viewport_duration;
            let time_end = viewport_start + f64::from(right) * viewport_duration;
            let pitch_span = pitch_max - pitch_min;
            let midi_max = pitch_max - f64::from(top) * pitch_span;
            let midi_min = pitch_max - f64::from(bottom) * pitch_span;
            let mut selected = base;
            for note in chart_notes(&editor.chart) {
                if note.end >= time_start
                    && note.start <= time_end
                    && note.midi >= midi_min
                    && note.midi <= midi_max
                {
                    selected.insert(note.index);
                }
            }
            editor.selected_note = selected.iter().next().copied();
            editor.selected_notes = selected;
            editor.selected_word = None;
            editor.selected_words.clear();
            editor.word_edit_focus = None;
        }
    }
    editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
}

fn surface_pitch_fraction(surface_fraction: f32) -> f32 {
    ((surface_fraction * 100.0 - EDITOR_PITCH_TOP_PERCENT) / EDITOR_PITCH_HEIGHT_PERCENT)
        .clamp(0.0, 1.0)
}

fn move_chart_note(
    pitch_notes: &mut serde_json::Value,
    index: usize,
    start: f64,
    end: f64,
    midi: f64,
) -> bool {
    let Some(note) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|notes| notes.get_mut(index))
    else {
        return false;
    };
    note["start"] = serde_json::Value::from(start);
    note["end"] = serde_json::Value::from(end);
    note["midi"] = serde_json::Value::from(midi);
    true
}

fn resize_chart_note(
    pitch_notes: &mut serde_json::Value,
    index: usize,
    start: f64,
    end: f64,
) -> bool {
    let Some(note) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|notes| notes.get_mut(index))
    else {
        return false;
    };
    note["start"] = serde_json::Value::from(start);
    note["end"] = serde_json::Value::from(end);
    true
}

fn insert_chart_note(
    pitch_notes: &mut serde_json::Value,
    note: serde_json::Value,
) -> Option<usize> {
    let notes = pitch_notes.get_mut("notes")?.as_array_mut()?;
    let start = note
        .get("start")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let index = notes.partition_point(|existing| {
        let existing = existing
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        existing <= start
    });
    notes.insert(index, note);
    Some(index)
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn copy_chart_notes(
    pitch_notes: &serde_json::Value,
    indices: &BTreeSet<usize>,
) -> Vec<serde_json::Value> {
    let Some(notes) = pitch_notes
        .get("notes")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    let mut copied = indices
        .iter()
        .filter_map(|index| notes.get(*index).cloned())
        .collect::<Vec<_>>();
    copied.sort_by(|left, right| {
        note_number(left, "start", 0.0).total_cmp(&note_number(right, "start", 0.0))
    });
    let origin = copied
        .first()
        .map(|note| note_number(note, "start", 0.0))
        .unwrap_or(0.0);
    for note in &mut copied {
        note["start"] =
            serde_json::Value::from(round_millis(note_number(note, "start", 0.0) - origin));
        note["end"] =
            serde_json::Value::from(round_millis(note_number(note, "end", 0.03) - origin));
    }
    copied
}

fn paste_chart_notes(
    pitch_notes: &mut serde_json::Value,
    clipboard: &[serde_json::Value],
    at: f64,
) -> BTreeSet<usize> {
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return BTreeSet::new();
    };
    let mut combined = notes
        .drain(..)
        .map(|note| (note, false))
        .collect::<Vec<_>>();
    combined.extend(clipboard.iter().cloned().map(|mut note| {
        let start = round_millis((at + note_number(&note, "start", 0.0)).max(0.0));
        let end = round_millis((at + note_number(&note, "end", 0.03)).max(start + 0.03));
        note["start"] = serde_json::Value::from(start);
        note["end"] = serde_json::Value::from(end);
        (note, true)
    }));
    combined.sort_by(|(left, _), (right, _)| {
        note_number(left, "start", 0.0)
            .total_cmp(&note_number(right, "start", 0.0))
            .then_with(|| {
                note_number(left, "end", 0.03).total_cmp(&note_number(right, "end", 0.03))
            })
    });
    let selected = combined
        .iter()
        .enumerate()
        .filter_map(|(index, (_, inserted))| inserted.then_some(index))
        .collect();
    *notes = combined.into_iter().map(|(note, _)| note).collect();
    selected
}

fn remove_chart_notes(pitch_notes: &mut serde_json::Value, indices: &BTreeSet<usize>) -> usize {
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let before = notes.len();
    let mut index = 0usize;
    notes.retain(|_| {
        let keep = !indices.contains(&index);
        index += 1;
        keep
    });
    before - notes.len()
}

fn split_chart_notes(
    pitch_notes: &mut serde_json::Value,
    indices: &BTreeSet<usize>,
    playhead: f64,
) -> BTreeSet<usize> {
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return BTreeSet::new();
    };
    let originals = std::mem::take(notes);
    let mut selected = BTreeSet::new();
    for (index, note) in originals.into_iter().enumerate() {
        let start = note_number(&note, "start", 0.0);
        let end = note_number(&note, "end", start + 0.03);
        if !indices.contains(&index) || end - start < 0.06 {
            notes.push(note);
            if indices.contains(&index) {
                selected.insert(notes.len() - 1);
            }
            continue;
        }
        let split = if playhead > start + 0.03 && playhead < end - 0.03 {
            playhead
        } else {
            (start + end) / 2.0
        };
        let mut left = note.clone();
        left["end"] = serde_json::Value::from(round_millis(split));
        notes.push(left);
        selected.insert(notes.len() - 1);
        let mut right = note;
        right["start"] = serde_json::Value::from(round_millis(split));
        notes.push(right);
        selected.insert(notes.len() - 1);
    }
    selected
}

fn merge_chart_notes(
    pitch_notes: &mut serde_json::Value,
    indices: &BTreeSet<usize>,
    primary: Option<usize>,
) -> Option<usize> {
    if indices.len() < 2 {
        return None;
    }
    let notes = pitch_notes.get_mut("notes")?.as_array_mut()?;
    let ordered = indices
        .iter()
        .copied()
        .filter(|index| *index < notes.len())
        .collect::<Vec<_>>();
    if ordered.len() < 2 {
        return None;
    }
    let first = ordered[0];
    let mut merged = notes[primary
        .filter(|index| indices.contains(index))
        .unwrap_or(first)]
    .clone();
    let start = ordered
        .iter()
        .map(|index| note_number(&notes[*index], "start", 0.0))
        .reduce(f64::min)?;
    let end = ordered
        .iter()
        .map(|index| note_number(&notes[*index], "end", start + 0.03))
        .reduce(f64::max)?;
    let total_duration = ordered
        .iter()
        .map(|index| {
            (note_number(&notes[*index], "end", 0.03) - note_number(&notes[*index], "start", 0.0))
                .max(0.0)
        })
        .sum::<f64>();
    if total_duration > 0.0 {
        let confidence = ordered
            .iter()
            .map(|index| {
                let duration = (note_number(&notes[*index], "end", 0.03)
                    - note_number(&notes[*index], "start", 0.0))
                .max(0.0);
                note_number(&notes[*index], "confidence", 1.0) * duration
            })
            .sum::<f64>()
            / total_duration;
        merged["confidence"] = serde_json::Value::from((confidence * 10_000.0).round() / 10_000.0);
    }
    merged["start"] = serde_json::Value::from(round_millis(start));
    merged["end"] = serde_json::Value::from(round_millis(end));
    let mut insertion = 0usize;
    let mut output = Vec::with_capacity(notes.len() - ordered.len() + 1);
    for (index, note) in std::mem::take(notes).into_iter().enumerate() {
        if index == first {
            insertion = output.len();
            output.push(merged.clone());
        }
        if !indices.contains(&index) {
            output.push(note);
        }
    }
    *notes = output;
    Some(insertion)
}

fn quantize_chart_notes(
    pitch_notes: &mut serde_json::Value,
    indices: Option<&BTreeSet<usize>>,
    grid: f64,
) -> usize {
    if grid <= 0.0 {
        return 0;
    }
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let mut changed = 0;
    for (index, note) in notes.iter_mut().enumerate() {
        if indices.is_some_and(|indices| !indices.contains(&index)) {
            continue;
        }
        let start = round_millis((note_number(note, "start", 0.0) / grid).round() * grid).max(0.0);
        let snapped_end =
            round_millis((note_number(note, "end", start + grid) / grid).round() * grid);
        note["start"] = serde_json::Value::from(start);
        note["end"] =
            serde_json::Value::from(round_millis(snapped_end.max(start + 0.03f64.max(grid))));
        changed += 1;
    }
    changed
}

fn shift_chart_notes(
    pitch_notes: &mut serde_json::Value,
    indices: &BTreeSet<usize>,
    seconds: f64,
    semitones: f64,
    resize_end: bool,
) -> usize {
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let earliest = indices
        .iter()
        .filter_map(|index| notes.get(*index))
        .map(|note| note_number(note, "start", 0.0))
        .reduce(f64::min)
        .unwrap_or(0.0);
    let safe_seconds = seconds.max(-earliest);
    let mut changed = 0;
    for index in indices {
        let Some(note) = notes.get_mut(*index) else {
            continue;
        };
        let start = note_number(note, "start", 0.0);
        let end = note_number(note, "end", start + 0.03);
        if resize_end {
            note["end"] = serde_json::Value::from(round_millis((end + seconds).max(start + 0.03)));
        } else {
            note["start"] = serde_json::Value::from(round_millis(start + safe_seconds));
            note["end"] = serde_json::Value::from(round_millis(end + safe_seconds));
            let midi = note_number(note, "midi", 60.0);
            note["midi"] = serde_json::Value::from((midi + semitones).round().clamp(0.0, 127.0));
        }
        changed += 1;
    }
    changed
}

fn cycle_chart_note_kinds(pitch_notes: &mut serde_json::Value, indices: &BTreeSet<usize>) -> usize {
    let Some(notes) = pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let next_kind = indices
        .iter()
        .find_map(|index| notes.get(*index))
        .and_then(|note| note.get("kind"))
        .and_then(serde_json::Value::as_str)
        .map(|current| {
            next_choice(
                current,
                &["normal", "golden", "freestyle", "rap", "golden_rap"],
            )
        })
        .unwrap_or_else(|| "golden".to_string());
    let mut changed = 0;
    for index in indices {
        if let Some(note) = notes.get_mut(*index) {
            note["kind"] = serde_json::Value::from(next_kind.clone());
            changed += 1;
        }
    }
    changed
}

fn note_number(note: &serde_json::Value, key: &str, fallback: f64) -> f64 {
    note.get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn compact_lyric_language(language: &str) -> bool {
    ["zh", "ja", "ko"]
        .iter()
        .any(|prefix| language.to_ascii_lowercase().starts_with(prefix))
}

fn rebuild_segment_text(segment: &mut serde_json::Value, compact: bool) {
    let Some(words) = segment.get("words").and_then(serde_json::Value::as_array) else {
        return;
    };
    let values = words
        .iter()
        .filter_map(|word| word.get("word").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .collect::<Vec<_>>();
    let mut text = values.join(if compact { "" } else { " " });
    if !compact {
        for punctuation in [",", ".", "!", "?", ";", ":"] {
            text = text.replace(&format!(" {punctuation}"), punctuation);
        }
    }
    segment["text"] = serde_json::Value::from(text);
}

fn update_editor_word_text(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
    text: &str,
) -> bool {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segment) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|segments| segments.get_mut(selection.segment))
    else {
        return false;
    };
    let Some(word) = segment
        .get_mut("words")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|words| words.get_mut(selection.word))
    else {
        return false;
    };
    if word.get("word").and_then(serde_json::Value::as_str) == Some(text) {
        return false;
    }
    word["word"] = serde_json::Value::from(text);
    rebuild_segment_text(segment, compact);
    true
}

fn insert_editor_word(
    transcript: &mut serde_json::Value,
    selection: Option<WordSelection>,
    playhead: f64,
) -> Option<WordSelection> {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let segments = transcript.get_mut("segments")?.as_array_mut()?;
    let playhead = round_millis(playhead.max(0.0));
    let selected_segment = selection
        .filter(|selection| selection.segment < segments.len())
        .map(|selection| selection.segment);
    let containing_segment = segments.iter().position(|segment| {
        let start = note_number(segment, "start", 0.0);
        let end = note_number(segment, "end", start);
        playhead >= start - 0.05 && playhead <= end + 0.05
    });

    if let Some(segment_index) = containing_segment.or(selected_segment) {
        let segment = segments.get_mut(segment_index)?;
        let words = segment.get_mut("words")?.as_array_mut()?;
        let mut start = playhead;
        if let Some(selection) = selection.filter(|selection| selection.segment == segment_index)
            && let Some(selected) = words.get(selection.word)
        {
            let selected_start = note_number(selected, "start", 0.0);
            let selected_end = note_number(selected, "end", selected_start + 0.02);
            if playhead >= selected_start - 0.01 && playhead <= selected_end + 0.01 {
                start = selected_end;
            }
        }
        start = round_millis(start.max(0.0));
        let index = words.partition_point(|word| note_number(word, "start", 0.0) <= start);
        let next_start = words
            .get(index)
            .map(|word| note_number(word, "start", f64::INFINITY))
            .unwrap_or(f64::INFINITY);
        let end = round_millis(if next_start > start + 0.02 {
            (start + 0.35).min(next_start)
        } else {
            start + 0.35
        });
        words.insert(
            index,
            serde_json::json!({"word": "New lyric", "start": start, "end": end}),
        );
        refresh_segment_lyrics(segment, compact);
        return Some(WordSelection {
            segment: segment_index,
            word: index,
        });
    }

    let end = round_millis(playhead + 0.35);
    let segment = serde_json::json!({
        "start": playhead,
        "end": end,
        "text": "New lyric",
        "words": [{"word": "New lyric", "start": playhead, "end": end}]
    });
    let segment_index =
        segments.partition_point(|segment| note_number(segment, "start", 0.0) <= playhead);
    segments.insert(segment_index, segment);
    Some(WordSelection {
        segment: segment_index,
        word: 0,
    })
}

fn delete_editor_word(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
) -> (bool, Option<WordSelection>) {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segments) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return (false, None);
    };
    let Some(segment) = segments.get_mut(selection.segment) else {
        return (false, None);
    };
    let Some(words) = segment
        .get_mut("words")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return (false, None);
    };
    if selection.word >= words.len() {
        return (false, None);
    }
    words.remove(selection.word);
    if !words.is_empty() {
        let next_word = selection.word.min(words.len() - 1);
        refresh_segment_lyrics(segment, compact);
        return (
            true,
            Some(WordSelection {
                segment: selection.segment,
                word: next_word,
            }),
        );
    }

    segments.remove(selection.segment);
    let next = segments
        .get(selection.segment)
        .and_then(|segment| segment.get("words"))
        .and_then(serde_json::Value::as_array)
        .filter(|words| !words.is_empty())
        .map(|_| WordSelection {
            segment: selection.segment,
            word: 0,
        })
        .or_else(|| {
            let segment = selection.segment.checked_sub(1)?;
            let words = segments
                .get(segment)?
                .get("words")?
                .as_array()
                .filter(|words| !words.is_empty())?;
            Some(WordSelection {
                segment,
                word: words.len() - 1,
            })
        });
    (true, next)
}

fn delete_editor_words(
    transcript: &mut serde_json::Value,
    selections: &BTreeSet<WordSelection>,
) -> usize {
    let mut ordered = selections.iter().copied().collect::<Vec<_>>();
    ordered.sort_by(|left, right| right.cmp(left));
    ordered
        .into_iter()
        .filter(|selection| delete_editor_word(transcript, *selection).0)
        .count()
}

fn merge_selected_editor_words(
    transcript: &mut serde_json::Value,
    selections: &BTreeSet<WordSelection>,
) -> Option<WordSelection> {
    if selections.len() < 2 {
        return None;
    }
    let segment_index = selections.first()?.segment;
    if selections
        .iter()
        .any(|selection| selection.segment != segment_index)
    {
        return None;
    }
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let segment = transcript
        .get_mut("segments")?
        .as_array_mut()?
        .get_mut(segment_index)?;
    let words = segment.get_mut("words")?.as_array_mut()?;
    let indices = selections
        .iter()
        .map(|selection| selection.word)
        .collect::<BTreeSet<_>>();
    if indices.iter().any(|index| *index >= words.len()) {
        return None;
    }
    let first_index = *indices.first()?;
    let selected = indices
        .iter()
        .filter_map(|index| words.get(*index).cloned())
        .collect::<Vec<_>>();
    let mut merged = selected.first()?.clone();
    let text = selected
        .iter()
        .filter_map(|word| word.get("word").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(if compact { "" } else { " " });
    let start = selected
        .iter()
        .map(|word| note_number(word, "start", 0.0))
        .reduce(f64::min)?;
    let end = selected
        .iter()
        .map(|word| note_number(word, "end", start + 0.02))
        .reduce(f64::max)?;
    merged["word"] = serde_json::Value::from(text);
    merged["start"] = serde_json::Value::from(round_millis(start));
    merged["end"] = serde_json::Value::from(round_millis(end));

    let mut output = Vec::with_capacity(words.len() - indices.len() + 1);
    for (index, word) in std::mem::take(words).into_iter().enumerate() {
        if index == first_index {
            output.push(merged.clone());
        }
        if !indices.contains(&index) {
            output.push(word);
        }
    }
    *words = output;
    refresh_segment_lyrics(segment, compact);
    Some(WordSelection {
        segment: segment_index,
        word: first_index,
    })
}

fn split_selected_editor_words(
    transcript: &mut serde_json::Value,
    selections: &BTreeSet<WordSelection>,
    playhead: f64,
) -> BTreeSet<WordSelection> {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let single = (selections.len() == 1).then_some(playhead);
    let Some(segments) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return BTreeSet::new();
    };
    let mut output_selection = BTreeSet::new();
    for (segment_index, segment) in segments.iter_mut().enumerate() {
        let selected_words = selections
            .iter()
            .filter(|selection| selection.segment == segment_index)
            .map(|selection| selection.word)
            .collect::<BTreeSet<_>>();
        if selected_words.is_empty() {
            continue;
        }
        let Some(words) = segment
            .get_mut("words")
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        let originals = std::mem::take(words);
        for (word_index, word) in originals.into_iter().enumerate() {
            if !selected_words.contains(&word_index) {
                words.push(word);
                continue;
            }
            let start = note_number(&word, "start", 0.0);
            let end = note_number(&word, "end", start + 0.02);
            if end - start < 0.04 {
                words.push(word);
                output_selection.insert(WordSelection {
                    segment: segment_index,
                    word: words.len() - 1,
                });
                continue;
            }
            let split = single
                .filter(|playhead| *playhead > start + 0.02 && *playhead < end - 0.02)
                .unwrap_or((start + end) / 2.0);
            let characters = word
                .get("word")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .chars()
                .collect::<Vec<_>>();
            let text_index =
                (characters.len() / 2).clamp(1, characters.len().saturating_sub(1).max(1));
            let left_text = if characters.len() > 1 {
                characters[..text_index].iter().collect::<String>()
            } else {
                characters.iter().collect::<String>()
            };
            let right_text = if characters.len() > 1 {
                characters[text_index..].iter().collect::<String>()
            } else {
                String::new()
            };
            let mut left = word.clone();
            left["word"] = serde_json::Value::from(left_text);
            left["end"] = serde_json::Value::from(round_millis(split));
            words.push(left);
            output_selection.insert(WordSelection {
                segment: segment_index,
                word: words.len() - 1,
            });
            let mut right = word;
            right["word"] = serde_json::Value::from(right_text);
            right["start"] = serde_json::Value::from(round_millis(split));
            words.push(right);
            output_selection.insert(WordSelection {
                segment: segment_index,
                word: words.len() - 1,
            });
        }
        refresh_segment_lyrics(segment, compact);
    }
    output_selection
}

fn shift_editor_word(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
    delta: f64,
) -> bool {
    if !delta.is_finite() {
        return false;
    }
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segment) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|segments| segments.get_mut(selection.segment))
    else {
        return false;
    };
    let Some(word) = segment
        .get_mut("words")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|words| words.get_mut(selection.word))
    else {
        return false;
    };
    let start = note_number(word, "start", 0.0);
    let end = note_number(word, "end", start + 0.02);
    let safe_delta = delta.max(-start);
    word["start"] = serde_json::Value::from(round_millis(start + safe_delta));
    word["end"] = serde_json::Value::from(round_millis(end + safe_delta));
    refresh_segment_lyrics(segment, compact);
    true
}

fn set_editor_word_timing(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
    start: f64,
    end: f64,
) -> bool {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return false;
    }
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segment) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|segments| segments.get_mut(selection.segment))
    else {
        return false;
    };
    let Some(word) = segment
        .get_mut("words")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|words| words.get_mut(selection.word))
    else {
        return false;
    };
    word["start"] = serde_json::Value::from(round_millis(start));
    word["end"] = serde_json::Value::from(round_millis(end.max(start + 0.01)));
    refresh_segment_lyrics(segment, compact);
    true
}

fn refresh_segment_lyrics(segment: &mut serde_json::Value, compact: bool) {
    if let Some(words) = segment.get("words").and_then(serde_json::Value::as_array)
        && !words.is_empty()
    {
        let start = words
            .iter()
            .map(|word| note_number(word, "start", 0.0))
            .reduce(f64::min)
            .unwrap_or(0.0);
        let end = words
            .iter()
            .map(|word| note_number(word, "end", start + 0.02))
            .reduce(f64::max)
            .unwrap_or(start + 0.02);
        segment["start"] = serde_json::Value::from(start);
        segment["end"] = serde_json::Value::from(end);
    }
    rebuild_segment_text(segment, compact);
}

fn adjust_editor_word_boundary(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
    start_delta: f64,
    end_delta: f64,
) -> bool {
    let Some(segment) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|segments| segments.get_mut(selection.segment))
    else {
        return false;
    };
    let (segment_start, segment_end) = {
        let Some(words) = segment
            .get_mut("words")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return false;
        };
        let Some(word) = words.get_mut(selection.word) else {
            return false;
        };
        let start = note_number(word, "start", 0.0);
        let end = note_number(word, "end", start + 0.02);
        let next_start = round_millis((start + start_delta).clamp(0.0, end - 0.01));
        let next_end = round_millis((end + end_delta).max(next_start + 0.01));
        word["start"] = serde_json::Value::from(next_start);
        word["end"] = serde_json::Value::from(next_end);
        (
            words
                .first()
                .map(|first| note_number(first, "start", 0.0))
                .unwrap_or(next_start),
            words
                .last()
                .map(|last| note_number(last, "end", next_end))
                .unwrap_or(next_end),
        )
    };
    segment["start"] = serde_json::Value::from(segment_start);
    segment["end"] = serde_json::Value::from(segment_end);
    true
}

#[cfg(test)]
fn split_editor_word(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
    playhead: f64,
) -> Option<WordSelection> {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let segment = transcript
        .get_mut("segments")?
        .as_array_mut()?
        .get_mut(selection.segment)?;
    let words = segment.get_mut("words")?.as_array_mut()?;
    let word = words.get(selection.word)?.clone();
    let start = note_number(&word, "start", 0.0);
    let end = note_number(&word, "end", start + 0.02);
    if end - start < 0.04 {
        return None;
    }
    let split = if playhead > start + 0.02 && playhead < end - 0.02 {
        playhead
    } else {
        (start + end) / 2.0
    };
    let characters = word
        .get("word")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .chars()
        .collect::<Vec<_>>();
    let text_index = (characters.len() / 2).clamp(1, characters.len().saturating_sub(1).max(1));
    let left_text = if characters.len() > 1 {
        characters[..text_index].iter().collect::<String>()
    } else {
        characters.iter().collect::<String>()
    };
    let right_text = if characters.len() > 1 {
        characters[text_index..].iter().collect::<String>()
    } else {
        String::new()
    };
    let mut left = word.clone();
    left["word"] = serde_json::Value::from(left_text);
    left["end"] = serde_json::Value::from(round_millis(split));
    let mut right = word;
    right["word"] = serde_json::Value::from(right_text);
    right["start"] = serde_json::Value::from(round_millis(split));
    words.splice(selection.word..=selection.word, [left, right]);
    let segment_start = note_number(&words[0], "start", start);
    let segment_end = note_number(words.last().unwrap_or(&serde_json::Value::Null), "end", end);
    segment["start"] = serde_json::Value::from(segment_start);
    segment["end"] = serde_json::Value::from(segment_end);
    rebuild_segment_text(segment, compact);
    Some(WordSelection {
        segment: selection.segment,
        word: selection.word + 1,
    })
}

fn merge_editor_word(transcript: &mut serde_json::Value, selection: WordSelection) -> bool {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segment) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|segments| segments.get_mut(selection.segment))
    else {
        return false;
    };
    let Some(words) = segment
        .get_mut("words")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    if selection.word + 1 >= words.len() {
        return false;
    }
    let left = words[selection.word].clone();
    let right = words[selection.word + 1].clone();
    let mut merged = left.clone();
    let left_text = left
        .get("word")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    let right_text = right
        .get("word")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    let contiguous_split =
        (note_number(&left, "end", 0.0) - note_number(&right, "start", f64::INFINITY)).abs()
            <= 0.001;
    merged["word"] = serde_json::Value::from(format!(
        "{left_text}{}{right_text}",
        if compact || contiguous_split { "" } else { " " }
    ));
    merged["end"] =
        serde_json::Value::from(note_number(&right, "end", note_number(&left, "end", 0.02)));
    words.splice(selection.word..=selection.word + 1, [merged]);
    if let Some(last) = words.last() {
        segment["end"] = serde_json::Value::from(note_number(last, "end", 0.02));
    }
    rebuild_segment_text(segment, compact);
    true
}

fn split_editor_phrase(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
) -> Option<WordSelection> {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let segments = transcript.get_mut("segments")?.as_array_mut()?;
    let segment = segments.get(selection.segment)?.clone();
    let words = segment.get("words")?.as_array()?;
    if selection.word + 1 >= words.len() {
        return None;
    }
    let left_words = words[..=selection.word].to_vec();
    let right_words = words[selection.word + 1..].to_vec();
    let mut left = segment.clone();
    left["words"] = serde_json::Value::from(left_words.clone());
    left["start"] = serde_json::Value::from(note_number(&left_words[0], "start", 0.0));
    left["end"] = serde_json::Value::from(note_number(
        left_words.last().unwrap_or(&serde_json::Value::Null),
        "end",
        0.02,
    ));
    rebuild_segment_text(&mut left, compact);
    let mut right = segment;
    right["words"] = serde_json::Value::from(right_words.clone());
    right["start"] = serde_json::Value::from(note_number(&right_words[0], "start", 0.0));
    right["end"] = serde_json::Value::from(note_number(
        right_words.last().unwrap_or(&serde_json::Value::Null),
        "end",
        0.02,
    ));
    rebuild_segment_text(&mut right, compact);
    segments.splice(selection.segment..=selection.segment, [left, right]);
    Some(WordSelection {
        segment: selection.segment + 1,
        word: 0,
    })
}

fn merge_editor_phrase(
    transcript: &mut serde_json::Value,
    selection: WordSelection,
) -> Option<WordSelection> {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let segments = transcript.get_mut("segments")?.as_array_mut()?;
    if selection.segment + 1 >= segments.len() {
        return None;
    }
    let left = segments[selection.segment].clone();
    let right = segments[selection.segment + 1].clone();
    let left_count = left
        .get("words")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let mut words = left
        .get("words")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    words.extend(
        right
            .get("words")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default(),
    );
    words.sort_by(|left, right| {
        note_number(left, "start", 0.0).total_cmp(&note_number(right, "start", 0.0))
    });
    let mut merged = left;
    merged["words"] = serde_json::Value::from(words.clone());
    if let Some(first) = words.first() {
        merged["start"] = serde_json::Value::from(note_number(first, "start", 0.0));
    }
    if let Some(last) = words.last() {
        merged["end"] = serde_json::Value::from(note_number(last, "end", 0.02));
    }
    rebuild_segment_text(&mut merged, compact);
    segments.splice(selection.segment..=selection.segment + 1, [merged]);
    Some(WordSelection {
        segment: selection.segment,
        word: left_count.saturating_sub(1),
    })
}

#[derive(Default)]
struct ChartIssueSummary {
    errors: usize,
    warnings: usize,
    auto_fixable: bool,
}

impl ChartIssueSummary {
    fn total(&self) -> usize {
        self.errors + self.warnings
    }
}

fn analyze_chart_issues(chart: &app_core::ChartDocument) -> ChartIssueSummary {
    let notes = chart_notes(chart);
    let mut summary = ChartIssueSummary::default();
    for (index, note) in notes.iter().enumerate() {
        if note.confidence < 0.55 || note.end - note.start < 0.06 {
            summary.warnings += 1;
        }
        if let Some(previous) = index.checked_sub(1).and_then(|index| notes.get(index)) {
            if note.start < previous.start || note.start < previous.end - 0.001 {
                summary.errors += 1;
                summary.auto_fixable = true;
            }
            if note.start - previous.end < 0.25 && (note.midi - previous.midi).abs() > 12.0 {
                summary.warnings += 1;
            }
        }
    }
    for lyric in chart_lyrics(chart, &notes) {
        if lyric.text.trim().is_empty() {
            summary.errors += 1;
        }
        if !lyric.guided {
            summary.warnings += 1;
        }
    }
    summary
}

fn repair_editor_chart(chart: &mut app_core::ChartDocument) -> bool {
    let Some(notes) = chart
        .pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    for note in notes.iter_mut() {
        let start = round_millis(note_number(note, "start", 0.0).max(0.0));
        let end = round_millis(note_number(note, "end", start + 0.03).max(start + 0.03));
        note["start"] = serde_json::Value::from(start);
        note["end"] = serde_json::Value::from(end);
        note["midi"] =
            serde_json::Value::from(note_number(note, "midi", 60.0).round().clamp(0.0, 127.0));
        note["confidence"] =
            serde_json::Value::from(note_number(note, "confidence", 1.0).clamp(0.0, 1.0));
    }
    notes.sort_by(|left, right| {
        note_number(left, "start", 0.0)
            .total_cmp(&note_number(right, "start", 0.0))
            .then_with(|| {
                note_number(left, "end", 0.03).total_cmp(&note_number(right, "end", 0.03))
            })
    });
    for index in 1..notes.len() {
        let (left, right) = notes.split_at_mut(index);
        let previous = &mut left[index - 1];
        let current = &mut right[0];
        let previous_start = note_number(previous, "start", 0.0);
        let previous_end = note_number(previous, "end", previous_start + 0.03);
        let current_start = note_number(current, "start", previous_end + 0.01);
        let current_end = note_number(current, "end", current_start + 0.03);
        if current_start < previous_end {
            let room = previous_start + 0.04;
            if room <= current_end - 0.03 {
                let boundary = ((previous_end + current_start) / 2.0)
                    .clamp(previous_start + 0.03, current_end - 0.04);
                previous["end"] = serde_json::Value::from(round_millis(boundary));
                current["start"] = serde_json::Value::from(round_millis(boundary + 0.01));
            } else {
                let start = round_millis(previous_end + 0.01);
                current["start"] = serde_json::Value::from(start);
                current["end"] =
                    serde_json::Value::from(round_millis(current_end.max(start + 0.03)));
            }
        }
    }
    repair_transcript(&mut chart.transcript);
    true
}

fn repair_transcript(transcript: &mut serde_json::Value) {
    let compact = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .is_some_and(compact_lyric_language);
    let Some(segments) = transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for segment in segments.iter_mut() {
        let (segment_start, segment_end) = {
            let Some(words) = segment
                .get_mut("words")
                .and_then(serde_json::Value::as_array_mut)
            else {
                continue;
            };
            words.sort_by(|left, right| {
                note_number(left, "start", 0.0).total_cmp(&note_number(right, "start", 0.0))
            });
            for word in words.iter_mut() {
                let start = round_millis(note_number(word, "start", 0.0).max(0.0));
                let end = round_millis(note_number(word, "end", start + 0.02).max(start + 0.02));
                word["start"] = serde_json::Value::from(start);
                word["end"] = serde_json::Value::from(end);
            }
            for index in 1..words.len() {
                let (left, right) = words.split_at_mut(index);
                let previous = &mut left[index - 1];
                let current = &mut right[0];
                let previous_start = note_number(previous, "start", 0.0);
                let previous_end = note_number(previous, "end", previous_start + 0.02);
                let current_start = note_number(current, "start", previous_end);
                let current_end = note_number(current, "end", current_start + 0.02);
                if current_start < previous_end {
                    let boundary = round_millis((previous_end + current_start) / 2.0);
                    previous["end"] =
                        serde_json::Value::from((previous_start + 0.01).max(boundary));
                    current["start"] = serde_json::Value::from((current_end - 0.01).min(boundary));
                }
            }
            (
                words
                    .first()
                    .map(|first| note_number(first, "start", 0.0))
                    .unwrap_or(0.0),
                words
                    .last()
                    .map(|last| note_number(last, "end", 0.02))
                    .unwrap_or(0.02),
            )
        };
        segment["start"] = serde_json::Value::from(segment_start);
        segment["end"] = serde_json::Value::from(segment_end);
        rebuild_segment_text(segment, compact);
    }
    segments.sort_by(|left, right| {
        note_number(left, "start", 0.0).total_cmp(&note_number(right, "start", 0.0))
    });
}

fn shift_all_chart_timings(chart: &mut app_core::ChartDocument, seconds: f64) {
    let earliest_note = chart_notes(chart)
        .into_iter()
        .map(|note| note.start)
        .reduce(f64::min);
    let earliest_word = chart
        .transcript
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|segment| {
            segment
                .get("words")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|word| note_number(word, "start", 0.0))
        .reduce(f64::min);
    let earliest = earliest_note
        .into_iter()
        .chain(earliest_word)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let safe_seconds = seconds.max(-earliest);

    if let Some(notes) = chart
        .pitch_notes
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for note in notes {
            let start = note_number(note, "start", 0.0);
            let end = note_number(note, "end", start + 0.03);
            note["start"] = serde_json::Value::from(round_millis(start + safe_seconds));
            note["end"] = serde_json::Value::from(round_millis(end + safe_seconds));
        }
    }
    if let Some(segments) = chart
        .transcript
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    {
        for segment in segments {
            let start = note_number(segment, "start", 0.0);
            let end = note_number(segment, "end", start + 0.02);
            segment["start"] = serde_json::Value::from(round_millis(start + safe_seconds));
            segment["end"] = serde_json::Value::from(round_millis(end + safe_seconds));
            if let Some(words) = segment
                .get_mut("words")
                .and_then(serde_json::Value::as_array_mut)
            {
                for word in words {
                    let start = note_number(word, "start", 0.0);
                    let end = note_number(word, "end", start + 0.02);
                    word["start"] = serde_json::Value::from(round_millis(start + safe_seconds));
                    word["end"] = serde_json::Value::from(round_millis(end + safe_seconds));
                }
            }
        }
    }
}

fn update_editor_geometry(
    session: Res<StudioSession>,
    mut note_nodes: Query<(&EditorNoteNode, &mut Node)>,
    mut lyric_nodes: Query<(&EditorLyricNode, &mut Node), Without<EditorNoteNode>>,
) {
    let Some(editor) = session.editor.as_ref() else {
        return;
    };
    let notes = chart_notes(&editor.chart);
    for (marker, mut node) in &mut note_nodes {
        let Some(note) = notes.iter().find(|note| note.index == marker.0) else {
            node.display = Display::None;
            continue;
        };
        if note.end < editor.viewport_start || note.start > editor.viewport_end() {
            node.display = Display::None;
            continue;
        }
        let left = time_percent(note.start, editor);
        let right = time_percent(note.end, editor);
        node.display = Display::Flex;
        node.left = percent(left);
        node.top = percent(pitch_percent(note.midi, editor));
        node.width = percent((right - left).max(0.4));
    }
    for (marker, mut node) in &mut lyric_nodes {
        let Some((_, start, end)) = selected_editor_word(&editor.chart, marker.selection) else {
            node.display = Display::None;
            continue;
        };
        if end < editor.viewport_start || start > editor.viewport_end() {
            node.display = Display::None;
            continue;
        }
        let left = time_percent(start, editor);
        let right = time_percent(end, editor);
        node.display = Display::Flex;
        node.left = percent(left);
        node.width = percent((right - left).max(1.8));
    }
}

fn handle_folder_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<FolderEntryList>>,
) {
    if session.route != StudioRoute::Folders {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        wheel.clear();
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
}

fn handle_settings_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut session: ResMut<StudioSession>,
    mut contents: Query<(&ComputedNode, &mut ScrollPosition), With<SettingsContent>>,
) {
    if session.route != StudioRoute::Settings {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = contents.single_mut() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
    let tab_index = session.settings_tab.index();
    session.settings_scroll_offsets[tab_index] = position.y;
}

fn handle_library_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<LibrarySongList>>,
) {
    if session.route != StudioRoute::Library {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}

fn handle_song_detail_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut contents: Query<(&ComputedNode, &mut ScrollPosition), With<SongDetailContent>>,
) {
    if session.route != StudioRoute::SongDetail || session.lyrics_editor.is_some() {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = contents.single_mut() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}

fn sync_editor_audio(
    time: Res<Time>,
    mut timer: ResMut<EditorAudioSyncTimer>,
    audio: Res<NativeAudio>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        return;
    }
    let mut status_error = None;
    {
        let Some(editor) = session.editor.as_mut() else {
            return;
        };
        if timer.0.tick(time.delta()).just_finished() {
            match audio.0.status() {
                Ok(status) => {
                    if let Some(error) = status.error.clone() {
                        status_error = Some(format!("Editor audio stopped: {error}"));
                    }
                    editor.visible_position = status.position_secs;
                    editor.audio_status = status;
                    editor.last_audio_sync = Instant::now();
                }
                Err(error) => status_error = Some(error),
            }
        } else if editor.audio_status.playing {
            editor.visible_position = (editor.audio_status.position_secs
                + editor.last_audio_sync.elapsed().as_secs_f64())
            .min(
                editor
                    .audio_status
                    .duration_secs
                    .max(editor.audio_status.position_secs),
            );
        }

        if editor.audio_status.playing
            && Instant::now() >= editor.manual_scroll_until
            && editor.visible_position >= editor.viewport_start + editor.viewport_duration * 0.82
        {
            editor.viewport_start =
                (editor.visible_position - editor.viewport_duration * 0.28).max(0.0);
            invalidated.0 = true;
        }
    }
    if status_error.is_some() {
        session.notice = status_error;
    }
}

fn sync_library_audio(
    time: Res<Time>,
    mut timer: ResMut<LibraryAudioSyncTimer>,
    audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.library_playback.file_hash.is_none() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        let was_playing = session.library_playback.status.playing;
        let had_ended = session.library_playback.status.ended;
        match audio.0.status() {
            Ok(status) => {
                if let Some(error) = status.error.clone() {
                    session.notice = Some(format!("Library playback stopped: {error}"));
                    invalidated.0 = true;
                }
                session.library_playback.visible_position = status.position_secs;
                session.library_playback.status = status;
                session.library_playback.last_audio_sync = Instant::now();
                if session.library_playback.status.ended && !had_ended {
                    let repeat = session.library_playback.repeat;
                    let result = if repeat == LibraryRepeatMode::One {
                        restart_library_song(&audio.0, &mut session.library_playback)
                    } else {
                        advance_library_queue(
                            &audio.0,
                            &mut session.library_playback,
                            1,
                            repeat == LibraryRepeatMode::All,
                        )
                    };
                    if let Err(error) = result
                        && error != "This is the end of the queue."
                    {
                        session.notice = Some(error);
                    }
                    invalidated.0 = true;
                }
                if was_playing != session.library_playback.status.playing
                    || had_ended != session.library_playback.status.ended
                {
                    invalidated.0 = true;
                }
            }
            Err(error) => {
                session.notice = Some(error);
                invalidated.0 = true;
            }
        }
    } else if session.library_playback.status.playing {
        session.library_playback.visible_position =
            library_visible_position(&session.library_playback);
    }
}

fn update_editor_playhead(
    session: Res<StudioSession>,
    mut playheads: Query<&mut Node, With<EditorPlayhead>>,
    mut clocks: Query<&mut Text, With<EditorClockText>>,
) {
    let Some(editor) = session.editor.as_ref() else {
        return;
    };
    let position = time_percent(editor.visible_position, editor);
    for mut node in &mut playheads {
        node.left = percent(position);
    }
    let label = format_editor_clock(editor.visible_position, editor.audio_status.duration_secs);
    for mut text in &mut clocks {
        **text = label.clone();
    }
}

fn update_library_player_ui(
    session: Res<StudioSession>,
    mut progress: Query<&mut Node, With<LibraryPlayerProgress>>,
    mut clocks: Query<&mut Text, (With<LibraryPlayerClockText>, Without<LibraryPlayerProgress>)>,
) {
    let playback = &session.library_playback;
    if !playback.status.loaded {
        return;
    }
    let position = library_visible_position(playback);
    let duration = playback.status.duration_secs.max(0.001);
    let width = ((position / duration) * 100.0).clamp(0.0, 100.0) as f32;
    for mut node in &mut progress {
        node.width = percent(width);
    }
    let label = format_editor_clock(position, playback.status.duration_secs);
    for mut text in &mut clocks {
        **text = label.clone();
    }
}

fn validate_source_path(path: &std::path::Path, config: &AppConfig) -> Result<PathBuf, String> {
    let requested = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let mut allowed_roots = config.library_paths();
    if let Some(export_path) = config.export_path.as_ref() {
        allowed_roots.push(export_path.clone());
    }
    let allowed = allowed_roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| requested.starts_with(root))
            .unwrap_or(false)
    });
    allowed
        .then_some(requested)
        .ok_or_else(|| "Path is outside configured library and output locations".to_string())
}

fn open_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => match open::that_detached(&path) {
            Ok(()) => format!("Opened {}", path.display()),
            Err(error) => format!("Could not open {}: {error}", path.display()),
        },
        Err(error) => format!("Could not open this library item: {error}"),
    }
}

fn reveal_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => {
            let target = if path.is_dir() {
                path.as_path()
            } else if let Some(parent) = path.parent() {
                parent
            } else {
                path.as_path()
            };
            match open::that_detached(target) {
                Ok(()) => format!("Revealed {}", path.display()),
                Err(error) => format!("Could not reveal {}: {error}", path.display()),
            }
        }
        Err(error) => format!("Could not reveal this library item: {error}"),
    }
}

fn export_song(
    file_hash: &str,
    extension: &str,
    export_directory: Option<&std::path::Path>,
) -> String {
    let song = match app_core::load_song_by_hash(file_hash) {
        Ok(Some(song)) => song,
        Ok(None) => return format!("Song not found: {file_hash}"),
        Err(error) => return error.to_string(),
    };
    let file_name = format!("{}.{}", safe_file_stem(&song.title), extension);
    let mut dialog = rfd::FileDialog::new().set_file_name(file_name);
    if let Some(path) = export_directory {
        dialog = dialog.set_directory(path);
    }
    dialog = if extension == "utz" {
        dialog.add_filter("Uta package", &["utz"])
    } else {
        dialog.add_filter("UltraStar chart", &["txt"])
    };
    let Some(output) = dialog.save_file() else {
        return "Export cancelled.".to_string();
    };
    let result = if extension == "utz" {
        app_core::export_utz(file_hash, &output)
    } else {
        app_core::export_ultrastar(file_hash, &output)
    };
    match result {
        Ok(path) => format!("Exported {}", path.display()),
        Err(error) => format!("Export failed: {error}"),
    }
}

fn export_all_songs(songs: &[Song], extension: &str, export_directory: &std::path::Path) -> String {
    let mut title_counts = HashMap::<String, usize>::new();
    for song in songs {
        *title_counts
            .entry(safe_file_stem(&song.title).to_lowercase())
            .or_default() += 1;
    }

    let mut used_stems = HashMap::<String, usize>::new();
    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for song in songs {
        let title = safe_file_stem(&song.title);
        let mut stem = if title_counts
            .get(&title.to_lowercase())
            .copied()
            .unwrap_or_default()
            > 1
        {
            format!("{} — {}", title, safe_file_stem(&song.artist))
        } else {
            title
        };
        let collision_key = stem.to_lowercase();
        let collision = used_stems.entry(collision_key).or_default();
        if *collision > 0 {
            let suffix = song.file_hash.chars().take(8).collect::<String>();
            stem = format!("{stem} — {suffix}");
        }
        *collision += 1;

        let output = export_directory.join(format!("{stem}.{extension}"));
        if output.exists() {
            skipped += 1;
            continue;
        }
        let result = match extension {
            "utz" => app_core::export_utz(&song.file_hash, &output),
            "txt" => app_core::export_ultrastar(&song.file_hash, &output),
            _ => unreachable!("batch export extensions are fixed by the UI"),
        };
        match result {
            Ok(_) => exported += 1,
            Err(error) => failures.push(format!("{}: {error}", song.title)),
        }
    }

    let mut summary = format!(
        "Export all finished · {exported} exported · {skipped} already existed · {} failed · {}",
        failures.len(),
        export_directory.display()
    );
    if !failures.is_empty() {
        summary.push_str(" · ");
        summary.push_str(
            &failures
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
        );
        if failures.len() > 3 {
            summary.push_str(&format!("; and {} more", failures.len() - 3));
        }
    }
    summary
}

fn safe_file_stem(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect::<String>();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "Uta Studio Export".to_string()
    } else {
        value.to_string()
    }
}

fn refresh_library_while_scanning(
    time: Res<Time>,
    mut timer: ResMut<LibraryRefreshTimer>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !session.scanning || !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let previous = (
        session.meta.processed_count,
        session.meta.songs_count,
        session.meta.videos_count,
    );
    session.refresh_library();
    let current = (
        session.meta.processed_count,
        session.meta.songs_count,
        session.meta.videos_count,
    );
    if current != previous {
        invalidated.0 = true;
    }
    if session.meta.processed_count >= session.meta.count && session.meta.count > 0 {
        session.scanning = false;
        invalidated.0 = true;
    }
}

fn refresh_analysis_activity(
    time: Res<Time>,
    mut timer: ResMut<AnalysisRefreshTimer>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let tasks = app_core::load_analysis_tasks();
    let history = app_core::load_analysis_history(100);
    if tasks == session.analysis_tasks && history == session.analysis_history {
        return;
    }
    session.analysis_tasks = tasks;
    session.analysis_history = history;
    if session.route == StudioRoute::Library && session.library_view == LibraryView::Queue {
        session.refresh_library();
    }
    invalidated.0 = true;
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart_fixture(
        transcript: serde_json::Value,
        pitch_notes: serde_json::Value,
    ) -> app_core::ChartDocument {
        app_core::ChartDocument {
            file_hash: "fixture".to_string(),
            transcript,
            pitch_track: serde_json::json!({}),
            pitch_notes,
            audio: app_core::ChartAudio {
                instrumental: "instrumental.flac".to_string(),
                vocals: None,
                original: "original.flac".to_string(),
            },
            repaired_issues: Vec::new(),
        }
    }

    #[test]
    fn native_window_preserves_existing_desktop_geometry() {
        let window = studio_window(&AppConfig::default(), true);
        assert_eq!(window.title, "Uta Studio");
        assert_eq!(window.width(), 1280.0);
        assert_eq!(window.height(), 720.0);
        assert!(window.decorations);
        assert!(!window.transparent);
        assert_eq!(window.window_theme, Some(WindowTheme::Dark));
    }

    #[test]
    fn duration_format_matches_the_library_table() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(65.2), "1:05");
        assert_eq!(format_duration(f64::NAN), "0:00");
    }

    #[test]
    fn button_feedback_preserves_authored_surfaces_and_activity_backdrop() {
        let theme = StudioTheme::new(false);
        let resting = theme.card.with_alpha(0.46);
        assert_eq!(
            button_background(
                &UiAction::ToggleLibraryLayout,
                Interaction::None,
                resting,
                &theme,
            ),
            resting
        );
        assert_ne!(
            button_background(
                &UiAction::ToggleLibraryLayout,
                Interaction::Hovered,
                resting,
                &theme,
            ),
            Color::NONE
        );
        assert_eq!(
            button_background(
                &UiAction::CloseActivity,
                Interaction::Hovered,
                theme.background.with_alpha(0.54),
                &theme,
            ),
            theme.background.with_alpha(0.54)
        );
    }

    #[test]
    fn editor_audio_failure_does_not_block_chart_authoring() {
        let status = editor_audio_status(Err("missing typefind".to_string()));
        assert!(!status.loaded);
        assert_eq!(status.error.as_deref(), Some("missing typefind"));
    }

    #[test]
    fn setup_request_preserves_the_selected_backend_and_artifact() {
        let mut config = AppConfig {
            compute_backend: Some("intel".to_string()),
            ..AppConfig::default()
        };
        let folders = setup_folders(
            &config,
            SetupRequest {
                target: Some(app_core::ModelDownloadTarget::Pitch),
            },
        );
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Intel);
        assert_eq!(
            folders.model_target,
            Some(app_core::ModelDownloadTarget::Pitch)
        );

        config.compute_backend = Some("cuda".to_string());
        let folders = setup_folders(&config, SetupRequest { target: None });
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Cuda);
        assert_eq!(folders.model_target, None);
    }

    #[test]
    fn development_asset_root_contains_the_canonical_brand_assets() {
        let root = std::path::PathBuf::from(asset_root());
        assert!(root.join(LOGO_PATH).is_file());
        assert!(root.join(FONT_PATH).is_file());
        assert!(root.join(ICON_ATLAS_PATH).is_file());
        assert!(root.join("desktop/assets/icons/ui-icons.svg").is_file());
    }

    #[test]
    fn expected_icu_cjk_fallback_does_not_flood_desktop_logs() {
        assert!(studio_log_filter().contains("icu_provider=error"));
    }

    #[test]
    fn export_stem_cannot_escape_the_selected_directory() {
        assert_eq!(safe_file_stem("../A/B: C?.utz"), "_A_B_ C_.utz");
        assert_eq!(safe_file_stem("..."), "Uta Studio Export");
    }

    #[test]
    fn note_drag_updates_only_the_selected_note_fields() {
        let mut pitch_notes = serde_json::json!({
            "format_version": 1,
            "notes": [
                {"start": 1.0, "end": 1.5, "midi": 60, "kind": "golden", "confidence": 0.9},
                {"start": 2.0, "end": 2.5, "midi": 62, "confidence": 0.8}
            ]
        });
        assert!(move_chart_note(&mut pitch_notes, 0, 3.25, 3.75, 64.0));
        assert_eq!(pitch_notes["notes"][0]["start"], 3.25);
        assert_eq!(pitch_notes["notes"][0]["end"], 3.75);
        assert_eq!(pitch_notes["notes"][0]["midi"], 64.0);
        assert_eq!(pitch_notes["notes"][0]["kind"], "golden");
        assert_eq!(pitch_notes["notes"][1]["start"], 2.0);
        assert!(!move_chart_note(&mut pitch_notes, 9, 0.0, 1.0, 60.0));
    }

    #[test]
    fn overlapping_lyrics_use_separate_lanes_and_mark_missing_guidance() {
        let chart = chart_fixture(
            serde_json::json!({
                "segments": [{
                    "start": 0.0,
                    "end": 1.0,
                    "text": "one two three",
                    "words": [
                        {"start": 0.0, "end": 0.7, "word": "one"},
                        {"start": 0.2, "end": 0.8, "word": "two"},
                        {"start": 1.1, "end": 1.2, "word": "three"}
                    ]
                }]
            }),
            serde_json::json!({
                "notes": [{"start": 0.1, "end": 0.5, "midi": 60}]
            }),
        );
        let notes = chart_notes(&chart);
        let lyrics = chart_lyrics(&chart, &notes);
        assert_eq!(lyrics.len(), 3);
        assert_ne!(lyrics[0].lane, lyrics[1].lane);
        assert!(lyrics[0].guided);
        assert!(lyrics[1].guided);
        assert!(!lyrics[2].guided);
    }

    #[test]
    fn editor_viewport_maps_time_and_pitch_independently() {
        let mut editor = NativeEditor::new(
            chart_fixture(
                serde_json::json!({"segments": []}),
                serde_json::json!({"notes": []}),
            ),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            "instrumental",
        );
        editor.viewport_start = 10.0;
        editor.viewport_duration = 20.0;
        editor.pitch_min = 40.0;
        editor.pitch_max = 80.0;
        assert_eq!(time_percent(20.0, &editor), 50.0);
        assert_eq!(pitch_percent(60.0, &editor), 58.0);
        assert_eq!(time_percent(5.0, &editor), 0.0);
        assert_eq!(pitch_percent(100.0, &editor), 20.0);
        assert_eq!(surface_pitch_fraction(0.2), 0.0);
        assert_eq!(surface_pitch_fraction(0.96), 1.0);
        set_editor_pitch_span(&mut editor, 999.0);
        assert_eq!(editor.pitch_min, 0.0);
        assert_eq!(editor.pitch_max, 127.0);
    }

    #[test]
    fn group_authoring_preserves_relative_rhythm_and_primary_metadata() {
        let original = serde_json::json!({
            "notes": [
                {"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9, "kind": "golden"},
                {"start": 1.6, "end": 2.0, "midi": 62, "confidence": 0.8}
            ]
        });
        let selected = BTreeSet::from([0, 1]);
        let copied = copy_chart_notes(&original, &selected);
        assert_eq!(copied[0]["start"], 0.0);
        assert_eq!(copied[1]["start"], 0.6);

        let mut pasted = original.clone();
        let inserted = paste_chart_notes(&mut pasted, &copied, 3.0);
        assert_eq!(inserted.len(), 2);
        let starts = inserted
            .iter()
            .map(|index| pasted["notes"][*index]["start"].as_f64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(starts, vec![3.0, 3.6]);

        let split = split_chart_notes(&mut pasted, &BTreeSet::from([0]), 1.25);
        assert_eq!(split.len(), 2);
        let merged = merge_chart_notes(&mut pasted, &split, split.iter().next().copied()).unwrap();
        assert_eq!(pasted["notes"][merged]["start"], 1.0);
        assert_eq!(pasted["notes"][merged]["end"], 1.5);
        assert_eq!(pasted["notes"][merged]["kind"], "golden");
    }

    #[test]
    fn quantization_and_safe_repair_keep_valid_note_ranges() {
        let mut chart = chart_fixture(
            serde_json::json!({
                "language": "en",
                "segments": [{
                    "start": 1.0,
                    "end": 2.0,
                    "text": "hello world",
                    "words": [
                        {"word": "hello", "start": 1.0, "end": 1.7},
                        {"word": "world", "start": 1.5, "end": 2.0}
                    ]
                }]
            }),
            serde_json::json!({
                "notes": [
                    {"start": 1.023, "end": 1.071, "midi": 60.3, "confidence": 1.2},
                    {"start": 1.05, "end": 1.3, "midi": 61.0, "confidence": 1.0}
                ]
            }),
        );
        assert_eq!(quantize_chart_notes(&mut chart.pitch_notes, None, 0.05), 2);
        assert_eq!(chart.pitch_notes["notes"][0]["start"], 1.0);
        assert_eq!(chart.pitch_notes["notes"][0]["end"], 1.05);
        assert!(repair_editor_chart(&mut chart));
        let notes = chart_notes(&chart);
        assert!(notes[0].end <= notes[1].start);
        assert!(analyze_chart_issues(&chart).errors == 0);
    }

    #[test]
    fn lyric_word_and_phrase_edits_rebuild_text_and_boundaries() {
        let mut transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "start": 1.0,
                "end": 2.0,
                "text": "hello world",
                "words": [
                    {"word": "hello", "start": 1.0, "end": 1.5},
                    {"word": "world", "start": 1.6, "end": 2.0}
                ]
            }]
        });
        let first = WordSelection {
            segment: 0,
            word: 0,
        };
        let split = split_editor_word(&mut transcript, first, 1.25).unwrap();
        assert_eq!(
            transcript["segments"][0]["words"].as_array().unwrap().len(),
            3
        );
        assert!(merge_editor_word(&mut transcript, first));
        assert_eq!(transcript["segments"][0]["text"], "hello world");
        let next_phrase = split_editor_phrase(&mut transcript, first).unwrap();
        assert_eq!(transcript["segments"].as_array().unwrap().len(), 2);
        assert_eq!(next_phrase.segment, 1);
        assert!(merge_editor_phrase(&mut transcript, first).is_some());
        assert_eq!(transcript["segments"].as_array().unwrap().len(), 1);
        assert_eq!(split.word, 1);

        let inserted = insert_editor_word(&mut transcript, Some(first), 1.2).unwrap();
        assert_eq!(inserted.word, 1);
        assert_eq!(
            transcript["segments"][0]["words"].as_array().unwrap().len(),
            3
        );
        assert!(update_editor_word_text(&mut transcript, inserted, "dear"));
        let start = transcript["segments"][0]["words"][1]["start"]
            .as_f64()
            .unwrap();
        assert!(shift_editor_word(&mut transcript, inserted, 0.05));
        assert_eq!(
            transcript["segments"][0]["words"][1]["start"],
            round_millis(start + 0.05)
        );
        let (deleted, next) = delete_editor_word(&mut transcript, inserted);
        assert!(deleted);
        assert_eq!(
            next,
            Some(WordSelection {
                segment: 0,
                word: 1
            })
        );
        assert_eq!(transcript["segments"][0]["text"], "hello world");
    }

    #[test]
    fn lyric_multi_selection_can_split_merge_move_and_delete() {
        let mut transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "start": 1.0,
                "end": 2.5,
                "text": "one two three",
                "words": [
                    {"word": "one", "start": 1.0, "end": 1.4},
                    {"word": "two", "start": 1.5, "end": 1.9},
                    {"word": "three", "start": 2.0, "end": 2.5}
                ]
            }]
        });
        let first_two = [
            WordSelection {
                segment: 0,
                word: 0,
            },
            WordSelection {
                segment: 0,
                word: 1,
            },
        ]
        .into_iter()
        .collect();
        let merged = merge_selected_editor_words(&mut transcript, &first_two).unwrap();
        assert_eq!(transcript["segments"][0]["text"], "one two three");
        let selected = [merged].into_iter().collect();
        let split = split_selected_editor_words(&mut transcript, &selected, 1.45);
        assert_eq!(split.len(), 2);
        let before = transcript["segments"][0]["words"][0]["start"]
            .as_f64()
            .unwrap();
        for selection in &split {
            assert!(shift_editor_word(&mut transcript, *selection, 0.05));
        }
        assert_eq!(
            transcript["segments"][0]["words"][0]["start"],
            round_millis(before + 0.05)
        );
        assert_eq!(delete_editor_words(&mut transcript, &split), 2);
        assert_eq!(transcript["segments"][0]["text"], "three");
    }

    #[test]
    fn pitch_contour_is_bounded_and_confidence_weighted() {
        let frames = (0..100)
            .map(|index| ChartPitchFrame {
                time: f64::from(index) * 0.01,
                midi: 60.0 + f64::from(index % 3),
                confidence: if index % 2 == 0 { 1.0 } else { 0.2 },
            })
            .collect::<Vec<_>>();
        let contour = abstract_pitch_contour(&frames, 20);
        assert!(contour.len() <= 20);
        assert!(contour.iter().all(|frame| frame.midi.is_finite()));
        assert!(contour.windows(2).all(|pair| pair[0].time < pair[1].time));
    }

    #[test]
    fn pitch_evidence_converts_only_voiced_finite_frames() {
        let mut chart = chart_fixture(
            serde_json::json!({"segments": []}),
            serde_json::json!({"notes": []}),
        );
        chart.pitch_track = serde_json::json!({
            "frames": [
                {"time": 1.0, "hz": 440.0, "confidence": 0.9},
                {"time": 1.1, "hz": null, "confidence": 0.1}
            ]
        });
        let frames = chart_pitch_frames(&chart);
        assert_eq!(frames.len(), 1);
        assert!((frames[0].midi - 69.0).abs() < f64::EPSILON);
    }
}
