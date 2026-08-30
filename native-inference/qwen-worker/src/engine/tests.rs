use super::*;

#[test]
fn sanitize_json_control_characters_escapes_only_bytes_inside_strings() {
    // Real repro: the pinned aligner wrote a literal newline inside a
    // string value, which broke serde_json at "line 4 column 0".
    let raw = b"{\n  \"words\": [\"foo\nbar\", \"baz\"]\n}";
    let sanitized = sanitize_json_control_characters(raw);
    let parsed: serde_json::Value = serde_json::from_slice(&sanitized).unwrap();
    assert_eq!(parsed["words"][0], "foo\nbar");
    assert_eq!(parsed["words"][1], "baz");
}

#[test]
fn sanitize_json_control_characters_escapes_a_raw_quote_breaking_control_byte() {
    let raw = b"{\"word\": \"a\x01b\"}";
    let sanitized = sanitize_json_control_characters(raw);
    let parsed: serde_json::Value = serde_json::from_slice(&sanitized).unwrap();
    assert_eq!(parsed["word"], "a\u{1}b");
}

fn word(text: &str, start: f64, end: f64) -> AlignmentWord {
    AlignmentWord {
        word: text.to_string(),
        start,
        end,
    }
}

#[test]
fn alignment_units_preserve_words_and_segment_unspaced_cjk() {
    assert_eq!(
        alignment_text_units("one two three"),
        vec!["one", "two", "three"]
    );
    assert_eq!(
        alignment_text_units("春天在哪里"),
        vec!["春", "天", "在", "哪", "里"]
    );
}

#[test]
fn engine_output_reader_stops_at_the_combined_capture_limit() {
    let total = Arc::new(AtomicUsize::new(0));
    let oversized = Arc::new(AtomicBool::new(false));
    let bytes = read_bounded_engine_pipe(
        std::io::Cursor::new(vec![0_u8; MAX_ENGINE_OUTPUT_BYTES + 1]),
        Arc::clone(&total),
        Arc::clone(&oversized),
    );
    assert_eq!(bytes.len(), MAX_ENGINE_OUTPUT_BYTES);
    assert!(oversized.load(Ordering::SeqCst));
}

#[cfg(unix)]
fn unix_process_is_running(pid: i32) -> bool {
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| stat.rsplit_once(") ").map(|(_, tail)| tail.to_string()))
        .and_then(|tail| tail.chars().next());
    if state == Some('Z') {
        return false;
    }
    // SAFETY: signal 0 only probes process existence/permission.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(unix)]
