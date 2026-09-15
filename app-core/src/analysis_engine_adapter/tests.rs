use super::*;
use crate::analysis_experience::{
    AnalysisExperienceSettings, AnalysisQualityProfile, resolve_analysis_experience,
};

#[test]
fn analysis_language_uses_configured_override_when_lyrics_are_absent() {
    assert_eq!(
        resolve_analysis_language(Some("ja"), None).as_deref(),
        Some("ja")
    );
    assert_eq!(
        resolve_analysis_language(Some("ja"), Some("en")).as_deref(),
        Some("ja")
    );
    assert_eq!(resolve_analysis_language(Some("auto"), Some("en")), None);
    assert_eq!(
        resolve_analysis_language(None, Some("zh")).as_deref(),
        Some("zh")
    );
    assert_eq!(resolve_analysis_language(None, None), None);
}

#[test]
fn character_timed_lrc_reaches_alignment_as_clean_line_tokens_with_real_windows() {
    let tokens = studio_tokens_from_timed_lrc(
        "[00:08.86]穢[00:08.94]れ[00:09.02]な[00:09.10]き\n[00:10.87]この身が朽ちるとも",
        20.0,
    )
    .unwrap();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].text, "穢れなき");
    assert_eq!(tokens[0].start, Some(8_860_000));
    assert_eq!(tokens[0].end, Some(10_870_000));
    assert_eq!(tokens[1].text, "この身が朽ちるとも");
    assert!(tokens.iter().all(|token| !token.text.contains("[00:")));
}

#[test]
fn identical_plain_and_lrc_lines_recover_the_existing_time_anchors() {
    let lines = vec!["一行目".to_string(), "二行目".to_string()];
    let parsed = crate::lrc::parse_lrc("[00:40.67]一行目\n[00:47.16]二行目\n[00:52.76]").unwrap();
    let tokens = matching_lrc_tokens(&lines, &parsed.input_lines).unwrap();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].id, "lrc-0");
    assert_eq!(tokens[0].start, Some(40_670_000));
    assert_eq!(tokens[0].end, Some(47_160_000));
    assert_eq!(tokens[1].text, "二行目");
}

#[test]
fn edited_plain_lines_do_not_reuse_stale_lrc_time_anchors() {
    let lines = vec!["一行目".to_string(), "編集した二行目".to_string()];
    let parsed = crate::lrc::parse_lrc("[00:40.67]一行目\n[00:47.16]二行目\n[00:52.76]").unwrap();
    assert!(matching_lrc_tokens(&lines, &parsed.input_lines).is_none());
}

#[test]
fn mixed_timed_lrc_keeps_all_canonical_lines_and_only_supplied_windows() {
    let tokens = studio_tokens_from_timed_lrc(
        "intro\n[00:10.00]Hello <00:11.00>world\nbridge\n[00:20.00]repeat\nrepeat",
        30.0,
    )
    .unwrap();
    assert_eq!(
        tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["intro", "Hello world", "bridge", "repeat", "repeat"]
    );
    assert_eq!(
        tokens
            .iter()
            .map(|token| (token.start, token.end))
            .collect::<Vec<_>>(),
        [
            (None, None),
            (Some(10_000_000), Some(20_000_000)),
            (None, None),
            (Some(20_000_000), Some(30_000_000)),
            (None, None),
        ]
    );
    let ids = tokens
        .iter()
        .map(|token| token.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), tokens.len());
}

#[test]
fn matching_mixed_plain_lines_keeps_unknown_ranges_and_duplicate_occurrences() {
    let parsed =
        crate::lrc::parse_lrc("repeat\n[00:10.00]repeat\nbridge\n[00:20.00]repeat").unwrap();
    let mut lines = parsed
        .input_lines
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    let tokens = matching_lrc_tokens(&lines, &parsed.input_lines).unwrap();
    assert_eq!(tokens[0].start, None);
    assert_eq!(tokens[1].start, Some(10_000_000));
    assert_eq!(tokens[2].start, None);
    assert_eq!(tokens[3].start, Some(20_000_000));
    assert_ne!(tokens[0].id, tokens[1].id);
    assert_ne!(tokens[1].id, tokens[3].id);
    lines.swap(1, 2);
    assert!(matching_lrc_tokens(&lines, &parsed.input_lines).is_none());
}

