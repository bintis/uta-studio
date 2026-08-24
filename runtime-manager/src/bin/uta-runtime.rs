use std::process::ExitCode;

fn main() -> ExitCode {
    let code = uta_runtime_manager::cli::main_entry();
    ExitCode::from(u8::try_from(code).unwrap_or(70))
}
