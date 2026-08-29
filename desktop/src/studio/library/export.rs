use super::*;
use crate::studio::*;

pub(crate) fn open_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => match open::that_detached(&path) {
            Ok(()) => localized_message(
                config,
                UiMessage::PathOpened,
                &[("{path}", &path.display().to_string())],
            ),
            Err(error) => format!("Could not open {}: {error}", path.display()),
        },
        Err(error) => format!("Could not open this library item: {error}"),
    }
}

pub(crate) fn reveal_library_entry(path: &std::path::Path, config: &AppConfig) -> String {
    match validate_source_path(path, config) {
        Ok(path) => {
            let target = if path.is_dir() {
                path.as_path()
            } else if let Some(parent) = path.parent() {
                parent
            } else {
                path.as_path()
            };
            match open::that_detached(target) {
                Ok(()) => format!("Revealed {}", path.display()),
                Err(error) => format!("Could not reveal {}: {error}", path.display()),
            }
        }
        Err(error) => format!("Could not reveal this library item: {error}"),
    }
}

pub(crate) fn export_song(
    file_hash: &str,
    extension: &str,
    export_directory: Option<&std::path::Path>,
) -> String {
    let song = match app_core::load_song_by_hash(file_hash) {
        Ok(Some(song)) => song,
        Ok(None) => return format!("Song not found: {file_hash}"),
        Err(error) => return error.to_string(),
    };
    let file_name = format!("{}.{}", safe_file_stem(&song.title), extension);
    let mut dialog = rfd::FileDialog::new().set_file_name(file_name);
    if let Some(path) = export_directory {
        dialog = dialog.set_directory(path);
    }
    dialog = if extension == "utz" {
        dialog.add_filter("Uta package", &["utz"])
    } else {
        dialog.add_filter("UltraStar chart", &["txt"])
    };
    let Some(output) = dialog.save_file() else {
        return "Export cancelled.".to_string();
    };
    let result = if extension == "utz" {
        app_core::export_utz(file_hash, &output)
    } else {
        app_core::export_ultrastar(file_hash, &output)
    };
    match result {
        Ok(path) => format!("Exported {}", path.display()),
        Err(error) => format!("Export failed: {error}"),
    }
}

pub(crate) fn export_all_songs(
    songs: &[Song],
    extension: &str,
    export_directory: &std::path::Path,
) -> String {
    let mut title_counts = HashMap::<String, usize>::new();
    for song in songs {
        *title_counts
            .entry(safe_file_stem(&song.title).to_lowercase())
            .or_default() += 1;
    }

    let mut used_stems = HashMap::<String, usize>::new();
    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for song in songs {
        let title = safe_file_stem(&song.title);
        let mut stem = if title_counts
            .get(&title.to_lowercase())
            .copied()
            .unwrap_or_default()
            > 1
        {
            format!("{} — {}", title, safe_file_stem(&song.artist))
        } else {
            title
        };
        let collision_key = stem.to_lowercase();
        let collision = used_stems.entry(collision_key).or_default();
        if *collision > 0 {
            let suffix = song.file_hash.chars().take(8).collect::<String>();
            stem = format!("{stem} — {suffix}");
        }
        *collision += 1;

        let output = export_directory.join(format!("{stem}.{extension}"));
        if output.exists() {
            skipped += 1;
            continue;
        }
        let result = match extension {
            "utz" => app_core::export_utz(&song.file_hash, &output),
            "txt" => app_core::export_ultrastar(&song.file_hash, &output),
            _ => unreachable!("batch export extensions are fixed by the UI"),
        };
        match result {
            Ok(_) => exported += 1,
            Err(error) => failures.push(format!("{}: {error}", song.title)),
        }
    }

    let mut summary = format!(
        "Export all finished · {exported} exported · {skipped} already existed · {} failed · {}",
        failures.len(),
        export_directory.display()
    );
    if !failures.is_empty() {
        summary.push_str(" · ");
        summary.push_str(
            &failures
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
        );
        if failures.len() > 3 {
            summary.push_str(&format!("; and {} more", failures.len() - 3));
        }
    }
    summary
}

pub(crate) fn safe_file_stem(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect::<String>();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "Uta! Studio Export".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn refresh_library_while_scanning(
    time: Res<Time>,
    mut timer: ResMut<LibraryRefreshTimer>,
    mut library: ResMut<LibraryState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !library.scanning || !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let previous = (
        library.meta.processed_count,
        library.meta.songs_count,
        library.meta.videos_count,
    );
    library.refresh();
    let current = (
        library.meta.processed_count,
        library.meta.songs_count,
        library.meta.videos_count,
    );
    if current != previous {
        invalidated.invalidate(UiDirtyRegion::Library);
    }
    if library.meta.processed_count >= library.meta.count && library.meta.count > 0 {
        library.scanning = false;
        invalidated.invalidate(UiDirtyRegion::Library);
    }
}
