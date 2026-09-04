//! Manual, real-hardware GPU vs CPU parity check for the GAME native engine.
//!
//! Ignored by default: it needs a real GGUF model, a real audio file, and
//! (obviously) a working Vulkan adapter for the `gpu` feature. A pinned
//! nonzero `InferParams.seed` is required for this comparison to mean
//! anything — `Model::infer` randomizes the D3PM boundary-sampling seed via
//! `random_u64()` whenever `seed == 0`, so two separate process runs with
//! the default seed are *expected* to diverge on borderline note boundaries
//! even with a bit-identical backend; that is not a GPU bug.
//!
//! Run explicitly:
//! ```sh
//! GAME_GGUF_MODEL_PATH=/path/to/game-medium-f32.gguf \
//! GAME_TEST_AUDIO_PATH=/path/to/some.flac \
//! cargo test -p uta-game-worker --features gpu --test gpu_cpu_parity -- --ignored --nocapture
//! ```

#![cfg(feature = "gpu")]

use uta_game_worker::{Backend, InferParams, Model};

fn env_path(key: &str) -> std::path::PathBuf {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("set {key} to run this manual parity check"))
        .into()
}

#[test]
#[ignore]
fn gpu_matches_cpu_with_a_pinned_seed() {
    let model_path = env_path("GAME_GGUF_MODEL_PATH");
    let audio_path = env_path("GAME_TEST_AUDIO_PATH");
    let work_dir = std::env::temp_dir();
    let audio =
        uta_game_worker::audio::decode_mono(&audio_path, &work_dir, uta_game_worker::SAMPLE_RATE)
            .expect("decode test audio");

    let params = InferParams {
        seed: 424_242,
        d3pm_nsteps: 8,
        boundary_threshold: 0.2,
        boundary_radius: 2,
        note_threshold: 0.2,
        ..Default::default()
    };

    let cpu_model = Model::load(&model_path, Backend::Cpu).expect("load CPU model");
    let cpu_result = cpu_model.infer(&audio, &params).expect("cpu inference");

    let gpu_model = Model::load(&model_path, Backend::Gpu).expect("load GPU model");
    let gpu_result = gpu_model.infer(&audio, &params).expect("gpu inference");

    assert_eq!(
        cpu_result.notes.len(),
        gpu_result.notes.len(),
        "note count differs: cpu={} gpu={}",
        cpu_result.notes.len(),
        gpu_result.notes.len()
    );

    let mut max_start_delta = 0.0f32;
    let mut max_midi_delta = 0.0f32;
    for (index, (cpu_note, gpu_note)) in cpu_result
        .notes
        .iter()
        .zip(gpu_result.notes.iter())
        .enumerate()
    {
        assert_eq!(
            cpu_note.voiced, gpu_note.voiced,
            "note {index} voiced flag differs: cpu={cpu_note:?} gpu={gpu_note:?}"
        );
        let start_delta = (cpu_note.offset_seconds - gpu_note.offset_seconds).abs();
        let midi_delta = (cpu_note.pitch_midi - gpu_note.pitch_midi).abs();
        max_start_delta = max_start_delta.max(start_delta);
        max_midi_delta = max_midi_delta.max(midi_delta);
        assert!(
            start_delta < 0.02,
            "note {index} start differs by {start_delta}s: cpu={cpu_note:?} gpu={gpu_note:?}"
        );
        assert!(
            midi_delta < 0.5,
            "note {index} midi differs by {midi_delta}: cpu={cpu_note:?} gpu={gpu_note:?}"
        );
    }
    eprintln!(
        "parity ok: {} notes, max_start_delta={max_start_delta}s max_midi_delta={max_midi_delta}",
        cpu_result.notes.len()
    );
}