#[test]
fn run_engine_kills_descendants_that_outlive_the_direct_child() {
    let dir = std::env::temp_dir().join(format!("uta-qwen-engine-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pid_path = dir.join("descendant.pid");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(format!("sleep 30 & echo $! > '{}'", pid_path.display()));
    let output = run_engine(&mut command).unwrap();
    assert!(output.status.success());
    let descendant_pid: i32 = std::fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while unix_process_is_running(descendant_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !unix_process_is_running(descendant_pid),
        "a descendant left running by the pinned engine must not outlive it"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(windows)]
fn windows_process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: the queried handle is closed on every successful open and
    // the exit-code pointer refers to a live local `u32`.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut exit_code = 0_u32;
        let queried = GetExitCodeProcess(process, &mut exit_code) != 0;
        CloseHandle(process);
        queried && exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(windows)]
#[test]
fn run_engine_kills_descendants_that_outlive_the_direct_child() {
    let dir = std::env::temp_dir().join(format!("uta-qwen-engine-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pid_path = dir.join("descendant.pid");
    let escaped_pid_path = pid_path.to_string_lossy().replace('\'', "''");
    let script = dir.join("spawn-descendant.ps1");
    std::fs::write(
            &script,
            format!(
                "$child = Start-Process -FilePath \"$env:SystemRoot\\System32\\ping.exe\" -ArgumentList \"-t\",\"127.0.0.1\" -PassThru -WindowStyle Hidden\nSet-Content -LiteralPath '{escaped_pid_path}' -Value $child.Id\n"
            ),
        )
        .unwrap();
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script);
    let output = run_engine(&mut command).unwrap();
    assert!(output.status.success());
    let descendant_pid: u32 = std::fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while windows_process_is_alive(descendant_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !windows_process_is_alive(descendant_pid),
        "a descendant left running by the pinned engine must not outlive it"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn asr_runtime_arguments_preserve_detected_language_logging() {
    assert!(!ASR_RUNTIME_ARGS.contains(&"-q"));
    assert!(
        ASR_RUNTIME_ARGS
            .windows(2)
            .any(|pair| pair == ["--timestamps", "none"])
    );
}

#[test]
fn asr_language_contract_rejects_explicit_hints() {
    assert!(validate_asr_language_policy(&serde_json::json!({})).is_ok());
    let error = validate_asr_language_policy(&serde_json::json!({"language": "ja"})).unwrap_err();
    assert!(error.contains("language contract v1"));
}

#[test]
fn asr_evidence_uses_runtime_detected_language() {
    let (language, text) =
        parse_asr_result("<|ja|>歌詞です\n", b"", b"detected-language: ja\n").unwrap();
    assert_eq!(language, "ja");
    assert_eq!(text, "歌詞です");
    assert_eq!(
        language_from_log(b"Detected-Language : EN\n").unwrap(),
        Some("en".to_string())
    );
    assert!(parse_asr_result("歌詞です", b"", b"").is_err());
    assert!(parse_asr_result("<|ja|>歌詞です", b"detected-language: zh\n", b"").is_err());
    assert!(language_from_log(b"detected-language: en\ndetected language: ja\n").is_err());
}

#[test]
fn asr_window_plan_is_bounded_contiguous_and_complete() {
    let plan = plan_asr_segments(305.813_333).unwrap();
    assert_eq!(plan.len(), 4);
    assert_eq!(plan[0], (0.0, 90.0));
    assert_eq!(plan[3], (270.0, 305.813_333));
    assert!(plan.windows(2).all(|pair| pair[0].1 == pair[1].0));
    assert!(
        plan.iter()
            .all(|(start, end)| end - start <= ASR_WINDOW_MAX_SECONDS)
    );
}

#[test]
fn aligner_input_contract_normalizes_text_and_supported_language_codes() {
    let input = normalize_alignment_input(&serde_json::json!({
        "text": "  一行目\r\n二行目  ",
        "language": " JA "
    }))
    .unwrap();
    assert_eq!(
        input,
        NormalizedAlignmentInput {
            transcript: "一行目\n二行目".to_string(),
            language: Some("ja"),
            runtime_language: Some("japanese"),
        }
    );
    assert!(normalize_alignment_input(&serde_json::json!({"text": "  "})).is_err());
    assert!(
        normalize_alignment_input(&serde_json::json!({"text": "words", "language": "nl"})).is_err()
    );
}

#[test]
fn aligner_input_contract_preserves_inner_unicode_and_allows_no_language() {
    let input = normalize_alignment_input(&serde_json::json!({
        "text": "Ａ Ｂ。é"
    }))
    .unwrap();
    assert_eq!(input.transcript, "Ａ Ｂ。é");
    assert_eq!(input.language, None);
    assert_eq!(input.runtime_language, None);
}

#[test]
fn zero_duration_unicode_pieces_join_measured_segments_without_new_timing() {
    let normalized = normalize_alignment_words(vec![
        word("土", 0.0, 1.28),
        word("地", 1.28, 1.28),
        word("の", 1.28, 1.44),
        word("そ", 1.44, 1.44),
        word("の", 1.44, 1.44),
        word("歌", 1.68, 1.92),
    ])
    .unwrap();
    assert_eq!(
        normalized,
        [
            word("土地", 0.0, 1.28),
            word("のその", 1.28, 1.44),
            word("歌", 1.68, 1.92)
        ]
    );
}

#[test]
fn leading_zero_piece_joins_the_next_measured_segment() {
    let normalized =
        normalize_alignment_words(vec![word("前", 0.0, 0.0), word("語", 0.1, 0.5)]).unwrap();
    assert_eq!(normalized, [word("前語", 0.1, 0.5)]);
}

#[test]
fn all_zero_or_overlapping_output_fails_closed() {
    assert!(normalize_alignment_words(vec![word("x", 0.0, 0.0)]).is_err());
    assert!(normalize_alignment_words(vec![word("a", 0.0, 1.0), word("b", 0.5, 1.5)]).is_err());
}

#[test]
fn asphodelos_style_paragraph_collapsed_into_two_ticks_fails_closed() {
    // Captured failure shape: ordered, positive-duration timing that passes
    // the old structural checks, but merges many lyric lines onto one note.
    let collapsed = [word(
        "降り注ぐ光の雨に溶けた私の色は霞む景色の中に滲んであなたには聞こえてるでしょ？響く私の言葉が胸の奥ずっと",
        257.76,
        257.92,
    )];
    let error = validate_alignment_measurement_resolution(&collapsed).unwrap_err();
    assert!(error.starts_with("Qwen alignment output has invalid word timing:"));
    assert!(error.contains("0.16 seconds"));

    // A long CJK runtime piece is still valid when it has real measured
    // duration; the check targets collapsed resolution, not text length.
    assert!(
        validate_alignment_measurement_resolution(&[word(
            "失ったはずの証はもう見えないけど",
            127.12,
            139.60,
        )])
        .is_ok()
    );
}

#[test]
fn measured_boundary_cannot_merge_two_caller_lyric_lines() {
    let units = ["一行目".to_string(), "二行目".to_string()];
    assert!(
        validate_alignment_unit_boundaries(
            &[word("一行目", 1.0, 2.0), word("二行目", 2.0, 3.0)],
            &units,
        )
        .is_ok()
    );
    let error =
        validate_alignment_unit_boundaries(&[word("一行目二行目", 1.0, 3.0)], &units).unwrap_err();
    assert!(error.contains("merged multiple lyric units"));
}

#[test]
fn long_form_plan_is_bounded_complete_and_has_context() {
    let plan = plan_alignment_segments(305.813_375, 26, ALIGN_WINDOW_TARGET_SECONDS).unwrap();
    assert_eq!(plan.len(), 3);
    assert_eq!(
        plan.iter()
            .map(|segment| (segment.target_unit_start, segment.target_unit_end))
            .collect::<Vec<_>>(),
        [(0, 9), (9, 18), (18, 26)]
    );
    assert_eq!(plan[1].context_unit_start, 6);
    assert_eq!(plan[2].context_unit_start, 15);
    assert!(plan.iter().all(|segment| {
        segment.audio_end_seconds - segment.audio_start_seconds <= ALIGN_WINDOW_MAX_SECONDS + 0.001
    }));
    assert!((plan[2].audio_start_seconds / ALIGN_TIMESTAMP_TICK_SECONDS - 2072.0).abs() < 0.001);
}

#[test]
fn anchored_plan_windows_each_line_near_its_own_claimed_time_not_its_index_position() {
    // Real repro data: line 15 of a 26-line Timed LRC import (Asphodelos)
    // claimed a 34-second span (167.18s-201.57s) because Timed LRC only
    // stamps line starts and this line's `end` was synthesized from a
    // mistimed next line. Blind index-proportional planning let that one
    // bad line drag every later window's assumed position off by tens of
    // seconds, producing a real failure: 14 characters of the next line
    // collapsed into a 0.16-second measurement.
    let anchors = [
        (40.67, 47.16),
        (167.18, 201.57), // the mistimed line
        (201.57, 213.99),
    ];
    // A small window target keeps each line in its own group here, so
    // this test can check each window's own position/capping in
    // isolation; `anchored_plan_groups_several_lines_per_window_like_blind_planning`
    // below covers the (default-sized) grouped case.
    let plans = plan_alignment_segments_from_anchors(
        &anchors,
        3,
        305.813,
        20.0,
        ALIGN_ANCHOR_MARGIN_SECONDS,
    )
    .unwrap();
    assert_eq!(plans.len(), 3);
    // Real repro bug: computing each line's own unit count separately
    // (via `alignment_text_units` on that line's bare text) falls back
    // to a per-*character* split for whitespace-free CJK text, desyncing
    // from the global transcript's line-level unit index -- so a later
    // window's "one line" target silently grew to include a dozen
    // unrelated lines. Each anchor must map to exactly one global unit.
    assert_eq!(
        plans
            .iter()
            .map(|plan| (plan.target_unit_start, plan.target_unit_end))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2), (2, 3)]
    );
    // The mistimed line's own window is capped, not left to balloon to
    // its full 34-second claim.
    let mistimed = &plans[1];
    assert!(
        mistimed.audio_end_seconds - mistimed.audio_start_seconds
            <= ALIGN_ANCHOR_MAX_SPAN_SECONDS + 2.0 * ALIGN_ANCHOR_MARGIN_SECONDS + 0.1
    );
    // Each window still starts near its own line's real claimed time,
    // not at some position inferred from unit index within the whole
    // transcript.
    assert!((plans[0].audio_start_seconds - (40.67 - ALIGN_ANCHOR_MARGIN_SECONDS)).abs() < 0.2);
    assert!((plans[2].audio_start_seconds - (201.57 - ALIGN_ANCHOR_MARGIN_SECONDS)).abs() < 0.2);
    // A later line's window position does not depend on an earlier
    // line's mistiming: it is anchored to its own claimed start.
    assert!(plans[2].audio_start_seconds < 220.0);
}

#[test]
fn anchored_plan_excludes_a_preceding_line_when_only_its_audio_tail_remains() {
    // Real Asphodelos retry window around line 15. Its 157.76s slice start
    // retained all of line 14, but only 0.10s of line 13. Supplying all of
    // line 13's text for that remnant pushed the owned line to the end of
    // the window and made it overlap line 16's independent measurement.
    let anchors = [
        (151.65, 157.86),
        (157.86, 163.81),
        (163.81, 167.18),
        (167.18, 201.57),
    ];
    let plans = plan_alignment_segments_from_anchors(
        &anchors,
        anchors.len(),
        305.813,
        12.5,
        ALIGN_ANCHOR_MARGIN_SECONDS,
    )
    .unwrap();
    let line_15 = plans
        .iter()
        .find(|plan| plan.target_unit_start == 2)
        .unwrap();
    assert_eq!(line_15.context_unit_start, 1);
    let line_16 = plans
        .iter()
        .find(|plan| plan.target_unit_start == 3)
        .unwrap();
    assert_eq!(line_16.context_unit_start, 1);
}

#[test]
fn anchored_plan_groups_several_lines_per_window_like_blind_planning() {
    // One window per line measures every line boundary independently
    // against windows that overlap by design (each line's own margin);
    // on a real song that made *most* seams a genuine reconciliation
    // gamble instead of the rare edge case blind planning's seam logic
    // was built for. Grouping several consecutive lines per window --
    // window *position* still comes from real anchor times, not an
    // even split -- keeps seams rare, like blind planning.
    let anchors = [
        (0.0, 5.0),
        (5.0, 10.0),
        (10.0, 15.0),
        (200.0, 205.0), // far enough away to force a new window
    ];
    let plans = plan_alignment_segments_from_anchors(
        &anchors,
        4,
        305.813,
        110.0,
        ALIGN_ANCHOR_MARGIN_SECONDS,
    )
    .unwrap();
    assert_eq!(
        plans
            .iter()
            .map(|plan| (plan.target_unit_start, plan.target_unit_end))
            .collect::<Vec<_>>(),
        [(0, 3), (3, 4)]
    );
    assert_eq!(plans[0].audio_end_seconds, anchors[2].1);
    assert_eq!(
        plans[1].audio_end_seconds,
        anchors[3].1 + ALIGN_ANCHOR_MARGIN_SECONDS
    );
    assert!(plans[0].audio_end_seconds - plans[0].audio_start_seconds <= 110.0 + 20.0);
}

#[test]
fn anchored_plan_rejects_a_mismatched_anchor_and_unit_count() {
    assert!(plan_alignment_segments_from_anchors(&[(0.0, 1.0)], 2, 10.0, 110.0, 5.0).is_err());
}

#[test]
fn anchored_plan_rejects_an_inverted_or_non_finite_anchor() {
    assert!(plan_alignment_segments_from_anchors(&[(5.0, 5.0)], 1, 10.0, 110.0, 5.0).is_err());
    assert!(plan_alignment_segments_from_anchors(&[(f64::NAN, 5.0)], 1, 10.0, 110.0, 5.0).is_err());
}

#[test]
fn line_anchors_config_parses_a_present_array_and_treats_absence_or_null_as_blind_mode() {
    let with_anchors = serde_json::json!({
        "text": "a b",
        "line_anchors": [{"start": 1.0, "end": 2.0}, {"start": 2.5, "end": 4.0}]
    });
    assert_eq!(
        parsed_line_anchors(&with_anchors).unwrap(),
        Some(vec![(1.0, 2.0), (2.5, 4.0)])
    );
    let absent = serde_json::json!({"text": "a b"});
    assert_eq!(parsed_line_anchors(&absent).unwrap(), None);
    let null = serde_json::json!({"text": "a b", "line_anchors": null});
    assert_eq!(parsed_line_anchors(&null).unwrap(), None);
    let empty = serde_json::json!({"text": "a b", "line_anchors": []});
    assert_eq!(parsed_line_anchors(&empty).unwrap(), None);
    let malformed = serde_json::json!({"text": "a b", "line_anchors": [{"start": 1.0}]});
    assert!(parsed_line_anchors(&malformed).is_err());
}

#[test]
fn window_context_is_removed_without_dropping_target_text() {
    let selected = target_words_from_context(
        vec![
            word("anchor", 0.0, 0.4),
            word("歌", 0.5, 0.8),
            word("詞", 0.8, 1.0),
        ],
        "anchor 歌詞",
        "anchor".chars().count(),
        2,
    )
    .unwrap();
    assert_eq!(selected, [word("歌", 0.5, 0.8), word("詞", 0.8, 1.0)]);
    assert!(
        target_words_from_context(vec![word("anchortarget", 0.0, 1.0)], "anchor target", 6, 6,)
            .is_err()
    );
}

fn seam_plan(index: usize, audio_start: f64, audio_end: f64) -> AlignmentSegmentPlan {
    AlignmentSegmentPlan {
        index,
        audio_start_seconds: audio_start,
        audio_end_seconds: audio_end,
        context_unit_start: 0,
        target_unit_start: 0,
        target_unit_end: 1,
        anchor_start: None,
    }
}

#[test]
fn seam_reconciliation_leaves_non_overlapping_windows_untouched() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    let mut previous = word("恋", 64.00, 65.20);
    let mut next = word("花", 65.50, 66.00);
    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
    assert_eq!(previous, word("恋", 64.00, 65.20));
    assert_eq!(next, word("花", 65.50, 66.00));
}

#[test]
fn seam_reconciliation_accepts_touching_boundary_unchanged() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    let mut previous = word("恋", 64.00, 65.20);
    let mut next = word("花", 65.20, 66.00);
    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
    assert_eq!(previous.end, next.start);
    assert_eq!(previous.end, 65.20);
}

#[test]
fn seam_reconciliation_splits_a_small_overlap_deterministically() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    let mut previous = word("恋", 64.00, 65.20);
    let mut next = word("花", 65.04, 66.00);
    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
    // Overlap is [65.04, 65.20] = 2 ticks; split_ticks = 1 => seam = 65.04 + 0.08.
    assert!((previous.end - 65.12).abs() < 1e-9);
    assert_eq!(previous.end, next.start);
    assert!(previous.start < previous.end);
    assert!(next.start < next.end);
}

