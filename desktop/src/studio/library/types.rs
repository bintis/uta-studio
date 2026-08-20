//! Library route: browsing, filters, song rows, player, and export.

use crate::studio::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibrarySelectKind {
    Status,
    TranscriptSource,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibraryView {
    #[default]
    All,
    Queue,
    Completed,
    Videos,
    Artists,
    Albums,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LibraryFacet {
    Artist { value: String, label: String },
    Album { value: String, label: String },
    Playlist { value: String, label: String },
}

impl LibraryFacet {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Artist { label, .. }
            | Self::Album { label, .. }
            | Self::Playlist { label, .. } => label,
        }
    }
}

impl LibraryView {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::All => "Song Library",
            Self::Queue => "Analysis",
            Self::Completed => "Charts",
            Self::Videos => "Video",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
        }
    }

    pub(crate) fn eyebrow(self) -> &'static str {
        match self {
            Self::All => "ALL MUSIC",
            Self::Queue => "IN PROGRESS",
            Self::Completed | Self::Videos | Self::Artists | Self::Albums => "MY LIBRARY",
        }
    }

    pub(crate) fn filters(self) -> LibraryMenuFilters {
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

#[derive(Clone)]
pub(crate) struct SongContextMenu {
    pub(crate) song: Song,
    pub(crate) position: Vec2,
}

pub(crate) struct LibraryPlayback {
    pub(crate) file_hash: Option<String>,
    pub(crate) visible_position: f64,
    pub(crate) status: uta_studio_audio::EditorAudioStatus,
    pub(crate) last_audio_sync: Instant,
    pub(crate) queue: Vec<String>,
    pub(crate) queue_index: Option<usize>,
    pub(crate) queue_open: bool,
    pub(crate) shuffle: bool,
    pub(crate) shuffle_seed: u64,
    pub(crate) repeat: LibraryRepeatMode,
    pub(crate) volume: f64,
    pub(crate) volume_before_mute: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LibraryRepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl LibraryRepeatMode {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub(crate) fn label(self) -> &'static str {
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

pub(crate) fn load_songs(filters: LibraryMenuFilters) -> SongsStore {
    SongsStore::load(&LoadSongsParams {
        search: None,
        filters,
        skip: 0,
        take: 500,
    })
}

#[derive(Resource)]
pub(crate) struct NativeLibraryAudio(pub(crate) Arc<uta_studio_audio::EditorAudioPlayer>);

#[derive(Resource)]
pub(crate) struct LibraryRefreshTimer(pub(crate) Timer);

#[derive(Resource)]
pub(crate) struct LibraryAudioSyncTimer(pub(crate) Timer);

#[derive(Default)]
pub(crate) struct NativeExportJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<String>>>,
}

#[derive(Component)]
pub(crate) struct LibraryPlayerProgress;

#[derive(Component)]
pub(crate) struct LibraryPlayerClockText;

#[derive(Component)]
pub(crate) struct LibrarySongList;

#[derive(Component)]
pub(crate) struct LibrarySearchInput;
