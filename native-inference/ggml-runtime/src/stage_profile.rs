use std::time::{Duration, Instant};

/// Opt-in per-stage timing for the chunked audio graphs.
///
/// Enabled with `UTA_STUDIO_STAGE_PROFILE=1`. It answers a question the Vulkan
/// performance logger cannot: how a chunk's wall time divides between the host
/// frontend, the transfers, and the graph itself. While disabled every call is
/// a branch on one cached flag and nothing is recorded or allocated.
pub(crate) struct StageProfile {
    label: &'static str,
    enabled: bool,
    stages: Vec<(&'static str, Duration, u64)>,
    chunks: u64,
    started: Option<Instant>,
}

pub(crate) const STAGE_PROFILE_ENV: &str = "UTA_STUDIO_STAGE_PROFILE";

impl StageProfile {
    pub fn new(label: &'static str) -> Self {
        let enabled = std::env::var(STAGE_PROFILE_ENV)
            .map(|value| enabling_value(&value))
            .unwrap_or(false);
        Self {
            label,
            enabled,
            stages: Vec::new(),
            chunks: 0,
            started: enabled.then(Instant::now),
        }
    }

    /// Start of a stage, or `None` while profiling is off.
    pub fn mark(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub fn record(&mut self, stage: &'static str, since: Option<Instant>) {
        let Some(since) = since else {
            return;
        };
        let elapsed = since.elapsed();
        match self.stages.iter_mut().find(|(name, _, _)| *name == stage) {
            Some(entry) => {
                entry.1 += elapsed;
                entry.2 += 1;
            }
            None => self.stages.push((stage, elapsed, 1)),
        }
    }

    pub fn chunk_done(&mut self) {
        if self.enabled {
            self.chunks += 1;
        }
    }

    /// Writes the accumulated split to stderr and clears it, so a model that
    /// runs several passes reports each pass separately.
    pub fn report(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        let wall = started.elapsed().as_secs_f64();
        let measured = self
            .stages
            .iter()
            .map(|(_, duration, _)| duration.as_secs_f64())
            .sum::<f64>();
        eprintln!(
            "[{}] stage profile: {:.3} s wall over {} chunk(s)",
            self.label, wall, self.chunks
        );
        for (stage, duration, calls) in &self.stages {
            let seconds = duration.as_secs_f64();
            eprintln!(
                "[{}]   {:<14} {:>8.3} s  {:>5.1}%  {} call(s)",
                self.label,
                stage,
                seconds,
                if wall > 0.0 {
                    seconds / wall * 100.0
                } else {
                    0.0
                },
                calls
            );
        }
        eprintln!(
            "[{}]   {:<14} {:>8.3} s  {:>5.1}%",
            self.label,
            "unattributed",
            wall - measured,
            if wall > 0.0 {
                (wall - measured) / wall * 100.0
            } else {
                0.0
            }
        );
        self.stages.clear();
        self.chunks = 0;
        self.started = Some(Instant::now());
    }
}

fn enabling_value(value: &str) -> bool {
    !matches!(value.trim(), "" | "0" | "false" | "off" | "no")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_profile_records_nothing() {
        let mut profile = StageProfile {
            label: "test",
            enabled: false,
            stages: Vec::new(),
            chunks: 0,
            started: None,
        };
        let mark = profile.mark();
        assert!(mark.is_none());
        profile.record("stft", mark);
        profile.chunk_done();
        assert!(profile.stages.is_empty());
        assert_eq!(profile.chunks, 0);
    }

    #[test]
    fn repeated_stages_accumulate_under_one_name_in_first_seen_order() {
        let mut profile = StageProfile {
            label: "test",
            enabled: true,
            stages: Vec::new(),
            chunks: 0,
            started: Some(Instant::now()),
        };
        profile.record("stft", Some(Instant::now()));
        profile.record("compute", Some(Instant::now()));
        profile.record("stft", Some(Instant::now()));
        let names = profile
            .stages
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["stft", "compute"]);
        assert_eq!(profile.stages[0].2, 2);
        assert_eq!(profile.stages[1].2, 1);
    }

    #[test]
    fn only_an_affirmative_environment_value_enables_profiling() {
        assert!(enabling_value("1"));
        assert!(enabling_value("yes"));
        assert!(!enabling_value("0"));
        assert!(!enabling_value("false"));
        assert!(!enabling_value(" "));
    }
}
