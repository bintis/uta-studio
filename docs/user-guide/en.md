# Uta Studio User Guide

### 1. What Uta Studio does

Uta Studio turns local audio or video into an editable karaoke chart. Its normal workflow is:

1. Add one or more watched folders.
2. Scan supported local media into the library.
3. Configure and run the four-stage analysis pipeline.
4. Review lyrics, timing, and pitch in the built-in editor.
5. Save the authored chart.
6. Export either an **Uta package (`.utz`)** or **UltraStar 1.1 (`.txt`)** bundle.

Uta Studio does not move or delete source media. Generated stems, models, previews, charts, and temporary authoring data are stored separately.

### 2. Installation

Download the package for your system from the project’s GitHub Releases page. Release {{APP_VERSION}} provides Windows x86-64 ZIP, Debian, RPM, and portable Linux packages, together with SHA-256 checksum files.

#### Windows

1. Download `uta-studio-{{APP_VERSION}}-x86_64-windows.zip` and its checksum file.
2. Extract the ZIP to a writable folder.
3. Start `uta-studio.exe` from the extracted folder.
4. Keep the extracted files together; do not run only a copied executable without its packaged assets.

Windows 10/11 x86-64 is supported. Editor and library audition use the system WASAPI output and do not require a separately installed codec pack for FLAC, MP3, WAV, Ogg/Vorbis, or common AAC/MP4 inputs.

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_{{APP_VERSION}}-1_amd64.deb
```

#### Fedora / RHEL-compatible systems

```sh
sudo dnf install ./uta-studio-{{APP_VERSION}}-1.x86_64.rpm
```

#### Portable Linux build

```sh
chmod +x uta-studio-{{APP_VERSION}}-x86_64-linux.bin
./uta-studio-{{APP_VERSION}}-x86_64-linux.bin
```

The Linux desktop is Wayland-native. It does not enable an X11 backend or XWayland fallback.

#### Verify a download

Use the matching `.sha256` file before installing an artifact obtained through a mirror or shared storage:

```sh
sha256sum -c uta-studio-{{APP_VERSION}}-linux-deb.sha256
```

Use the checksum file matching the package type you downloaded.

### 3. First launch

#### 3.1 Choose the interface language

Open **Settings → General → Interface language** and choose:

- **System default** — follows a locale supplied by the operating environment; if no supported locale is available, English is used.
- **English**
- **简体中文**
- **日本語**

The selection is saved in Uta Studio’s configuration. Developers and portable-launch scripts may override it with `UTA_STUDIO_LOCALE=en`, `zh-CN`, or `ja`.

**Interface language is not the same as song analysis language.** Interface language changes menus and messages. Song analysis language controls transcription/alignment for an individual song and is edited from that song’s language action.

#### 3.2 Add music folders

Open **Settings → Storage** or **Folders**, then select **Add folder**. You can add multiple roots; their contents are merged into one library index.

Recommended folder layout:

```text
Music/
  Artist/
    Album/
      Song.flac
      Song.mp4