#[test]
fn seam_reconciliation_resolves_a_sub_tick_overlap() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    let mut previous = word("恋", 64.00, 65.20);
    let mut next = word("花", 65.18, 66.00);
    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
    assert_eq!(previous.end, next.start);
    assert!(previous.start < previous.end);
    assert!(next.start < next.end);
}

#[test]
fn seam_reconciliation_resolves_a_larger_overlap_within_bounds() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    // Both words carry ample internal margin, so a 10-tick (0.8s) overlap
    // still has a valid deterministic seam strictly inside both words.
    let mut previous = word("恋", 60.00, 66.00);
    let mut next = word("花", 65.20, 70.00);
    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
    // Overlap is [65.20, 66.00] = 10 ticks; split_ticks = 5 => seam = 65.20 + 0.40.
    assert!((previous.end - 65.60).abs() < 1e-9);
    assert_eq!(previous.end, next.start);
    assert!(previous.start < previous.end);
    assert!(next.start < next.end);
}

#[test]
fn anchored_seam_reconciliation_keeps_the_published_tick_grid() {
    let previous_plan = seam_plan(0, 40.64, 133.12);
    let mut next_plan = seam_plan(1, 121.12, 169.84);
    // Real Timed LRC repro: this centisecond anchor is not representable
    // on Qwen's 80 ms output grid.
    next_plan.anchor_start = Some(127.13);
    let mut previous = word("憶で", 126.32, 128.00);
    let mut next = word("失った", 127.04, 162.72);

    reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();

    assert!((previous.end - 127.12).abs() < 1e-9);
    assert_eq!(previous.end, next.start);
    assert!((previous.end / ALIGN_TIMESTAMP_TICK_SECONDS).fract().abs() < 1e-9);
}

