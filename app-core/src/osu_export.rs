//! Export a song straight into the installed osu!lazer.
//!
//! The uta! ruleset registers `.utz` with lazer's public file-import pipeline,
//! so handing lazer a package path is the whole import: a fresh instance
//! imports the file on start-up, and a running instance receives the path
//! over lazer's own IPC. Studio writes the package into its own hand-off
//! folder -- never into a user folder -- and launches lazer with that path.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::cache::uta_studio_dir;
use crate::error::UtaStudioError;
use crate::export_destination::{ExportPackageKind, last_export_destination, record_last_export};
use crate::utz_export::export_utz;
use crate::vendor::configured_file_path;

/// Explicit override for the osu!lazer launcher, checked before discovery.
pub const OSU_EXECUTABLE_VARIABLE: &str = "UTA_STUDIO_OSU_PATH";
/// Flatpak application id published by the osu!lazer Flathub package.
pub const OSU_FLATPAK_ID: &str = "sh.ppy.osu";
/// Studio-owned hand-off folder under the Uta! Studio data directory.
pub const OSU_HANDOFF_DIRECTORY: &str = "osu-imports";

/// A discovered osu!lazer launcher and where the discovery found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsuInstallation {
    /// Executable to run with the package path as its only argument.
    pub launcher: PathBuf,
    /// Human-readable discovery source for notices and diagnostics.
    pub source: OsuLauncherSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsuLauncherSource {
    Configured,
    Path,
    Flatpak,
    AppImage,
    WindowsInstall,
    MacApplication,
}

/// Result of a completed hand-off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsuExportOutcome {
    /// The package lazer was asked to import.
    pub package: PathBuf,
    /// The launcher that received the package.
    pub installation: OsuInstallation,
}

/// Locates the installed osu!lazer without spawning anything.
pub fn detect_osu_lazer() -> Option<OsuInstallation> {
    detect_osu_lazer_in(&std::env::var_os("PATH").unwrap_or_default(), &home_directory())
}

fn detect_osu_lazer_in(path_variable: &std::ffi::OsStr, home: &Option<PathBuf>) -> Option<OsuInstallation> {
    if let Some(launcher) = configured_file_path(OSU_EXECUTABLE_VARIABLE) {
        return Some(OsuInstallation {
            launcher,
            source: OsuLauncherSource::Configured,
        });
    }
    let names: &[&str] = if cfg!(windows) {
        &["osu!.exe"]
    } else {
        &["osu!", "osu-lazer", "osu!.AppImage", "osu.AppImage"]
    };
    if let Some(launcher) = std::env::split_paths(path_variable)
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
    {
        return Some(OsuInstallation {
            launcher,
            source: OsuLauncherSource::Path,
        });
    }
    if cfg!(windows)
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        let launcher = PathBuf::from(local).join("osulazer").join("osu!.exe");
        if launcher.is_file() {
            return Some(OsuInstallation {
                launcher,
                source: OsuLauncherSource::WindowsInstall,
            });
        }
    }
    if cfg!(target_os = "macos") {
        let launcher = PathBuf::from("/Applications/osu!.app/Contents/MacOS/osu!");
        if launcher.is_file() {
            return Some(OsuInstallation {
                launcher,
                source: OsuLauncherSource::MacApplication,
            });
        }
    }
    if let Some(launcher) = flatpak_export_bins(home)
        .into_iter()
        .find(|candidate| candidate.is_file())
    {
        return Some(OsuInstallation {
            launcher,
            source: OsuLauncherSource::Flatpak,
        });
    }
    if let Some(launcher) = home
        .iter()
        .flat_map(|home| [home.join("Applications"), home.join(".local/bin"), home.join("AppImages")])
        .find_map(|directory| app_image_in(&directory))
    {
        return Some(OsuInstallation {
            launcher,
            source: OsuLauncherSource::AppImage,
        });
    }
    None
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Flatpak exports a launcher script per application; running it needs no
/// `flatpak run` wrapper and works for both user and system installs.
fn flatpak_export_bins(home: &Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data).join("flatpak"));
    }
    if let Some(home) = home {
        roots.push(home.join(".local/share/flatpak"));
    }
    roots.push(PathBuf::from("/var/lib/flatpak"));
    roots
        .into_iter()
        .map(|root| root.join("exports/bin").join(OSU_FLATPAK_ID))
        .collect()
}