```

The layout is optional, but good metadata and consistent folders make browsing easier.

#### 3.3 Choose a default export folder

In **Settings → Storage → Default export folder**, choose where Save As dialogs should open first. Each export can still choose another destination.

Batch export requires a default export folder so files can be written without opening one dialog per song.

#### 3.4 Set up models and runtime

Open **Settings → Models & runtime**.

1. Select the acceleration target: CPU, NVIDIA CUDA, or Intel Arc where supported.
2. Review **Runtime status**.
3. Choose **Set up…** or **Reconfigure…**.
4. Read the confirmation, including model size and license notices.
5. Confirm the setup explicitly.

Uta Studio may reuse compatible local `ffmpeg`, `uv`, Python, and existing model files. It does not download models merely because the application was launched.

#### 3.5 Open the Documentation Center

The user guide is also available inside the application. Open **Settings → General → Open user guide**, choose **Documentation** in Settings navigation, or press **F1**. See [Documentation Center](guide:documentation). Analysis outputs are inspected from the song’s analysis graph; see [Analysis artifacts](guide:artifacts).

### 4. Quick-start workflow

1. Add a watched folder.
2. Run **Rescan all** and wait for the library scan to finish.
3. In **Settings → Models & runtime**, complete runtime setup.
4. In **Settings → Analysis**, choose the engines and quality profile.
5. Open a song and select **Analyze**.
6. Review the resulting lyrics and chart.
7. Select **Edit chart** and correct timing, syllables, and note pitches.
8. Save the chart.
9. Export `.utz` for the `uta!` game or `.txt` for an UltraStar-compatible workflow.

### 5. Library and folders

#### Library views

Browse contains all music and analysis progress. My Library contains charts, video sources, playlists, and folders. Search can filter tracks, artists, albums, and playlists.

#### Watched folders

- Adding a folder starts or enables scanning.
- **Rescan all** refreshes the merged library index.
- Removing a watched folder only removes that location from the index. It does not delete source media.
- The Folders page can browse watched roots and the configured output folder.

#### Playback queue

Library playback includes previous/next, pause/play, repeat modes, shuffle, mute, and volume. Playback is for review and authoring; scoring belongs to the separate `uta!` player.

### 6. Analysis pipeline

Uta Studio executes an explicit node-based DAG. Generated files are typed **analysis artifacts**; inspect revisions, lineage, real node progress, and editor routing from the song’s analysis graph. See [Analysis artifacts](guide:artifacts).

#### 01 · Vocal and BGM separation

Runs independent vocal and BGM separation branches. Each branch selects its own separation model, followed by two ordered post-processing slots; each slot can be Off, denoise, or dereverb. The BGM output feeds chart construction directly, while the vocal output feeds pitch and lyrics analysis.

Available choices depend on the configured runtime and can include BS-RoFormer vocals, MelBand accompaniment, Karaoke 2, denoise, dereverb, and HTDemucs 6-stem. Catalog models are installed only from **Settings > Models & runtime** after you confirm the name, source, size, and license. Analysis itself stays offline; existing chart data changes only after re-analysis.

Use a balanced profile first. Memory-saving profiles reduce peak use; quality profiles usually take longer and can require more memory.

#### 02 · Lyrics transcription

Recognizes lyrics from the vocal source. Whisper and Parakeet-family options may be available, depending on the configured runtime and downloaded files.

A larger recognition model can improve difficult material but costs more memory and processing time. Confirm the selected model is installed on **Models & runtime**.

#### 03 · Word timing and alignment

Refines recognized or supplied lyrics into editable word timings. Supported backends can include WhisperX, CTC forced alignment, Qwen forced alignment, and the optional Japanese MMS Karaoke backend.

The optional NextFire MMS Karaoke model is separately licensed under AGPL-3.0 and is downloaded only after a dedicated confirmation. Use it only when its license and Japanese-specific behavior fit your project.

#### 04 · Pitch analysis

Extracts pitch evidence and MIDI note targets for chart authoring. The editor remains authoritative: inspect and correct notes instead of treating automatic output as final.

#### Re-analysis rules

Changing an analysis setting does not silently rewrite an existing authored chart. Existing stems or charts change only after the corresponding re-analysis action. This protects manual edits and makes destructive changes explicit.

#### Automatic analysis

When **Auto-analyze** is enabled, newly scanned, unanalyzed songs are queued automatically. Leave it off when model setup is incomplete or when you want to review files before using compute resources.

### 7. Lyrics and language

#### Replace or edit lyrics

From a song, open the lyrics action to:

- enter plain lyrics;
- enter timed LRC;
- search LRCLIB;
- review a candidate before saving;
- optionally queue alignment after saving.

Always verify spelling, repeated lines, punctuation, and omitted vocalizations before alignment.

#### Set song analysis language

Use the song’s **Language** action to choose automatic detection or an explicit language. Saving with reprocessing enabled queues the required analysis again.

This setting affects the song’s recognition/alignment pipeline. It does not change the application interface.

### 8. Chart editor

Open an analyzed song and select **Edit chart**. The editor supports waveform and pitch evidence, lyric/phrase boundaries, note bars, multiple tracks, and named undo history.

Common operations include:

- play, pause, seek, and audition selected material;
- edit lyrics and phrase boundaries;
- drag note timing and pitch;
- select multiple notes with a marquee;
- move, transpose, resize, split, merge, duplicate, copy, and paste;
- apply quantization;
- use tap timing against playback;
- assign UltraStar note types such as normal, golden, freestyle, rap, and golden rap;
- inspect chart problems and apply conservative timing repairs;
- use Lock mode to prevent accidental dragging;
- author lead/backing or duet tracks.

Save after meaningful edit groups. Named undo entries help verify what will be reverted.

#### Safe editing practices

- Keep Lock enabled while only reviewing.
- Audition timing at phrase boundaries and after quantization.
- Inspect low-confidence pitch regions rather than bulk-accepting them.
- Re-open the exported chart in the target player before publishing.

### 9. Export

#### Uta package (`.utz`)

Use this format for the independent `uta!` game and for a self-contained, versioned package. The package can include chart data and the media/artifacts required by that workflow.

#### UltraStar (`.txt`)

Exports UTF-8 UltraStar 1.1 text plus sibling media. Export preserves normal, golden, freestyle, rap, and golden-rap note markers and supports multi-track/duet output where authored.

#### Export safety

- Exports are written to a user-selected destination.
- Batch export uses collision-safe behavior rather than silently overwriting another song.
- Source media is not modified.
- Test the final package in its target application.

### 10. Storage, cache, and backup

**Settings → Storage** reports generated storage by songs, models, and other data.

- **Clear generated cache** removes generated stems, charts/previews, and temporary authoring data covered by that cache action. It does not delete source media.
- **Clear models** removes downloaded model artifacts and makes runtime status report them as missing until setup is run again.

The default settings/data root is `~/.uta-studio`, unless a different data location is configured. Before migrating or reinstalling:

1. Close Uta Studio.
2. Back up `~/.uta-studio` or the configured data root.
3. Back up authored/exported `.utz` and UltraStar bundles.
4. Keep original source media separately.
5. Restore the data root before launching the new installation when practical.

Do not rely on generated cache as the only copy of finished work; retain exported packages.

### 11. Logs and diagnostics

In **Settings → General**:

- **Application log → View log** opens the current log when one exists.
- **Feature API diagnostics → Run checks** verifies local APIs, native audio, and real temporary UTZ/UltraStar exports. The diagnostic temporary folder is removed after the check.

Each analysis run writes one detailed JSONL file under `analysis-logs/`. A DAG node’s **View logs** action opens that run and filters by `node_id`; legacy runs clearly report that no dedicated log exists. Analysis progress, model output, and tracebacks stay out of `app.log`. Confirmed history clearing also removes only the referenced files inside `analysis-logs/`.

Include the application version, platform, selected runtime, relevant log excerpt, and reproducible steps in a bug report. Do not attach copyrighted source media unless you have permission.

### 12. Troubleshooting

#### “Setup required” or missing model

Open **Settings → Models & runtime**, press **Check again**, then install or repair the missing stage. Reconfigure after changing CPU/CUDA/Intel acceleration.

#### Analysis button is disabled

Complete runtime/model setup first. Also confirm the song is local and the source path remains readable.

#### A scan finds no music

Confirm the folder is still watched, permissions allow reading it, and the files are supported local audio/video. Run **Rescan all**.

#### Poor lyrics

Verify the analysis language, try a more suitable transcription model, provide corrected lyrics manually, then run alignment rather than repeating the entire pipeline unnecessarily.

#### Poor timing

Use a backend suited to the language, check that vocal separation is clean, then correct word/phrase boundaries in the editor. For Japanese, evaluate the optional MMS Karaoke backend and its separate license.

#### Poor pitch notes

Audition the vocal source, inspect the pitch trace, and correct MIDI notes manually. Harmony, vibrato, noise, and residual accompaniment can confuse automatic pitch extraction.

#### Linux window does not start

Confirm the session is Wayland, not an X11-only session, and that the graphics stack supports the packaged renderer. Capture the application log for reporting.

#### Language remains English

Choose a language manually in **Settings → General → Interface language**. System default depends on locale variables exposed by the launch environment. Also check whether `UTA_STUDIO_LOCALE` is overriding the saved setting.

### 13. Privacy and licenses

Analysis runs locally with the configured runtime. LRCLIB search is an explicit network-facing lyrics lookup. Model setup is explicit and may contact model hosts after confirmation.

Uta Studio is GPL-3.0. Optional third-party models and tools retain their own licenses. Review the confirmation shown before downloading separately licensed artifacts.

### 14. Documentation Center

The Documentation Center is an offline, native page. It does not open a browser, fetch remote pages, or run scripts.

Open it from:

- **Settings → General → Open user guide**;
- **Documentation** in the Settings navigation;
- **F1**, which opens context help for the current page or selected analysis node;
- a DAG node **Help** action, or an artifact menu **About this artifact** action.

The document language follows **Settings → General → Interface language**. The article body is not re-translated at runtime; Uta Studio selects the matching English, Simplified Chinese, or Japanese source first. Viewer chrome still uses the interface catalogue.

#### Browse and search the guide

Search is local to the embedded guide. Headings rank higher than body lines. CJK search uses character substring matching, so no extra tokenizer is required. A result jumps to the matching section.

On a wide window the page uses a contents column, the article, and search or history. On a narrow window those columns stack. Back and Forward remember the sections you opened.

If **F1** is pressed from the chart editor while the chart has unsaved changes, Uta Studio asks before leaving the editor.

#### Context help and safe links

Stable deep links include `guide:getting-started`, `guide:analysis`, `guide:lyrics`, `guide:editor`, `guide:export`, `guide:documentation`, and `guide:artifacts`. Node help (`node:lyrics.align`) opens the matching workflow chapter. Artifact help (`artifact:TimedTranscript`) opens this Artifact Workbench chapter.

Only in-guide, `guide:`, `node:`, `artifact:`, `problem:`, and `https://` links are followed. Remote images, `file://` links, and scripts are not executed.

