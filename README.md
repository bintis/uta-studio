# Uta Studio

Uta Studio is the GPL-3.0 desktop authoring application for the Uta Project.
It is a Rust/Tauri application focused exclusively on producing
editable song charts from local audio and video files.

## Product boundary

- Select and scan a local folder.
- Separate instrumental and guide-vocal stems with local AI tooling.
- Transcribe and align word-level lyrics.
- Analyse pitch, then manually correct lyric timing and MIDI note bars in the
  built-in timeline editor.
- Export a self-contained `.utz` package for the independent `uta!` game or a
  UTF-8 UltraStar 1.1 `.txt` bundle with sibling media.

Uta Studio does not connect to Plex, Jellyfin, Navidrome, or another media
server. It does not ship a self-hosted Web server, Docker deployment,
player profiles, microphone capture, Sonos discovery, persistent scoring, or
result screens. Playback and scoring belong to `uta`'s versioned Rust/WASM
engine; edited note MIDI is the authoritative target there.

The canonical UTZ format lives in the independent sibling `utz` repository.
Studio vendors a pinned MIT-licensed `utz` 0.1.0 crate snapshot under
`vendor/utz` so a standalone Studio checkout and its Nix build never depend on
a particular parent-directory layout.

The default settings directory is `~/.uta-studio`; generated songs, models, and
runtime data may be assigned independent locations in Settings.

Durable product, audio, API, testing, and AI-agent rules live in
[`docs/engineering-constraints.md`](docs/engineering-constraints.md) and the
repository-level [`AGENTS.md`](AGENTS.md).

## Build

```sh
cargo test -p uta-studio-core --lib
pnpm --dir client build
cargo check --workspace
```

Run the desktop app with:

```sh
pnpm --dir client tauri dev
```

## Editing and export

Select an analysed song and choose **Edit chart** to audition local audio. The
editor includes a decoded waveform, pitch trace, multi-note marquee selection,
group move/transpose/resize, split/merge, clipboard operations, configurable
quantization, phrase and word boundary editing, UltraStar note types, global
gap correction, and a chart-issue inspector with conservative automatic timing
repairs. The song's Actions offer both **Uta package (.utz)** and **UltraStar
(.txt)**; UltraStar export preserves normal, golden, freestyle, rap, and golden
rap note markers.

A small `.utz` CLI is also available for automation:

```sh
cargo run -p uta-studio-export -- list
cargo run -p uta-studio-export -- export <file-hash> /path/to/song.utz
```

The editor interaction model was informed by open-source karaoke chart tools,
especially [USKMaker](https://github.com/walterfr/UltraStarKaraokeMaker), with
[Yass](https://github.com/SarutaSan72/Yass) and
[UltraStar Play](https://github.com/UltraStar-Deluxe/Play) used as format and
workflow references. USKMaker and UltraStar Play are MIT-licensed; Yass is
GPL-3.0-or-later. Their interaction patterns and algorithms were studied, while
Uta Studio's React/Rust implementation was written for this codebase. Uta
Studio keeps seconds and MIDI as its internal source model; target-specific beat
quantization happens only during export.

The interface uses a cover-first, information-dense music-library layout
informed by Roon's public product UI: persistent navigation, a command area,
right-side metadata inspector, and a bottom authoring dock. It is an adaptation
for chart production rather than a copy of Roon branding or playback behavior.

The root `icon.png` is the canonical brand artwork. Tauri platform icon files
and the optimized square derivative are generated from that artwork.

## License

GPL-3.0.