fn app_image_in(directory: &Path) -> Option<PathBuf> {
    let mut candidates = std::fs::read_dir(directory)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_ascii_lowercase)
                    .is_some_and(|name| name.starts_with("osu") && name.ends_with(".appimage"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

/// The folder Studio writes hand-off packages into.
pub fn osu_handoff_directory() -> PathBuf {
    uta_studio_dir().join(OSU_HANDOFF_DIRECTORY)
}

/// Exports the song as a UTZ package into the hand-off folder and opens it
/// with the installed osu!lazer. Only a previous hand-off package for the
/// same song, inside the Studio-owned folder, is removed first; nothing
/// outside that folder is ever touched.
pub fn export_to_osu(file_hash: &str) -> Result<OsuExportOutcome, UtaStudioError> {
    let installation = detect_osu_lazer().ok_or_else(|| {
        UtaStudioError::Other(format!(
            "osu!lazer was not found. Install osu!lazer with the uta! ruleset, or point {OSU_EXECUTABLE_VARIABLE} at its executable."
        ))
    })?;
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    let directory = osu_handoff_directory();
    std::fs::create_dir_all(&directory)?;
    remove_previous_handoff(file_hash, &directory);
    let package = unique_package_path(&directory, &handoff_stem(&song.title, &song.artist));
    let package = export_utz(file_hash, &package)?;
    let _ = record_last_export(file_hash, ExportPackageKind::OsuLazer, &package);
    launch(&installation.launcher, &package).map_err(|error| {
        UtaStudioError::Other(format!(
            "exported {} but could not start osu!lazer ({}): {error}",
            package.display(),
            installation.launcher.display()
        ))
    })?;
    tracing::info!(
        "[export] Sent {} to osu!lazer via {}",
        package.display(),
        installation.launcher.display()
    );
    Ok(OsuExportOutcome {
        package,
        installation,
    })
}

fn remove_previous_handoff(file_hash: &str, directory: &Path) {
    let Some(previous) = last_export_destination(file_hash, ExportPackageKind::OsuLazer) else {
        return;
    };
    let inside_handoff = previous
        .parent()
        .zip(directory.canonicalize().ok())
        .and_then(|(parent, directory)| parent.canonicalize().ok().map(|parent| parent == directory))
        .unwrap_or(false);
    if inside_handoff && previous.is_file() {
        let _ = std::fs::remove_file(previous);
    }
}

fn handoff_stem(title: &str, artist: &str) -> String {
    let clean = |value: &str| {
        value
            .chars()
            .map(|character| match character {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => character,
            })
            .collect::<String>()
            .trim()
            .trim_matches('.')
            .to_string()
    };
    match (clean(title), clean(artist)) {
        (title, artist) if title.is_empty() && artist.is_empty() => "Uta! Studio song".to_string(),
        (title, artist) if artist.is_empty() => title,
        (title, artist) if title.is_empty() => artist,
        (title, artist) => format!("{artist} - {title}"),
    }
}

fn unique_package_path(directory: &Path, stem: &str) -> PathBuf {
    let first = directory.join(format!("{stem}.utz"));
    if !first.exists() {
        return first;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut attempt = 0u32;
    loop {
        let candidate = directory.join(if attempt == 0 {
            format!("{stem} ({stamp}).utz")
        } else {
            format!("{stem} ({stamp}-{attempt}).utz")
        });
        if !candidate.exists() {
            return candidate;
        }
        attempt += 1;
    }
}

/// Starts lazer detached from Studio's console and reaps it on a helper
/// thread so a short-lived forwarding instance never lingers as a zombie.
fn launch(launcher: &Path, package: &Path) -> std::io::Result<()> {
    let mut command = Command::new(launcher);
    command
        .arg(package)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = launcher.parent().filter(|directory| directory.is_dir()) {
        command.current_dir(directory);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS: lazer owns its own lifetime and console state.
        command.creation_flags(0x0000_0008);
    }
    let mut child = command.spawn()?;
    std::thread::Builder::new()
        .name("osu-lazer-handoff".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-osu-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn discovery_prefers_path_then_flatpak_then_app_image() {
        let root = scratch("discover");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(home.join("Applications")).unwrap();
        std::fs::create_dir_all(home.join(".local/share/flatpak/exports/bin")).unwrap();
        let empty_path = std::ffi::OsString::new();
        assert_eq!(detect_osu_lazer_in(&empty_path, &Some(home.clone())), None);

        let app_image = home.join("Applications/osu-lazer.AppImage");
        std::fs::write(&app_image, b"").unwrap();
        assert_eq!(
            detect_osu_lazer_in(&empty_path, &Some(home.clone())),
            Some(OsuInstallation {
                launcher: app_image.clone(),
                source: OsuLauncherSource::AppImage,
            })
        );

        let flatpak = home.join(".local/share/flatpak/exports/bin").join(OSU_FLATPAK_ID);
        std::fs::write(&flatpak, b"").unwrap();
        assert_eq!(
            detect_osu_lazer_in(&empty_path, &Some(home.clone())).map(|found| found.source),
            Some(OsuLauncherSource::Flatpak)
        );

        let on_path = bin.join(if cfg!(windows) { "osu!.exe" } else { "osu!" });
        std::fs::write(&on_path, b"").unwrap();
        let path_variable = std::env::join_paths([bin.clone()]).unwrap();
        assert_eq!(
            detect_osu_lazer_in(&path_variable, &Some(home.clone())),
            Some(OsuInstallation {
                launcher: on_path,
                source: OsuLauncherSource::Path,
            })
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn handoff_names_are_filesystem_safe_and_never_overwrite() {
        assert_eq!(handoff_stem("Asphodelos", "Rena"), "Rena - Asphodelos");
        assert_eq!(handoff_stem("A/B: C?", ""), "A_B_ C_");
        assert_eq!(handoff_stem("", ""), "Uta! Studio song");

        let root = scratch("unique");
        let first = unique_package_path(&root, "song");
        assert_eq!(first, root.join("song.utz"));
        std::fs::write(&first, b"").unwrap();
        let second = unique_package_path(&root, "song");
        assert_ne!(second, first);
        assert!(!second.exists());
        assert_eq!(second.extension().and_then(|ext| ext.to_str()), Some("utz"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_launcher_is_reported_before_any_export_work() {
        let root = scratch("missing");
        let launcher = root.join("not-osu");
        let error = launch(&launcher, &root.join("song.utz")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(root);
    }
}
