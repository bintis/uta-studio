# Native inference refactor baseline audit

- Branch: `native-inference`
- Baseline HEAD: `56fdbec50444939360caf2832a7b1d958941fe6b`
- Captured: 2026-08-22 design baseline
- Existing user changes preserved: `docs/native-inference-rewrite-plan.md`, `docs/validation/qwen-runtime-validation.md`.

## API capabilities before refactor

```text
frontend_ready
get_log_path
get_recent_logs
analysis_log_path_for
analysis_log_lines
window_immersive
minimize_window
load_config
save_config
list_audio_models
get_audio_model_status
install_audio_model
reinstall_audio_model
remove_audio_model
validate_audio_processing_profile
preview_effective_audio_params
calculate_cache_stats
clear_models_command
clear_all
trigger_scan
set_library_source
add_library_folder
remove_library_folder
list_library_folder
open_library_entry
reveal_library_entry
open_artifact_entry
reveal_artifact_entry
open_export_folder
clear_library_source
load_songs
load_song_by_hash
load_songs_meta
load_analysis_queue
load_analysis_tasks
update_song_settings
load_analysis_history
load_analysis_node_attempts
compare_analysis_runs
compare_node_attempt_with_previous_run
clear_analysis_history
load_library_menu_items
enqueue_one
enqueue_all
delete_song_cache
reanalyze_transcript
reanalyze_full
reanalyze_pitch
realign
reanalyze_force_transcribe
run_analysis_plan
run_analysis_node
run_analysis_node_downstream
disable_analysis_node_for_run
freeze_analysis_node_outputs_for_run
bypass_analysis_node_with_original_mix_for_run
cancel_analysis_run
stop_analysis_run
get_analysis_graph
preview_analysis_plan
load_analysis_artifacts
load_artifact_revisions
import_legacy_artifacts
intermediate_capture_request
set_intermediate_capture_request
set_active_artifact_revision
delete_artifact_revision
invalidate_artifact_revision
compare_artifact_revisions
get_song_analysis_profile
set_song_analysis_profile
reset_song_analysis_profile
replace_authored_chart_with_fresh_analysis
cached_artifact_presence_for_song
resolve_song_authoring_state
preview_full_analysis_plan
shift_key
shift_tempo
migrate_analyzer_chart
load_lyrics
search_lrclib_lyrics
save_lyrics
provide_lrc
apply_timed_lyrics
export_utz
export_ultrastar
export_all_utz
export_all_ultrastar
chart_readiness
load_chart
load_chart_audio
editor_audio_load
editor_audio_play
editor_audio_pause
editor_audio_seek
editor_audio_status
editor_audio_stop
library_audio_load
library_audio_play
library_audio_pause
library_audio_seek
library_audio_volume
library_audio_queue
library_audio_playback_options
library_audio_status
library_audio_stop
save_vocal_chart
save_vocal_chart_from_revision
editor_actions
load_transcript
analysis_runtime_status
trigger_setup
inspect_analysis_node_io
inspect_artifact
preview_artifact
artifact_lineage
preview_artifact_downstream_impact
preview_node_downstream_impact
compare_artifacts_typed
set_artifact_pinned
capture_analysis_run_artifacts
resolve_artifact_for_run
begin_artifact_edit
preview_artifact_edit_impact
preview_frozen_downstream_impact
run_analysis_request
resolve_graph_edge_binding
inspect_export_node
validate_export_node
record_last_export
commit_artifact_edit
merge_chart_revisions
ui_interaction_capabilities
dispatch_ui_interaction
api_capabilities
run_feature_diagnostics
```

## Tracked Python deletion checklist

```text
app-core/analyzer/align.py
app-core/analyzer/analyze.py
app-core/analyzer/audio.py
app-core/analyzer/audio_models/__init__.py
app-core/analyzer/audio_models/catalog.py
app-core/analyzer/audio_models/errors.py
app-core/analyzer/audio_models/install.py
app-core/analyzer/audio_models/parameters.py
app-core/analyzer/audio_models/plan.py
app-core/analyzer/audio_models/schema.py
app-core/analyzer/audio_models/yaml_util.py
app-core/analyzer/audio_processors/__init__.py
app-core/analyzer/audio_processors/contracts.py
app-core/analyzer/audio_processors/executor.py
app-core/analyzer/audio_processors/outputs.py
app-core/analyzer/audio_processors/runners/__init__.py
app-core/analyzer/audio_processors/runners/base.py
app-core/analyzer/audio_processors/runners/demucs_torch.py
app-core/analyzer/audio_processors/runners/mdx_onnx.py
app-core/analyzer/audio_processors/runners/mdxc_torch.py
app-core/analyzer/audio_processors/xpu_segmented.py
app-core/analyzer/audio_processors/xpu_worker.py
app-core/analyzer/audio_separator_adapter/__init__.py
app-core/analyzer/audio_separator_adapter/offline.py
app-core/analyzer/cjk.py
app-core/analyzer/ctc_align.py
app-core/analyzer/gpu.py
app-core/analyzer/hallucination.py
app-core/analyzer/key_detect.py
app-core/analyzer/language.py
app-core/analyzer/mms_karaoke.py
app-core/analyzer/model_setup.py
app-core/analyzer/openvino_mdx.py
app-core/analyzer/openvino_separation.py
app-core/analyzer/openvino_whisper.py
app-core/analyzer/parakeet.py
app-core/analyzer/pipeline.py
app-core/analyzer/pitch.py
app-core/analyzer/qwen_align.py
app-core/analyzer/rhythm.py
app-core/analyzer/server.py
app-core/analyzer/stems.py
app-core/analyzer/test_audio_hardware_smoke.py
app-core/analyzer/test_audio_model_catalog.py
app-core/analyzer/test_audio_model_load_smoke.py
app-core/analyzer/test_audio_parameters.py
app-core/analyzer/test_audio_runner_contracts.py
app-core/analyzer/test_audio_separator_adapter.py
app-core/analyzer/test_intermediate_capture.py
app-core/analyzer/test_mms_karaoke.py
app-core/analyzer/test_model_setup.py
app-core/analyzer/test_node_events.py
app-core/analyzer/test_pipeline_cache.py
app-core/analyzer/test_progress_accounting.py
app-core/analyzer/test_run_pipeline_flags.py
app-core/analyzer/test_stems.py
app-core/analyzer/test_transcribe_recognized_text.py
app-core/analyzer/test_transcript_artifacts.py
app-core/analyzer/test_xpu_segmented.py
app-core/analyzer/test_xpu_worker.py
app-core/analyzer/test_yaml_util.py
app-core/analyzer/transcribe.py
app-core/analyzer/whisper_compat.py
scripts/build-user-guide.py
tools/import_uvr_audio_catalog.py
```

## Source files above 1600 lines

```text
  1601 app-core/src/analyzer/control.rs
  1941 app-core/src/analyzer/tests.rs
  1995 desktop/src/studio/analysis_layout.rs
  1995 desktop/src/studio/analysis_model.rs
  1699 desktop/src/studio/analysis_render/overview.rs
  1757 desktop/src/studio/artifact_workbench_ui.rs
  1605 desktop/src/studio/editor/actions.rs
  1608 vendor/utz/src/lib.rs
```

## Active Python/uv integration locations

The cutover checklist includes `flake.nix`, `AGENTS.md`, `docs/engineering-constraints.md`, `app-core/src/vendor*`, `app-core/src/analyzer/server.rs`, `app-core/src/audio_processing.rs`, and the tracked Python files above.

Source media, configured model directories, and user caches are migration inputs only; this refactor does not delete or rewrite them.
