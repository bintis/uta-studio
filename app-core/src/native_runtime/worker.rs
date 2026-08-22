use std::path::PathBuf;

pub fn native_analyzer_path() -> Option<PathBuf> {
    if let Some(path) = super::component_executable("native_analyzer") {
        return Some(path);
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    let names: &[&str] = if cfg!(windows) {
        &["uta-native-analyzer.exe"]
    } else {
        &["uta-native-analyzer"]
    };
    names
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}
