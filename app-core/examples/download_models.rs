//! Dev tool: downloads the models needed for a real end-to-end pipeline
//! run (separator + pitch), using the exact same production code path the
//! Settings UI's "Download" buttons call (`app_core::step_download_model`).
//! Needed to safely verify a Phase 4 §4.2 pipeline split against real
//! inference instead of guessing.
//!
//! Usage: cargo run -p uta-studio-core --example download_models

fn main() {
    for target in [
        app_core::ModelDownloadTarget::Separator,
        app_core::ModelDownloadTarget::Pitch,
    ] {
        println!("=== downloading {target:?} ===");
        let result = app_core::step_download_model(target, |line| println!("{line}"));
        match result {
            Ok(()) => println!("=== {target:?} done ==="),
            Err(e) => {
                eprintln!("=== {target:?} FAILED: {e} ===");
                std::process::exit(1);
            }
        }
    }
}