#[test]
fn seam_reconciliation_fails_closed_when_one_word_would_collapse() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    // previous is exactly one tick wide, and the overlap consumes the
    // entire word: no seam can keep previous.start < previous.end.
    let mut previous = word("恋", 65.12, 65.20);
    let mut next = word("花", 65.04, 65.28);
    let previous_before = previous.clone();
    let next_before = next.clone();
    let error =
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap_err();
    assert!(error.contains("could not be reconciled"));
    // Fail-closed must never partially mutate either word.
    assert_eq!(previous, previous_before);
    assert_eq!(next, next_before);
}

#[test]
fn seam_reconciliation_fails_closed_outside_shared_window_audio() {
    // The two windows share no audio (window ranges do not overlap), so
    // no candidate seam can be grounded in evidence either window
    // actually measured.
    let previous_plan = seam_plan(0, 0.0, 65.0);
    let next_plan = seam_plan(1, 65.5, 200.0);
    let mut previous = word("恋", 64.00, 66.00);
    let mut next = word("花", 65.00, 67.00);
    let previous_before = previous.clone();
    let next_before = next.clone();
    let error =
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap_err();
    assert!(error.contains("could not be reconciled"));
    assert_eq!(previous, previous_before);
    assert_eq!(next, next_before);
}

#[test]
fn seam_reconciliation_is_deterministic_across_repeated_calls() {
    let previous_plan = seam_plan(0, 0.0, 140.0);
    let next_plan = seam_plan(1, 60.0, 200.0);
    let run = || {
        let mut previous = word("恋", 64.00, 65.20);
        let mut next = word("花", 65.04, 66.00);
        reconcile_alignment_seam(&previous_plan, &next_plan, &mut previous, &mut next).unwrap();
        (previous, next)
    };
    assert_eq!(run(), run());
}

#[test]
fn asr_truncation_marker_matches_the_captured_production_failure() {
    let real = "pinned Qwen engine failed with exit status: 1: [debug] ggml_vulkan: \
            Found 1 Vulkan devices:\n[warn] qwen3_asr run: output truncated at 1024 tokens \
            \u{2014} decode reached the generation budget before end-of-stream; the \
            transcript may be incomplete.\n[info] timings: load=1825.62 ms";
    assert!(is_generation_budget_truncation(real));
    assert!(!is_generation_budget_truncation(
        "pinned Qwen engine failed with exit status: 1: some unrelated Vulkan error"
    ));
    assert!(!is_generation_budget_truncation(
        "Qwen ASR returned an empty transcript"
    ));
    assert!(!is_generation_budget_truncation(
        "could not start pinned Qwen engine: No such file or directory"
    ));
}

#[test]
fn asr_split_midpoint_halves_until_the_floor_then_stops() {
    assert_eq!(asr_split_midpoint(0.0, 90.0, 0), Some(45.0));
    assert_eq!(asr_split_midpoint(0.0, 45.0, 1), Some(22.5));
    assert_eq!(asr_split_midpoint(0.0, 22.5, 2), Some(11.25));
    // Half of 11.25s is 5.625s, below the 10s floor.
    assert_eq!(asr_split_midpoint(0.0, 11.25, 3), None);
}

#[test]
fn asr_split_midpoint_respects_the_max_depth_even_with_room_to_spare() {
    assert_eq!(asr_split_midpoint(0.0, 1000.0, ASR_MAX_SPLIT_DEPTH), None);
    assert!(asr_split_midpoint(0.0, 1000.0, ASR_MAX_SPLIT_DEPTH - 1).is_some());
}

// ---- End-to-end fixtures: a fake pinned-engine executable stands in for
// the real Vulkan binary so `run_align`/`run_asr` exercise their real
// window-stitching logic without GPU/model dependencies. The fake engine
// only understands "-o <path>", copying the next canned response file
// from its own control directory (one response per call, in order).
//
// The fake engine is a `#!/bin/sh` script, so everything from
// here to the end of this module is Unix-only; the pure-function
// seam/split-budget tests above already cover the same orchestration
// logic (including the platform-portable `ProcessTreeGuard` machinery in
// `run_engine`) without depending on a shell.

#[cfg(unix)]
fn synthetic_silent_wav(duration_seconds: f64) -> Vec<u8> {
    let byte_rate: u32 = 32_000;
    let data_bytes = (duration_seconds * f64::from(byte_rate)).round() as u32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&16_000_u32.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    wav.resize(wav.len() + data_bytes as usize, 0);
    wav
}

