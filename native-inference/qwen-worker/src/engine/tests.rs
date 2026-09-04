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
fn alignment_units_segment_cjk_even_when_latin_words_share_the_transcript() {
    // A real production transcript mixed unspaced Japanese lyrics with a
    // stray English phrase (an ASR hallucination); the old whole-transcript
    // heuristic saw *any* whitespace-separated run anywhere in the document
    // and stopped segmenting the CJK portion entirely, which the aligner
    // then measured as one giant "word" and collapsed into a near-zero
    // span. Each side must keep its own correct grouping regardless of
    // what else shares the document.
    assert_eq!(
        alignment_text_units("春天 in the sky"),
        vec!["春", "天", "in", "the", "sky"]
    );
}

#[test]
fn alignment_units_by_line_maps_each_caller_line_to_its_own_unit_range() {
    // Real repro: a Timed LRC import's caller lines are whole lyric lines,
    // not single characters -- a multi-character CJK line (or a multi-word
    // English one) must still map to exactly one line-index entry, just
    // covering several units, instead of desyncing the global unit array
    // from the per-line anchor index.
    let (units, ranges) = alignment_text_units_by_line("春天在哪里\none two\n三");
    assert_eq!(
        units,
        vec!["春", "天", "在", "哪", "里", "one", "two", "三"]
    );
    assert_eq!(ranges, vec![(0, 5), (5, 7), (7, 8)]);
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
fn asr_silent_window_produces_empty_evidence_without_error() {
    // A window covering a purely instrumental passage decodes to an empty
    // transcript and never logs a detected-language line: there is no
    // speech to report a language for. That's a valid silent window, not a
    // runtime failure (confirmed against a real song's instrumental outro).
    let (language, text) = parse_asr_result("", b"", b"").unwrap();
    assert_eq!(language, "");
    assert_eq!(text, "");
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
    // "一行目" is 3 characters, so the line-1/line-2 seam sits at offset 3.
    assert!(
        validate_alignment_unit_boundaries(
            &[word("一行目", 1.0, 2.0), word("二行目", 2.0, 3.0)],
            &[3],
        )
        .is_ok()
    );
    let error =
        validate_alignment_unit_boundaries(&[word("一行目二行目", 1.0, 3.0)], &[3]).unwrap_err();
    assert!(error.contains("merged multiple lyric lines"));
}

#[test]
fn measured_boundary_may_merge_characters_within_one_line() {
    // Real repro: a single-line window's target is "穢れなき薔薇十字" (8
    // characters, each its own `alignment_text_units` unit); the model
    // reasonably measured "薔薇" and "十字" as single two-character words.
    // With no *line* boundary inside a one-line window, that must not be
    // rejected -- only a boundary between two different caller lines may
    // never be straddled by one measured word.
    assert!(
        validate_alignment_unit_boundaries(
            &[
                word("穢", 0.0, 0.5),
                word("れ", 0.5, 1.0),
                word("な", 1.0, 1.5),
                word("き", 1.5, 2.0),
                word("薔薇", 2.0, 3.0),
                word("十字", 3.0, 4.0),
            ],
            &[],
        )
        .is_ok()
    );
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
fn blind_plan_ramps_a_collapsed_leading_run_instead_of_duplicating_windows() {
    // Real repro: a heavily retried plan (final attempt's 27.5s window
    // target) packed a 354.88s song into 13 segments, and segments 0-2's
    // centers all sat within half a window's width of the start, collapsing
    // every one of their audio windows to the identical [0, 140s] span --
    // confirmed as the root cause of a production "could not be reconciled
    // at the window seam" failure (segment 1 measured its own, later target
    // text starting back at 0.00s, before segment 0's last word even
    // began).
    let plan = plan_alignment_segments(354.88, 40, 27.5).unwrap();
    assert_eq!(plan.len(), 13);
    let starts = plan
        .iter()
        .map(|segment| segment.audio_start_seconds)
        .collect::<Vec<_>>();
    assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(starts[0], 0.0);
    assert!((starts[1] / ALIGN_TIMESTAMP_TICK_SECONDS - 106.0).abs() < 0.001);
    assert!((starts[2] / ALIGN_TIMESTAMP_TICK_SECONDS - 212.0).abs() < 0.001);
    // The first segment whose own centering already escaped the clamp
    // keeps its exact original position -- the ramp only touches the
    // collapsed prefix in front of it.
    assert!((starts[3] / ALIGN_TIMESTAMP_TICK_SECONDS - 319.0).abs() < 0.001);
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
        &[(0, 1), (1, 2), (2, 3)],
        305.813,
        20.0,
        ALIGN_ANCHOR_MARGIN_SECONDS,
    )
    .unwrap();
    assert_eq!(plans.len(), 3);
    // Each line here happens to be exactly one global unit wide, so target
    // ranges line up with line indices; `anchored_plan_allows_a_line_wider_than_one_alignment_unit`
    // below covers real multi-character/multi-word lines.
    assert_eq!(
        plans
            .iter()
            .map(|plan| (plan.target_unit_start, plan.target_unit_end))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2), (2, 3)]
    );
    // A small window target keeps every group here down to one line, so
    // there is no interior line seam to protect.
    assert!(plans.iter().all(|plan| plan.line_boundary_units.is_empty()));
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
    let line_unit_ranges: Vec<(usize, usize)> = (0..anchors.len()).map(|i| (i, i + 1)).collect();
    let plans = plan_alignment_segments_from_anchors(
        &anchors,
        &line_unit_ranges,
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
        &[(0, 1), (1, 2), (2, 3), (3, 4)],
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
    // The first window groups 3 lines, each exactly one unit wide here, so
    // its interior seams sit right after units 1 and 2; the second window
    // owns only its one line, with no interior seam to protect.
    assert_eq!(plans[0].line_boundary_units, vec![1, 2]);
    assert!(plans[1].line_boundary_units.is_empty());
    // A window with a successor ends on the tick grid (its last frame is
    // then whole, so it cannot spill into the successor's first frame);
    // the final window keeps its exact margin-extended end.
    assert_eq!(plans[0].audio_end_seconds, tick_floor(anchors[2].1));
    assert!(plans[0].audio_end_seconds <= anchors[2].1);
    assert_eq!(
        plans[1].audio_end_seconds,
        anchors[3].1 + ALIGN_ANCHOR_MARGIN_SECONDS
    );
    assert!(plans[0].audio_end_seconds - plans[0].audio_start_seconds <= 110.0 + 20.0);
}

