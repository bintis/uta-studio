#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod studio;
mod theme;

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("__GL_THREADED_OPTIMIZATIONS", "0");
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }

    if let Err(error) = app_core::startup() {
        eprintln!("Uta Studio could not initialize: {error}");
        let description = if error.to_ascii_lowercase().contains("disk is full")
            || error
                .to_ascii_lowercase()
                .contains("database or disk is full")
        {
            format!(
                "Uta Studio could not open its library because the disk is full. Free some space and reopen the application.\n\nTechnical detail: {error}"
            )
        } else {
            format!(
                "Uta Studio could not open its local library. Your source media was not changed.\n\nTechnical detail: {error}"
            )
        };
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Uta Studio could not start")
            .set_description(description)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }

    studio::run();
    app_core::shutdown_server();
}