/// Write a fake engine whose control directory is baked into the script
/// itself (never a shared process-wide env var), so concurrently running
/// tests never interfere with each other's call counters.
#[cfg(unix)]
fn write_fake_engine(script_path: &Path, control: &Path) {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\nset -euo pipefail\ncontrol={control:?}\nout=\"\"\nprev=\"\"\nfor arg in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\ncount_file=\"$control/count\"\nn=0\nif [ -f \"$count_file\" ]; then n=$(cat \"$count_file\"); fi\necho $((n+1)) > \"$count_file\"\nif [ -f \"$control/truncate-$n\" ]; then\n  echo '[warn] qwen3_asr run: output truncated at 1024 tokens \u{2014} decode reached the generation budget before end-of-stream; the transcript may be incomplete.' >&2\n  exit 1\nfi\ncp \"$control/response-$n\" \"$out\"\n",
    );
    let staging = script_path.with_extension("part");
    {
        let mut file = std::fs::File::create(&staging).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    let mut permissions = std::fs::metadata(&staging).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&staging, permissions).unwrap();
    std::fs::rename(staging, script_path).unwrap();
}

#[cfg(unix)]
fn reset_fake_engine_calls(control: &Path) {
    let _ = std::fs::remove_file(control.join("count"));
}

#[cfg(unix)]
fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uta-qwen-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("control")).unwrap();
    dir
}

#[cfg(unix)]
fn assert_words_are_ordered_and_non_overlapping(words: &[serde_json::Value]) {
    let mut previous_end = 0.0_f64;
    for word in words {
        let start = word["start"].as_f64().unwrap();
        let end = word["end"].as_f64().unwrap();
        assert!(start < end, "word {word:?} is not positive-duration");
        assert!(
            start >= previous_end - 1e-9,
            "word {word:?} overlaps the previous word (previous_end={previous_end})"
        );
        previous_end = end;
    }
}

#[cfg(unix)]
#[test]
fn execute_alignment_window_retries_an_unmodified_call_after_corrupt_output() {
    // Real repro class: the external engine's own JSON write came out
    // structurally broken in a way `sanitize_json_control_characters`
    // can't repair (truncated mid-object, unlike the raw-control-byte
    // case that function does fix). Two consecutive real failures
    // landed at different byte offsets for otherwise-identical input,
    // which is what motivated retrying the unmodified call rather than
    // trying to parse harder.
    let test_dir = fixture_dir("align-window-corrupt-retry");
    let control = test_dir.join("control");
    std::fs::write(
        control.join("response-0"),
        b"{\"words\": [{\"word\": \"a\"".to_vec(),
    )
    .unwrap();
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({
            "words": [{"word": "a", "start": 0.0, "end": 1.0}]
        }))
        .unwrap(),
    )
    .unwrap();
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(1.0)).unwrap();
    let raw_path = test_dir.join("raw.json");
    let words = execute_alignment_window(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &raw_path,
        "a",
        None,
    )
    .unwrap();
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].word, "a");
    assert!(
        !raw_path.exists(),
        "successful parse should clean up the raw file"
    );
}

#[cfg(unix)]
#[test]
fn execute_alignment_window_fails_closed_after_exhausting_corrupt_output_retries() {
    let test_dir = fixture_dir("align-window-corrupt-exhausted");
    let control = test_dir.join("control");
    for attempt in 0..ALIGNMENT_WINDOW_PARSE_ATTEMPTS {
        std::fs::write(
            control.join(format!("response-{attempt}")),
            b"not json".to_vec(),
        )
        .unwrap();
    }
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(1.0)).unwrap();
    let raw_path = test_dir.join("raw.json");
    let error = execute_alignment_window(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &raw_path,
        "a",
        None,
    )
    .unwrap_err();
    assert!(error.starts_with("Qwen alignment output is invalid:"));
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        ALIGNMENT_WINDOW_PARSE_ATTEMPTS.to_string()
    );
}

#[cfg(unix)]
#[test]
fn run_align_reconciles_a_real_seam_overlap_for_dense_cjk_lyrics() {
    let test_dir = fixture_dir("cjk");
    let control = test_dir.join("control");
    let transcript = "风吹沙蝶恋花千古佳话";
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    // Window 0 owns chars[0..5]; context == target (no prefix).
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "风", "start": 0.00, "end": 0.08},
            {"word": "吹", "start": 0.08, "end": 0.16},
            {"word": "沙", "start": 0.16, "end": 0.24},
            {"word": "蝶", "start": 0.24, "end": 0.32},
            {"word": "恋", "start": 64.00, "end": 65.20}
        ]}))
        .unwrap(),
    )
    .unwrap();
    // Window 1 context is chars[2..10]; chars[2..5] are discarded prefix,
    // chars[5..10] are the owned target. "花" is deliberately timed to
    // overlap the previous window's "恋" by 2 ticks after offsetting.
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "沙", "start": 0.00, "end": 0.08},
            {"word": "蝶", "start": 0.08, "end": 0.16},
            {"word": "恋", "start": 0.16, "end": 0.24},
            {"word": "花", "start": 5.04, "end": 6.00},
            {"word": "千", "start": 6.00, "end": 6.08},
            {"word": "古", "start": 6.08, "end": 6.16},
            {"word": "佳", "start": 6.16, "end": 6.24},
            {"word": "话", "start": 6.24, "end": 6.32}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let destination = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 10);
    let recovered: String = words
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, transcript);
    assert_words_are_ordered_and_non_overlapping(words);
    // The deliberate 2-tick seam overlap between "恋" and "花" reconciles
    // to the deterministic midpoint tick, 65.12s.
    assert!((words[4]["end"].as_f64().unwrap() - 65.12).abs() < 1e-9);
    assert!((words[5]["start"].as_f64().unwrap() - 65.12).abs() < 1e-9);

    std::fs::remove_dir_all(&test_dir).unwrap();
}

