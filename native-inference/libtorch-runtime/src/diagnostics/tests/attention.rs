use super::*;

const OPERATOR_PHASES: [&str; 3] = [
    "attention_operator_begin",
    "attention_operator_await",
    "attention_operator_complete",
];

const OPERATOR_DETAILS: [&str; 6] = [
    "qwen.strict.output_allocate",
    "qwen.strict.kv_pack",
    "qwen.strict.query_view",
    "qwen.strict.scores",
    "qwen.strict.softmax",
    "qwen.strict.values",
];

fn append_event(
    buffer: &mut RecordBuffer,
    writer: &mut ObservedWriter,
    phase: &str,
    detail: &str,
    focus: Option<&str>,
    elapsed: Duration,
) -> io::Result<()> {
    buffer.append(
        writer,
        &serde_json::json!({"phase": phase, "detail": detail}),
        requires_sync(phase, detail, focus),
        elapsed,
    )
}

fn call_count(writer: &ObservedWriter, name: &str) -> usize {
    writer
        .calls
        .borrow()
        .iter()
        .filter(|call| **call == name)
        .count()
}

fn records(writer: &ObservedWriter) -> Vec<serde_json::Value> {
    std::str::from_utf8(&writer.bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn only_known_qwen_operator_triplets_are_batched() {
    for phase in OPERATOR_PHASES {
        for operation in OPERATOR_DETAILS {
            let detail = format!("{operation} batch=0 kv_head=0 query_head=0");
            assert!(!requires_sync(phase, &detail, None));
            assert!(!requires_sync(phase, &detail, Some("unrelated")));
            assert!(requires_sync(phase, &detail, Some("qwen.strict")));
            assert!(requires_sync(phase, &detail, Some(operation)));
        }
        for detail in [
            "",
            "other.strict.values batch=0",
            "qwen.strict.unknown batch=0",
            "qwen.strict.values_extra batch=0",
            "qwen.strict.values.suffix batch=0",
        ] {
            assert!(requires_sync(phase, detail, None), "{phase}: {detail}");
        }
    }

    for phase in [
        "attention_operator_error",
        "native_error",
        "device_create_begin",
        "model_request",
        "forward_begin",
        "compute_begin",
        "compute_complete",
        "forward_complete",
        "model_free_begin",
        "model_free_complete",
        "library_unload_begin",
        "library_unload_complete",
    ] {
        assert!(requires_sync(phase, "qwen.strict.values", None), "{phase}");
    }

    assert!(!requires_sync(
        "attention_operator_begin",
        "qwen.strict.values batch=0",
        Some("qwen.strict.value")
    ));
}

#[test]
fn layer_entries_attention_entries_and_completed_tiles_remain_durable() {
    for (phase, detail) in [
        ("qwen_encoder_layer_begin", "enc.blocks.0."),
        ("qwen_decoder_layer_begin", "dec.blocks.13."),
        ("qwen_encoder_layer_begin", "audio.encoder.blk.0."),
        ("qwen_decoder_layer_begin", "blk.13."),
        ("qwen_attention_begin", "strict"),
        ("qwen_stage_complete", "encoder.attention_window"),
        ("qwen_stage_complete", "decoder.attention_tile"),
    ] {
        assert!(requires_sync(phase, detail, None), "{phase}: {detail}");
        let mut buffer = RecordBuffer::default();
        let mut writer = ObservedWriter::default();
        append_event(
            &mut buffer,
            &mut writer,
            phase,
            detail,
            None,
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
        assert_eq!(records(&writer)[0]["phase"], phase);
    }
}

// One ordinary attention tile: 1 allocation, 8 physical KV preparations and
// 16 query heads with 4 steps each. This is a synthetic journal workload only,
// not a GPU execution, a system-call trace or a speed/stability measurement.
fn ordinary_attention_steps() -> Vec<&'static str> {
    let mut steps = vec!["qwen.strict.output_allocate"];
    for _ in 0..8 {
        steps.push("qwen.strict.kv_pack");
        for _ in 0..2 {
            steps.extend_from_slice(&[
                "qwen.strict.query_view",
                "qwen.strict.scores",
                "qwen.strict.softmax",
                "qwen.strict.values",
            ]);
        }
    }
    steps
}

#[test]
fn both_qwen_routes_preserve_every_event_with_fewer_disk_sync_requests() {
    let steps = ordinary_attention_steps();
    assert_eq!(steps.len(), 73);
    for (model, layer, completed_tile) in [
        ("qwen3_asr_1_7b", "dec.blocks.13.", "decoder.attention_tile"),
        (
            "qwen3_forced_aligner_0_6b",
            "blk.13.",
            "decoder.attention_tile",
        ),
        (
            "qwen3_asr_1_7b",
            "enc.blocks.0.",
            "encoder.attention_window",
        ),
    ] {
        for focus in [None, Some("qwen.strict")] {
            let mut buffer = RecordBuffer::default();
            let mut writer = ObservedWriter::default();
            let mut expected = Vec::new();
            let layer_phase = if completed_tile == "encoder.attention_window" {
                "qwen_encoder_layer_begin"
            } else {
                "qwen_decoder_layer_begin"
            };
            for (phase, detail) in [
                ("model_request", model),
                (layer_phase, layer),
                ("qwen_attention_begin", "strict"),
            ] {
                append_event(
                    &mut buffer,
                    &mut writer,
                    phase,
                    detail,
                    focus,
                    Duration::ZERO,
                )
                .unwrap();
                expected.push(serde_json::json!({"phase": phase, "detail": detail}));
            }
            assert_eq!(call_count(&writer, "sync"), 3);
            for detail in &steps {
                for phase in OPERATOR_PHASES {
                    append_event(
                        &mut buffer,
                        &mut writer,
                        phase,
                        detail,
                        focus,
                        Duration::from_millis(10),
                    )
                    .unwrap();
                    expected.push(serde_json::json!({"phase": phase, "detail": detail}));
                }
            }
            // Without focus, all short detail records still fit the existing
            // byte budget and must not add one disk sync for each event.
            let detail_syncs = if focus.is_some() { steps.len() * 3 } else { 0 };
            assert_eq!(call_count(&writer, "sync"), 3 + detail_syncs);
            append_event(
                &mut buffer,
                &mut writer,
                "qwen_stage_complete",
                completed_tile,
                focus,
                Duration::from_millis(20),
            )
            .unwrap();
            expected.push(
                serde_json::json!({"phase": "qwen_stage_complete", "detail": completed_tile}),
            );
            assert_eq!(call_count(&writer, "sync"), 4 + detail_syncs);
            assert_eq!(records(&writer), expected);
        }
    }
}

#[test]
fn focused_event_flushes_preceding_details_before_it_returns() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    append_event(
        &mut buffer,
        &mut writer,
        "attention_operator_complete",
        "qwen.strict.scores batch=0",
        Some("qwen.strict.values"),
        Duration::ZERO,
    )
    .unwrap();
    assert!(writer.calls.borrow().is_empty());
    append_event(
        &mut buffer,
        &mut writer,
        "attention_operator_begin",
        "qwen.strict.values batch=0",
        Some("qwen.strict.values"),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(*writer.calls.borrow(), ["write", "flush", "sync"]);
    let saved = records(&writer);
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0]["phase"], "attention_operator_complete");
    assert_eq!(saved[1]["phase"], "attention_operator_begin");
}

