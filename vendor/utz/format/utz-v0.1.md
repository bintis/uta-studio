# Uta Package Format (`.utz`) 0.1

Status: implementable draft. The Rust implementation in this repository is the
reference implementation for this version.

## Container

An `.utz` file is a ZIP archive. `manifest.json` MUST exist at the archive root
and MUST be UTF-8 JSON conforming to `manifest-v0.1.schema.json`.

Every content path MUST be relative, use `/` separators, and contain only
normal path components. Absolute paths, empty paths, `.`/`..` components, and
backslashes are invalid. Readers MUST defend against duplicate names, path
traversal, excessive file counts, and excessive uncompressed size before
exposing package contents.

## Required logical content

- one instrumental audio asset;
- a timed transcript;
- a frame-level reference pitch track;
- segmented pitch notes used by the scrolling guide.

Guide vocals, the original mix, cover artwork, and background video are
optional. Roles are declared by the manifest; filenames have no semantics.

Each declared asset records its MIME type, byte count, and lowercase SHA-256.
Readers MUST verify declared byte counts and digests. Extra files are allowed
for future extensions, but version 0.1 consumers may ignore them.

## Versioning

`format_version` is semantic-version shaped. A reader supporting 0.1 accepts
the `0.x` development line only when it explicitly understands that minor
version. Once version 1 is published, readers accept compatible minor/patch
updates and reject unsupported major versions.

The scoring algorithm is separately identified by `scoring.engine` and
`scoring.version`; changing game balance does not require changing the ZIP
container version.

## Pitch and time conventions

- All timestamps and durations are seconds from the beginning of package audio.
- MIDI note 69 is A4 = 440 Hz; fractional live pitches are allowed internally.
- Pitch frames use `null` for unvoiced regions.
- `audio_offset_seconds` shifts package audio relative to chart time.
- A Game must score from package pitch data rather than re-analyzing the mix.

## Media guidance

Browsers must be able to decode the selected media. Producers should prefer
Opus in Ogg or MP3 for audio, WebP/JPEG/PNG for covers, and H.264/AAC MP4 or
WebM for optional video. The manifest MIME type, not an extension guess, is
authoritative.

## Copyright and provenance

The format does not grant permission to redistribute included media. Producers
should record source and rights notes in `provenance`; distribution systems may
apply stricter policies.
