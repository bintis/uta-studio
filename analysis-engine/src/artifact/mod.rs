mod acoustic;
mod advanced_notes;
mod alignment;
mod basic_pitch;
mod firered;
mod game;
mod io;
mod jbm555;
mod pitch;
mod singing_analysis;
mod timed_notes;
mod transcript;
mod vocal_chart;

pub use acoustic::{
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
    AcousticEvidenceV1,
};
pub use advanced_notes::{
    AdvancedNoteEvidenceV1, AdvancedRawGlobalStyleV1, AdvancedRawNoteV1, AdvancedRawStyleHeadV1,
    AdvancedRawTechniqueV1, DependencyIdentity, DependencyKind, GlobalStyleIntervalV1,
    TechniqueEvidenceV1, TechniqueIntervalV1, parse_advanced_note_evidence,
};
pub use alignment::{
    AlignmentArtifactV1, AlignmentItemV1, parse_qwen_alignment, qwen_alignment_uses_coarse_fallback,
};
pub use basic_pitch::{BasicPitchEvidenceV3, BasicPitchFrameV3, parse_basic_pitch_evidence};
pub use firered::parse_firered_transcript;
pub use game::{GameEvidenceV1, GameNoteEvidenceV1, parse_game_evidence};
pub use io::{artifact_ref_for_existing, write_json_artifact};
pub use jbm555::{
    JBM555_DECODE_PROFILE, JBM555_FRONTEND_PROFILE, JBM555_MODEL_ID, JBM555_OFFSET_THRESHOLD,
    JBM555_ONSET_THRESHOLD, Jbm555EvidenceV1, Jbm555ExpectedInputsV1, Jbm555NoteEvidenceV1,
    parse_jbm555_evidence,
};
pub use pitch::{PitchEvidenceV03, parse_fcpe_pitch, parse_rmvpe_pitch};
pub use singing_analysis::{
    SINGING_ANALYSIS_CONTRACT, SINGING_ANALYSIS_FORMAT_VERSION, SINGING_ANALYSIS_VERSION,
    SingingAnalysisChartReferencesV1, SingingAnalysisProvenanceV1, SingingAnalysisV1,
};
pub use timed_notes::{
    TIMED_NOTE_EVIDENCE_CONTRACT, TIMED_NOTE_EVIDENCE_VERSION, TimedNoteExpertEvidenceV1,
    TimedNoteHypothesisV1,
};
pub use transcript::{
    TranscriptArtifactV1, TranscriptAuthorityV1, TranscriptTokenV1, parse_qwen_transcript,
};
pub use vocal_chart::{CandidateVocalChartV1, finalize_candidate_vocal_chart};