#[test]
fn anchored_zero_margin_windows_never_share_a_frame_at_a_line_seam() {
    // Real repro: the final zero-margin retry (one line per window) cut the
    // window for "さよなら愛するこの国よ" at its successor's 287.59s line
    // time -- off the 80 ms grid -- so that window's last frame was
    // [287.52s, 287.60s), the very frame its successor started on
    // (tick_floor(287.59) = 287.52). Both windows pinned their edge word to
    // it ("よ" and "ずっ" both at [287.52s, 287.60s]), an unsplittable tie.
    let anchors = [(283.90, 287.59), (287.59, 291.71), (291.71, 295.15)];
    let plans = plan_alignment_segments_from_anchors(
        &anchors,
        &[(0, 11), (11, 24), (24, 36)],
        354.88,
        anchor_window_target_seconds(2),
        anchor_margin_seconds(2),
    )
    .unwrap();
    assert_eq!(plans.len(), 3);
    for pair in plans.windows(2) {
        let (earlier, later) = (&pair[0], &pair[1]);
        // The earlier window ends exactly where the later one starts, and
        // that point is on the grid, so no frame belongs to both.
        assert_eq!(earlier.audio_end_seconds, later.audio_start_seconds);
        let ticks = earlier.audio_end_seconds / ALIGN_TIMESTAMP_TICK_SECONDS;
        assert!((ticks - ticks.round()).abs() < 1e-6);
    }
    assert_eq!(plans[0].audio_end_seconds, tick_floor(287.59));
    // The last window has nothing after it and keeps its real end.
    assert_eq!(plans[2].audio_end_seconds, 295.15);
}

#[test]
fn anchored_plan_rejects_a_mismatched_anchor_and_line_count() {
    assert!(
        plan_alignment_segments_from_anchors(&[(0.0, 1.0)], &[(0, 1), (1, 2)], 10.0, 110.0, 5.0)
            .is_err()
    );
}

#[test]
fn anchored_plan_allows_a_line_wider_than_one_alignment_unit() {
    // A real lyric line is almost never exactly one `alignment_text_units`
    // unit -- a multi-word English line, or any CJK line longer than one
    // character, both split into several units. Anchors map to their own
    // line's *range* of units instead of assuming a 1:1 index match.
    let plans = plan_alignment_segments_from_anchors(
        &[(0.0, 5.0), (5.0, 10.0)],
        &[(0, 6), (6, 9)],
        10.0,
        110.0,
        5.0,
    )
    .unwrap();
    assert_eq!(
        plans
            .iter()
            .map(|plan| (plan.target_unit_start, plan.target_unit_end))
            .collect::<Vec<_>>(),
        [(0, 9)]
    );
    // The seam between the two lines sits after unit 6, where line 0's own
    // range ends -- this is the only boundary a measured word may never
    // straddle; nothing inside either line's own 6- or 3-unit span is one.
    assert_eq!(plans[0].line_boundary_units, vec![6]);
}