#[test]
fn pure_timed_canonical_tokens_expand_repeats_chronologically() {
    let tokens = studio_tokens_from_timed_lrc(
        "[00:30.00][00:10.00]repeat\n[00:20.00]middle\n[00:40.00]",
        50.0,
    )
    .unwrap();
    assert_eq!(
        tokens
            .iter()
            .map(|token| (token.text.as_str(), token.start, token.end))
            .collect::<Vec<_>>(),
        [
            ("repeat", Some(10_000_000), Some(20_000_000)),
            ("middle", Some(20_000_000), Some(30_000_000)),
            ("repeat", Some(30_000_000), Some(40_000_000)),
        ]
    );
}

fn effective(target: AnalysisDefaultTarget) -> EffectiveAnalysisExperience {
    resolve_analysis_experience(
        &AnalysisExperienceSettings {
            quality_profile: AnalysisQualityProfile::Balanced,
            default_target: target,
            ..Default::default()
        },
        None,
        None,
    )
}

fn source_fixture(label: &str, bytes: &[u8]) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "uta-studio-true-source-{label}-{}-{unique}.flac",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn automatic_queue_request_ids_are_unique_and_studio_owned() {
    let first = automatic_request_id();
    let second = automatic_request_id();
    assert_ne!(first, second);
    assert!(first.starts_with("studio-auto-"));
    assert!(second.starts_with("studio-auto-"));
}

#[test]
fn deep_step_one_cache_hit_keeps_every_earlier_semantic_source() {
    let decision = crate::chain_cache::ChainCacheDecision {
        role: AudioRoleWire::CleanLeadVocal,
        source_path: Some(PathBuf::from("/cache/clean.flac")),
        cached_sources: vec![
            crate::chain_cache::CachedChainSource {
                role: AudioRoleWire::GuideVocals,
                path: PathBuf::from("/cache/guide.flac"),
                identity: "guide".to_string(),
            },
            crate::chain_cache::CachedChainSource {
                role: AudioRoleWire::Instrumental,
                path: PathBuf::from("/cache/instrumental.flac"),
                identity: "instrumental".to_string(),
            },
            crate::chain_cache::CachedChainSource {
                role: AudioRoleWire::LeadVocal,
                path: PathBuf::from("/cache/lead.flac"),
                identity: "lead".to_string(),
            },
            crate::chain_cache::CachedChainSource {
                role: AudioRoleWire::CleanLeadVocal,
                path: PathBuf::from("/cache/clean.flac"),
                identity: "clean".to_string(),
            },
        ],
        ..Default::default()
    };

    let sources = cached_step_one_audio_sources(
        &decision,
        AudioRoleWire::CleanLeadVocal,
        Path::new("/cache/clean.flac"),
    );

    assert_eq!(
        sources.iter().map(|source| source.role).collect::<Vec<_>>(),
        vec![
            AudioRoleWire::GuideVocals,
            AudioRoleWire::Instrumental,
            AudioRoleWire::LeadVocal,
        ]
    );
    assert!(sources.iter().all(|source| !source.primary));
}