/// Builds a valid raw-engine response covering exactly one plan segment's
/// context range, with tick-spaced sequential local timestamps starting
/// at zero. Safe/non-conflicting by construction: real window seams are
/// only ever tested by deliberately overriding specific entries.
#[cfg(unix)]
fn sequential_context_response(text_units: &[String], plan: &AlignmentSegmentPlan) -> Vec<u8> {
    let mut t = 0.0_f64;
    let words: Vec<serde_json::Value> = text_units[plan.context_unit_start..plan.target_unit_end]
        .iter()
        .map(|unit| {
            let entry = serde_json::json!({"word": unit, "start": t, "end": t + 0.08});
            t += 0.08;
            entry
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({"words": words})).unwrap()
}

#[test]
fn align_window_target_shrinks_each_attempt_so_retries_replan_windows() {
    assert_eq!(align_window_target_seconds(0), ALIGN_WINDOW_TARGET_SECONDS);
    assert_eq!(
        align_window_target_seconds(1),
        ALIGN_WINDOW_TARGET_SECONDS / 2.0
    );
    assert_eq!(
        align_window_target_seconds(2),
        ALIGN_WINDOW_TARGET_SECONDS / 4.0
    );
    // A shorter target plans strictly more windows for the same audio,
    // so a retry genuinely changes what the model is asked to align
    // rather than replaying an identical, deterministic computation.
    let attempt0 = plan_alignment_segments(200.0, 10, align_window_target_seconds(0)).unwrap();
    let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
    assert!(attempt1.len() > attempt0.len());
}

#[test]
fn anchored_window_target_reaches_line_sized_windows_on_the_final_attempt() {
    assert_eq!(anchor_window_target_seconds(0), 50.0);
    assert_eq!(anchor_window_target_seconds(1), 12.5);
    assert_eq!(anchor_window_target_seconds(2), 3.125);
    assert_eq!(anchor_margin_seconds(0), 6.0);
    assert_eq!(anchor_margin_seconds(1), 3.0);
    assert_eq!(anchor_margin_seconds(2), 0.0);
}

#[cfg(unix)]
#[test]
fn run_align_retries_a_real_measurement_after_a_transient_unresolvable_seam() {
    let test_dir = fixture_dir("retry-success");
    let control = test_dir.join("control");
    let transcript = "风吹沙蝶恋花千古佳话";
    let text_units = alignment_text_units(transcript);
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    // Attempt 0 (2 windows, the default target): identical to the
    // deterministic-seam fixture's data, but with "花" placed so far
    // from "恋" that no seam can satisfy previous.start < seam.
    let attempt0 = plan_alignment_segments(200.0, 10, align_window_target_seconds(0)).unwrap();
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "风", "start": 0.00, "end": 0.08},
            {"word": "吹", "start": 0.08, "end": 0.16},
            {"word": "沙", "start": 0.16, "end": 0.24},
            {"word": "蝶", "start": 0.24, "end": 0.32},
            {"word": "恋", "start": 64.00, "end": 65.20}
        ]}))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "沙", "start": 0.00, "end": 0.08},
            {"word": "蝶", "start": 0.08, "end": 0.16},
            {"word": "恋", "start": 0.16, "end": 0.24},
            {"word": "花", "start": 0.08, "end": 0.90},
            {"word": "千", "start": 0.90, "end": 1.00},
            {"word": "古", "start": 1.00, "end": 1.10},
            {"word": "佳", "start": 1.10, "end": 1.20},
            {"word": "话", "start": 1.20, "end": 1.30}
        ]}))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        attempt0.len(),
        2,
        "test fixture assumes 2 windows at the default target"
    );

    // Attempt 1 (retry, a shorter target -> more/different windows):
    // every window is measured with simple sequential, non-conflicting
    // timestamps, so this attempt succeeds cleanly on its own merits.
    let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
    assert!(
        attempt1.len() > attempt0.len(),
        "the retry must actually replan with different windows"
    );
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!("response-{}", attempt0.len() + plan.index)),
            sequential_context_response(&text_units, plan),
        )
        .unwrap();
    }

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress_calls = Vec::new();
    let mut progress = |completed: u64, total: u64, _message: &'static str| {
        progress_calls.push((completed, total));
        Ok(())
    };
    let destination = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 10);
    assert_words_are_ordered_and_non_overlapping(words);
    let recovered: String = words
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, transcript);
    // The failed attempt's progress is never surfaced: the caller only
    // ever sees the winning attempt's own monotonic, non-regressing,
    // complete (final == total) sequence.
    assert_eq!(progress_calls.len(), attempt1.len());
    assert!(progress_calls.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(
        progress_calls.last().unwrap().0,
        progress_calls.last().unwrap().1
    );
    // Exactly 2 attempts were made (attempt 0's 2 windows + attempt 1's
    // windows): bounded, not endless.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        (attempt0.len() + attempt1.len()).to_string()
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

/// The real production failure this retry path was widened for: not a
/// seam conflict, but a window whose raw output collapses to every word
/// pinned at a single timestamp (`normalize_alignment_words`'s "no
/// measured boundaries" fail-closed path). `run_align_once` bails out of
/// window 0 immediately via `?`, so attempt 0 only ever spends one real
/// call before this retries with a re-planned (shorter-target) window
/// set, exactly like the seam case.
#[cfg(unix)]
#[test]
fn run_align_retries_a_real_measurement_after_a_transient_all_zero_window() {
    let test_dir = fixture_dir("retry-success-zero-boundaries");
    let control = test_dir.join("control");
    let transcript = "风吹沙蝶恋花千古佳话";
    let text_units = alignment_text_units(transcript);
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    // Attempt 0, window 0: every word pinned to the same timestamp, so
    // `normalize_alignment_words` finds nothing measured and fails
    // closed. `run_align_once` never asks for window 1's response.
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "风", "start": 0.0, "end": 0.0},
            {"word": "吹", "start": 0.0, "end": 0.0},
            {"word": "沙", "start": 0.0, "end": 0.0},
            {"word": "蝶", "start": 0.0, "end": 0.0},
            {"word": "恋", "start": 0.0, "end": 0.0}
        ]}))
        .unwrap(),
    )
    .unwrap();

    // Attempt 1 (retry, a shorter target -> more/different windows):
    // every window is measured with simple sequential, non-conflicting
    // timestamps, so this attempt succeeds cleanly on its own merits.
    // Attempt 0 consumed exactly one real call (it bailed on window 0),
    // so attempt 1's responses continue the global call index at 1.
    let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!("response-{}", 1 + plan.index)),
            sequential_context_response(&text_units, plan),
        )
        .unwrap();
    }

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress_calls = Vec::new();
    let mut progress = |completed: u64, total: u64, _message: &'static str| {
        progress_calls.push((completed, total));
        Ok(())
    };
    let destination = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 10);
    assert_words_are_ordered_and_non_overlapping(words);
    let recovered: String = words
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, transcript);
    // The failed attempt's progress is never surfaced.
    assert_eq!(progress_calls.len(), attempt1.len());
    // Attempt 0's single bailed-out call + attempt 1's real windows:
    // bounded, not endless.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        (1 + attempt1.len()).to_string()
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_replans_after_an_asphodelos_style_collapsed_paragraph() {
    let test_dir = fixture_dir("retry-collapsed-paragraph");
    let control = test_dir.join("control");
    let transcript =
        "abcdefgh ijklmnop qrstuvwx yzabcdef ghijklmn opqrstuv wxyzabcd efghijkl mnopqrst uvwxyzab";
    let text_units = alignment_text_units(transcript);
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    let attempt0 = plan_alignment_segments(200.0, 10, align_window_target_seconds(0)).unwrap();
    assert_eq!(attempt0.len(), 2);
    let collapsed_text =
        text_units[attempt0[0].target_unit_start..attempt0[0].target_unit_end].concat();
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [{
            "word": collapsed_text,
            "start": 0.0,
            "end": 0.16
        }]}))
        .unwrap(),
    )
    .unwrap();

    // The collapsed first attempt must stop immediately and retry with a
    // genuinely different, shorter-window plan. These clean measurements
    // then succeed without any synthetic timestamp repair.
    let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!("response-{}", 1 + plan.index)),
            sequential_context_response(&text_units, plan),
        )
        .unwrap();
    }

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress_calls = Vec::new();
    let mut progress = |completed: u64, total: u64, _message: &'static str| {
        progress_calls.push((completed, total));
        Ok(())
    };
    let destination = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let recovered: String = evidence["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(
        recovered,
        transcript
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    );
    assert_eq!(progress_calls.len(), attempt1.len());
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        (1 + attempt1.len()).to_string()
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