#[test]
fn batched_operator_records_keep_the_existing_time_and_byte_triggers() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    for elapsed in [Duration::from_millis(999), Duration::from_secs(1)] {
        append_event(
            &mut buffer,
            &mut writer,
            "attention_operator_await",
            "qwen.strict.values batch=0",
            None,
            elapsed,
        )
        .unwrap();
        let expected_syncs = usize::from(elapsed >= Duration::from_secs(1));
        assert_eq!(call_count(&writer, "sync"), expected_syncs);
    }
    assert_eq!(records(&writer).len(), 2);

    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    let detail = format!("qwen.strict.values diagnostic={}", "x".repeat(4096));
    for _ in 0..100 {
        append_event(
            &mut buffer,
            &mut writer,
            "attention_operator_complete",
            &detail,
            None,
            Duration::ZERO,
        )
        .unwrap();
    }
    assert!(call_count(&writer, "write") > 0);
    assert!(call_count(&writer, "write") < 10);
    assert_eq!(call_count(&writer, "sync"), 0);
    append_event(
        &mut buffer,
        &mut writer,
        "qwen_stage_complete",
        "decoder.attention_tile",
        None,
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(call_count(&writer, "sync"), 1);
    assert_eq!(records(&writer).len(), 101);
}

#[test]
fn interruption_does_not_fabricate_a_completed_attention_tile() {
    let mut buffer = RecordBuffer::default();
    let mut writer = ObservedWriter::default();
    append_event(
        &mut buffer,
        &mut writer,
        "qwen_attention_begin",
        "strict",
        None,
        Duration::ZERO,
    )
    .unwrap();
    for phase in ["attention_operator_begin", "attention_operator_await"] {
        append_event(
            &mut buffer,
            &mut writer,
            phase,
            "qwen.strict.values",
            None,
            Duration::from_millis(10),
        )
        .unwrap();
    }
    drop(buffer);
    let saved = records(&writer);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["phase"], "qwen_attention_begin");
    // Losing buffered detail is intentional and explicitly documented. This
    // mock drop is not a real power-loss test or a disk durability guarantee.
}

