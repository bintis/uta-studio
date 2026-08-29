# Uta Package Format (`.utz`) 0.3 — Final Clean Development Spec

Status: implementable development snapshot.

## Version policy

UTZ 0.x minor lines are breaking development snapshots. A 0.3 implementation
accepts 0.3.x and rejects 0.1.x/0.2.x.

The same policy applies to embedded Uta subformats:

```text
Uta package       0.3.x
VocalChart        0.3.x
PitchEvidence     0.3.x
```

Current media types:

```text
application/vnd.uta.vocal-chart+json;version=0.3
application/vnd.uta.pitch-evidence+json;version=0.3
```

Feature identifiers use the breaking pre-1.0 minor line:

```text
vocal-chart/0.3
pitch-evidence/0.3
```

After stabilization, a feature line uses only its stable major number, e.g.
`vocal-chart/1`.


## Canonical timeline

UTZ 0.3 uses exactly `1,000,000` integer units per second for all Uta-owned
timelines. `song.duration` is stored in these units. VocalChart, PitchEvidence,
and visual timing timebase fields MUST equal `1,000,000`.

All assets under `audio.assets` share the instrumental time zero and song-time
interval. Per-stem offsets or hidden time warps are forbidden.

## Extension classification

Every `extensions` key MUST be classified in exactly one of
`required_features` or `optional_features`. Neither and both are invalid.

`representations` are always safely ignorable and do not participate in feature
negotiation.

## Identifier reservation

UTZ reserves unnamespaced standard audio/visual identifiers. Third-party
audio/visual roles MUST use a reverse-DNS-style namespace.

Conventional representation IDs and their dot-suffixed variants are reserved
for the corresponding external formats; unrelated producer-defined
representation IDs MUST be namespaced.

## Patch-version rule

`0.3.x` patch releases are schema/semantic compatible bugfix releases only.
Breaking format changes require a new pre-1.0 minor line such as `0.4.0`.

## Format philosophy

UTZ is strict about semantics and integrity, open about descriptive/advisory data.

Unknown data may be ignored only when ignoring it cannot change correct
playback, timing, authored-note interpretation, or required scoring behavior.

## Song

Core:

```text
title
artist
duration
language?
bpm?
key?
```

Open descriptive data:

```text
metadata {}
```

## Audio

Audio is role-keyed:

```json
{
  "audio": {
    "assets": {
      "instrumental": {"path": "...", "media_type": "...", "sha256": "...", "bytes": 1},
      "guide_vocals": {"path": "...", "media_type": "...", "sha256": "...", "bytes": 1},
      "lead_vocal": {"path": "...", "media_type": "...", "sha256": "...", "bytes": 1}
    },
    "loudness_lufs": {
      "instrumental": -14.2,
      "guide_vocals": -16.0,
      "lead_vocal": -15.1
    }
  }
}
```

`instrumental` is required.

Standard optional roles are:

```text
guide_vocals
original
lead_vocal
backing_vocal
harmony_vocal
```

Unknown roles are allowed and safely ignorable. Producer-specific roles SHOULD
use a namespace under the role-key grammar.

A loudness key MUST reference an existing audio asset role.

## Provenance

Core provenance hints:

```text
generator?
source?
rights?
```

Open provenance detail:

```text
metadata {}
```

Implementations must preserve arbitrary provenance metadata during read/write.

## Scoring hint

Manifest-level scoring is advisory:

```json
{
  "scoring": {
    "engine": "uta.pitch",
    "version": 1,
    "parameters": {
      "octave_tolerance": false,
      "pitch_window_cents": 50
    }
  }
}
```

`engine` and `version` are typed.

`parameters` is engine-specific and open.

Required scoring semantics must not hide in `parameters`; they belong in
`required_features` and/or the authored VocalChart semantics.

## VocalChart 0.3

The chart remains strict and authoritative.

No arbitrary note/phrase/token metadata is introduced.

Machine confidence, technique probabilities, alternative candidates, model
agreement, and fusion evidence belong in a separate versioned analysis extension
referencing stable chart IDs.

