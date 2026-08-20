use super::*;
mod last_successful_run_tests {
    //! §8.2 Overview's "Last successful run" row -- previously recorded as
    //! blocked on a not-yet-built Phase 3 history writer, which was stale:
    //! `analysis_history` already carried everything this needs.
    use super::last_successful_run_copy;
    use app_core::{AnalysisProgressSnapshot, AnalysisRunHistory};

    fn run(file_hash: &str, status: &str, finished_at_ms: i64) -> AnalysisRunHistory {
        AnalysisRunHistory {
            id: 1,
            file_hash: file_hash.to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            status: status.to_string(),
            started_at_ms: finished_at_ms - 1000,
            finished_at_ms,
            error_message: None,
            snapshot: AnalysisProgressSnapshot {
                stage: "complete".to_string(),
                stage_progress: 100,
                operation: String::new(),
                detail: String::new(),
                implementation: String::new(),
                model: String::new(),
                device: String::new(),
                requested_device: String::new(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                stage_routes: Vec::new(),
                node_id: None,
                node_event: None,
                artifact_reused_reason: None,
            },
        }
    }

    #[test]
    fn finds_the_most_recent_completed_run_for_this_song() {
        let history = vec![
            run("songA", "completed", 2_000),
            run("songB", "completed", 5_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(2_000)
        );
    }

    #[test]
    fn ignores_a_failed_run_and_falls_back_to_an_earlier_success() {
        let history = vec![
            run("songA", "failed", 9_000),
            run("songA", "completed", 3_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(3_000)
        );
    }

    #[test]
    fn a_newest_first_ordered_list_returns_the_first_match_not_just_any_match() {
        let history = vec![
            run("songA", "completed", 9_000),
            run("songA", "completed", 1_000),
        ];
        assert_eq!(
            last_successful_run_copy(&history, "songA"),
            super::format_epoch_ms(9_000)
        );
    }

    #[test]
    fn a_song_with_no_completed_run_shows_none_yet() {
        let history = vec![run("songA", "failed", 1_000)];
        assert_eq!(last_successful_run_copy(&history, "songA"), "None yet");
    }

    #[test]
    fn a_different_songs_completed_run_is_not_matched() {
        let history = vec![run("songB", "completed", 1_000)];
        assert_eq!(last_successful_run_copy(&history, "songA"), "None yet");
    }
}

#[cfg(test)]
mod candidate_availability_copy_tests {
    //! Phase 5 §5.5 "New candidate analysis is available" -- the Overview
    //! panel's "Candidate availability" row.
    use super::candidate_availability_copy;
    use app_core::{CandidateChartStatus, CandidateChartSummary};

    #[test]
    fn not_authored_yet_omits_the_row_entirely() {
        assert_eq!(
            candidate_availability_copy(&CandidateChartStatus::NotAuthoredYet),
            None
        );
    }

    #[test]
    fn up_to_date_reports_up_to_date() {
        assert_eq!(
            candidate_availability_copy(&CandidateChartStatus::UpToDate),
            Some("Up to date".to_string())
        );
    }

    #[test]
    fn candidate_available_names_what_changed_and_the_note_counts() {
        let copy = candidate_availability_copy(&CandidateChartStatus::CandidateAvailable(
            CandidateChartSummary {
                authored_phrase_count: 2,
                authored_note_count: 10,
                candidate_phrase_count: 3,
                candidate_note_count: 14,
                lyrics_changed: true,
                pitch_evidence_changed: true,
            },
        ))
        .unwrap();
        assert!(copy.contains("lyrics"));
        assert!(copy.contains("pitch"));
        assert!(copy.contains("14 notes"));
        assert!(copy.contains("10 authored"));
    }

    #[test]
    fn candidate_available_only_names_the_input_that_actually_changed() {
        let copy = candidate_availability_copy(&CandidateChartStatus::CandidateAvailable(
            CandidateChartSummary {
                authored_phrase_count: 1,
                authored_note_count: 5,
                candidate_phrase_count: 1,
                candidate_note_count: 5,
                lyrics_changed: true,
                pitch_evidence_changed: false,
            },
        ))
        .unwrap();
        assert!(copy.contains("lyrics"));
        assert!(!copy.contains("pitch"));
    }
}

#[cfg(test)]
mod chart_issue_count_copy_tests {
    //! Phase 8 "Chart issue count" -- the Overview panel's "Chart issues"
    //! row.
    use super::chart_issue_count_copy;

    #[test]
    fn no_data_omits_the_row_entirely() {
        assert_eq!(chart_issue_count_copy(None), None);
    }

    #[test]
    fn zero_problems_reports_none() {
        assert_eq!(chart_issue_count_copy(Some(0)), Some("None".to_string()));
    }

    #[test]
    fn one_problem_uses_the_singular() {
        assert_eq!(chart_issue_count_copy(Some(1)), Some("1 issue".to_string()));
    }

    #[test]
    fn multiple_problems_use_the_plural() {
        assert_eq!(
            chart_issue_count_copy(Some(4)),
            Some("4 issues".to_string())
        );
    }
}

#[cfg(test)]
mod music_analysis_row_copy_tests {
    //! §9.2 Music Analysis acceptance: "Unknown Key shows as Warning, not
    //! Failure" / "BPM-only fallback correctly displayed" / "Descriptors
    //! unavailable shows Not Applicable" -- the Overview panel's "Detected
    //! key" / "Musical BPM" / "Extra descriptors" rows.
    use super::{detected_key_copy, extra_descriptors_copy, musical_bpm_copy};
    use app_core::{MusicAnalysisDescriptors, MusicKeyAnalysis, MusicRhythmAnalysis};

    #[test]
    fn unknown_key_reads_as_plain_unknown_never_a_failure() {
        let copy = detected_key_copy(&MusicKeyAnalysis {
            tonic: None,
            scale: None,
            confidence: 0.0,
        });
        assert!(copy.starts_with("Unknown"));
        assert!(!copy.to_lowercase().contains("fail"));
    }

    #[test]
    fn a_detected_key_names_tonic_and_scale() {
        let copy = detected_key_copy(&MusicKeyAnalysis {
            tonic: Some("F#".to_string()),
            scale: Some("minor".to_string()),
            confidence: 0.8,
        });
        assert_eq!(copy, "F# minor (confidence 0.80)");
    }

    #[test]
    fn no_bpm_is_unavailable() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: None,
            confidence: 0.0,
            beats: vec![],
        });
        assert_eq!(copy, "Unavailable");
    }

