//! Explicit pitch activity in the covering candidate graph.

use serde::{Deserialize, Serialize};

/// A covering state can represent observed absence of pitched singing without
/// inventing a MIDI target. Continuous F0 remains an independent raw curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateTarget {
    Pitched { midi: u8, center_hz: f32 },
    Unpitched,
}

impl CandidateTarget {
    pub fn as_pitched(self) -> Option<(u8, f32)> {
        match self {
            Self::Pitched { midi, center_hz } => Some((midi, center_hz)),
            Self::Unpitched => None,
        }
    }

    pub fn midi(self) -> Option<u8> {
        self.as_pitched().map(|(midi, _)| midi)
    }

    pub fn center_hz(self) -> Option<f32> {
        self.as_pitched().map(|(_, center_hz)| center_hz)
    }

    pub fn is_pitched(self) -> bool {
        matches!(self, Self::Pitched { .. })
    }
}
