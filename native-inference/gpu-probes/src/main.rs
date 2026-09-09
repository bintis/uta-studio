fn main() -> std::process::ExitCode {
    match uta_gpu_probes::probe_vulkan()
        .and_then(|probe| serde_json::to_string_pretty(&probe).map_err(|error| error.to_string()))
    {
        Ok(json) => {
            println!("{json}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