## PitchEvidence 0.3

Fixed-hop F0 evidence remains an optional editor/analyzer aid.

Its `model` object remains open provenance for the evidence generator.

PitchEvidence never overrides authored VocalChart notes.

## Feature negotiation

Pre-1.0 feature suffix grammar:

```text
name/0.<minor>
```

Stable feature suffix grammar after 1.0:

```text
name/<major>
```

Unknown required features cause rejection.

Optional features may be ignored.

Extension assets continue to use the same feature identifier grammar.


## Visuals

Visual assets use the same role-keyed pattern as audio:

```json
{
  "visuals": {
    "assets": {
      "cover": {
        "path": "visuals/cover.webp",
        "media_type": "image/webp",
        "sha256": "...",
        "bytes": 1
      },
      "background": {
        "path": "visuals/background.webp",
        "media_type": "image/webp",
        "sha256": "...",
        "bytes": 1
      },
      "video": {
        "path": "visuals/background.mp4",
        "media_type": "video/mp4",
        "sha256": "...",
        "bytes": 1
      }
    },
    "timing": {
      "video": {
        "timebase": 1000000,
        "offset": 0
      }
    }
  }
}
```

Standard advisory roles are:

```text
cover
background
video
thumbnail
```

Unknown roles are allowed and safely ignorable. Producer-specific roles SHOULD
use a namespaced role.

`visuals.timing` is keyed by visual role and uses integer timebase units.
Every timing key MUST refer to an existing `visuals.assets` role.

`offset` defines the instrumental-time position corresponding to visual time
zero. Negative values start the visual before the instrumental; positive values
start it after the instrumental.

UTZ 0.3 does not use floating `video_offset_seconds`.

Time-varying presentation semantics such as scene changes, stage cues, scripted
overlays, or lyric-animation events require a separately versioned extension.
They must not hide required behavior inside arbitrary visual metadata.


## Alternate representations

A UTZ package MAY carry alternate external-format representations of the same
song under the root `representations` map.

Example:

```json
{
  "representations": {
    "midi": {
      "path": "representations/song.mid",
      "media_type": "audio/midi",
      "sha256": "...",
      "bytes": 12345
    },
    "kar": {
      "path": "representations/song.kar",
      "media_type": "audio/midi",
      "sha256": "...",
      "bytes": 23456
    },
    "ustx": {
      "path": "representations/song.ustx",
      "media_type": "application/x-openutau-project",
      "sha256": "...",
      "bytes": 34567
    },
    "ultrastar": {
      "path": "representations/song.txt",
      "media_type": "text/plain",
      "sha256": "...",
      "bytes": 45678
    }
  }
}
```

Conventional identifiers include:

```text
midi
kar
ust
ustx
musicxml
ultrastar
lrc
srt
```

Identifiers are producer-chosen and MAY use `.` to distinguish variants, for
example:

```text
midi.quantized
midi.raw
ustx.editable
ultrastar.duet
```

A representation is:

- optional;
- safely ignorable;
- an alternate/derived serialization;
- never authoritative over the UTZ VocalChart;
- never a replacement for required UTZ semantic data.

If a representation disagrees with the VocalChart, the VocalChart wins.

`representations` does not participate in feature negotiation because consumers
are never required to understand any of its entries in order to interpret the
UTZ package correctly.

### Representations vs extensions

Use `representations` when the asset is another external representation of
already-existing song/chart information:

```text
MIDI
KAR
UST / USTX
MusicXML
UltraStar
LRC
SRT
```

Use `extensions` when the asset introduces a new structured UTZ semantic or
analysis domain:

```text
singing-analysis/0.3
tempo-map/0.3
chords/0.3
presentation/0.3
```

A multi-file external project SHOULD be wrapped into one archive representation
unless/until that external format receives a dedicated container convention.

## Strict structures

The following remain closed:

- package root;
- `AssetRef`;
- chart/analysis/visual containers;
- VocalChart track/phrase/note/token structures;
- note pitch/timing/scoring semantics.

This prevents semantic behavior from leaking into unversioned metadata.
