#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod studio;
mod theme;

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("__GL_THREADED_OPTIMIZATIONS", "0");
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");

        // COSMIC can terminate some Vulkan Wayland surfaces under sustained UI
        // interaction, so GLES remains the compatibility default there. Mesa
        // Intel's Wayland GLES surface can instead report no present modes;
        // those devices use the verified Vulkan path. Preserve every explicit
        // user/backend override for diagnostics and future drivers.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let has_explicit_backend = std::env::var_os("WGPU_BACKEND").is_some();
        if automatic_wgpu_backend(&desktop, has_explicit_backend, has_intel_drm_adapter())
            == Some("gl")
        {
            std::env::set_var("WGPU_BACKEND", "gl");
        }
    }

    if let Err(error) = app_core::startup() {
        eprintln!("Uta! Studio could not initialize: {error}");
        let description = if error.to_ascii_lowercase().contains("disk is full")
            || error
                .to_ascii_lowercase()
                .contains("database or disk is full")
        {
            format!(
                "Uta! Studio could not open its library because the disk is full. Free some space and reopen the application.\n\nTechnical detail: {error}"
            )
        } else {
            format!(
                "Uta! Studio could not open its local library. Your source media was not changed.\n\nTechnical detail: {error}"
            )
        };
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Uta! Studio could not start")
            .set_description(description)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }

    studio::run();
}

#[cfg(target_os = "linux")]
fn automatic_wgpu_backend(
    desktop: &str,
    has_explicit_backend: bool,
    has_intel_adapter: bool,
) -> Option<&'static str> {
    (desktop.to_ascii_lowercase().contains("cosmic") && !has_explicit_backend && !has_intel_adapter)
        .then_some("gl")
}

#[cfg(target_os = "linux")]
fn has_intel_drm_adapter() -> bool {
    std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("device/vendor")).ok())
        .any(|vendor| vendor.trim().eq_ignore_ascii_case("0x8086"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn cosmic_intel_uses_the_verified_vulkan_default() {
        assert_eq!(automatic_wgpu_backend("COSMIC", false, true), None);
    }

    #[test]
    fn cosmic_non_intel_keeps_the_existing_gles_compatibility_path() {
        assert_eq!(automatic_wgpu_backend("COSMIC", false, false), Some("gl"));
    }

    #[test]
    fn explicit_renderer_selection_always_wins() {
        assert_eq!(automatic_wgpu_backend("COSMIC", true, false), None);
    }
}
