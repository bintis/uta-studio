//! Typed chart problems.
//!
//! Editing deliberately allows a chart to be temporarily wrong: a drag passes
//! through an overlap, a syllable is briefly empty while it is retyped. The
//! format does not allow saving that, so problems are reported with a location
//! the editor can jump to rather than blocking the pointer.
//!
//! An [`Severity::Error`] problem is one the format rejects, so it blocks
//! saving. A warning is authoring advice.

use super::document::{EditorDocument, LyricAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Blocks saving: the chart would fail format validation.
    Error,
    /// Worth a look, but the chart is still valid.
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    OverlappingNotes,
    NoteTooShort,
    MissingPitchTarget,
    UnresolvedContinuation,
    EmptyLyric,
    ScorableNoteWithoutLyric,
    LyricWithoutPitch,
    LargeIntervalLeap,
    PhrasesTouch,
    UnusualGoldenShare,
}

impl ProblemKind {
    pub fn severity(self) -> Severity {
        match self {
            Self::OverlappingNotes
            | Self::MissingPitchTarget
            | Self::UnresolvedContinuation
            | Self::EmptyLyric => Severity::Error,
            Self::NoteTooShort
            | Self::ScorableNoteWithoutLyric
            | Self::LyricWithoutPitch
            | Self::LargeIntervalLeap
            | Self::PhrasesTouch
            | Self::UnusualGoldenShare => Severity::Warning,
        }
    }

    /// Whether the conservative automatic repair resolves this problem.
    pub fn auto_fixable(self) -> bool {
        matches!(self, Self::OverlappingNotes | Self::NoteTooShort)
    }
}

#[derive(Debug, Clone)]
pub struct ChartProblem {
    pub kind: ProblemKind,
    pub message: String,
    /// Which track the problem is on. Saving validates every track, so the
    /// panel must be able to point at one the user is not editing.
    pub track: usize,
    /// Where in the timeline to look, in seconds.
    pub time: f64,
    /// Flattened note index, when the problem belongs to one note.
    pub note: Option<usize>,
    pub lyric: Option<LyricAddress>,
}

impl ChartProblem {
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProblemReport {
    pub problems: Vec<ChartProblem>,
}

impl ProblemReport {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.severity() == severity)
            .count()
    }

    pub fn total(&self) -> usize {
        self.problems.len()
    }

    pub fn auto_fixable(&self) -> bool {
        self.problems
            .iter()
            .any(|problem| problem.kind.auto_fixable())
    }

    /// True when the chart cannot be saved as it stands.
    pub fn blocks_saving(&self) -> bool {
        self.errors() > 0
    }
}

/// A golden share outside this band usually means notes were marked by
/// accident rather than to reward a phrase.
const GOLDEN_SHARE_LIMIT: f64 = 0.5;
/// Two notes closer than this with a leap this wide is usually an octave error
/// in the analyzer output rather than an authored jump.
const LEAP_WINDOW_SECONDS: f64 = 0.25;
const LEAP_SEMITONES: f64 = 12.0;
const SHORT_NOTE_SECONDS: f64 = 0.06;

pub(crate) fn report(document: &EditorDocument) -> ProblemReport {
    let mut problems = Vec::new();
    for track in 0..document.track_count() {
        report_track(document, track, &mut problems);
    }
    problems.sort_by(|left, right| {
        left.severity()
            .cmp(&right.severity())
            .then_with(|| left.time.total_cmp(&right.time))
    });
    ProblemReport { problems }
}

