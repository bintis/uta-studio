use super::*;

fn primary(segments: impl IntoIterator<Item = (u64, u64, f32)>) -> BoundaryEvidenceSet {
    BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: segments
            .into_iter()
            .map(|(start, end, midi)| BoundarySegmentEvidence {
                range: TimeRange::new(start, end).unwrap(),
                fractional_midi: Some(midi),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
            .collect(),
    }
}

fn peer(start: u64, end: u64, midi: f32) -> BoundaryAlternative {
    BoundaryAlternative {
        source_expert: "peer".to_string(),
        range: TimeRange::new(start, end).unwrap(),
        kind: BoundaryEvidenceKind::AdvancedNote,
        fractional_midi: Some(midi),
        source_local_score: Some(0.8),
        source_local_pitch_score: Some(0.9),
        calibrated_boundary_confidence: None,
        calibrated_pitch_confidence: None,
        hard: false,
    }
}

fn fuse(boundaries: &BoundaryEvidenceSet, peers: &[BoundaryAlternative]) -> SingingFusionEvidence {
    fuse_singing_evidence_with_challengers(
        &[],
        boundaries,
        "rmvpe",
        &[],
        None,
        &[],
        None,
        None,
        true,
        None,
        peers,
        &[],
    )
    .unwrap()
}

#[test]
fn dense_peer_pitch_estimates_do_not_expand_every_fragment_onto_a_long_duration() {
    let boundaries = primary((0..96).map(|index| {
        (
            100_000 + index * 100_000,
            200_000 + index * 100_000,
            69.0 + index as f32 * 0.001,
        )
    }));
    let spanning = peer(100_000, 9_700_000, 69.0);
    let fused = fuse(&boundaries, &[spanning]);
    let spanning_states = fused
        .candidates
        .iter()
        .filter(|candidate| candidate.boundary_source == "peer")
        .collect::<Vec<_>>();
    assert_eq!(
        spanning_states.len(),
        2,
        "one pitch per participating expert"
    );
    for segment in &boundaries.segments {
        assert!(
            fused.candidates.iter().any(|candidate| {
                candidate.boundary_source == "game"
                    && candidate.target_pitch_source == "game"
                    && candidate.range == segment.range
                    && candidate.boundary_fractional_midi == segment.fractional_midi
            }),
            "raw note geometry and exact fractional pitch must remain auditable"
        );
    }
    crate::fusion::decode_candidate_graph(&fused.candidates).unwrap();
}

#[test]
fn peer_pitch_summary_uses_local_duration_not_note_count_or_raw_duration() {
    let boundaries = primary([(100_000, 500_000, 69.0)]);
    let mut peers = vec![peer(100_000, 400_000, 69.25)];
    peers.extend(
        (0..9).map(|index| peer(400_000 + index * 10_000, 410_000 + index * 10_000, 81.25)),
    );
    // This very long neighbor overlaps the target for only ten milliseconds.
    peers.push(peer(490_000, 10_000_000, 81.25));
    let fused = fuse(&boundaries, &peers);
    let primary_peer_states = fused
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.boundary_source == "game" && candidate.target_pitch_source == "peer"
        })
        .collect::<Vec<_>>();
    assert_eq!(primary_peer_states.len(), 1);
    assert!((primary_peer_states[0].center_pitch_hz - midi_hz(69.25)).abs() < 0.001);
    assert!(
        primary_peer_states[0]
            .boundary_alternatives
            .iter()
            .any(|alternative| alternative.fractional_midi == Some(81.25))
    );
    assert!(
        fused.candidates.iter().any(|candidate| {
            candidate.boundary_source == "peer"
                && candidate.target_pitch_source == "peer"
                && candidate.range.start == 400_000
                && candidate.target_midi == 81
        }),
        "real short-note proposals retain their own duration states"
    );
    peers.reverse();
    assert_eq!(fused, fuse(&boundaries, &peers));
}