#[test]
fn anchored_plan_rejects_an_inverted_or_non_finite_anchor() {
    assert!(
        plan_alignment_segments_from_anchors(&[(5.0, 5.0)], &[(0, 1)], 10.0, 110.0, 5.0).is_err()
    );
    assert!(
        plan_alignment_segments_from_anchors(&[(f64::NAN, 5.0)], &[(0, 1)], 10.0, 110.0, 5.0)
            .is_err()
    );
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
        line_boundary_units: Vec::new(),
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
fn exact_tick_seam_tie_break_favors_whichever_line_the_anchor_claims_more_of() {
    // Real repro: two adjacent lines' own independent windows both placed a
    // word at [287.52s, 287.60s], one 80 ms tick wide -- no seam exists that
    // leaves both sides positive duration. The next line's own real anchor
    // (287.59s) sits 0.07s into that tick and only 0.01s from its end, so
    // the *previous* line claims the overwhelming majority of it.
    let previous = word("よ", 287.52, 287.60);
    let next = word("すっ", 287.52, 287.60);
    assert_eq!(
        exact_tick_seam_tie_break(&previous, &next, Some(287.59)),
        Some(true)
    );
    // Flip which line's anchor sits closer to which edge: now the *next*
    // line claims the majority of the same tied tick.
    assert_eq!(
        exact_tick_seam_tie_break(&previous, &next, Some(287.53)),
        Some(false)
    );
    // The two windows reach the same frame through different tick-aligned
    // offsets (588 ticks + 3006 ticks versus 3594 ticks), which differ by
    // an ulp -- far more than `f64::EPSILON`, and exactly how the first
    // version of this check missed the real production seam.
    let noisy_start = 588.0 * ALIGN_TIMESTAMP_TICK_SECONDS + 3006.0 * ALIGN_TIMESTAMP_TICK_SECONDS;
    let exact_start = 3594.0 * ALIGN_TIMESTAMP_TICK_SECONDS;
    assert_ne!(noisy_start, exact_start);
    assert!((noisy_start - exact_start).abs() > f64::EPSILON);
    let noisy_previous = word(
        "よ",
        noisy_start,
        noisy_start + ALIGN_TIMESTAMP_TICK_SECONDS,
    );
    let exact_next = word(
        "ずっ",
        exact_start,
        exact_start + ALIGN_TIMESTAMP_TICK_SECONDS,
    );
    assert_eq!(
        exact_tick_seam_tie_break(&noisy_previous, &exact_next, Some(287.59)),
        Some(true)
    );
}

#[test]
fn exact_tick_seam_tie_break_only_fires_on_a_true_unsplittable_tie() {
    let previous = word("よ", 287.52, 287.60);
    let next = word("すっ", 287.52, 287.60);
    // Blind planning carries no anchor at all.
    assert_eq!(exact_tick_seam_tie_break(&previous, &next, None), None);
    // A genuine partial overlap (not an exact tie) has real room for
    // `reconcile_alignment_seam`'s own split logic; this helper must not
    // intercept it.
    let partial_next = word("すっ", 287.56, 287.68);
    assert_eq!(
        exact_tick_seam_tie_break(&previous, &partial_next, Some(287.59)),
        None
    );
    // Two identical spans *wider* than one frame still have an interior
    // tick for `reconcile_alignment_seam` to split on (the next line's own
    // anchor tick, here 287.60s), which keeps both lines' text separate;
    // merging them would throw that away.
    let wide_previous = word("よ", 287.52, 287.68);
    let wide_next = word("すっ", 287.52, 287.68);
    assert_eq!(
        exact_tick_seam_tie_break(&wide_previous, &wide_next, Some(287.59)),
        None
    );
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
    std::fs::write(control.join("response-0"), b"{\"words\": [{\"word\": \"a\"").unwrap();
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
fn execute_alignment_window_snaps_raw_engine_output_to_the_tick_grid() {
    // Real GPU repro: the pinned aligner measured "も" as [105.28s, 105.33s]
    // -- 105.33s is not a multiple of the promised 80 ms tick, which the
    // analysis engine's own downstream contract check rejects outright. The
    // raw engine is the only source of that drift; snap it here so every
    // later consumer can trust the "qwen-align-token-word-80ms-v1" profile
    // without needing its own tolerance.
    let test_dir = fixture_dir("align-window-tick-snap");
    let control = test_dir.join("control");
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({
            "words": [{"word": "も", "start": 105.28, "end": 105.33}]
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
        "も",
        None,
    )
    .unwrap();
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].start, 105.28);
    assert_eq!(words[0].end, 105.36);
}

#[cfg(unix)]
#[test]
fn execute_alignment_window_fails_closed_after_exhausting_corrupt_output_retries() {
    let test_dir = fixture_dir("align-window-corrupt-exhausted");
    let control = test_dir.join("control");
    for attempt in 0..ALIGNMENT_WINDOW_PARSE_ATTEMPTS {
        std::fs::write(control.join(format!("response-{attempt}")), b"not json").unwrap();
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
    // Window 1's plan starts at the tail anchor (60.00s), but its own
    // context includes window 0's real last word "恋" (chars[2..5] are
    // discarded prefix): chaining raises the actual offset used to
    // 63.20s (window 0's "恋" ends at 65.20s, minus the chain's 2.00s
    // back margin). "花" (the owned target, chars[5..10]) is deliberately
    // timed -- relative to that 63.20s offset -- to still overlap the
    // previous window's "恋" by 2 ticks after offsetting.
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "沙", "start": 0.00, "end": 0.08},
            {"word": "蝶", "start": 0.08, "end": 0.16},
            {"word": "恋", "start": 0.16, "end": 0.24},
            {"word": "花", "start": 1.84, "end": 2.80},
            {"word": "千", "start": 2.80, "end": 2.88},
            {"word": "古", "start": 2.88, "end": 2.96},
            {"word": "佳", "start": 2.96, "end": 3.04},
            {"word": "话", "start": 3.04, "end": 3.12}
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

#[cfg(unix)]
#[test]
fn run_align_merges_an_exact_tick_tie_instead_of_failing_the_seam() {
    // Real repro, reproduced at a small scale: two adjacent anchored lines'
    // own independent windows both measure a word at the exact same single
    // tick. Line 1's own real anchor sits 0.07s into that shared tick and
    // only 0.01s from its end, so line 0 ("よ") claims the overwhelming
    // majority of it; line 1's window independently measures its own first
    // character ("す") at that identical tick. Before the seam-tie fix,
    // `run_align_once` failed the whole song here; now "す" folds into "よ"
    // instead, and line 1's remaining word ("あ") keeps its own timing.
    let test_dir = fixture_dir("anchored-exact-tick-tie");
    let control = test_dir.join("control");
    let transcript = "よ\nすあ";
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(12.0)).unwrap();

    // Window 0: line 0's own window is [0.0s, 5.12s] (its 5.19s line seam
    // ends on the grid, see `plan_alignment_segments_from_anchors`). A real
    // engine can no longer produce a frame past that end, but this fake
    // one still answers with "よ" at [5.12s, 5.20s] to exercise the
    // fold path on exactly the production tie it was written for.
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "よ", "start": 5.12, "end": 5.20}
        ]}))
        .unwrap(),
    )
    .unwrap();
    // Window 1 starts at 5.12s (line 1's raw 5.19s anchor, tick-floored
    // down): local time is measured relative to that offset, so "す"'s
    // local [0.00s, 0.08s] is the same absolute [5.12s, 5.20s] tick window 0
    // already claimed; "あ" measures the next tick over, uncontested.
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "す", "start": 0.00, "end": 0.08},
            {"word": "あ", "start": 0.08, "end": 0.16}
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
    let config = serde_json::json!({
        "text": transcript,
        "line_anchors": [
            {"start": 0.0, "end": 5.19},
            {"start": 5.19, "end": 10.19},
        ],
    });
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let destination = run_align_once(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        anchor_window_target_seconds(2),
        anchor_margin_seconds(2),
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    // "す" merged into "よ" instead of the seam failing; "あ" keeps its own
    // uncontested timing.
    assert_eq!(words.len(), 2);
    assert_eq!(words[0]["word"].as_str().unwrap(), "よす");
    assert_eq!(words[0]["start"].as_f64().unwrap(), 5.12);
    assert_eq!(words[0]["end"].as_f64().unwrap(), 5.20);
    assert_eq!(words[1]["word"].as_str().unwrap(), "あ");
    assert_eq!(words[1]["start"].as_f64().unwrap(), 5.20);
    assert_eq!(words[1]["end"].as_f64().unwrap(), 5.28);

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_folds_a_tick_tie_the_other_way_when_the_favored_window_cannot_yield() {
    // Same tie as above, but line 1 is a single character: its window
    // measures exactly one word, and a window must keep at least one
    // measured word of its own. The anchor still favors line 0 for the
    // tick, yet line 1 cannot give its only word up, so the fold runs the
    // other way -- line 0's "よ" joins line 1's "す" -- and line 0's window
    // keeps "あ" as its own (now single) measured word.
    let test_dir = fixture_dir("anchored-tick-tie-fallback");
    let control = test_dir.join("control");
    let transcript = "あよ\nす";
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(12.0)).unwrap();
    std::fs::write(
        control.join("response-0"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "あ", "start": 5.04, "end": 5.12},
            {"word": "よ", "start": 5.12, "end": 5.20}
        ]}))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "す", "start": 0.00, "end": 0.08}
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
    let config = serde_json::json!({
        "text": transcript,
        "line_anchors": [
            {"start": 0.0, "end": 5.19},
            {"start": 5.19, "end": 10.19},
        ],
    });
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let destination = run_align_once(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        anchor_window_target_seconds(2),
        anchor_margin_seconds(2),
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let words = evidence["words"].as_array().unwrap();
    assert_eq!(words.len(), 2);
    assert_eq!(words[0]["word"].as_str().unwrap(), "あ");
    assert_eq!(words[0]["start"].as_f64().unwrap(), 5.04);
    assert_eq!(words[0]["end"].as_f64().unwrap(), 5.12);
    assert_eq!(words[1]["word"].as_str().unwrap(), "よす");
    assert_eq!(words[1]["start"].as_f64().unwrap(), 5.12);
    assert_eq!(words[1]["end"].as_f64().unwrap(), 5.20);
    // Each window's evidence still owns exactly the words it kept.
    let segments = evidence["long_input"]["segments"].as_array().unwrap();
    assert_eq!(segments[0]["measured_units"], 1);
    assert_eq!(segments[1]["measured_units"], 1);

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
        std::fs::write(control.join(format!("response-{index}")), b"not json").unwrap();
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

#[test]
fn trailing_drop_finds_nothing_worth_dropping_in_a_short_transcript() {
    let text_units: Vec<String> = "one two three four five six seven eight nine ten"
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let plans = plan_alignment_segments(
        200.0,
        text_units.len(),
        align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1),
    )
    .unwrap();
    assert!(trailing_drop_transcript(&text_units, &plans).is_none());
}