### 15. Analysis artifacts

Analysis writes generated files into Uta Studio’s cache, not over your source media. The Artifact Workbench inspects those files as typed revisions from the song’s analysis graph.

#### Revisions, Active, and Pin

- A **revision** is one immutable snapshot. A later analysis creates another revision; it does not rewrite the old bytes.
- **Active** is the revision later actions use by default.
- **Pin** protects a revision from delete and cache cleanup. Unpin it first if you really want it removed.
- Older runs that predate exact binding records are labelled **Legacy / untracked**. Uta Studio does not invent missing lineage from a filename or modification time.
- Temporary preprocess audio is labelled **Ephemeral** unless you explicitly capture it.

#### Inspect Node I/O

Open a song, open its analysis graph, and select a node. The Node I/O workbench has **Overview**, **Inputs**, **Outputs**, **Attempts**, **Logs**, and **Help**.

The banner says **Exact run bindings** when the selected history record stored attempt-specific input and output rows. Otherwise it says the inspector is using the current inventory as a fallback, and that exact run lineage was not recorded.

Inputs and outputs distinguish the node’s declared slots from the concrete files bound to the selected run.

#### Artifact menu actions

Right-click an artifact node for the actions that are valid for that kind and state:

- Preview or Play;
- Open in a compatible editor, when the kind supports it;
- Set Active;
- Compare with Active, or Compare a candidate chart with the authored chart;
- Pin or Unpin;
- Lineage;
- Impact;
- Inspect provenance;
- Reveal in the file manager;
- About this artifact;
- Invalidate;
- Delete, unless the revision is pinned.

