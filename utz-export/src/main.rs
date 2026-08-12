use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("uta-studio-export: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    app_core::init_library().map_err(|error| error.to_string())?;
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("list") => {
            let songs = app_core::list_exportable_songs().map_err(|error| error.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&songs).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        Some("export") => {
            let hash = arguments.next().ok_or_else(usage)?;
            let output = PathBuf::from(arguments.next().ok_or_else(usage)?);
            if arguments.next().is_some() {
                return Err(usage());
            }
            let path = app_core::export_utz(&hash, output).map_err(|error| error.to_string())?;
            println!("{}", path.display());
            Ok(())
        }
        Some("export-ultrastar") => {
            let hash = arguments.next().ok_or_else(usage)?;
            let output = PathBuf::from(arguments.next().ok_or_else(usage)?);
            if arguments.next().is_some() {
                return Err(usage());
            }
            let path =
                app_core::export_ultrastar(&hash, output).map_err(|error| error.to_string())?;
            println!("{}", path.display());
            Ok(())
        }
        Some("check") => {
            let hash = arguments.next().ok_or_else(usage)?;
            if arguments.next().is_some() {
                return Err(usage());
            }
            app_core::load_chart(&hash).map_err(|error| error.to_string())?;
            println!("{hash}: editable");
            Ok(())
        }
        Some("check-all") => {
            if arguments.next().is_some() {
                return Err(usage());
            }
            let songs = app_core::list_exportable_songs().map_err(|error| error.to_string())?;
            let mut checked = 0usize;
            let mut failures = Vec::new();
            for song in songs {
                let readiness = app_core::chart_readiness(&song.file_hash)
                    .map_err(|error| error.to_string())?;
                if !readiness.ready {
                    continue;
                }
                checked += 1;
                if let Err(error) = app_core::load_chart(&song.file_hash) {
                    failures.push(format!("{} · {}: {error}", song.artist, song.title));
                }
            }
            println!("checked {checked} editable charts");
            if failures.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{} chart(s) failed:\n{}",
                    failures.len(),
                    failures.join("\n")
                ))
            }
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: uta-studio-export list | export <file-hash> <output.utz> | export-ultrastar <file-hash> <output.txt> | check <file-hash> | check-all".into()
}