#[test]
fn trailing_drop_removes_a_real_collapse_sized_tail() {
    // 30 "real" words followed by 10 longer "hallucinated" words: with
    // ALIGN_SEAM_RETRY_ATTEMPTS's finest target and a 200s track, planning
    // always produces 8 windows regardless of word count (segment count is
    // duration-driven, not text-driven -- see `plan_alignment_segments`),
    // so the last window's own slice lands inside the hallucinated tail
    // and is comfortably over the collapse-size threshold.
    let mut words: Vec<String> = (0..30).map(|index| format!("real{index}")).collect();
    words.extend((0..10).map(|index| format!("hallucinated{index}")));
    let target = align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1);
    let plans = plan_alignment_segments(200.0, words.len(), target).unwrap();
    let adjusted = trailing_drop_transcript(&words, &plans).unwrap();
    assert!(!adjusted.contains("hallucinated9"), "{adjusted}");
    assert!(adjusted.contains("real0"), "{adjusted}");
    assert!(adjusted.len() < words.join(" ").len());
}

#[test]
fn drop_unit_range_transcript_excises_a_middle_span_and_splices_the_remainder() {
    let words: Vec<String> = (0..10)
        .map(|index| format!("keepA{index}"))
        .chain(std::iter::once("collapsedwordabc".to_string()))
        .chain((0..10).map(|index| format!("keepB{index}")))
        .collect();
    let adjusted = drop_unit_range_transcript(&words, 10, 11).unwrap();
    assert!(!adjusted.contains("collapsedwordabc"), "{adjusted}");
    assert!(adjusted.contains("keepA0"), "{adjusted}");
    assert!(adjusted.contains("keepB9"), "{adjusted}");
    // The two surviving halves are joined directly together, not left with
    // a gap in their place.
    assert!(adjusted.contains("keepA9 keepB0"), "{adjusted}");
}

