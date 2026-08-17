//! Scratch smoke test for Phase 9 §9.2/§9.5's real UTZ/UltraStar export
//! acceptance items -- run manually, not part of the crate's test suite.
//! Exports a real analyzed song from the configured library to a scratch
//! directory and validates the result. Read-only toward the library; never
//! writes to the user's configured export folder.
//!
//! Usage: cargo run -p uta-studio-core --example export_smoke_test -- <file_hash> <out_dir>

fn main() {
    let mut args = std::env::args().skip(1);
    let file_hash = args.next().expect("usage: <file_hash> <out_dir>");
    let out_dir = args.next().expect("usage: <file_hash> <out_dir>");

    app_core::init_library().expect("init_library");

    let out_dir = std::path::PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let utz_path = out_dir.join(format!("{file_hash}.utz"));
    match app_core::export_utz(&file_hash, &utz_path) {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("UTZ export OK: {} ({size} bytes)", path.display());
        }
        Err(error) => println!("UTZ export FAILED: {error}"),
    }

    let ultrastar_path = out_dir.join(format!("{file_hash}.txt"));
    match app_core::export_ultrastar(&file_hash, &ultrastar_path) {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("UltraStar export OK: {} ({size} bytes)", path.display());
            match app_core::validate_ultrastar_chart(&path) {
                Ok(()) => println!("UltraStar validation OK"),
                Err(error) => println!("UltraStar validation FAILED: {error}"),
            }
        }
        Err(error) => println!("UltraStar export FAILED: {error}"),
    }
}
