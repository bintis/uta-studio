mod docs;

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("docs") {
        return match docs::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }

    let (cmd, desktop_args) = match args.first().map(|s| s.as_str()) {
        Some("dev") => ("run", &args[1..]),
        Some("build") => ("build", &args[1..]),
        _ => {
            eprintln!(
                "Usage: cargo desktop <dev|build> [extra cargo args...]\n       cargo xtask docs <build|check>"
            );
            return ExitCode::FAILURE;
        }
    };

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live one level below workspace root");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(workspace_root)
        .arg(cmd)
        .args(["-p", "uta-studio-desktop"])
        .args(desktop_args)
        .env("WINIT_UNIX_BACKEND", "wayland")
        .env_remove("DISPLAY");

    match command.status() {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Failed to run the Uta! Studio desktop command: {e}");
            ExitCode::FAILURE
        }
    }
}
