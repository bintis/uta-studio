fn main() {
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-native-analyzer requires --stdio-json");
        std::process::exit(2);
    }
    if let Err(error) = uta_analysis_engine::worker::compatibility_worker_main() {
        eprintln!("native analyzer compatibility worker failed: {error}");
        std::process::exit(3);
    }
}
