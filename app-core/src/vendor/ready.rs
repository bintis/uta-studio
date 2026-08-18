use super::*;
use crate::vendor_scripts;

pub fn refresh_analyzer_scripts_if_ready() -> Result<(), String> {
    let managed_environment_ready = std::fs::read_to_string(ready_marker())
        .is_ok_and(|value| ready_marker_is_compatible(&value))
        && python_path().is_file();
    if !managed_environment_ready {
        return Ok(());
    }

    vendor_scripts::write_scripts(&analyzer_dir())
        .map_err(|e| format!("Failed to refresh analyzer scripts: {e}"))
}

pub fn mark_ready() -> Result<(), String> {
    std::fs::write(ready_marker(), expected_ready_marker())
        .map_err(|e| format!("Failed to mark ready: {e}"))
}