#[test]
fn cached_chain_primary_still_carries_the_library_original_mix() {
    let mix = PathBuf::from("/library/song.wav");
    let library_source = ResolvedAnalysisSource {
        library_file_hash: "songhash".to_string(),
        path: mix.clone(),
        sha256: "mix-digest".to_string(),
        role: AudioRoleWire::OriginalMix,
    };
    let mut sources = vec![AudioSourceWire {
        id: "true_source".to_string(),
        kind: AudioSourceKindWire::LocalFile,
        path: PathBuf::from("/cache/clean.flac"),
        sha256: "mix-digest".to_string(),
        role: AudioRoleWire::CleanLeadVocal,
        primary: true,
        timeline: SourceTimelineWire {
            timebase: CANONICAL_TIMEBASE,
            source_start: 0,
        },
    }];
    ensure_original_mix_source(&library_source, &mut sources);
    ensure_original_mix_source(&library_source, &mut sources);
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].role, AudioRoleWire::CleanLeadVocal);
    assert_eq!(sources[1].id, "original_mix");
    assert_eq!(sources[1].role, AudioRoleWire::OriginalMix);
    assert_eq!(sources[1].path, mix);
    assert!(!sources[1].primary);
}

#[test]
fn true_source_resolution_reuses_library_identity_without_hash_verification() {
    let path = source_fixture("identity", b"lossless source fixture bytes");
    let library_hash = crate::song::compute_file_hash(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let source = resolve_true_source_path(&library_hash, &path).unwrap();
    assert_eq!(source.library_file_hash.len(), 32);
    assert_eq!(source.library_file_hash, source.sha256);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn request_compiler_maps_product_targets_without_backend_types() {
    let path = std::env::temp_dir().join("source.flac");
    for target in [
        AnalysisDefaultTarget::Transcript,
        AnalysisDefaultTarget::Alignment,
        AnalysisDefaultTarget::PitchEvidence,
        AnalysisDefaultTarget::Instrumental,
    ] {
        let request = compile_analyze_request(
            AnalysisRequestIntent {
                model_settings: Default::default(),
                request_id: format!("test-{}", target.as_str()),
                turbo_acceleration: false,
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: path.clone(),
                    sha256: "a".repeat(64),
                    role: AudioRoleWire::OriginalMix,
                },
                lyrics: if target == AnalysisDefaultTarget::Alignment {
                    StudioLyricsContext {
                        mode: StudioLyricsMode::Canonical,
                        language_hint: Some("ja".to_string()),
                        tokens: vec![StudioLyricToken {
                            id: "token-1".to_string(),
                            text: "歌".to_string(),
                            reading: None,
                            phonemes: None,
                            start: None,
                            end: None,
                        }],
                    }
                } else {
                    StudioLyricsContext::default()
                },
                target_override: Some(target),
                requested_outputs: None,
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &effective(target),
        )
        .unwrap();
        assert_eq!(
            request.execution_policy.runtime_policy,
            RuntimePolicyWire::Production
        );
        assert!(!request.analysis.enable_quantization);
        match target {
            AnalysisDefaultTarget::Transcript => {
                assert!(request.requested_artifacts.transcript)
            }
            AnalysisDefaultTarget::Alignment => {
                assert!(request.requested_artifacts.alignment);
                assert!(!request.requested_artifacts.transcript);
            }
            AnalysisDefaultTarget::PitchEvidence => {
                assert!(request.requested_artifacts.pitch_evidence)
            }
            AnalysisDefaultTarget::Instrumental => assert_eq!(
                request.requested_artifacts.stems,
                [AudioRoleWire::Instrumental, AudioRoleWire::GuideVocals]
            ),
            AnalysisDefaultTarget::FullCandidate => unreachable!(),
        }
    }
}

#[test]
fn request_compiler_preserves_independent_multi_output_run_sheet() {
    let outputs = AnalysisOutputSelection {
        candidate_chart: false,
        pitch_evidence: false,
        transcript: true,
        alignment: false,
        instrumental: true,
    };
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "multi-output".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: None,
            requested_outputs: Some(outputs),
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::FullCandidate),
    )
    .unwrap();
    assert!(request.requested_artifacts.transcript);
    assert_eq!(
        request.requested_artifacts.stems,
        [AudioRoleWire::Instrumental, AudioRoleWire::GuideVocals]
    );
    assert!(!request.requested_artifacts.vocal_chart);
    assert!(!request.requested_artifacts.pitch_evidence);
    assert!(!request.requested_artifacts.alignment);
    assert!(!request.requested_artifacts.singing_analysis);
}