/// The real production failure the previous test's fix immediately ran
/// into: a sparsely-segmented transcript where the *next* attempt's
/// halved window target needs more windows than there are lyric units,
/// so `plan_alignment_segments` itself fails before any engine call.
/// That planning artifact must never replace a real prior attempt's
/// measurement error -- every later attempt only shrinks the window
/// further, so it can never become feasible, and reporting "not enough
/// lyric units" instead of the real "no measured boundaries" would send
/// whoever reads it chasing the wrong problem.
#[cfg(unix)]
#[test]
fn run_align_prefers_a_real_measurement_error_over_a_later_infeasible_plan() {
    let test_dir = fixture_dir("retry-infeasible-plan");
    let control = test_dir.join("control");
    let transcript = "风吹沙"; // 3 lyric units.
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    let attempt0 = plan_alignment_segments(200.0, 3, align_window_target_seconds(0)).unwrap();
    assert_eq!(
        attempt0.len(),
        2,
        "test fixture assumes 2 windows at attempt 0"
    );
    let attempt1 = plan_alignment_segments(200.0, 3, align_window_target_seconds(1));
    assert!(
        attempt1.is_err(),
        "test fixture assumes attempt 1 needs more windows (4) than lyric units (3)"
    );

    // Attempt 0, window 0: every word pinned to the same timestamp, so
    // `normalize_alignment_words` fails closed with "no measured
    // boundaries". `run_align_once` never asks for window 1's response,
    // and attempt 1 never reaches the engine at all -- its own plan is
    // rejected before any window is built.
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "风", "start": 0.0, "end": 0.0},
            {"word": "吹", "start": 0.0, "end": 0.0}
        ]}))
        .unwrap(),
    )
    .unwrap();

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let error = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap_err();
    assert!(error.starts_with("Qwen Forced Aligner returned no measured boundaries"));
    // Exactly one real call: attempt 0's window 0. Attempt 1 never
    // reaches the engine because its plan is rejected first.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        "1"
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_retries_a_persistent_output_corruption_with_a_reshaped_window() {
    // Real repro: a specific window's audio/text corrupted the aligner's
    // JSON write identically -- same error, same byte offset -- on every
    // one of `execute_alignment_window`'s own unmodified retries, and
    // again on a completely separate later run of the same unchanged
    // window. That rules out a transient write race for this failure;
    // only a *different* window (this test's outer re-planned retry)
    // has a real chance of not tripping whatever in the window's content
    // corrupts the write.
    let test_dir = fixture_dir("retry-persistent-corruption");
    let control = test_dir.join("control");
    let transcript = "风吹沙蝶恋花千古佳话";
    let text_units = alignment_text_units(transcript);
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    // Attempt 0, window 0: corrupted (not just invalid JSON, but
    // corrupted in the "no sanitizer can save it" sense) on every one of
    // `ALIGNMENT_WINDOW_PARSE_ATTEMPTS` real calls. Window 1 is never
    // called: `run_align_once`'s per-window loop bails on window 0 first.
    for index in 0..ALIGNMENT_WINDOW_PARSE_ATTEMPTS {
        std::fs::write(
            control.join(format!("response-{index}")),
            b"not json".to_vec(),
        )
        .unwrap();
    }

    // Attempt 1 (re-planned, shorter target -> more/different windows):
    // clean responses for every window this attempt actually needs.
    // The global call index continues at ALIGNMENT_WINDOW_PARSE_ATTEMPTS.
    let attempt1 = plan_alignment_segments(200.0, 10, align_window_target_seconds(1)).unwrap();
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!(
                "response-{}",
                ALIGNMENT_WINDOW_PARSE_ATTEMPTS + plan.index as u32
            )),
            sequential_context_response(&text_units, plan),
        )
        .unwrap();
    }

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress = |_: u64, _: u64, _: &'static str| Ok(());
    let destination = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 10);
    assert_words_are_ordered_and_non_overlapping(words);
    let recovered: String = words
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, transcript);
    // Exactly the corrupt window's exhausted inner retries, plus attempt
    // 1's real windows: bounded, not endless, and the corruption did not
    // silently eat a whole outer attempt's budget for nothing.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        (ALIGNMENT_WINDOW_PARSE_ATTEMPTS as usize + attempt1.len()).to_string()
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_fails_closed_after_exhausting_seam_retries() {
    let test_dir = fixture_dir("retry-exhausted");
    let control = test_dir.join("control");
    let transcript = "风吹沙蝶恋花千古佳话";
    let text_units = alignment_text_units(transcript);
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    // `run_align_once` bails out at the first unresolvable seam, so only
    // each attempt's first two windows are ever measured regardless of
    // how many windows that attempt's (shrinking-target) plan has. Window
    // 0 always starts at local/global time 0, so pinning its last target
    // word's *local* time near the window's own 140s ceiling pushes its
    // *global* end far past any later window's naturally small audio
    // start + local offset, reproducing an unresolvable gap under every
    // attempt's own text/audio boundaries without depending on exactly
    // where window 1 happens to start.
    let mut response_index = 0_usize;
    let mut total_windows = 0_usize;
    for attempt in 0..ALIGN_SEAM_RETRY_ATTEMPTS {
        let plan =
            plan_alignment_segments(200.0, 10, align_window_target_seconds(attempt)).unwrap();
        total_windows += plan.len();
        for (position, segment) in plan.iter().take(2).enumerate() {
            let mut response = sequential_context_response(&text_units, segment);
            if position == 0 {
                // Force window 0's last (target) word far into its own
                // window, well past where window 1's small, unmodified
                // sequential timestamps can possibly reach it.
                let mut value: serde_json::Value = serde_json::from_slice(&response).unwrap();
                let words = value["words"].as_array_mut().unwrap();
                let last = words.len() - 1;
                words[last]["start"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.5);
                words[last]["end"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.42);
                response = serde_json::to_vec(&value).unwrap();
            }
            std::fs::write(control.join(format!("response-{response_index}")), response).unwrap();
            response_index += 1;
        }
    }
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let error = run_align(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        &mut progress,
    )
    .unwrap_err();
    assert!(error.contains("could not be reconciled at the window seam"));
    assert!(
        total_windows > response_index,
        "the fixture must exercise fewer real calls than total planned \
             windows, proving the bail-out-at-first-seam behavior"
    );
    // Exactly ALIGN_SEAM_RETRY_ATTEMPTS attempts were made (2 real calls
    // each): bounded, not endless, and not silently retried forever.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        response_index.to_string()
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_reconciles_a_seam_overlap_for_whitespace_lyrics_and_is_deterministic() {
    let test_dir = fixture_dir("latin");
    let control = test_dir.join("control");
    let transcript = "one two three four five six seven eight nine ten";
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "one", "start": 0.00, "end": 0.32},
            {"word": "two", "start": 0.32, "end": 0.64},
            {"word": "three", "start": 0.64, "end": 0.96},
            {"word": "four", "start": 0.96, "end": 1.28},
            {"word": "five", "start": 64.00, "end": 65.20}
        ]}))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "three", "start": 0.00, "end": 0.32},
            {"word": "four", "start": 0.32, "end": 0.64},
            {"word": "five", "start": 0.64, "end": 0.96},
            {"word": "six", "start": 5.04, "end": 6.00},
            {"word": "seven", "start": 6.00, "end": 6.32},
            {"word": "eight", "start": 6.32, "end": 6.64},
            {"word": "nine", "start": 6.64, "end": 6.96},
            {"word": "ten", "start": 6.96, "end": 7.28}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": transcript});

    let run = || {
        reset_fake_engine_calls(&control);
        let mut progress_calls = Vec::new();
        let mut progress = |completed: u64, total: u64, _message: &'static str| {
            progress_calls.push((completed, total));
            Ok(())
        };
        let destination = run_align(
            &runtime,
            Path::new("/fake-model.gguf"),
            &audio_path,
            &test_dir,
            &config,
            &mut progress,
        )
        .unwrap();
        (std::fs::read(&destination).unwrap(), progress_calls)
    };

    let (first_bytes, first_progress) = run();
    let (second_bytes, second_progress) = run();
    assert_eq!(
        first_bytes, second_bytes,
        "repeated runs must be deterministic"
    );
    assert_eq!(first_progress, second_progress);
    assert_eq!(first_progress, vec![(1, 2), (2, 2)]);

    let evidence: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 10);
    let recovered: String = words
        .iter()
        .map(|word| word["word"].as_str().unwrap())
        .collect();
    assert_eq!(
        recovered,
        transcript
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    );
    assert_words_are_ordered_and_non_overlapping(words);
    assert!((words[4]["end"].as_f64().unwrap() - 65.12).abs() < 1e-9);
    assert!((words[5]["start"].as_f64().unwrap() - 65.12).abs() < 1e-9);

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_asr_recovers_from_truncation_by_splitting_the_offending_window() {
    let test_dir = fixture_dir("asr-retry");
    let control = test_dir.join("control");
    let audio_path = test_dir.join("source.wav");
    // 60s fits in a single top-level 90s-max plan window.
    std::fs::write(&audio_path, synthetic_silent_wav(60.0)).unwrap();
    // Call 0: the whole-file attempt truncates.
    std::fs::write(control.join("truncate-0"), "").unwrap();
    // Call 1: the [0,30) half succeeds.
    std::fs::write(control.join("response-1"), "<|zh|>chunk-a").unwrap();
    // Call 2: the [30,60) half succeeds.
    std::fs::write(control.join("response-2"), "<|zh|>chunk-b").unwrap();
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let mut progress_calls = Vec::new();
    let mut progress = |completed: u64, total: u64, _message: &'static str| {
        progress_calls.push((completed, total));
        Ok(())
    };
    let destination = run_asr(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &serde_json::json!({}),
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    assert_eq!(evidence["text"], "chunk-a chunk-b");
    let segments = evidence["long_input"]["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0]["audio_start_seconds"], 0.0);
    assert_eq!(segments[0]["audio_end_seconds"], 30.0);
    assert_eq!(segments[1]["audio_start_seconds"], 30.0);
    assert_eq!(segments[1]["audio_end_seconds"], 60.0);
    // Progress reports real audio-time coverage (ms), monotonically
    // reaching the true total despite the retry/split, never a guessed
    // window-count percentage.
    assert_eq!(progress_calls, vec![(30_000, 60_000), (60_000, 60_000)]);

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_asr_fails_closed_when_truncation_cannot_be_resolved_within_policy_bounds() {
    let test_dir = fixture_dir("asr-retry-limit");
    let control = test_dir.join("control");
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(20.0)).unwrap();
    // Call 0: the whole-file attempt on [0,20) truncates, so it splits
    // to [0,10) and [10,20). Call 1: [0,10) truncates too; half of that
    // is 5s, below the 10s floor, so it cannot split again and must fail
    // closed immediately rather than retrying forever or ever reaching
    // the untried [10,20) half.
    std::fs::write(control.join("truncate-0"), "").unwrap();
    std::fs::write(control.join("truncate-1"), "").unwrap();
    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let error = run_asr(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &serde_json::json!({}),
        &mut progress,
    )
    .unwrap_err();
    assert!(error.contains("could not be split further within policy bounds"));
    // Exactly 2 calls were made (the untried [10,20) half is never
    // attempted once its sibling fails closed): no unbounded retry loop.
    assert_eq!(
        std::fs::read_to_string(control.join("count"))
            .unwrap()
            .trim(),
        "2"
    );

    std::fs::remove_dir_all(&test_dir).unwrap();
}
