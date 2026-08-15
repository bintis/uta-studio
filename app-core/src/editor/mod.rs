//! UI-agnostic editing model for the UTZ 0.2 vocal chart.
//!
//! The editor edits [`utz::VocalChartV1`] directly. Analyzer-era transcript and
//! pitch-note JSON stay derived projections for export and compatibility, so an
//! edit never round trips through a lossy re-migration.
//!
//! Chart positions are integer timebase units, as the format requires. Seconds
//! appear only at the rendering and audio-seek boundary.

mod document;
mod problems;

pub use document::{
    ChartLyric, ChartNote, ClipboardNote, EditorDocument, LyricAddress, MIN_NOTE_SECONDS, NoteKind,
};
pub use problems::{ChartProblem, ProblemKind, ProblemReport, Severity};

pub(crate) fn seconds_to_units(seconds: f64, timebase: u64) -> u64 {
    if !seconds.is_finite() {
        return 0;
    }
    (seconds.max(0.0) * timebase as f64).round() as u64
}

pub(crate) fn units_to_seconds(units: u64, timebase: u64) -> f64 {
    units as f64 / timebase.max(1) as f64
}

/// Rounds to whole milliseconds, matching the precision the analyzer produced
/// and the UltraStar exporter expects.
pub(crate) fn round_units_to_millis(units: u64, timebase: u64) -> u64 {
    let step = (timebase / 1_000).max(1);
    ((units + step / 2) / step) * step
}

#[cfg(test)]
mod timebase_tests {
    use super::*;
    use utz::DEFAULT_TIMEBASE;

    #[test]
    fn milliseconds_round_trip_without_drift() {
        for millis in [0u64, 1, 33, 1_500, 240_000] {
            let seconds = millis as f64 / 1_000.0;
            let units = seconds_to_units(seconds, DEFAULT_TIMEBASE);
            assert_eq!(units, millis * (DEFAULT_TIMEBASE / 1_000));
            assert_eq!(units_to_seconds(units, DEFAULT_TIMEBASE), seconds);
        }
    }

    #[test]
    fn rounding_snaps_to_the_nearest_millisecond() {
        assert_eq!(round_units_to_millis(1_499, DEFAULT_TIMEBASE), 1_000);
        assert_eq!(round_units_to_millis(1_500, DEFAULT_TIMEBASE), 2_000);
        assert_eq!(round_units_to_millis(0, DEFAULT_TIMEBASE), 0);
    }
}