#[test]
fn drop_unit_range_transcript_rejects_an_undersized_or_empty_result() {
    let short_span = vec!["a".to_string(), "short".to_string(), "b".to_string()];
    assert!(
        drop_unit_range_transcript(&short_span, 0, 2).is_none(),
        "a dropped span under the collapse-size floor must not be excised"
    );
    let whole_transcript = vec!["twelvecharword".to_string()];
    assert!(
        drop_unit_range_transcript(&whole_transcript, 0, 1).is_none(),
        "dropping the entire transcript must not leave an empty result"
    );
    assert!(drop_unit_range_transcript(&whole_transcript, 0, 0).is_none());
    assert!(drop_unit_range_transcript(&whole_transcript, 1, 5).is_none());
}

#[test]
fn parse_collapsed_unit_range_reads_the_embedded_range_and_rejects_other_errors() {
    let error = "Qwen alignment output has invalid word timing: collapsed 19 characters into \
                  0.08 seconds (window 2: audio=[177.28s, 317.28s] target=\"...\" unit_range=[12,14))";
    assert_eq!(parse_collapsed_unit_range(error), Some((12, 14)));

    assert_eq!(
        parse_collapsed_unit_range(
            "Qwen long-form alignment windows produced overlapping timing that could not be \
             reconciled at the window seam (seam 0->1, previous=\"a\" [0.0s, 0.1s], next=\"b\" \
             [0.0s, 0.1s], next_anchor=None)"
        ),
        None,
        "a seam error must never be mistaken for a collapse error"
    );
    assert_eq!(
        parse_collapsed_unit_range(
            "Qwen alignment output has invalid word timing: one measured boundary merged multiple lyric units"
        ),
        None,
        "a collapse-prefixed error with no embedded range must not panic or fabricate one"
    );
}