#[test]
fn request_compiler_rejects_an_empty_run_sheet() {
    let error = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "empty-output-sheet".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: None,
            requested_outputs: Some(AnalysisOutputSelection {
                candidate_chart: false,
                pitch_evidence: false,
                transcript: false,
                alignment: false,
                instrumental: false,
            }),
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::FullCandidate),
    )
    .unwrap_err();
    assert_eq!(error, "select at least one analysis output");
}

#[test]
fn request_compiler_normalizes_explicit_ggml_selection() {
    for configured in ["ggml", "ggml_vulkan", "vulkan"] {
        let request = compile_analyze_request(
            AnalysisRequestIntent {
                model_settings: Default::default(),
                request_id: format!("backend-{configured}"),
                turbo_acceleration: false,
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWire::OriginalMix,
                },
                lyrics: StudioLyricsContext::default(),
                target_override: Some(AnalysisDefaultTarget::PitchEvidence),
                requested_outputs: None,
                compute_backend: Some(configured.to_string()),
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &effective(AnalysisDefaultTarget::PitchEvidence),
        )
        .unwrap();
        assert_eq!(
            request.execution_policy.requested_backend,
            Some(NativeBackendWire::Ggml)
        );
        assert_eq!(
            request.execution_policy.runtime_policy,
            RuntimePolicyWire::Production
        );
    }
}

#[test]
fn request_compiler_forwards_the_explicit_libtorch_xpu_selection() {
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "backend-libtorch".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::PitchEvidence),
            requested_outputs: None,
            compute_backend: Some("libtorch_xpu".to_string()),
            model_backend_overrides: BTreeMap::from([(
                "rmvpe".to_string(),
                "ggml_vulkan".to_string(),
            )]),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::PitchEvidence),
    )
    .unwrap();
    assert_eq!(
        request.execution_policy.requested_backend,
        Some(NativeBackendWire::LibtorchXpu)
    );
    // A global backend routes every model: the saved rmvpe choice stays
    // persisted but is not sent until Custom routing is selected.
    assert!(request.execution_policy.model_backend_overrides.is_empty());
    assert_eq!(
        serde_json::to_value(&request.execution_policy).unwrap()["requested_backend"],
        "libtorch_xpu"
    );
    let rejected = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "backend-rocm".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::PitchEvidence),
            requested_outputs: None,
            compute_backend: Some("libtorch_rocm".to_string()),
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::PitchEvidence),
    )
    .unwrap_err();
    assert!(
        rejected.contains("unsupported analysis compute backend"),
        "{rejected}"
    );
}

#[test]
fn request_compiler_preserves_per_model_backend_choices() {
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "model-backends".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::Instrumental),
            requested_outputs: None,
            compute_backend: Some("custom".to_string()),
            model_backend_overrides: BTreeMap::from([
                (
                    "bs_roformer_leap_xe90_vocals".to_string(),
                    "ggml".to_string(),
                ),
                ("rmvpe".to_string(), "ggml".to_string()),
            ]),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::Instrumental),
    )
    .unwrap();
    assert_eq!(request.execution_policy.requested_backend, None);
    assert_eq!(
        request.execution_policy.runtime_policy,
        RuntimePolicyWire::Production
    );
    assert_eq!(
        request
            .execution_policy
            .model_backend_overrides
            .get("bs_roformer_leap_xe90_vocals"),
        Some(&NativeBackendWire::Ggml)
    );
    assert_eq!(
        request
            .execution_policy
            .model_backend_overrides
            .get("rmvpe"),
        Some(&NativeBackendWire::Ggml)
    );
}

