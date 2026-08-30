use std::path::PathBuf;

use bevy::prelude::Component;

use super::{
    ArtifactAuditionSlot, CacheClearScope, EditorAction, EditorDockSelectKind, LibraryFacet,
    LibrarySelectKind, LibraryView, ProblemsFilter, SettingsSelectKind, SettingsTab,
    TranscriptBoundaryEdge, TranscriptBoundaryTarget, UiDirtyRegion, WaveformSource, WaveformStyle,
    WordSelection,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AppCommand {
    Back,
    Home,
    ToggleGlobalSearch,
    Folders,
    Settings,
    Documentation,
    OpenDocumentation(Option<String>),
    DocumentationBack,
    DocumentationForward,
    ToggleActivity,
    CloseActivity,
    OpenAbout,
    CloseAbout,
    ToggleFullscreen,
    OpenLog,
    RunDiagnostics,
    CancelLeave,
    ConfirmLeave,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LibraryCommand {
    SetLibraryView(LibraryView),
    SetLibraryFacet(LibraryFacet),
    LoadMoreSongs,
    ApplyLibrarySearch,
    ClearLibrarySearch,
    ToggleLibraryLayout,
    ToggleExportAllMenu,
    ExportAllUtz,
    ExportAllUltraStar,
    OpenLibrarySelect(LibrarySelectKind),
    SelectLibraryValue(LibrarySelectKind, String),
    AnalyzeAll,
    RescanLibrary,
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
    OpenSong(String),
    AnalyzeSong(String),
    ChooseEditorFile,
    OpenEditor(String),
    ExportUtz(String),
    ExportUltraStar(String),
    OpenSource(PathBuf),
    RevealSource(PathBuf),
    DismissSongContext,
    PlayLibrarySong(String),
    ToggleLibraryPlayback,
    SeekLibraryRelative(i8),
    PreviousLibrarySong,
    NextLibrarySong,
    ToggleLibraryShuffle,
    CycleLibraryRepeat,
    AdjustLibraryVolume(i8),
    ToggleLibraryMute,
    ToggleLibraryAudioSourceMenu,
    SelectLibraryAudioSource(String),
    ToggleLibraryQueue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SettingsCommand {
    SettingsTab(SettingsTab),
    RefreshRuntimeStatus,
    OpenModelDownloads,
    CloseModelDownloads,
    OpenSettingsSelect(SettingsSelectKind),
    SelectSettingsValue(SettingsSelectKind, String),
    SetModelBackend(String, Option<String>),
    SetModelDevice(String, Option<String>),
    SetAnalysisQuality(app_core::AnalysisQualityProfile),
    TogglePreserveContinuousPitch,
    ToggleAnalysisQuantization,
    RequestSetup(Option<app_core::ModelDownloadTarget>),
    InstallAudioModel(String),
    RemoveAudioModel(String),
    CancelSetup,
    ConfirmSetup,
    ToggleTheme,
    AdjustUiFontScale(i8),
    ToggleAutoAnalyze,
    RestoreAnalysisDefaults,
    RequestClearCache(CacheClearScope),
    CancelClearCache,
    ConfirmClearCache,
    ChooseFusionAgentAdapter,
    ClearFusionAgentAdapter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisCommand {
    StartAnalysis(String),
    StartQueuedAnalysis(String),
    MergeSelectedCandidatePhrase(app_core::ArtifactRef, app_core::ArtifactRef),
    MergeSelectedCandidateRange(app_core::ArtifactRef, app_core::ArtifactRef),
    KeepAuthoredChart,
    SelectAnalysisHistory(Option<i64>),
    OpenSongAnalysis(String),
    OpenSongModelSelection(String),
    OpenProcessingStudio(String),
    AnalyzeNow(String),
    OpenEmptyProcessingStudio,
    SelectWorkflowNode(String),
    MoveWorkflowNode(String, bool),
    DuplicateWorkflowNode(String),
    RemoveWorkflowNode(String),
    SetWorkflowNodeModel(String, String),
    SetWorkflowSeparationStrategy(String, app_core::SeparationStrategyV1),
    AddWorkflowProcessor(String, String, String, Option<String>),
    AddOptionalWorkflowCard(String, String, app_core::OptionalWorkflowCardV1),
    SetWorkflowParameter(String, String, serde_json::Value),
    SetWorkflowPolicy(String, app_core::ExecutionPolicy),
    SetWorkflowSkipIfUnchanged(String, bool),
    AdjustWorkflowPriority(String, i32),
    RebindWorkflowAnalyzer(String, String, String),
    SaveWorkflow,
    PreviewWorkflow,
    RunWorkflow,
    OpenAnalysisInspect(String, String),
    #[allow(dead_code)] // Ctrl+wheel is the visible gesture; automation still uses this command
    AdjustAnalysisGraphZoom(i32),
    ToggleAnalysisMiniView,
    ToggleAnalysisModelPanel,
    CloseAnalysisModelPanel,
    FitAnalysisGraph(i32),
    DismissAnalysisNodeContext,
    RequestClearAnalysisHistory,
    CancelClearAnalysisHistory,
    ConfirmClearAnalysisHistory,
    CompareNodeAttemptWithPrevious(String, String, i64),
    ClosePlanPreview,
    QueueExactPreview,
    TogglePlanPreviewOutput(app_core::AnalysisOutputKind),
    ResetPlanPreviewOutputs,
    SetPlanPreviewQuality(app_core::AnalysisQualityProfile),
    ResetPlanPreviewQuality,
    OpenAnalysisLogViewer(String, String),
    CloseAnalysisLogViewer,
    RequestDeleteSongCache(String),
    CancelAnalysisRun(String),
    CancelDeleteSongCache,
    ConfirmDeleteSongCache,
    RequestDeleteAuthoredChart(String),
    CancelDeleteAuthoredChart,
    ConfirmDeleteAuthoredChart,
    RequestReplaceAuthoredChart(String),
    CancelReplaceAuthoredChart,
    ConfirmReplaceAuthoredChart,
    RequestRemoveSong(String),
    CancelRemoveSong,
    ConfirmRemoveSong,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorCommand {
    OpenLyricsEditor(String),
    CloseLyricsEditor,
    ToggleLyricsInputMode,
    SearchLrclibLyrics,
    ExtractLyrics,
    PreviousLrclibCandidate,
    NextLrclibCandidate,
    UseLrclibPlain,
    UseLrclibTimed,
    SaveLyricsEditor,
    SaveLyricsEditorAndRunDownstream,
    AdjustTranscriptBoundary(TranscriptBoundaryTarget, TranscriptBoundaryEdge, i32),
    PreviewTranscriptAt(String, i64),
    OpenLanguageEditor(String),
    CloseLanguageEditor,
    ToggleLanguageReprocess,
    ToggleLanguagePicker,
    SelectAnalysisLanguage(String),
    SaveLanguageEditor,
    OpenSongSettings(String),
    CloseSongSettings,
    ChooseBackgroundVideo,
    ClearBackgroundVideo,
    SaveSongSettings,
    ShiftSongKey(String, i8),
    ShiftSongTempo(String, i8),
    Editor(EditorAction),
    FocusChartProblem(usize, u64),
    OpenEditorSelect(EditorDockSelectKind),
    SelectEditorValue(EditorDockSelectKind, String),
    SelectEditorWord(usize, usize, u64),
    SelectEditorTrack(usize),
    MoveSelectionToTrack(usize),
    SetNoteKind(app_core::NoteKind),
    ToggleEditorFileMenu,
    DismissEditorFileMenu,
    SaveEditorAsUtz,
    SaveEditorAsUltraStar,
    ToggleEditorLayoutMenu,
    DismissEditorLayoutMenu,
    DismissLyricContext,
    DismissNoteContext,
    SelectWaveformSource(WaveformSource),
    SelectArtifactAudition(ArtifactAuditionSlot, app_core::ArtifactRef),
    ActivateArtifactAudition(ArtifactAuditionSlot),
    SelectArtifactWaveform(app_core::ArtifactRef),
    SelectWaveformStyle(WaveformStyle),
    DismissWaveformContext,
    ToggleEvidence(app_core::EvidenceKind),
    ReviewPrevious,
    ReviewNext,
    MarkReviewRegion,
    AcceptSuggestion(String),
    IgnoreSuggestion(String),
    SetProblemsFilter(ProblemsFilter),
    ApplyAllLyricsEdit,
    ExtendLyricOverNote(WordSelection, usize),
    DismissProblemsPanel,
    DismissShortcutsPanel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UiCommand {
    App(AppCommand),
    Library(LibraryCommand),
    Settings(SettingsCommand),
    Analysis(AnalysisCommand),
    Editor(EditorCommand),
}

impl UiCommand {
    pub(crate) const fn dirty_region(&self) -> UiDirtyRegion {
        match self {
            Self::App(_) => UiDirtyRegion::Chrome,
            Self::Library(
                LibraryCommand::SetLibraryView(_)
                | LibraryCommand::OpenSong(_)
                | LibraryCommand::ChooseEditorFile
                | LibraryCommand::OpenEditor(_),
            ) => UiDirtyRegion::Chrome,
            Self::Library(_) => UiDirtyRegion::Library,
            Self::Settings(_) => UiDirtyRegion::Settings,
            // This command changes the top-level route.  Rebuilding only the
            // analysis workspace leaves the top bar and the persistent
            // overlay tree on the old route for a frame.  Replacing the
            // whole route tree in one deferred command batch keeps Bevy
            // from submitting that mixed-generation tree to Wayland.
            Self::Analysis(
                AnalysisCommand::OpenAnalysisInspect(_, _)
                | AnalysisCommand::OpenSongAnalysis(_)
                | AnalysisCommand::OpenProcessingStudio(_)
                | AnalysisCommand::OpenEmptyProcessingStudio
                | AnalysisCommand::OpenSongModelSelection(_),
            ) => UiDirtyRegion::Chrome,
            Self::Analysis(
                AnalysisCommand::StartAnalysis(_)
                | AnalysisCommand::ClosePlanPreview
                | AnalysisCommand::QueueExactPreview
                | AnalysisCommand::TogglePlanPreviewOutput(_)
                | AnalysisCommand::ResetPlanPreviewOutputs
                | AnalysisCommand::SetPlanPreviewQuality(_)
                | AnalysisCommand::ResetPlanPreviewQuality
                | AnalysisCommand::OpenAnalysisLogViewer(_, _)
                | AnalysisCommand::CloseAnalysisLogViewer
                | AnalysisCommand::DismissAnalysisNodeContext
                | AnalysisCommand::RequestDeleteAuthoredChart(_)
                | AnalysisCommand::CancelDeleteAuthoredChart
                | AnalysisCommand::ConfirmDeleteAuthoredChart
                | AnalysisCommand::RequestReplaceAuthoredChart(_)
                | AnalysisCommand::CancelReplaceAuthoredChart
                | AnalysisCommand::ConfirmReplaceAuthoredChart
                | AnalysisCommand::RequestRemoveSong(_)
                | AnalysisCommand::CancelRemoveSong
                | AnalysisCommand::ConfirmRemoveSong,
            ) => UiDirtyRegion::Dialog,
            Self::Analysis(_) => UiDirtyRegion::Analysis,
            Self::Editor(
                EditorCommand::OpenSongSettings(_)
                | EditorCommand::CloseSongSettings
                | EditorCommand::ChooseBackgroundVideo
                | EditorCommand::ClearBackgroundVideo
                | EditorCommand::SaveSongSettings
                | EditorCommand::ToggleEditorFileMenu
                | EditorCommand::DismissEditorFileMenu
                | EditorCommand::ToggleEditorLayoutMenu
                | EditorCommand::DismissEditorLayoutMenu
                | EditorCommand::DismissLyricContext
                | EditorCommand::DismissNoteContext
                | EditorCommand::DismissWaveformContext,
            ) => UiDirtyRegion::Dialog,
            Self::Editor(
                EditorCommand::OpenLyricsEditor(_)
                | EditorCommand::CloseLyricsEditor
                | EditorCommand::ToggleLyricsInputMode
                | EditorCommand::SearchLrclibLyrics
                | EditorCommand::ExtractLyrics
                | EditorCommand::PreviousLrclibCandidate
                | EditorCommand::NextLrclibCandidate
                | EditorCommand::UseLrclibPlain
                | EditorCommand::UseLrclibTimed
                | EditorCommand::SaveLyricsEditor
                | EditorCommand::SaveLyricsEditorAndRunDownstream
                | EditorCommand::AdjustTranscriptBoundary(_, _, _)
                | EditorCommand::PreviewTranscriptAt(_, _)
                | EditorCommand::OpenLanguageEditor(_)
                | EditorCommand::CloseLanguageEditor
                | EditorCommand::ToggleLanguageReprocess
                | EditorCommand::ToggleLanguagePicker
                | EditorCommand::SelectAnalysisLanguage(_)
                | EditorCommand::SaveLanguageEditor
                | EditorCommand::ShiftSongKey(_, _)
                | EditorCommand::ShiftSongTempo(_, _),
            ) => UiDirtyRegion::Library,
            Self::Editor(_) => UiDirtyRegion::Editor,
        }
    }
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiAction(pub(crate) UiCommand);

macro_rules! impl_ui_action_from {
    ($command:ty, $variant:ident) => {
        impl From<$command> for UiAction {
            fn from(command: $command) -> Self {
                Self(UiCommand::$variant(command))
            }
        }
    };
}

impl_ui_action_from!(AppCommand, App);
impl_ui_action_from!(LibraryCommand, Library);
impl_ui_action_from!(SettingsCommand, Settings);
impl_ui_action_from!(AnalysisCommand, Analysis);
impl_ui_action_from!(EditorCommand, Editor);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_commands_map_to_scoped_dirty_regions() {
        assert_eq!(
            UiCommand::App(AppCommand::Home).dirty_region(),
            UiDirtyRegion::Chrome
        );
        assert_eq!(
            UiCommand::Library(LibraryCommand::LoadMoreSongs).dirty_region(),
            UiDirtyRegion::Library
        );
        for command in [
            LibraryCommand::SetLibraryView(LibraryView::Queue),
            LibraryCommand::OpenSong("song".to_string()),
            LibraryCommand::OpenEditor("song".to_string()),
        ] {
            assert_eq!(
                UiCommand::Library(command).dirty_region(),
                UiDirtyRegion::Chrome
            );
        }
        assert_eq!(
            UiCommand::Settings(SettingsCommand::RefreshRuntimeStatus).dirty_region(),
            UiDirtyRegion::Settings
        );
        assert_eq!(
            UiCommand::Analysis(AnalysisCommand::ToggleAnalysisMiniView).dirty_region(),
            UiDirtyRegion::Analysis
        );
        assert_eq!(
            UiCommand::Editor(EditorCommand::DismissLyricContext).dirty_region(),
            UiDirtyRegion::Dialog
        );
    }

    #[test]
    fn commands_that_only_change_persistent_overlays_are_dialog_scoped() {
        assert_eq!(
            UiCommand::Analysis(AnalysisCommand::DismissAnalysisNodeContext).dirty_region(),
            UiDirtyRegion::Dialog
        );
        for command in [
            AnalysisCommand::StartAnalysis("song".to_string()),
            AnalysisCommand::StartAnalysis("song".to_string()),
            AnalysisCommand::ClosePlanPreview,
            AnalysisCommand::QueueExactPreview,
            AnalysisCommand::TogglePlanPreviewOutput(app_core::AnalysisOutputKind::Transcript),
            AnalysisCommand::ResetPlanPreviewOutputs,
            AnalysisCommand::SetPlanPreviewQuality(app_core::AnalysisQualityProfile::Maximum),
            AnalysisCommand::ResetPlanPreviewQuality,
            AnalysisCommand::OpenAnalysisLogViewer("song".to_string(), "pitch.extract".to_string()),
            AnalysisCommand::CloseAnalysisLogViewer,
        ] {
            assert_eq!(
                UiCommand::Analysis(command).dirty_region(),
                UiDirtyRegion::Dialog
            );
        }
        assert_eq!(
            UiCommand::Editor(EditorCommand::CloseSongSettings).dirty_region(),
            UiDirtyRegion::Dialog
        );
    }

    #[test]
    fn opening_analysis_routes_rebuilds_the_route_chrome_as_one_tree() {
        for command in [
            AnalysisCommand::OpenAnalysisInspect("pitch.extract".to_string(), "pitch".to_string()),
            AnalysisCommand::OpenSongAnalysis("song".to_string()),
            AnalysisCommand::OpenProcessingStudio("song".to_string()),
            AnalysisCommand::OpenEmptyProcessingStudio,
            AnalysisCommand::OpenSongModelSelection("song".to_string()),
        ] {
            assert_eq!(
                UiCommand::Analysis(command).dirty_region(),
                UiDirtyRegion::Chrome
            );
        }
    }

    #[test]
    fn song_detail_editors_rebuild_the_workspace_instead_of_the_hidden_editor_region() {
        for command in [
            EditorCommand::OpenLyricsEditor("song-a".to_string()),
            EditorCommand::ToggleLyricsInputMode,
            EditorCommand::SaveLyricsEditor,
            EditorCommand::OpenLanguageEditor("song-a".to_string()),
            EditorCommand::SelectAnalysisLanguage("ja".to_string()),
            EditorCommand::SaveLanguageEditor,
            EditorCommand::ShiftSongKey("song-a".to_string(), 1),
            EditorCommand::ShiftSongTempo("song-a".to_string(), -1),
        ] {
            assert_eq!(
                UiCommand::Editor(command).dirty_region(),
                UiDirtyRegion::Library
            );
        }
    }
}