#[cfg(unix)]
#[test]
fn run_align_with_span_dropped_recovers_from_a_collapse_in_the_middle_of_the_song() {
    // Unlike the hallucinated-tail case, this excises from the *middle*:
    // the collapsed span sits between two groups of otherwise-alignable
    // content, so recovery must splice the surviving halves together
    // rather than merely truncate -- confirmed against a real production
    // song where the collapse was nowhere near the transcript's own end.
    let test_dir = fixture_dir("span-drop-recover");
    let control = test_dir.join("control");
    let words: Vec<String> = (0..10)
        .map(|index| format!("keepA{index}"))
        .chain(std::iter::once("collapsedwordabc".to_string()))
        .chain((0..10).map(|index| format!("keepB{index}")))
        .collect();
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    let finest_target = align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1);
    let adjusted_transcript = drop_unit_range_transcript(&words, 10, 11).unwrap();
    let adjusted_units = alignment_text_units(&adjusted_transcript);
    let minimum_safe_target = finest_target.max(200.0 / adjusted_units.len() as f64);
    assert_eq!(
        minimum_safe_target, finest_target,
        "test fixture assumes the drop alone already satisfies the finest target"
    );
    let attempt0 =
        plan_alignment_segments(200.0, adjusted_units.len(), minimum_safe_target).unwrap();
    for plan in &attempt0 {
        std::fs::write(
            control.join(format!("response-{}", plan.index)),
            sequential_context_response(&adjusted_units, plan),
        )
        .unwrap();
    }
    // Attempt 0's own blind plan happens to land its last two windows only
    // one tick apart (a real, frequently-hit case of blind planning's own
    // tail-anchor-vs-centered collision -- see `plan_alignment_segments`'s
    // doc comment), so even this "clean" fixture measures a genuine
    // inversion there; attempt 1's widened, differently-shaped grid is
    // given real sequential data too and recovers cleanly.
    let attempt1_target = minimum_safe_target * 1.5;
    let attempt1 = plan_alignment_segments(200.0, adjusted_units.len(), attempt1_target).unwrap();
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!("response-{}", attempt0.len() + plan.index)),
            sequential_context_response(&adjusted_units, plan),
        )
        .unwrap();
    }
    // Attempt 1 hits the same tail-collision case one window count down;
    // attempt 2's coarser grid finally puts real margin between every
    // window and recovers cleanly.
    let attempt2_target = minimum_safe_target * 1.5 * 1.5;
    let attempt2 = plan_alignment_segments(200.0, adjusted_units.len(), attempt2_target).unwrap();
    for plan in &attempt2 {
        std::fs::write(
            control.join(format!(
                "response-{}",
                attempt0.len() + attempt1.len() + plan.index
            )),
            sequential_context_response(&adjusted_units, plan),
        )
        .unwrap();
    }

    let script_path = test_dir.join("engine.sh");
    write_fake_engine(&script_path, &control);
    let runtime = crate::runtime::ValidatedRuntime {
        engine: script_path,
        manifest_sha256: "0".repeat(64),
    };
    let config = serde_json::json!({"text": words.join(" ")});
    let mut progress = |_: u64, _: u64, _: &'static str| Ok(());
    let destination = run_align_with_span_dropped(
        &runtime,
        Path::new("/fake-model.gguf"),
        &audio_path,
        &test_dir,
        &config,
        10,
        11,
        &mut progress,
    )
    .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    let recovered = evidence["transcript"].as_str().unwrap();
    assert!(
        !recovered.contains("collapsedwordabc"),
        "the collapsed middle span must not survive: {recovered}"
    );
    assert!(recovered.contains("keepA0"), "{recovered}");
    assert!(recovered.contains("keepB9"), "{recovered}");

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_with_trailing_content_dropped_recovers_from_a_hallucinated_tail() {
    // Mirrors the real production repro: a transcript whose true content
    // ends well before the audio does, with a long fabricated tail a
    // transcribing worker produced for what was actually a silent
    // instrumental outro. No window plan can align invented text to audio
    // it was never spoken over, so this exercises the fallback directly
    // rather than re-proving `plan_alignment_segments`'s own retry
    // mechanics (already covered above).
    let test_dir = fixture_dir("trailing-drop");
    let control = test_dir.join("control");
    // Short tokens: `sequential_context_response` gives every unit a flat
    // one-tick (0.08s) duration regardless of its own character count, so a
    // unit of 12+ characters would trip the collapse check on that alone --
    // a fixture artifact unrelated to what this test exercises. Two windows
    // (a short 50s track against the finest retry target) keeps this
    // aligned with the other synthetic-fixture seam tests above, which are
    // already proven reliable at that same window count.
    let mut words: Vec<String> = (0..5).map(|index| format!("real{index}")).collect();
    words.extend((0..5).map(|index| format!("fake{index}")));
    let transcript = words.join(" ");
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(50.0)).unwrap();

    let target = align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1);
    let original_plans = plan_alignment_segments(50.0, words.len(), target).unwrap();
    let adjusted_transcript = trailing_drop_transcript(&words, &original_plans).unwrap();
    let adjusted_units = alignment_text_units(&adjusted_transcript);
    let adjusted_plans = plan_alignment_segments(50.0, adjusted_units.len(), target).unwrap();
    for plan in &adjusted_plans {
        std::fs::write(
            control.join(format!("response-{}", plan.index)),
            sequential_context_response(&adjusted_units, plan),
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
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let destination = run_align_with_trailing_content_dropped(
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
    let recovered = evidence["transcript"].as_str().unwrap();
    assert!(
        !recovered.contains("fake9"),
        "the fabricated tail must not survive: {recovered}"
    );
    assert!(recovered.contains("real0"), "{recovered}");

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_with_trailing_content_dropped_widens_target_when_the_drop_underflows_the_finest_plan()
{
    // Real production repro: a lyric-sparse song (long instrumental/spoken
    // stretches) whose total unit count sits right at the finest retry
    // target's own segment count. Dropping the collapsed tail removes one
    // unit too many for that *same* fine-grained target to replan against --
    // `plan_alignment_segments`'s segment count is duration-driven, not
    // text-driven, so the shrunken transcript alone can't satisfy it. The
    // fallback must widen its own target rather than fail closed on an
    // arithmetic artifact of the drop it just made, silently discarding a
    // drop that would otherwise have recovered the song.
    let test_dir = fixture_dir("trailing-drop-underflow");
    let control = test_dir.join("control");
    let mut words: Vec<String> = (0..7).map(|index| format!("real{index}")).collect();
    words.push("abcdefghijkl".to_string()); // 12 characters: exactly the collapse-size floor.
    let transcript = words.join(" ");
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    let finest_target = align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1);
    let original_plans = plan_alignment_segments(200.0, words.len(), finest_target).unwrap();
    assert_eq!(
        original_plans.len(),
        8,
        "test fixture assumes 8 windows at the finest target"
    );
    let adjusted_transcript = trailing_drop_transcript(&words, &original_plans).unwrap();
    let adjusted_units = alignment_text_units(&adjusted_transcript);
    assert_eq!(adjusted_units.len(), 7);
    assert!(
        plan_alignment_segments(200.0, adjusted_units.len(), finest_target).is_err(),
        "test fixture assumes the finest target alone can't replan the shortened transcript"
    );

    let retry_target = finest_target.max(200.0 / adjusted_units.len() as f64);
    let adjusted_plans =
        plan_alignment_segments(200.0, adjusted_units.len(), retry_target).unwrap();
    for plan in &adjusted_plans {
        std::fs::write(
            control.join(format!("response-{}", plan.index)),
            sequential_context_response(&adjusted_units, plan),
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
    let mut progress = |_completed: u64, _total: u64, _message: &'static str| Ok(());
    let destination = run_align_with_trailing_content_dropped(
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
    let recovered = evidence["transcript"].as_str().unwrap();
    assert!(
        !recovered.contains("abcdefghijkl"),
        "the collapsed tail must not survive: {recovered}"
    );
    assert!(recovered.contains("real0"), "{recovered}");

    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn run_align_with_trailing_content_dropped_retries_a_transient_unresolvable_seam() {
    // Real production repro: after dropping the tail, the fallback's own
    // single alignment attempt hit an unresolvable seam between two windows
    // (independent measurements disagreed enough to invert a multi-character
    // run against the very next character) with no further recourse -- one
    // shot, no retry, so a transient disagreement anywhere in the *kept*
    // transcript killed the whole recovery. Mirrors
    // `run_align_retries_a_real_measurement_after_a_transient_unresolvable_seam`'s
    // technique (pin window 0's last target word far into its own window) to
    // force exactly that failure on the fallback's first attempt, then
    // proves its own retry (a widened, differently-shaped window grid)
    // recovers cleanly.
    let test_dir = fixture_dir("trailing-drop-retry-seam");
    let control = test_dir.join("control");
    // Short tokens throughout, unlike `trailing_drop_removes_a_real_collapse_sized_tail`'s
    // "hallucinated{index}": `sequential_context_response` gives every unit a
    // flat one-tick (0.08s) duration regardless of its own character count,
    // so any *retained* (post-drop) unit of 12+ characters would trip the
    // collapse check on that alone -- a fixture artifact unrelated to what
    // this test exercises (see the hallucinated-tail recovery test's own
    // note on the same constraint). `trailing_drop_transcript` only needs
    // the *summed* dropped tail at or above that threshold, not any single
    // word, so short "dropN" tokens still trigger a real drop.
    let mut words: Vec<String> = (0..30).map(|index| format!("real{index}")).collect();
    words.extend((0..10).map(|index| format!("drop{index}")));
    let transcript = words.join(" ");
    let audio_path = test_dir.join("source.wav");
    std::fs::write(&audio_path, synthetic_silent_wav(200.0)).unwrap();

    let finest_target = align_window_target_seconds(ALIGN_SEAM_RETRY_ATTEMPTS - 1);
    let original_plans = plan_alignment_segments(200.0, words.len(), finest_target).unwrap();
    let adjusted_transcript = trailing_drop_transcript(&words, &original_plans).unwrap();
    let adjusted_units = alignment_text_units(&adjusted_transcript);
    let minimum_safe_target = finest_target.max(200.0 / adjusted_units.len() as f64);
    assert_eq!(
        minimum_safe_target, finest_target,
        "test fixture assumes the drop alone already satisfies the finest target"
    );

    // Attempt 0 (the fallback's own first try, at `minimum_safe_target`):
    // force an unresolvable seam between windows 0 and 1.
    let attempt0 =
        plan_alignment_segments(200.0, adjusted_units.len(), minimum_safe_target).unwrap();
    assert!(
        attempt0.len() >= 2,
        "test fixture assumes attempt 0 has at least 2 windows"
    );
    for (position, plan) in attempt0.iter().take(2).enumerate() {
        let mut response = sequential_context_response(&adjusted_units, plan);
        if position == 0 {
            let mut value: serde_json::Value = serde_json::from_slice(&response).unwrap();
            let response_words = value["words"].as_array_mut().unwrap();
            let last = response_words.len() - 1;
            response_words[last]["start"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.5);
            response_words[last]["end"] = serde_json::json!(ALIGN_WINDOW_MAX_SECONDS - 0.42);
            response = serde_json::to_vec(&value).unwrap();
        }
        std::fs::write(control.join(format!("response-{position}")), response).unwrap();
    }

    // Attempt 1 (widened target -> a different window grid): every window is
    // measured with plain sequential, non-conflicting *local* timestamps --
    // but `sequential_context_response`'s flat one-tick-per-unit model still
    // accumulates real seconds as a window's own target grows, and this
    // grid's last two windows sit only one tick apart (a real,
    // frequently-hit case of blind planning's own tail-anchor-vs-centered
    // collision -- see `plan_alignment_segments`'s doc comment), so even
    // wholly "clean" data still measures a genuine inversion here. This
    // attempt is expected to fail too, on its own honest merits.
    let attempt1_target = minimum_safe_target * 1.5;
    let attempt1 = plan_alignment_segments(200.0, adjusted_units.len(), attempt1_target).unwrap();
    assert_ne!(
        attempt1.len(),
        attempt0.len(),
        "the retry must actually replan with a different window grid"
    );
    for plan in &attempt1 {
        std::fs::write(
            control.join(format!("response-{}", 2 + plan.index)),
            sequential_context_response(&adjusted_units, plan),
        )
        .unwrap();
    }

    // Attempt 2 (widened again): a coarser, 4-window grid with real margin
    // between every window puts each window's own flat-tick accumulation
    // safely inside its neighbor's gap, so this attempt succeeds cleanly.
    let attempt2_target = minimum_safe_target * 1.5 * 1.5;
    let attempt2 = plan_alignment_segments(200.0, adjusted_units.len(), attempt2_target).unwrap();
    assert_ne!(
        attempt2.len(),
        attempt1.len(),
        "the second retry must actually replan with yet another window grid"
    );
    for plan in &attempt2 {
        std::fs::write(
            control.join(format!("response-{}", 2 + attempt1.len() + plan.index)),
            sequential_context_response(&adjusted_units, plan),
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
    let destination = run_align_with_trailing_content_dropped(
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
    let words_out = evidence["words"].as_array().unwrap();
    assert_eq!(words_out.len(), adjusted_units.len());
    assert_words_are_ordered_and_non_overlapping(words_out);
    // Only the successful attempt's progress reaches the real callback --
    // attempt 0 and attempt 1's own buffered progress must never leak
    // through and appear to regress once attempt 2's real sequence begins.
    assert_eq!(progress_calls.len(), attempt2.len());

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
    // Window 1's plan starts at the tail anchor (60.00s), but chaining
    // raises the actual offset used to 63.20s (window 0's "five" ends at
    // 65.20s, minus the chain's 2.00s back margin) once window 0's real
    // measurement is known. "six" (the owned target) is deliberately
    // timed -- relative to that 63.20s offset -- to still overlap the
    // previous window's "five" by 2 ticks after offsetting.
    std::fs::write(
        control.join("response-1"),
        serde_json::to_vec(&serde_json::json!({"words": [
            {"word": "three", "start": 0.00, "end": 0.32},
            {"word": "four", "start": 0.32, "end": 0.64},
            {"word": "five", "start": 0.64, "end": 0.96},
            {"word": "six", "start": 1.84, "end": 2.80},
            {"word": "seven", "start": 2.80, "end": 3.12},
            {"word": "eight", "start": 3.12, "end": 3.44},
            {"word": "nine", "start": 3.44, "end": 3.76},
            {"word": "ten", "start": 3.76, "end": 4.08}
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
