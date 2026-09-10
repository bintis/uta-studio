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
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidence, AcousticEvidenceFrame,
};
pub use advanced_notes::{
    AdvancedNoteEvidence, AdvancedRawGlobalStyle, AdvancedRawNote, AdvancedRawStyleHead,
    AdvancedRawTechnique, DependencyIdentity, DependencyKind, GlobalStyleInterval,
    TechniqueEvidence, TechniqueInterval, parse_advanced_note_evidence,
};
pub use alignment::{AlignmentArtifact, AlignmentItem, parse_alignment_artifact};
pub use basic_pitch::{BasicPitchEvidence, BasicPitchFrame, parse_basic_pitch_evidence};
pub use firered::parse_firered_transcript;
pub use game::{GameEvidence, GameNoteEvidence, parse_game_evidence};
pub use io::{artifact_ref_for_existing, write_json_artifact};
pub use jbm555::{
    JBM555_DECODE_PROFILE, JBM555_FRONTEND_PROFILE, JBM555_MODEL_ID, JBM555_OFFSET_THRESHOLD,
    JBM555_ONSET_THRESHOLD, Jbm555Evidence, Jbm555ExpectedInputs, Jbm555NoteEvidence,
    parse_jbm555_evidence,
};
pub use pitch::{PitchEvidence, parse_fcpe_pitch, parse_rmvpe_pitch};
pub use singing_analysis::{
    SINGING_ANALYSIS_CONTRACT, SINGING_ANALYSIS_FORMAT_VERSION, SINGING_ANALYSIS_VERSION,
    SingingAnalysis, SingingAnalysisChartReferences, SingingAnalysisProvenance,
};
pub use timed_notes::{
    TIMED_NOTE_EVIDENCE_CONTRACT, TIMED_NOTE_EVIDENCE_VERSION, TimedNoteExpertEvidence,
    TimedNoteHypothesis,
};
pub use transcript::{
    TranscriptArtifact, TranscriptAuthority, TranscriptToken, parse_transcript_artifact,
};
pub use vocal_chart::{CandidateVocalChart, finalize_candidate_vocal_chart};
