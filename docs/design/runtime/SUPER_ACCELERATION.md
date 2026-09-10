# Super acceleration

## Authorized scope and current status

User request, 2026-09-10: put a super-acceleration switch in Settings; preload the next model
when VRAM permits; compute on the AMD integrated GPU and Intel B580 together; retain useful
song intermediates until their later consumers finish; avoid repeated decoding and improve
end-to-end throughput. **Implementation in progress, not an available acceleration claim.**

Reuse the existing stable `turbo_acceleration` setting, default off. Settings > Models & runtime
owns it. Save errors must be visible and must not leave the UI claiming an unsaved change.
Each new exact analysis request snapshots the value; changing the preference must not mutate
already queued/running requests. Existing load/save configuration APIs represent the setting.

## Execution ownership

Studio sends execution intent through the packaged Analysis Engine protocol only. Analysis
Engine owns dependency ordering, cancellation and the lifetime of this analysis's resources.
The native GGML worker/runtime owns model weights, device-local tensors and GPU submission.
No Studio/backend crate imports, model scripts, network services or CPU inference fallback.

The normal execution mode remains available. The super mode does not change models, precision,
chunk context/overlap, fusion rules, or quality thresholds. Device numerics may differ when
independent chunks run on different GPUs; compare output numerically and document that fact.
The current worker makes the F32 matmul choice process-wide before initializing GGML. Weight
residency/session reuse must preserve that per-model choice rather than silently reusing the
first model's precision for every subsequent model.

## Work and residency

- Distribute independent RoFormer chunks dynamically across selected Vulkan GPUs. Device rates
  are unequal: do not split evenly or turn each pair of chunks into a lockstep barrier. Account
  for the slower device's final outstanding chunk. Preserve source timing, overlap-add ordering,
  measured progress, failure propagation and cleanup.
- Preload only the next actually needed model, after current graph allocations are visible.
  Use an actual free/budget observation, not installed VRAM size. An unavailable observation or
  insufficient headroom skips speculative preloading, not the requested analysis. Allocation
  observations are advisory and cannot reserve memory against other applications.
- Retain reusable decoded/resampled representations and genuinely consumed GPU intermediates
  within one analysis. Ownership ends at the last consumer or run cancellation/failure. Keep
  artifact publication separate from the hot path; FLAC publication must still be correct and
  atomic. Do not upload unused raw media simply to claim a GPU cache.
- Avoid duplicate FFmpeg decode/resample passes for the same authorized source and representation.
  Retained resources must be bounded; no user-library/model mutation, destructive global cache
  operation, stale cross-run reuse, hash verification or compatibility/migration path.
- Expose real device work, cache reuse and preload outcomes through local analysis diagnostics/
  lifecycle events. Distinguish a requested mode from measured dual-device work and cache hits.

## Verification and handoff

Use isolated CPU scheduling/cache fixtures for uneven workers, ordered output, insufficient or
unknown memory budget, unused preloads, cancellation and failure cleanup. Test setting persistence,
exact-request propagation and surfaced UI errors. Keep source files below 2,000 lines.

Record and commit each independent implementation before executing it. Before GPU runs, inspect
recorded host load and GPU snapshots. Use both Vulkan ICDs for dual-GPU measurements; the previous
full-song debug command exposed only the Intel ICD and is not a dual-device fixture. Start with
bounded same-input ordinary/super runs, observe real work on both DRM devices, compare outputs,
report wall time/cache hits/peak residency, then exercise the full pipeline if relevant checks
justify it. No automatic retries or hardware isolation claims. Do not compare performance against
the 2.27 GB full-debug logging run. Normal non-debug observations must be matched.

Task progress and accepted measurements belong in
[`STATE.md`](../../../tasks/remaining-models/STATE.md). Engineering and component boundaries remain
those in [`engineering-constraints.md`](../../engineering-constraints.md) and
[`design/README.md`](../README.md). The older
[`performance direction`](../../PERFORMANCE_DIRECTION_2026-09-09.md) motivates this work but its
pre-attention-optimization device-rate estimates are not current measurements.
