//! Read-only playlist storage for the local source folder.

use std::collections::HashSet;

use rusqlite::params;

use super::connection::with_conn_mut;

#[derive(Debug, Clone)]
pub struct PlaylistDefinition {
    pub id: String,
    pub name: String,
    /// Local song paths in playlist order.
    pub song_keys: Vec<String>,
}

/// Atomically replace playlist navigation data for the active library source.
/// Entries not present in the scanned song catalogue are ignored.
pub fn replace_all_playlists(playlists: &[PlaylistDefinition]) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        let tx = c.transaction()?;
        tx.execute("DELETE FROM playlists", [])?;

        {
            let mut insert_playlist =
                tx.prepare("INSERT INTO playlists (id, name) VALUES (?1, ?2)")?;
            let mut insert_entry = tx.prepare(
                "INSERT INTO playlist_songs (playlist_id, song_id, position)
                 VALUES (?1, ?2, ?3)",
            )?;
            let mut find_local = tx.prepare("SELECT id FROM songs WHERE path = ?1 LIMIT 1")?;
            for playlist in playlists {
                if playlist.id.is_empty() || playlist.name.trim().is_empty() {
                    continue;
                }
                insert_playlist.execute(params![playlist.id, playlist.name.trim()])?;

                // Duplicate entries break stable song identity and add little value
                // in navigation. Keep the first occurrence and upstream ordering.
                let mut seen_song_ids = HashSet::new();
                for (position, key) in playlist.song_keys.iter().enumerate() {
                    let song_id = find_local.query_row([key], |r| r.get::<_, i64>(0)).ok();
                    let Some(song_id) = song_id else {
                        continue;
                    };
                    if !seen_song_ids.insert(song_id) {
                        continue;
                    }
                    insert_entry.execute(params![playlist.id, song_id, position as i64])?;
                }
            }
        }

        tx.commit()
    })
}
