use crate::studio::*;

const REFERENCE_METRICS_COPY: &str = "SDR / SI-SDR / SIR / SAR: unavailable — no aligned ground-truth stems were evaluated. The original mix, another model's estimate and mixture reconstruction are not ground truth. Published dataset scores are not scores for this song.";

pub(crate) fn is_separation_capability(capability: &str) -> bool {
    matches!(
        capability,
        "audio.separate_vocal_bgm" | "audio.extract_vocals" | "audio.extract_instrumental"
    )
}

fn stem_measurement_rows(
    stem: &app_core::SeparatedStemMeasurementWire,
) -> Vec<(&'static str, String)> {
    vec![
        (
            "FORMAT / DURATION",
            format!(
                "{} Hz · {} channels · {:.3} s · {} frames",
                stem.sample_rate, stem.channels, stem.duration_seconds, stem.frame_count
            ),
        ),
        (
            "PEAK / RMS",
            format!(
                "{:.6} / {:.6} linear amplitude",
                stem.peak_amplitude, stem.rms_amplitude
            ),
        ),
        (
            "NEAR FULL SCALE",
            format!(
                "{:.4}% of samples · |sample| ≥ 0.999",
                stem.near_full_scale_ratio * 100.0
            ),
        ),
        (
            "SILENT SAMPLES",
            format!(
                "{:.4}% of samples · |sample| ≤ 0.0001",
                stem.silent_sample_ratio * 100.0
            ),
        ),
        (
            "FINITE SAMPLES",
            format!(
                "{} · {} samples",
                if stem.finite_samples {
                    "All finite"
                } else {
                    "Nonfinite samples reported"
                },
                stem.sample_count
            ),
        ),
    ]
}