    #[test]
    fn bpm_with_no_beats_is_named_as_the_fallback_explicitly() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: Some(120.0),
            confidence: 0.5,
            beats: vec![],
        });
        assert!(copy.contains("BPM-only"));
        assert!(!copy.contains("0 beats"));
    }

    #[test]
    fn bpm_with_a_full_beat_grid_counts_the_beats() {
        let copy = musical_bpm_copy(&MusicRhythmAnalysis {
            bpm: Some(120.0),
            confidence: 0.9,
            beats: vec![0.5, 1.0, 1.5],
        });
        assert!(copy.contains("3 beats"));
        assert!(!copy.contains("BPM-only"));
    }

    #[test]
    fn missing_descriptors_is_not_applicable() {
        assert_eq!(extra_descriptors_copy(None), "Not Applicable");
    }

    #[test]
    fn present_descriptors_are_formatted() {
        let descriptors = MusicAnalysisDescriptors {
            danceability: 0.72,
            dynamic_complexity_db: 8.3,
            loudness_db: -12.4,
        };
        let copy = extra_descriptors_copy(Some(&descriptors));
        assert!(copy.contains("0.72"));
        assert!(copy.contains("8.3"));
        assert!(copy.contains("-12.4"));
    }
}

#[cfg(test)]
mod view_song_analysis_tests {
    use super::{completed_analysis_run_id, view_song_analysis_action};
    use crate::studio::{AnalysisCommand, UiAction};
    use app_core::{AnalysisProgressSnapshot, AnalysisRunHistory};

    fn run(id: i64, file_hash: &str, status: &str) -> AnalysisRunHistory {
        AnalysisRunHistory {
            id,
            file_hash: file_hash.to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            status: status.to_string(),
            started_at_ms: id * 100,
            finished_at_ms: id * 100 + 50,
            error_message: None,
            snapshot: AnalysisProgressSnapshot {
                stage: "complete".to_string(),
                stage_progress: 100,
                operation: String::new(),
                detail: String::new(),
                implementation: String::new(),
                model: String::new(),
                device: String::new(),
                requested_device: String::new(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                stage_routes: Vec::new(),
                node_id: None,
                node_event: None,
                artifact_reused_reason: None,
            },
        }
    }

    #[test]
    fn song_detail_analysis_button_targets_the_clicked_song() {
        assert_eq!(
            view_song_analysis_action("song-a"),
            UiAction::from(AnalysisCommand::OpenSongAnalysis("song-a".to_string()))
        );
    }

    #[test]
    fn clicked_song_selects_its_newest_completed_analysis() {
        let history = vec![
            run(3, "song-a", "failed"),
            run(2, "song-b", "completed"),
            run(1, "song-a", "completed"),
        ];

        assert_eq!(completed_analysis_run_id(&history, "song-a"), Some(1));
        assert_eq!(completed_analysis_run_id(&history, "song-b"), Some(2));
    }

    #[test]
    fn clicked_song_does_not_open_an_unrelated_or_failed_analysis() {
        let history = vec![
            run(3, "song-a", "failed"),
            run(2, "song-b", "completed"),
        ];

        assert_eq!(completed_analysis_run_id(&history, "song-a"), None);
        assert_eq!(completed_analysis_run_id(&history, "missing"), None);
    }
}
