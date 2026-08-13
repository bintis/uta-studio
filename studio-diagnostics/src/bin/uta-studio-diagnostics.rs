use std::process::ExitCode;

use uta_studio_diagnostics::{DiagnosticRequest, run_feature_diagnostics};

fn main() -> ExitCode {
    let mut request = DiagnosticRequest::default();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--exports" => request.include_export_smoke = true,
            "--file-hash" => request.file_hash = args.next(),
            "--help" | "-h" => {
                println!(
                    "Usage: uta-studio-diagnostics [--exports] [--file-hash HASH]\n\
                     Diagnostics never alter source media or settings. Export smoke files are\n\
                     created only in a unique temporary directory and removed before exit."
                );
                return ExitCode::SUCCESS;
            }
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                return ExitCode::FAILURE;
            }
        }
    }

    let report = run_feature_diagnostics(request);
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("Could not serialize diagnostic report: {error}");
            return ExitCode::FAILURE;
        }
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