#[test]
fn error_flushes_pending_details_and_sync_failure_is_not_retried() {
    for (fail_write, fail_sync) in [(false, false), (true, false), (false, true)] {
        let mut buffer = RecordBuffer::default();
        let mut writer = ObservedWriter::default();
        append_event(
            &mut buffer,
            &mut writer,
            "attention_operator_begin",
            "qwen.strict.values",
            None,
            Duration::ZERO,
        )
        .unwrap();
        assert!(writer.calls.borrow().is_empty());
        writer.fail_write = fail_write;
        writer.fail_sync = fail_sync;
        let result = append_event(
            &mut buffer,
            &mut writer,
            "native_error",
            "fixture: attention completion failed",
            None,
            Duration::ZERO,
        );
        assert_eq!(result.is_err(), fail_write || fail_sync);
        assert_eq!(call_count(&writer, "write"), 1);
        assert_eq!(call_count(&writer, "sync"), usize::from(!fail_write));
        if !fail_write {
            let saved = records(&writer);
            assert_eq!(saved.len(), 2);
            assert_eq!(saved[1]["phase"], "native_error");
            assert!(
                !saved
                    .iter()
                    .any(|record| record["phase"] == "qwen_stage_complete")
            );
        }
    }
}

#[test]
fn actual_journal_uses_the_policy_and_publishes_its_limits() {
    let fixture = Fixture::new();
    let mut journal = Journal::create(&fixture.0).unwrap();
    // No process environment mutation; the fixture checks the unfocused path.
    journal.focus = None;
    journal.record("qwen_attention_begin", "strict").unwrap();
    journal
        .record("attention_operator_begin", "qwen.strict.query_view")
        .unwrap();
    journal
        .record("attention_operator_await", "qwen.strict.query_view")
        .unwrap();
    journal
        .record("attention_operator_complete", "qwen.strict.query_view")
        .unwrap();
    journal
        .record("qwen_stage_complete", "decoder.attention_tile")
        .unwrap();
    let files = fs::read_dir(&fixture.0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let text = fs::read_to_string(files[0].path()).unwrap();
    let saved: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(saved.len(), 7);
    let policy: serde_json::Value =
        serde_json::from_str(saved[1]["detail"].as_str().unwrap()).unwrap();
    assert!(
        policy["details"]
            .as_str()
            .unwrap()
            .contains("Qwen strict operator triplets")
    );
    assert!(
        policy["durable"]
            .as_str()
            .unwrap()
            .contains("completed attention tiles")
    );
    assert!(
        policy["limitation"]
            .as_str()
            .unwrap()
            .contains("next event")
    );
    for (index, record) in saved.iter().enumerate() {
        assert_eq!(record["sequence"], index);
    }
    assert_eq!(saved.last().unwrap()["detail"], "decoder.attention_tile");
}