#[test]
fn canonical_full_candidate_does_not_request_redundant_asr() {
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "known-candidate".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext {
                mode: StudioLyricsMode::Canonical,
                language_hint: Some("ja".to_string()),
                tokens: vec![StudioLyricToken {
                    id: "known-0".to_string(),
                    text: "歌".to_string(),
                    reading: None,
                    phonemes: None,
                    start: None,
                    end: None,
                }],
            },
            target_override: Some(AnalysisDefaultTarget::FullCandidate),
            requested_outputs: None,
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::FullCandidate),
    )
    .unwrap();
    assert!(request.requested_artifacts.vocal_chart);
    assert!(request.requested_artifacts.alignment);
    assert!(!request.requested_artifacts.transcript);
    let projection = project_lyrics_context_for_request(
        &studio_lyrics_from_wire(&request.lyrics),
        &request.requested_artifacts,
    );
    assert!(projection.alignment_requested);
    assert!(!projection.transcript_requested);
}

#[test]
fn mixed_timed_lrc_full_candidate_does_not_request_asr_or_drop_untimed_lines() {
    let tokens = studio_tokens_from_timed_lrc(
        "intro\n[00:10.00]Hello <00:11.00>world\nbridge\n[00:20.00]repeat\nrepeat",
        30.0,
    )
    .unwrap();
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "mixed-lrc-candidate".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext {
                mode: StudioLyricsMode::Canonical,
                language_hint: Some("en".to_string()),
                tokens: tokens.clone(),
            },
            target_override: Some(AnalysisDefaultTarget::FullCandidate),
            requested_outputs: None,
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective(AnalysisDefaultTarget::FullCandidate),
    )
    .unwrap();
    assert_eq!(request.lyrics.mode, LyricsModeWire::Canonical);
    assert!(request.requested_artifacts.vocal_chart);
    assert!(request.requested_artifacts.alignment);
    assert!(!request.requested_artifacts.transcript);
    assert_eq!(request.lyrics.tokens.len(), tokens.len());
    for (actual, expected) in request.lyrics.tokens.iter().zip(tokens) {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.text, expected.text);
        assert_eq!((actual.start, actual.end), (expected.start, expected.end));
    }
}

#[test]
fn disabling_continuous_pitch_omits_only_the_published_pitch_artifact() {
    let mut settings = effective(AnalysisDefaultTarget::FullCandidate);
    settings.preserve_continuous_pitch.value = false;
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "candidate-without-pitch-artifact".to_string(),
            turbo_acceleration: false,
            source: ResolvedAnalysisSource {
                library_file_hash: "library".to_string(),
                path: std::env::temp_dir().join("source.flac"),
                sha256: "a".repeat(64),
                role: AudioRoleWire::OriginalMix,
            },
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::FullCandidate),
            requested_outputs: None,
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &settings,
    )
    .unwrap();
    assert!(request.requested_artifacts.vocal_chart);
    assert!(request.requested_artifacts.singing_analysis);
    assert!(!request.requested_artifacts.pitch_evidence);
    assert!(!request.analysis.preserve_continuous_pitch);
}

#[test]
fn lyrics_projection_is_studio_owned_and_truthful() {
    let context = StudioLyricsContext {
        mode: StudioLyricsMode::Reference,
        language_hint: Some("en".to_string()),
        tokens: vec![StudioLyricToken {
            id: "one".to_string(),
            text: "sing".to_string(),
            reading: None,
            phonemes: None,
            start: None,
            end: None,
        }],
    };
    let projection = project_lyrics_context(&context, AnalysisDefaultTarget::Alignment);
    assert!(
        projection.text_supplied && projection.tokens_supplied && projection.alignment_requested
    );
    assert!(!projection.transcript_requested);
}

