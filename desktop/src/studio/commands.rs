use std::path::PathBuf;

use bevy::prelude::Component;

use super::{
    AnalysisAdvancedSection, ArtifactInspectorTab, CacheClearScope, EditorAction,
    EditorDockSelectKind, LibraryFacet, LibrarySelectKind, LibraryView, LineageScope,
    ProblemsFilter, SettingsSelectKind, SettingsTab, TranscriptBoundaryEdge,
    TranscriptBoundaryTarget, UiDirtyRegion, WaveformSource, WaveformStyle, WordSelection,
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
    OpenEditor(String),
    ExportUtz(String),
    ExportUltraStar(String),
    OpenSource(PathBuf),
    RevealSource(PathBuf),
    DismissSongContext,
    PlayLibrarySong(String),
    PlayArtifactRevision(PathBuf),
    ToggleLibraryPlayback,
    SeekLibraryRelative(i8),
    PreviousLibrarySong,
    NextLibrarySong,
    ToggleLibraryShuffle,
    CycleLibraryRepeat,
    AdjustLibraryVolume(i8),
    ToggleLibraryMute,
    ToggleLibraryQueue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SettingsCommand {
    SettingsTab(SettingsTab),
    RefreshRuntimeStatus,
    OpenSettingsSelect(SettingsSelectKind),
    SelectSettingsValue(SettingsSelectKind, String),
    ToggleAnalysisAdvanced(AnalysisAdvancedSection),
    RequestSetup(Option<app_core::ModelDownloadTarget>),
    InstallAudioModel(String),
    RemoveAudioModel(String),
    CancelSetup,
    ConfirmSetup,
    ToggleTheme,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisCommand {
    SelectArtifactInspectorTab(ArtifactInspectorTab),
    ToggleArtifactPinned(app_core::ArtifactRef),
    OpenArtifactCompatibleEditor(app_core::ArtifactRef),
    MergeCandidateChart(
        app_core::ArtifactRef,
        app_core::ArtifactRef,
        app_core::ChartRevisionMergeMode,
    ),
    MergeSelectedCandidatePhrase(app_core::ArtifactRef, app_core::ArtifactRef),
    MergeSelectedCandidateRange(app_core::ArtifactRef, app_core::ArtifactRef),
    KeepAuthoredChart,
    ShowArtifactLineage(app_core::ArtifactRef),
    ShowArtifactImpact(app_core::ArtifactRef),
    SetArtifactLineageScope(LineageScope),
    SelectArtifactLineageRevision(app_core::ArtifactRef),
    CloseArtifactLineage,
    CloseArtifactImpact,
    ConfirmArtifactImpact,
    DismissAnalysisArtifactContext,
    ToggleAnalysisLineageMode,
    DismissAnalysisExportContext,
    ValidateExportNode(String, app_core::ExportPackageKind),
    RevealLastExport(String, app_core::ExportPackageKind),
    SelectAnalysisHistory(Option<i64>),
    OpenSongAnalysis(String),
    OpenAnalysisInspect(String),
    AdjustAnalysisGraphZoom(i32),
    ToggleAnalysisMiniView,
    FitAnalysisGraph(i32),
    FocusAnalysisGraphNode(i32, String),
    DismissAnalysisNodeContext,
    RequestClearAnalysisHistory,
    CancelClearAnalysisHistory,
    ConfirmClearAnalysisHistory,
    RealignSong(String),
    ReanalyzeTranscript(String),
    ForceTranscribe(String),
    ReanalyzePitch(String),
    ReanalyzeFull(String),
    RunAnalysisNodeOnly(String, String),
    RunAnalysisNodeDownstream(String, String),
    DisableAnalysisNodeForRun(String, String),
    FreezeAnalysisNodeOutputs(String, String),
    BypassAnalysisNodeWithOriginalMix(String, String),
    CompareNodeAttemptWithPrevious(String, String, i64),
    SaveNodeConfigAsSongProfile(String, String),
    OpenNodeConfigDialog(String, String),
    CloseNodeConfigDialog,
    ToggleNodeConfigPicker,
    SelectNodeConfigValue(String),
    RunNodeConfigDialog,
    OpenPlanPreview(String),
    ClosePlanPreview,
    TogglePlanPreviewDisabledNode(String),
    RunPlanPreviewDraft,
    OpenAppLogViewer(String, String),
    CloseAppLogViewer,
    OpenAppLogFile,
    ToggleAnalysisCompoundNode(String),
    RequestDeleteSongCache(String),
    CancelAnalysisRun(String),
    CancelDeleteSongCache,
    ConfirmDeleteSongCache,
    RequestReplaceAuthoredChart(String),
    CancelReplaceAuthoredChart,
    ConfirmReplaceAuthoredChart,
    SyncArtifactRevisions(String),
    SetActiveArtifactRevision(app_core::ArtifactRevision),
    CancelSetActiveArtifactRevision,
    ConfirmSetActiveArtifactRevision,
    RequestCaptureIntermediate(String),
    CancelCaptureIntermediate,
    ConfirmCaptureIntermediateOnce,
    ConfirmCaptureIntermediatePersistent,
    ConfirmDisableIntermediateCapture,
    OpenArtifactRevision(PathBuf),
    PreviewArtifactRevision(PathBuf),
    RevealArtifactRevision(PathBuf),
    RequestDeleteArtifactRevision(app_core::ArtifactRevision),
    CancelDeleteArtifactRevision,
    ConfirmDeleteArtifactRevision,
    RequestInvalidateArtifactRevision(app_core::ArtifactRevision),
    CancelInvalidateArtifactRevision,
    ConfirmInvalidateArtifactRevision,
    InspectArtifactProvenance(app_core::ArtifactRevision),
    CompareArtifactRevisions(app_core::ArtifactRevision, app_core::ArtifactRef),
    CloseArtifactDiff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorCommand {
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
    DismissLyricContext,
    DismissNoteContext,
    SelectWaveformSource(WaveformSource),
    SelectWaveformStyle(WaveformStyle),
    DismissWaveformContext,
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
            Self::Library(_) => UiDirtyRegion::Library,
            Self::Settings(_) => UiDirtyRegion::Settings,
            Self::Analysis(
                AnalysisCommand::DismissAnalysisNodeContext
                | AnalysisCommand::ShowArtifactLineage(_)
                | AnalysisCommand::SetArtifactLineageScope(_)
                | AnalysisCommand::SelectArtifactLineageRevision(_)
                | AnalysisCommand::CloseArtifactLineage
                | AnalysisCommand::ShowArtifactImpact(_)
                | AnalysisCommand::CloseArtifactImpact
                | AnalysisCommand::ConfirmArtifactImpact
                | AnalysisCommand::RequestReplaceAuthoredChart(_)
                | AnalysisCommand::CancelReplaceAuthoredChart
                | AnalysisCommand::ConfirmReplaceAuthoredChart
                | AnalysisCommand::SetActiveArtifactRevision(_)
                | AnalysisCommand::CancelSetActiveArtifactRevision
                | AnalysisCommand::ConfirmSetActiveArtifactRevision
                | AnalysisCommand::RequestCaptureIntermediate(_)
                | AnalysisCommand::CancelCaptureIntermediate
                | AnalysisCommand::ConfirmCaptureIntermediateOnce
                | AnalysisCommand::ConfirmCaptureIntermediatePersistent
                | AnalysisCommand::ConfirmDisableIntermediateCapture
                | AnalysisCommand::RequestDeleteArtifactRevision(_)
                | AnalysisCommand::CancelDeleteArtifactRevision
                | AnalysisCommand::ConfirmDeleteArtifactRevision
                | AnalysisCommand::RequestInvalidateArtifactRevision(_)
                | AnalysisCommand::CancelInvalidateArtifactRevision
                | AnalysisCommand::ConfirmInvalidateArtifactRevision
                | AnalysisCommand::CompareArtifactRevisions(_, _)
                | AnalysisCommand::CloseArtifactDiff,
            ) => UiDirtyRegion::Dialog,
            Self::Analysis(_) => UiDirtyRegion::Analysis,
            Self::Editor(
                EditorCommand::OpenSongSettings(_)
                | EditorCommand::CloseSongSettings
                | EditorCommand::ChooseBackgroundVideo
                | EditorCommand::ClearBackgroundVideo
                | EditorCommand::SaveSongSettings,
            ) => UiDirtyRegion::Dialog,
            Self::Editor(
                EditorCommand::OpenLyricsEditor(_)
                | EditorCommand::CloseLyricsEditor
                | EditorCommand::ToggleLyricsInputMode
                | EditorCommand::ToggleLyricsSeparateStems
                | EditorCommand::SearchLrclibLyrics
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
            UiDirtyRegion::Editor
        );
    }

    #[test]
    fn commands_that_only_change_persistent_overlays_are_dialog_scoped() {
        assert_eq!(
            UiCommand::Analysis(AnalysisCommand::CancelDeleteArtifactRevision).dirty_region(),
            UiDirtyRegion::Dialog
        );
        assert_eq!(
            UiCommand::Analysis(AnalysisCommand::CloseArtifactDiff).dirty_region(),
            UiDirtyRegion::Dialog
        );
        assert_eq!(
            UiCommand::Analysis(AnalysisCommand::DismissAnalysisNodeContext).dirty_region(),
            UiDirtyRegion::Dialog
        );
        assert_eq!(
            UiCommand::Editor(EditorCommand::CloseSongSettings).dirty_region(),
            UiDirtyRegion::Dialog
        );
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
