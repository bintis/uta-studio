use crate::studio::*;

pub(crate) const FONT_PATH: &str = "desktop/assets/fonts/NotoSansCJKsc-Regular.otf";

pub(crate) const LOGO_PATH: &str = "icon.png";
pub(crate) const MUSIC_PLACEHOLDER_PATH: &str = "desktop/assets/icons/music-placeholder.png";

/// Baked into the binary (see `setup`'s `BrandImages`) rather than loaded
/// via `AssetServer` like `LOGO_PATH` -- neither needs to be user-replaceable
/// at runtime, and embedding means one less file the packaged build has to
/// carry and locate correctly.
pub(crate) const LOGO_BYTES: &[u8] = include_bytes!("../../../icon.png");

pub(crate) const BANNER_BYTES: &[u8] = include_bytes!("../../../Banner.png");
pub(crate) const STARTUP_BANNER_BYTES: &[u8] = include_bytes!("../../../Banner0.png");
pub(crate) const STARTUP_BANNER_FADE_IN_SECONDS: f32 = 0.40;
pub(crate) const STARTUP_BANNER_HOLD_SECONDS: f32 = 0.30;
pub(crate) const STARTUP_BANNER_FADE_OUT_SECONDS: f32 = 0.40;
pub(crate) const STARTUP_BANNER_WIDTH: f32 = 620.0;
pub(crate) const STARTUP_BANNER_HEIGHT: f32 = STARTUP_BANNER_WIDTH * 3.0 / 4.0;

/// Decoded once in `setup` from embedded bytes and reused by
/// every `rebuild_ui` pass after that, the same "decode once, hand out
/// cheap `Handle` clones" shape `LocalImages` already uses for cover art.
#[derive(Resource, Clone)]
pub(crate) struct BrandImages {
    pub(crate) logo: Handle<Image>,
    pub(crate) banner: Handle<Image>,
    pub(crate) startup_banner: Handle<Image>,
}

#[derive(Component)]
pub(crate) struct StartupBannerRoot;

#[derive(Component)]
pub(crate) struct StartupBannerImage;

#[derive(Resource)]
pub(crate) struct StartupBannerState {
    pub(crate) timer: Timer,
    pub(crate) done: bool,
    pub(crate) restore_window_mode: WindowMode,
}

impl Default for StartupBannerState {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(
                STARTUP_BANNER_FADE_IN_SECONDS
                    + STARTUP_BANNER_HOLD_SECONDS
                    + STARTUP_BANNER_FADE_OUT_SECONDS,
                TimerMode::Once,
            ),
            done: false,
            restore_window_mode: WindowMode::Windowed,
        }
    }
}

impl StartupBannerState {
    pub(crate) fn for_launch(restore_window_mode: WindowMode) -> Self {
        Self {
            restore_window_mode,
            ..Self::default()
        }
    }
}

impl StartupBannerState {
    pub(crate) fn alpha(&self) -> f32 {
        if self.done {
            return 0.0;
        }

        let elapsed = self.timer.elapsed_secs();
        if elapsed < STARTUP_BANNER_FADE_IN_SECONDS {
            elapsed / STARTUP_BANNER_FADE_IN_SECONDS
        } else if elapsed < STARTUP_BANNER_FADE_IN_SECONDS + STARTUP_BANNER_HOLD_SECONDS {
            1.0
        } else if elapsed
            < STARTUP_BANNER_FADE_IN_SECONDS
                + STARTUP_BANNER_HOLD_SECONDS
                + STARTUP_BANNER_FADE_OUT_SECONDS
        {
            1.0 - (elapsed - STARTUP_BANNER_FADE_IN_SECONDS - STARTUP_BANNER_HOLD_SECONDS)
                / STARTUP_BANNER_FADE_OUT_SECONDS
        } else {
            0.0
        }
    }
}

pub(crate) fn decode_embedded_png(bytes: &[u8], images: &mut Assets<Image>) -> Handle<Image> {
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .expect("brand PNGs embedded at compile time are always well-formed");
    images.add(image)
}

pub(crate) const SIDEBAR_WIDTH: f32 = 265.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StudioRoute {
    #[default]
    Library,
    Folders,
    SongDetail,
    Settings,
    Documentation,
    Editor,
    ProcessingStudio,
    AnalysisInspect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingLeave {
    Exit,
    Back,
    Home,
    Documentation,
}

#[derive(Resource)]
pub(crate) struct NativeAudio(pub(crate) Arc<uta_studio_audio::EditorAudioPlayer>);

/// The synthesized pitch stream. It is a second player so auditioning a note
/// target never alters, mixes into, or re-encodes the song audio.
#[derive(Resource)]
pub(crate) struct NativePitchAudition(pub(crate) Arc<uta_studio_audio::PitchAudition>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDirection {
    Previous,
    Next,
}

#[derive(Resource, Default)]
pub(crate) struct NavigationInputState {
    pub(crate) held_direction: Option<NavigationDirection>,
    pub(crate) repeat_at: Option<Instant>,
    pub(crate) activated: Option<Entity>,
}

#[derive(Component)]
pub(crate) struct StudioUiRoot;

#[derive(Component)]
pub(crate) struct StudioBodyRoot;

#[derive(Component)]
pub(crate) struct WorkspaceRegionRoot;

#[derive(Component)]
pub(crate) struct EditorRegionRoot;

#[derive(Component)]
pub(crate) struct OverlayRegionRoot;

#[derive(Resource)]
pub(crate) struct DebugScreenshotState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) settled_frames: u16,
    pub(crate) requested: bool,
}

impl Default for DebugScreenshotState {
    fn default() -> Self {
        Self {
            path: std::env::var_os("UTA_STUDIO_DEBUG_SCREENSHOT_PATH").map(PathBuf::from),
            settled_frames: 0,
            requested: false,
        }
    }
}
