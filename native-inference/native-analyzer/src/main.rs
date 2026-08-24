fn main() {
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-native-analyzer requires --stdio-json");
        std::process::exit(2);
    }
    if std::env::var("UTA_STUDIO_RUNTIME_LOCK_SHA256")
        .ok()
        .is_some_and(|expected| expected != uta_runtime_manager::runtime_lock::RUNTIME_LOCK_SHA256)
    {
        eprintln!("runtime-lock identity does not match this native analyzer build");
        std::process::exit(3);
    }
    if let Err(error) = uta_analysis_engine::worker::compatibility_worker_main() {
        eprintln!("native analyzer compatibility worker failed: {error}");
        std::process::exit(3);
    }
}