pub(crate) fn spawn_separation_quality_inspection(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    engine: Option<&app_core::EngineRunHistoryProjection>,
    node_id: &str,
) {
    parent.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(10),
            border: UiRect::all(px(1)),
            border_radius: studio_card_radius(),
            ..default()
        },
        studio_card_background(theme),
        studio_card_border(theme),
        studio_card_shadow(theme),
    )).with_children(|card| {
        spawn_text(card, font.clone(), "04 · SEPARATION MEASUREMENTS", 7.5, theme.primary);
        spawn_text(card, font.clone(), "Output signal statistics", 14.0, theme.foreground);
        spawn_wrapped_text(card, font.clone(), REFERENCE_METRICS_COPY, 9.0, theme.editor_warning);
        let Some(engine) = engine else {
            spawn_wrapped_text(card, font.clone(), "No execution result is available for this plan. Run analysis to record separation measurements.", 9.0, theme.muted_foreground);
            return;
        };
        let inspection = match app_core::inspect_separation_quality(engine, node_id) {
            Ok(inspection) => inspection,
            Err(error) => {
                spawn_wrapped_text(card, font.clone(), error, 9.0, theme.destructive);
                return;
            }
        };
        spawn_wrapped_text(card, font.clone(), &inspection.explanation, 9.0, theme.muted_foreground);
        let Some(evidence) = inspection.evidence else { return; };
        spawn_wrapped_text(card, font.clone(), format!("Request: {} · Node: {} · Model: {}", inspection.request_id, evidence.node_id, evidence.model_id), 8.5, theme.muted_foreground);
        spawn_wrapped_text(card, font.clone(), format!("Measurement: {} · Reference status: {}", evidence.measurement, evidence.reference_status), 8.5, theme.muted_foreground);
        for stem in evidence.stems {
            card.spawn(Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(5),
                ..default()
            }).with_children(|section| {
                spawn_text(section, font.clone(), &stem.role, 12.0, theme.primary);
                spawn_wrapped_text(section, font.clone(), format!("Run artifact: {}", stem.artifact_path.display()), 8.5, theme.muted_foreground);
                for (label, value) in stem_measurement_rows(&stem) {
                    spawn_wrapped_text(section, font.clone(), format!("{label} · {value}"), 9.0, theme.foreground);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separation_inspector_labels_signal_units_without_fabricating_scores() {
        let stem = app_core::SeparatedStemMeasurementWire {
            role: "instrumental".into(),
            artifact_path: "stems/instrumental.flac".into(),
            sample_rate: 44_100,
            channels: 2,
            frame_count: 88_200,
            duration_seconds: 2.0,
            sample_count: 176_400,
            finite_samples: true,
            peak_amplitude: 0.5,
            rms_amplitude: 0.125,
            near_full_scale_ratio: 0.01,
            silent_sample_ratio: 0.25,
        };
        let rows = stem_measurement_rows(&stem);
        assert!(rows[0].1.contains("44100 Hz · 2 channels · 2.000 s"));
        assert!(rows[1].1.contains("0.500000 / 0.125000 linear"));
        assert!(rows[2].1.starts_with("1.0000%"));
        assert!(rows[3].1.starts_with("25.0000%"));
        assert!(rows[4].1.contains("All finite · 176400 samples"));
        assert!(REFERENCE_METRICS_COPY.contains("no aligned ground-truth stems"));
        assert!(!rows.iter().any(|(label, _)| label.contains("SDR")));
    }

    fn rendered_text(engine: Option<&app_core::EngineRunHistoryProjection>) -> String {
        let mut world = World::new();
        let mut queue = bevy::ecs::system::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_separation_quality_inspection(
                parent, Handle::default(), &StudioTheme::new(true), engine, "separate",
            );
        });
        queue.apply(&mut world);
        world.query::<&Text>().iter(&world).map(|text| text.0.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn separation_inspector_spawns_pending_and_error_states_without_audio_reads() {
        let pending = rendered_text(None);
        assert!(pending.contains("No execution result"));
        assert!(pending.contains("SDR / SI-SDR / SIR / SAR: unavailable"));
        let engine = app_core::EngineRunHistoryProjection {
            request_id: "selected".into(), request_json: "{}".into(),
            request_digest: "fixture".into(), plan_json: "{}".into(),
            result_json: Some("{broken".into()), fingerprint: None,
            source_sha256: "fixture".into(),
        };
        assert!(rendered_text(Some(&engine)).contains("Could not read this run's separation measurements"));
    }

    #[test]
    fn separation_inspector_spawns_recorded_stems_with_provenance() {
        let engine = app_core::EngineRunHistoryProjection {
            request_id: "selected".into(), request_json: "{}".into(),
            request_digest: "fixture".into(), plan_json: "{}".into(),
            result_json: Some(serde_json::json!({
                "request_id": "selected", "diagnostics": {"separation_quality": [{
                    "node_id": "separate", "model_id": "fixture_separator",
                    "measurement": "decoded_separation_output",
                    "reference_status": "unavailable_no_ground_truth_stems",
                    "stems": [{"role": "instrumental", "artifact_path": "not-read.flac",
                        "sample_rate": 44100, "channels": 2, "frame_count": 44100,
                        "duration_seconds": 1.0, "sample_count": 88200, "finite_samples": true,
                        "peak_amplitude": 0.5, "rms_amplitude": 0.125,
                        "near_full_scale_ratio": 0.0, "silent_sample_ratio": 0.25}]
                }]}
            }).to_string()),
            fingerprint: None, source_sha256: "fixture".into(),
        };
        let text = rendered_text(Some(&engine));
        assert!(text.contains("Request: selected · Node: separate · Model: fixture_separator"));
        assert!(text.contains("Run artifact: not-read.flac"));
        assert!(text.contains("0.500000 / 0.125000 linear amplitude"));
        assert!(text.contains("25.0000%"));
    }

    #[test]
    fn separation_inspector_does_not_label_cleanup_or_pitch_as_separation() {
        assert!(is_separation_capability("audio.separate_vocal_bgm"));
        assert!(is_separation_capability("audio.extract_instrumental"));
        assert!(!is_separation_capability("audio.denoise"));
        assert!(!is_separation_capability("pitch.track"));
    }
}