fn exact_preview_fixture() -> (EngineRunPreview, ResolvedAnalysisSource) {
    let path = source_fixture("queue", b"exact source bytes");
    let library_hash = crate::song::compute_file_hash(&path).unwrap();
    let source = resolve_true_source_path(&library_hash, &path).unwrap();
    let effective = effective(AnalysisDefaultTarget::Transcript);
    let request = compile_analyze_request(
        AnalysisRequestIntent {
            model_settings: Default::default(),
            request_id: "exact-preview-1".to_string(),
            turbo_acceleration: false,
            source: source.clone(),
            lyrics: StudioLyricsContext::default(),
            target_override: Some(AnalysisDefaultTarget::Transcript),
            requested_outputs: None,
            compute_backend: None,
            model_backend_overrides: BTreeMap::new(),
            default_device_class: None,
            model_device_overrides: BTreeMap::new(),
        },
        &effective,
    )
    .unwrap();
    let request_json = serde_json::to_string(&request).unwrap();
    let plan: AnalysisPlanWire = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan", "schema_version":1,
            "request_id":"exact-preview-1",
            "source_route":{"primary_source_id":"true_source","input_role":"original_mix","preparation":[]},
            "requested_outputs":["transcript"], "required_capabilities":[], "optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[], "execution_nodes":[], "quality_gates":[],
            "fallback_policy":[],
            "artifact_declarations":[{"semantic_type":"transcript","required":true,"media_type":"application/vnd.uta.transcript+json;version=1"}]
        })).unwrap();
    (
        EngineRunPreview {
            request_id: request.request_id,
            request_digest: digest_json(&request_json),
            request_json,
            engine_plan: plan,
            effective_settings: effective,
            lyrics_context: StudioLyricsContextProjection {
                mode: StudioLyricsMode::None,
                text_supplied: false,
                tokens_supplied: false,
                language_hint: None,
                transcript_requested: true,
                alignment_requested: false,
            },
            source: source.clone(),
            ready: true,
            blockers: Vec::new(),
            created_at_ms: now_ms(),
            invalidated: false,
        },
        source,
    )
}

#[test]
fn exact_plan_rejects_a_fusion_mode_mismatch() {
    let (preview, source) = exact_preview_fixture();
    let mut request: AnalyzeRequestWire = serde_json::from_str(&preview.request_json).unwrap();
    let request_workflow = crate::workflow::WorkflowExecutionWire {
        contract: "uta.workflow-execution".to_string(),
        version: 1,
        workflow_schema_version: crate::workflow::WORKFLOW_SCHEMA_VERSION,
        workflow_id: "workflow:test".to_string(),
        workflow_revision: 7,
        quality_mode: "balanced".to_string(),
        definition_digest: "digest".to_string(),
        nodes: Vec::new(),
        bindings: Vec::new(),
        terminal_outputs: Vec::new(),
        fusion_policy: None,
        fusion_mode: crate::workflow::WorkflowFusionModeWire::AiJudgment,
    };
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        serde_json::to_value(&request_workflow).unwrap(),
    );
    let mut plan = preview.engine_plan;
    plan.workflow_execution = Some(crate::backend_cli::WorkflowExecutionPlanWire {
        identity: crate::backend_cli::WorkflowPlanIdentityWire {
            contract: request_workflow.contract,
            version: request_workflow.version,
            workflow_schema_version: request_workflow.workflow_schema_version,
            workflow_id: request_workflow.workflow_id,
            workflow_revision: request_workflow.workflow_revision,
            definition_digest: request_workflow.definition_digest,
        },
        nodes: Vec::new(),
        terminal_outputs: Vec::new(),
        fusion_policy: None,
        fusion_mode: crate::backend_cli::FusionModeWire::Algorithm,
    });
    assert_eq!(
        validate_workflow_plan_identity(&request, &plan).unwrap_err(),
        "Analysis CLI workflow decision mode does not match the exact request snapshot"
    );
    std::fs::remove_file(source.path).unwrap();
}

