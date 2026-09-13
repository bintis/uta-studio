//! Reanalyze one song through Studio's configured Engine queue and publication.
//!
//! Usage: cargo run -p uta-studio-core --example reanalyze_song -- <file_hash>

use std::process::ExitCode;
use std::time::Duration;

use app_core::{AnalysisDefaultTarget, AnalysisQueue, QueuedStatus};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reanalyze_song: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let file_hash = arguments
        .next()
        .ok_or_else(|| "usage: reanalyze_song <file_hash>".to_string())?;
    if arguments.next().is_some() {
        return Err("usage: reanalyze_song <file_hash>".to_string());
    }

    app_core::init_library().map_err(|error| error.to_string())?;
    println!("{file_hash}: preparing analysis with current Studio settings");
    let staged = app_core::preview_and_stage_engine_run(
        &file_hash,
        Some(AnalysisDefaultTarget::FullCandidate),
    )?;
    println!("{file_hash}: Staged (request {})", staged.request_id);
    app_core::start_queued_analysis(&file_hash)?;

    let mut previous = Some(QueuedStatus::Staged);
    loop {
        let status = AnalysisQueue::load()
            .entries
            .get(&file_hash)
            .cloned()
            .ok_or_else(|| format!("{file_hash}: analysis queue entry disappeared"))?;
        if previous.as_ref() != Some(&status) {
            println!("{file_hash}: {status:?}");
            previous = Some(status.clone());
        }
        match status {
            QueuedStatus::Completed => return Ok(()),
            QueuedStatus::Failed(error) => return Err(format!("{file_hash}: {error}")),
            _ => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}