fn stable_vibrato() -> Vec<F0Point> {
    (0..60)
        .map(|index| F0Point {
            time: 100_000 + index * 10_000,
            hz: 440.0 * 2.0_f32.powf(if index % 4 < 2 { -0.025 } else { 0.025 }),
            confidence: Some(0.9),
        })
        .collect()
}

fn flux_spike(time: u64) -> AcousticEvidence {
    AcousticEvidence {
        contract: crate::artifact::ACOUSTIC_EVIDENCE_CONTRACT.to_string(),
        version: crate::artifact::ACOUSTIC_EVIDENCE_VERSION,
        algorithm: crate::fingerprint::ACOUSTIC_DSP_VERSION.to_string(),
        timebase: 1_000_000,
        start: 0,
        hop: 10_000,
        sample_rate: 16_000,
        window_samples: 512,
        semantic_audio_role: "lead_vocal".to_string(),
        decoded_audio_sha256: "a".repeat(64),
        frames: (0..80)
            .map(|index| AcousticEvidenceFrame {
                start: index * 10_000,
                rms: 0.2,
                spectral_flux: (index > 0).then_some(if index * 10_000 == time {
                    0.3
                } else {
                    0.01
                }),
                periodicity: 0.8,
                snr_db: 20.0,
                fundamental_hz: Some(440.0),
                vibrato_activation: 0.8,
                glide_activation: 0.0,
                ornament_activation: 0.0,
                breath_activation: 0.0,
                voicing_transition_activation: 0.0,
            })
            .collect(),
    }
}

#[test]
fn flux_only_spikes_do_not_block_coherent_note_consolidation() {
    let boundaries = primary([(100_000, 400_000, 69.0), (400_000, 700_000, 69.0)]);
    let curve = stable_vibrato();
    // Exercise both the join-local test and the whole-span interior test.
    for time in [400_000, 250_000] {
        let acoustic = flux_spike(time);
        let fused = fuse_singing_evidence_with_challengers(
            &[],
            &boundaries,
            "rmvpe",
            &curve,
            None,
            &[],
            None,
            Some(&acoustic),
            true,
            None,
            &[],
            &[],
        )
        .unwrap();
        let selected = crate::fusion::decode_candidate_graph(&fused.candidates).unwrap();
        assert_eq!(selected.len(), 1, "flux-only spike at {time}");
        assert_eq!(
            selected[0].boundary_kind,
            BoundaryEvidenceKind::F0Consolidation
        );
        assert_eq!(selected[0].range, TimeRange::new(100_000, 700_000).unwrap());
    }
}

#[test]
fn corroborated_repeat_attacks_still_block_consolidation() {
    let boundaries = primary([(100_000, 400_000, 69.0), (400_000, 700_000, 69.0)]);
    let curve = stable_vibrato();
    for time in [400_000, 250_000] {
        let mut acoustic = flux_spike(time);
        acoustic.frames[(time / acoustic.hop) as usize].rms = 0.3;
        let challengers = f0_consolidation_challengers(
            &boundaries,
            &[],
            "rmvpe",
            &curve,
            Some(&acoustic),
            None,
            &[],
        )
        .unwrap();
        assert!(challengers.is_empty(), "measured attack at {time}");
    }
}

#[test]
fn peer_summary_preserves_distinct_experts_and_fractional_pitch() {
    let boundaries = primary([(100_000, 500_000, 69.0)]);
    let mut independent = peer(100_000, 500_000, 69.37);
    independent.source_expert = "independent".to_string();
    let fused = fuse(&boundaries, &[peer(100_000, 500_000, 81.13), independent]);
    let states = fused
        .candidates
        .iter()
        .filter(|candidate| candidate.boundary_source == "game")
        .collect::<Vec<_>>();
    assert_eq!(states.len(), 3);
    for (source, midi) in [("peer", 81.13), ("independent", 69.37)] {
        assert!(states.iter().any(|candidate| {
            candidate.target_pitch_source == source && candidate.center_pitch_hz == midi_hz(midi)
        }));
    }
}