Actions that cannot succeed are hidden instead of shown as errors. Graph export boxes show readiness, the last recorded destination when one exists, and a menu to validate, re-export, or reveal that file. Export packages are not analysis Artifact revisions.

#### Edit without overwriting analysis output

- **LyricsInput** and **RecognizedText** can open the lyrics editor. Saving writes a new user revision. Analyzer output is not overwritten in place.
- **TimedTranscript** opens a timing surface that keeps word-level times and unknown JSON fields. Historical transcript bytes stay unchanged.
- **CandidateChart** can be compared with **AuthoredChart**. Merge actions can replace the working copy, take candidate lyrics timing, take candidate pitch, replace the selected phrase, or replace the selected note range. Phrase and range merges use the current chart-editor selection. Save the authored chart first if it has unsaved edits.
- **AuthoredChart** opens the selected revision, not a silent substitute for “whatever is current”. Saving creates a new authored revision. **Replace Authored** asks for confirmation. **Keep Authored** leaves the saved chart unchanged. A pinned authored chart cannot be replaced until it is unpinned.
- **PitchTrack** and **PitchNoteCandidates** are evidence. They can provide context in the chart editor; the evidence file itself is not rewritten.

**Save Only** writes the new revision and does not queue analysis. **Save and Run Downstream** shows an impact preview first. Confirming that preview is what queues work.