#[test]
fn exact_preview_blocks_missing_and_unusable_fusion_adapters() {
    let (mut preview, source) = exact_preview_fixture();
    preview.engine_plan.resolved_resources = vec![
        serde_json::from_value(serde_json::json!({
            "requirement": {
                "resource": "tool:fusion_agent_adapter",
                "required": true,
                "reason": "fusion.candidate_graph / ai_judgment"
            },
            "status": null,
            "resolution_error": "resource_missing: adapter is not configured"
        }))
        .unwrap(),
    ];
    assert_eq!(
        plan_resource_blockers(&preview.engine_plan),
        [
            "tool:fusion_agent_adapter could not be resolved (resource_missing: adapter is not configured)"
        ]
    );

    preview.engine_plan.resolved_resources[0] = serde_json::from_value(serde_json::json!({
        "requirement": {
            "resource": "tool:fusion_agent_adapter",
            "required": true,
            "reason": "fusion.candidate_graph / ai_judgment"
        },
        "status": {
            "resource": "tool:fusion_agent_adapter",
            "install_state": "absent",
            "origin": "missing",
            "integrity_verified": false,
            "runnable": false,
            "validation_state": "production_pinned",
            "dependencies_ready": true,
            "executable_ready": false,
            "usable": false,
            "reasons": ["executable_missing"]
        }
    }))
    .unwrap();
    assert_eq!(
        plan_resource_blockers(&preview.engine_plan),
        [
            "tool:fusion_agent_adapter is not runnable under the requested policy (executablemissing)"
        ]
    );
    std::fs::remove_file(source.path).unwrap();
}

#[test]
fn exact_preview_snapshot_is_persisted_without_recompilation() {
    let (preview, source) = exact_preview_fixture();
    let intent = exact_queue_intent(&preview, &source).unwrap();
    assert_eq!(intent.request_id, preview.request_id);
    assert_eq!(
        intent.request_json.as_bytes(),
        preview.request_json.as_bytes()
    );
    assert_eq!(intent.request_digest, preview.request_digest);
    std::fs::remove_file(source.path).unwrap();
}

#[test]
fn exact_preview_accepts_a_cached_chain_input_for_an_unchanged_true_source() {
    let (mut preview, source) = exact_preview_fixture();
    let cached_path = source_fixture("cached-guide", b"cached guide vocal bytes");
    let mut request: AnalyzeRequestWire = serde_json::from_str(&preview.request_json).unwrap();
    request.audio_sources[0].path = cached_path.clone();
    request.audio_sources[0].role = AudioRoleWire::GuideVocals;
    preview.engine_plan.source_route.input_role = AudioRoleWire::GuideVocals;
    preview.request_json = serde_json::to_string(&request).unwrap();
    preview.request_digest = digest_json(&preview.request_json);

    let intent = exact_queue_intent(&preview, &source).unwrap();
    assert_eq!(intent.source_path, source.path);
    assert_eq!(
        serde_json::from_str::<AnalyzeRequestWire>(&intent.request_json)
            .unwrap()
            .audio_sources[0]
            .path,
        cached_path
    );

    std::fs::remove_file(cached_path).unwrap();
    std::fs::remove_file(source.path).unwrap();
}

#[test]
fn exact_preview_still_rejects_a_changed_library_true_source() {
    let (preview, source) = exact_preview_fixture();
    let replacement_path = source_fixture("replacement", b"replacement source bytes");
    let mut replacement = source.clone();
    replacement.path = replacement_path.clone();

    assert!(
        exact_queue_intent(&preview, &replacement)
            .unwrap_err()
            .contains("source_identity_changed")
    );

    std::fs::remove_file(replacement_path).unwrap();
    std::fs::remove_file(source.path).unwrap();
}

#[test]
fn invalidated_preview_is_rejected_but_digest_metadata_is_not_verified() {
    let (mut preview, source) = exact_preview_fixture();
    preview.invalidated = true;
    assert!(
        exact_queue_intent(&preview, &source)
            .unwrap_err()
            .contains("invalidated")
    );
    preview.invalidated = false;
    preview.request_digest = "opaque-digest-metadata".to_string();
    assert!(exact_queue_intent(&preview, &source).is_ok());
    std::fs::remove_file(source.path).unwrap();
}
