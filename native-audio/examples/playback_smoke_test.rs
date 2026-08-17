//! Scratch smoke test for Phase 9 §9.2/§9.5's real audio decode/playback
//! and PipeWire acceptance items -- run manually, not part of the crate's
//! test suite. Loads and plays a real audio file through the same
//! `EditorAudioPlayer` the desktop app's editor and library playback use,
//! independent of the Bevy UI, and reports whatever the native pipeline
//! itself reports.
//!
//! Usage: cargo run -p uta-studio-audio --example playback_smoke_test -- <path> <seconds>

use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: <path> <seconds>");
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);

    let player = uta_studio_audio::EditorAudioPlayer::new();
    let path = std::path::PathBuf::from(path);

    match player.load_path(&path) {
        Ok(status) => println!("load_path OK: {status:?}"),
        Err(error) => {
            println!("load_path FAILED: {error}");
            std::process::exit(1);
        }
    }

    match player.play() {
        Ok(status) => println!("play() OK: {status:?}"),
        Err(error) => {
            println!("play() FAILED: {error}");
            std::process::exit(1);
        }
    }

    for i in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        match player.status() {
            Ok(status) => println!("t+{}s status: {status:?}", i + 1),
            Err(error) => println!("t+{}s status FAILED: {error}", i + 1),
        }
    }

    match player.stop() {
        Ok(status) => println!("stop() OK: {status:?}"),
        Err(error) => println!("stop() FAILED: {error}"),
    }
}
