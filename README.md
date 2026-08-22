# Uta Studio

Uta Studio is a GPL-3.0 desktop application for authoring playable song charts.
It is a native Rust/Bevy application focused exclusively on producing
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

## Documentation and localization

- **[User guide / 用户说明书 / ユーザーガイド](docs/USER_GUIDE.md)** — installation, first-run setup, analysis, the offline Documentation Center, analysis artifacts, editing, export, backup, and troubleshooting in English, Simplified Chinese, and Japanese.
- **[Documentation Center & Artifact Workbench design](docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md)** — current design plus honest implementation status. Remaining work is listed in [`docs/UTA_STUDIO_REMAINING_DEVELOPMENT_AGENT_GUIDE.md`](docs/UTA_STUDIO_REMAINING_DEVELOPMENT_AGENT_GUIDE.md).
- **[Internationalization guide](docs/I18N.md)** — locale resolution, catalog maintenance, dynamic messages, tests, and migration guidance.

The native interface supports English, Simplified Chinese, and Japanese. Select the language in **Settings > General > Interface language**; English remains the fallback for untranslated copy.

## Supported platforms

- **Linux:** supported on native Wayland desktops. Editor and library audition use the packaged GStreamer runtime with PipeWire/Pulse output.
- **Windows 10/11 x86-64:** supported by the portable ZIP. Editor and library audition use the system WASAPI output; FLAC, MP3, WAV, Ogg/Vorbis, and common AAC/MP4 inputs are decoded in process, with no separately installed codec pack.

Analysis tools and models are installed only after explicit confirmation in
**Settings > Models & runtime**. A platform package being present never starts
an automatic runtime or model download.

## Build

```sh
nix develop path:.
cargo xtask docs check
cargo test --workspace
cargo check --workspace
```

Run the desktop app with:

```sh
cargo desktop dev
```

Build a local release binary with the installed rustup toolchain (Nix only
provides native libraries and runtime tools):

```sh
./build.sh
```

Release packages are built with `nix build path:.#uta-studio`. The generated
offline documentation bundle is embedded in the desktop executable; runtime
source Markdown files are not required.

The Linux desktop uses Wayland directly. Uta Studio does not enable an X11
backend and does not fall back to XWayland.

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

## Acknowledgements

Uta Studio thanks the following projects for technical and interface references:

- **[BSRoformer.cpp](https://github.com/yasoukyoku/BSRoformer.cpp)** and
  **[GGML](https://github.com/ggml-org/ggml)** for the native RoFormer graph and
  Vulkan runtime foundations. Exact vendored source and patch identities are
  recorded with the packaged runtime notices.
- **[transcribe.cpp](https://github.com/handy-computer/transcribe.cpp)** and
  **[qwen3-asr.cpp](https://github.com/predict-woo/qwen3-asr.cpp)** for the two
  separately pinned Qwen native runtime recipes. Their exact commits, GGML
  revisions, and model identities are locked in `native-inference/runtime-lock.json`.
- **[USKMaker](https://github.com/walterfr/UltraStarKaraokeMaker)**,
  **[Yass](https://github.com/SarutaSan72/Yass)**, and
  **[UltraStar Play](https://github.com/UltraStar-Deluxe/Play)** for editor
  interaction patterns, karaoke workflow references, and format conventions.
  USKMaker and UltraStar Play are MIT-licensed; Yass is GPL-3.0-or-later.
  Uta Studio keeps a seconds/MIDI internal source model and applies export-time
  quantization only when writing targets.
- **[NextFire MMS karaoke-tuned model](https://huggingface.co/NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn)**.
  This AGPL-3.0 model is not shipped by Uta Studio; users install it explicitly
  in **Settings > Models & runtime**. For aligned timing, use
  **Settings > Analysis > Word timing & alignment** to enable **MMS Karaoke
  (Japanese)**, then configure it in **Models & runtime > Word timing &
  alignment**.
- **Roon's public product UI** as an interaction-direction reference for the
  cover-first information layout and command-area flow in this application's
  music-library and charting environment.
- **Root `icon.png`** in this repository is the canonical brand artwork.
  Packaged desktop icons and the square derivative are generated from this file.
- **[Nightingale](https://github.com/rzru/nightingale/)** for additional
  inspiration around lightweight audio-centric tooling and charting workflow
  patterns.

## License

GPL-3.0.