fn report_track(document: &EditorDocument, track: usize, problems: &mut Vec<ChartProblem>) {
    let notes = document.track_notes(track);
    let lyrics = document.track_lyrics(track);

    for (index, note) in notes.iter().enumerate() {
        if note.end - note.start < SHORT_NOTE_SECONDS {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::NoteTooShort,
                message: format!(
                    "Note is only {} ms long",
                    ((note.end - note.start) * 1000.0).round() as i64
                ),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        }
        if !note.pitched && note.scores_pitch {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::MissingPitchTarget,
                message: "Pitch scoring needs a pitch target".into(),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        }
        if note.scores && note.lyric.is_none() && !note.continues_lyric {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::ScorableNoteWithoutLyric,
                message: "Scored note has no syllable to sing".into(),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        }

        let Some(previous) = index.checked_sub(1).and_then(|index| notes.get(index)) else {
            continue;
        };
        if note.start < previous.end - 0.001 {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::OverlappingNotes,
                message: "Notes overlap; the format needs one voice per track".into(),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        } else if note.phrase != previous.phrase && note.start < previous.end + f64::EPSILON {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::PhrasesTouch,
                message: "Lyric lines meet with no gap to read them".into(),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        }
        if note.pitched
            && previous.pitched
            && note.start - previous.end < LEAP_WINDOW_SECONDS
            && (note.midi - previous.midi).abs() > LEAP_SEMITONES
        {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::LargeIntervalLeap,
                message: format!(
                    "{} semitone leap with no room to breathe",
                    (note.midi - previous.midi).abs().round() as i64
                ),
                time: note.start,
                note: Some(note.index),
                lyric: None,
            });
        }
    }

    for lyric in &lyrics {
        if lyric.text.trim().is_empty() {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::EmptyLyric,
                message: "Syllable has no text".into(),
                time: lyric.start,
                note: Some(lyric.note),
                lyric: Some(lyric.address),
            });
        }
        if !lyric.guided {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::LyricWithoutPitch,
                message: format!("\"{}\" has no pitch to follow", lyric.text.trim()),
                time: lyric.start,
                note: Some(lyric.note),
                lyric: Some(lyric.address),
            });
        }
    }

    for (id, time) in document.unresolved_continuations(track) {
        problems.push(ChartProblem {
            track,
            kind: ProblemKind::UnresolvedContinuation,
            message: format!("Held syllable {id} has nothing to continue"),
            time,
            note: None,
            lyric: None,
        });
    }

    let golden = notes.iter().filter(|note| note.golden).count();
    if !notes.is_empty() {
        let share = golden as f64 / notes.len() as f64;
        if share > GOLDEN_SHARE_LIMIT {
            problems.push(ChartProblem {
                track,
                kind: ProblemKind::UnusualGoldenShare,
                message: format!(
                    "{}% of notes are golden, which stops rewarding anything",
                    (share * 100.0).round() as i64
                ),
                time: notes.first().map(|note| note.start).unwrap_or(0.0),
                note: None,
                lyric: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor::NoteKind, vocal_chart::migrate_analyzer_chart};
    use std::collections::BTreeSet;

    fn document(notes: &[(f64, f64, u8, &str)]) -> EditorDocument {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "start": notes.first().map(|note| note.0).unwrap_or(0.0),
                "end": notes.last().map(|note| note.1).unwrap_or(0.0),
                "text": notes.iter().map(|note| note.3).collect::<Vec<_>>().join(" "),
                "words": notes
                    .iter()
                    .map(|(start, end, _, text)| {
                        serde_json::json!({"word": text, "start": start, "end": end})
                    })
                    .collect::<Vec<_>>(),
            }]
        });
        let pitch_notes = serde_json::json!({
            "notes": notes
                .iter()
                .map(|(start, end, midi, _)| serde_json::json!({
                    "start": start, "end": end, "midi": midi, "confidence": 1.0,
                }))
                .collect::<Vec<_>>(),
        });
        EditorDocument::new(migrate_analyzer_chart(&transcript, &pitch_notes).unwrap())
    }

    fn kinds(report: &ProblemReport) -> Vec<ProblemKind> {
        report.problems.iter().map(|problem| problem.kind).collect()
    }

    #[test]
    fn a_clean_chart_reports_nothing_and_can_be_saved() {
        let document = document(&[(0.0, 0.9, 60, "one"), (1.0, 1.9, 62, "two")]);
        let report = document.problems();
        assert_eq!(report.total(), 0, "unexpected: {:?}", kinds(&report));
        assert!(!report.blocks_saving());
    }

    #[test]
    fn an_overlap_is_an_error_that_blocks_saving_and_repair_clears_it() {
        let mut document = document(&[(0.0, 1.0, 60, "one"), (1.0, 2.0, 62, "two")]);
        document.move_note(1, 0.5, 1.5, 62.0);
        let report = document.problems();
        assert!(kinds(&report).contains(&ProblemKind::OverlappingNotes));
        assert!(report.blocks_saving());
        assert!(report.auto_fixable());
        // The located problem points at the offending note.
        let overlap = report
            .problems
            .iter()
            .find(|problem| problem.kind == ProblemKind::OverlappingNotes)
            .unwrap();
        assert_eq!(overlap.note, Some(1));

        document.repair();
        assert!(!document.problems().blocks_saving());
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn an_empty_syllable_blocks_saving_and_points_at_its_lyric() {
        let mut document = document(&[(0.0, 1.0, 60, "one")]);
        let address = LyricAddress {
            segment: 0,
            word: 0,
        };
        document.set_lyric_text(address, "  ");
        let report = document.problems();
        let empty = report
            .problems
            .iter()
            .find(|problem| problem.kind == ProblemKind::EmptyLyric)
            .expect("an empty syllable is an error");
        assert_eq!(empty.lyric, Some(address));
        assert!(report.blocks_saving());
    }

    #[test]
    fn a_lyric_without_pitch_is_advice_rather_than_a_blocker() {
        let mut document = document(&[(0.0, 1.0, 60, "one")]);
        document.insert_lyric(None, 5.0).unwrap();
        let report = document.problems();
        assert!(kinds(&report).contains(&ProblemKind::LyricWithoutPitch));
        assert!(!report.blocks_saving());
    }

    #[test]
    fn a_very_short_note_is_a_repairable_warning() {
        let mut document = document(&[(0.0, 1.0, 60, "one")]);
        document.resize_note(0, 0.0, 0.04);
        let report = document.problems();
        assert!(kinds(&report).contains(&ProblemKind::NoteTooShort));
        assert!(!report.blocks_saving());
        assert!(report.auto_fixable());
    }

    #[test]
    fn an_octave_sized_leap_between_touching_notes_is_flagged() {
        let document = document(&[(0.0, 0.9, 48, "low"), (1.0, 1.9, 72, "high")]);
        assert!(kinds(&document.problems()).contains(&ProblemKind::LargeIntervalLeap));
    }

    #[test]
    fn marking_most_notes_golden_is_flagged_as_pointless() {
        let mut document = document(&[
            (0.0, 0.9, 60, "one"),
            (1.0, 1.9, 62, "two"),
            (2.0, 2.9, 64, "three"),
        ]);
        document.set_note_kind(&BTreeSet::from([0, 1, 2]), NoteKind::Golden);
        assert!(kinds(&document.problems()).contains(&ProblemKind::UnusualGoldenShare));
        // Still a valid chart, just questionable authoring.
        assert!(!document.problems().blocks_saving());
    }

    #[test]
    fn errors_sort_ahead_of_warnings() {
        let mut document = document(&[(0.0, 1.0, 60, "one"), (1.0, 2.0, 62, "two")]);
        document.resize_note(0, 0.0, 0.04);
        document.move_note(1, 0.02, 1.5, 62.0);
        let report = document.problems();
        let severities = report
            .problems
            .iter()
            .map(|problem| problem.severity())
            .collect::<Vec<_>>();
        assert!(severities.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