#### Lineage panel and impact preview

**Lineage** can stay on the main analysis graph. Turn it on from the VIEW row or from an artifact menu. Upstream, downstream, and full scope highlight the matching nodes and edges and fade the rest. MINI view keeps only compute nodes; lineage then highlights those producers and consumers. Missing legacy links appear as explicit gaps. Edge labels show the artifact kind and a short revision id. Selecting a revision opens that revision.

**Impact** is a read-only preview built from one frozen analysis plan. It includes the current song profile, staged Freeze / Bypass / Disable intents, and Pin. Groups are will run, will reuse, will become stale, will be blocked, will remain preserved, and exports that need regeneration. **Queue this plan** submits that same request. Cancel leaves the library unchanged.

#### Capture preprocessed audio

Preprocessed audio stays ephemeral on ordinary runs. From the relevant node or artifact menu you can request a one-shot or persistent capture of the real preprocessed FLAC on the next run. The confirmation states storage and privacy implications. A successful one-shot request clears itself. Failed capture leaves the request armed.

#### Analysis node reference

- `preflight` — checks that the local source and runtime are usable before work starts.
- `music.analysis` — key, rhythm, and descriptor analysis used later for timing and charting.
- `stems.separate` — MINI-view aggregate derived from the real stem child nodes; it is not an executable Extract step.
- `stems.vocals` — runs the selected vocal separation model.
- `vocals.denoise` / `vocals.dereverb` — optional vocal post-processing in the configured slot order.
- `stems.instrumental` — runs the independent BGM separation model.
- `instrumental.denoise` / `instrumental.dereverb` — optional BGM post-processing in the configured slot order.
- `stems.bind_analysis_outputs` — validates and binds the final vocal and BGM products for downstream consumers.
- `pitch.extract` — pitch curve and note-candidate evidence.
- `lyrics.preprocess` (Vocal Preprocessing) — vocal audio prepared for recognition and alignment; ephemeral unless captured.
- `lyrics.transcribe` — recognized text.
- `lyrics.align` — word-timed transcript.
- `lyrics.import_timed` — imported timed lyrics.
- `chart.build_candidate` — builds a **CandidateChart** without replacing the authored chart.

#### Artifact kind reference

- **SourceMedia** — the read-only local song or video. Never moved or deleted by analysis.
- **MusicAnalysis**, **KeyAnalysis**, **RhythmAnalysis**, **AudioDescriptors** — music-analysis JSON used by later nodes.
- **VocalStem** and **InstrumentalStem** — separated audio.
- **PreprocessedAudio** — ephemeral recognition input unless you capture it as FLAC.
- **RecognizedText** and **AsrSegments** — transcription evidence.
- **LyricsInput** — user-supplied or promoted lyrics.
- **TimedTranscript** — word-timed lyrics used by alignment and charting.
- **PitchTrack** and **PitchNoteCandidates** — pitch evidence.
- **CandidateChart** — analyzer-proposed chart. Distinct from the authored chart.
- **AuthoredChart** — the chart you edit and export.

---
