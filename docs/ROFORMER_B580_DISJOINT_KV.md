# B580 attention: installed fragment backend and disjoint K/V experiments

## Installed application runtime

On 2026-09-10 the user requested installation of the accepted attention backend before
continuing toward 20 TFLOPS. The existing recipe had advanced to ten patches, so this
installation retained the floating-storage and compact-row-scale changes rather than
downgrading to the earlier eight-patch snapshot.

Evidence root (repository-relative):
`test-artifacts/xe90-20t-continuation.Vi1m7G/`.
The accepted recipe snapshot is under `recipe/native-inference/ggml-worker/`, captured
from project commit `44b3234`. Upstream GGML remains pinned to
`8c63e70982c95ceb862e3a1073a2c1beef75d60a`.

The recorded two-job build completed successfully at
`test-artifacts/operations/20260910T054531-9e508b3b8f0d/`.
Its output is `runtime/lib` under the evidence root. A connector error occurred after
launch and the connector replay created a second, separate build directory; that
second build was not the installation source. A failed tool response was not treated
as evidence that the compiler never ran.

The application libraries were installed by an atomic `renameat2` exchange at
`/home/bintis/.local/share/uta-studio/runtime/ggml-vulkan/lib`.
The existing application entry `ggml-vulkan-v1 -> ggml-vulkan` was retained.
All four installed libraries compare byte-for-byte with the newly built output.
The previous directory is preserved at:

```text
/home/bintis/.local/share/uta-studio/runtime-backups/before-fragment-workgroups-eazhq7t_/ggml-vulkan
```

`install-intent.json`, `install-receipt.json` and `installation-verification.json`
record this operation. No model, source media, application launcher or user setting
was modified. Existing processes were not killed to force library reload.

### Installed Worker execution, not just a build check

The Worker belonging to the current installed launcher was invoked through its normal
runtime lookup, without `UTA_STUDIO_GGML_RUNTIME_DIR` or `UTA_TEST_GGML_RUNTIME_DIR`.
It returned `done` with `status: ok` and published two six-second FLAC outputs.
The Worker PID was 141095; `loader-installed.141095` proves that all four GGML
libraries loaded by that process came from the installation directory.
Separate FFmpeg child loader files may contain its packaged whisper dependency;
those are not evidence that the model Worker loaded another GGML library set.

The actual installed Worker logged:

```text
query-owned attention Br=64 Bc=32 subgroups=8 lanes=32 K=f16 V=f16
```

Its XE90 time-attention mean was 43.7146 ms, or 12.5029 TFLOPS under the QK+PV
FLOP convention; frequency attention was 5.07868 ms. This is one real Worker smoke,
not a whole-song benchmark. Both output files decode as FLAC, 44.1 kHz, stereo,
6.000000 seconds, with 529,200 finite samples each. See `worker-smoke/` and
`installed-audio-verification.json`. The instrumental output reaches full scale;
this work does not alter the previously documented FLAC saturation behavior.

The freshly built accepted library passed 63 numerical fixture instances:
30 query-owned, 26 floating/mixed-storage and seven short-query/H72 fallback cases.
Their maximum NMSE values are respectively 2.659816524e-7, 3.144515097e-7 and
1.852794324e-8. These are seven explicitly invoked test functions, not 63 test
functions. Ordinary package tests were not rerun for this installation.

## Disjoint K/V staging candidate

Commit `652a469` adds the isolated backend experiment
`native-inference/ggml-worker/experiments/attention-disjoint-kv-staging.patch`.
It is not part of the installed ten-patch recipe.

After invariant Q fragments are loaded, the eight-subgroup implementation reserves
8 KiB of operand scratch but uses only half for alternating K and V tiles. The
candidate puts K and V into disjoint halves and publishes both at the start of each
key block. Score handoff remains subgroup-local. It removes the K-to-V overwrite
workgroup barrier and the separate V publication barrier, reducing hot-loop
workgroup barriers from four to two. The final workgroup barrier remains to protect
operand readers before the next key block. Four-subgroup dispatch retains the
phase-ordered path.

Shared allocation stays at 20,768 bytes. QK, rounded F16 probabilities, blockwise
F32 accumulation, masks, tensor strides and full key context are unchanged. No
matrix-component-to-lane mapping is assumed. The candidate's separate compiler
invocation reports 2,380 instructions, 13 spills, 111 fills, 228 sends and 128 GRFs,
versus the accepted compact-scale study's 2,475 instructions, 18 spills, 122 fills
and 230 sends. These are compiler diagnostics, not measured hardware stall counts.

The candidate `runtime-disjoint/lib` passed the same 63 numerical cases with the
same maximum NMSE values. Two large-shape invocations additionally check every
output for finiteness and sample a full-context F64 reference.

### Initial and confirmation performance observations

Each shape uses eight warmups followed by eight measured GPU calls. TFLOPS counts
546,561,146,880 FLOPs for time attention and 28,565,913,600 for frequency attention,
divided by the complete fused-kernel GPU duration. No warmups or slow samples are
silently substituted for the declared measured interval.

The initial control/candidate/candidate/control time means were 53.652, 50.125,
50.945 and 53.059 ms. The two return cases recorded other compute clients at
58.93% and 44.49% peak engine activity. They are preserved as contended observations,
not used to assert a clean throughput gain.

A later interleaved series, after a 1% GPU snapshot, recorded:

| Case | Time attention mean | Effective TFLOPS | Frequency attention mean |
| --- | ---: | ---: | ---: |
| Accepted control, start | 43.174 ms | 12.659 | 5.147 ms |
| Disjoint K/V, eight subgroups, start | 40.458 ms | 13.509 | 4.905 ms |
| Disjoint K/V, twelve subgroups | 37.215 ms | 14.687 | 3.466 ms |
| Disjoint K/V, eight subgroups, return | 40.727 ms | 13.420 | 5.173 ms |
| Accepted control, return | 53.255 ms | 10.263 | 5.733 ms |

No external CCS client above the summarizer's reporting threshold was observed in
those five cases, but the last control still drifted substantially. This is not an
exclusive-device or fixed-clock experiment; do not turn the slow return control into
an inflated percentage improvement. `measurement-summary.json` retains raw samples,
standard deviations and competing-client observations.

### Complete XE90 model outputs

The same six-second input, installed F32 GGUF and Rust test executable were run twice
per configuration. The production Rust model, frontend and synthesis are included;
loading, Worker IPC and final codec/publication are excluded from the process timer.

| Second complete pass | Accepted control | Disjoint candidate, eight subgroups |
| --- | ---: | ---: |
| Time attention | 48.8659 ms | 43.0744 ms |
| Frequency attention | 5.46929 ms | 5.33272 ms |
| Complete processing | 7.464934 s | 7.176703 s |

This pair observes lower latency, but it is not a statistical whole-model guarantee.
All 529,200 samples match exactly. Both complete corresponding WAV files also compare
byte-for-byte, and both configurations repeat identically. Evidence:
`xe-control/`, `xe-candidate/`, `model-comparison.json` and
`complete-wav-comparison.json`.

## Intermediate subgroup experiment

Commit `f17c451` adds `attention-intermediate-shared-groups.patch`. It preserves the
candidate's default eight-subgroup dispatch and exposes twelve through the existing
explicit shared-group variable, subject to ordinary device resource limits.
Each subgroup still owns eight queries: twelve groups cover 96 queries, use 384
invocations and allocate 31,136 shared bytes. The actual twelve-group pipeline was
observed and its 30 query-owned numerical cases passed. The 14.687-TFLOPS value above
is an isolated shape measurement, not a measured twelve-group model result.

## Observation and acceptance boundaries

The installed Worker and installation numerics ran in boot
`ad1bdfc3-25a9-4f60-818f-d480ac877e37`. Subsequent candidate GPU tests ran in boot
`d47a2a86-dd02-411d-9ea8-6cade35e10b2`. The reason for the intervening reboot is not
established. Each cited performance comparison stays within the latter boot, and a
post-reboot file audit confirms the accepted libraries remain installed. Exit zero
and matching boot IDs within a run do not establish post-exit host stability.

Twenty TFLOPS has not been established. For the same time-attention FLOP count it
requires 27.328057344 ms. No same-shape XPU comparison, whole-song qualification,
Windows execution, AMD/NVIDIA regression or Nix release acceptance was performed.
The new experimental libraries are isolated; only the accepted ten-patch runtime
was installed. Subsequent candidate results must be recorded separately rather than
silently replacing this installation's provenance.

## One-time Q staging waves

Commit `a0a22bc` adds `attention-wave-query-staging.patch` on top of the two
experiments above. Cached Q fragments are unchanged, but their one-time staging
uses waves of at most eight subgroups. A workgroup barrier between waves prevents
one group's Q stores from overwriting another group's outstanding reads. The
hot key loop still uses cached Q and disjoint K/V; it does not reload Q from SLM.

The matching host accounting is `min(groups, 8) * 1024 + groups * 1568 + 32`
bytes. Twelve groups use 27,040 bytes instead of 31,136; fourteen use 30,176;
sixteen use 33,312. Default selection remains eight, with larger group counts
available only as explicit experiments. These resource limits do not establish
that a particular hardware occupancy threshold caused any measured change.

The wave build completed at
`test-artifacts/operations/20260910T063324-a20c78d82798/`, exit zero. Its boot ID
is `e56ad005-bfa7-47ea-9891-9b96928ffd25`, indicating another intervening reboot
of unknown cause. This series therefore uses fresh control measurements in that
boot rather than mixing its absolute times with the preceding boot.

Each of twelve, fourteen and sixteen groups passed all 30 query-owned numerical
cases. Fourteen additionally passed 26 floating/mixed-storage and seven fallback
cases, for 63 fixture instances in that configuration. A separately recorded same-boot
sequence used eight warmups and eight measured GPU calls per shape:

| Configuration | Time attention | Effective TFLOPS | Frequency attention |
| --- | ---: | ---: | ---: |
| Non-wave twelve-group control, start | 36.427 ms | 15.004 | 3.465 ms |
| Wave twelve groups | 35.920 ms | 15.216 | 3.604 ms |
| Wave fourteen groups | 34.644 ms | 15.777 | 3.635 ms |
| Wave sixteen groups | 41.449 ms | 13.186 | 5.480 ms |
| Non-wave twelve-group control, return | 36.502 ms | 14.974 | 3.545 ms |

The pre-series snapshot showed 1.08% aggregate CPU and 0% GPU utilization.
No external CCS client above the summarizer reporting threshold appeared in these
cases; visibility remains partial. The sixteen-group configuration is slower,
not a default to promote merely because it shares K/V across more queries.
The controls bracket the series closely, but this remains a bounded shape test.

The separate fourteen-group compiler diagnostic reports 30,176 shared bytes,
2,275 instructions, six spills, 53 fills, 250 sends and 128 GRFs. Non-SSA register
count is 153. These counts must not be conflated with GPU stall percentages or
zero-spill execution. `compiler-wave-fourteen` timings are not benchmark samples.

## Fifteen-group probe and final real-model comparison

Commit `314a18f` adds `attention-fifteen-shared-groups.patch`. Fifteen wave-staged
subgroups use 31,744 shared bytes and 480 invocations. The build completed at
`20260910T064028-45960f20a954`, exit zero. This remains an explicitly selected
experiment rather than a new default. It passed 30 query-owned, 26 floating/mixed
and seven fallback fixtures, with no relaxed tolerances.

The final isolated series, in order, was accepted control (43.843 ms), wave fourteen
(45.580 ms), wave fifteen (37.719 ms), wave fifteen return (35.401 ms), accepted
control return (44.023 ms). Corresponding effective time-attention rates are
12.466, 11.991, 14.490, 15.439 and 12.415 TFLOPS. The fourteen-group result is slower
than its earlier 34.644-ms observation; it is retained, not discarded or relabeled.
The 15.777-TFLOPS earlier observation is therefore not a sustained-throughput promise.
The pre-series GPU snapshot was 15%; the per-run observations did not report an
external CCS client above the summarizer threshold. These observations still do not
establish fixed clocks or exclusive access.

The final model series ran the accepted control, fifteen-group candidate,
fourteen-group candidate, then the accepted control again. Each performed two full
passes through the same production Rust XE90 graph on the same six-second input.
All eight complete corresponding output WAVs match, each with 529,200 finite samples.
Both candidate versions preserve this input's output bytes exactly.

| Second complete pass, execution order | Time attention | Frequency attention | Complete processing |
| --- | ---: | ---: | ---: |
| Accepted control, start | 56.4022 ms | 6.44163 ms | 8.100729 s |
| Wave fifteen groups | 36.9618 ms | 4.04708 ms | 7.128944 s |
| Wave fourteen groups | 36.3168 ms | 3.75014 ms | 6.953229 s |
| Accepted control, return | 45.0765 ms | 5.34905 ms | 7.251583 s |

The fourteen-group real-model attention is 15.0498 TFLOPS. Compared with its adjacent
return control, the observed complete process reduction is 4.1143%. The initial
control is substantially slower; do not use its 8.100729 seconds to present a reliable
12% end-to-end gain for fifteen groups. The model results remain bounded pairs, not
whole-song or statistical qualification. No external CCS client above the reporting
threshold appeared in these four cases, but the control drift remains observable.

`final-model-fifteen.json`, `final-model-fourteen.json` and `final-verification.json`
retain the numeric results. A replay of all ten accepted patches plus the four
experimental patches from a fresh pinned GGML checkout reproduces the tested C++
and query-owned shader exactly (`20260910T065001-4406c9ab4896`). The final byte audit
also confirms that the installed four libraries remain the accepted ten-patch build;
no candidate library was installed. The current audit boot is
`e56ad005-bfa7-47ea-9891-9b96928ffd25`.

### Reproducing an isolated candidate

The incremental experimental order after the ten accepted recipe patches is:

1. `attention-disjoint-kv-staging.patch` (`652a469`).
2. `attention-intermediate-shared-groups.patch` (`f17c451`).
3. `attention-wave-query-staging.patch` (`a0a22bc`).
4. `attention-fifteen-shared-groups.patch` (`314a18f`).

The tested combined candidate is `runtime-wave-plus/lib` under the evidence root.
It still defaults to eight groups; `UTA_STUDIO_GGML_FA_SHARED_GROUPS=14` or `15`
explicitly selects the reported candidate. Use the existing recorded test harness
and a separate library directory. Do not put these experimental patches into the
accepted recipe or replace the installed runtime merely to replay measurements.

The remaining distance to 20 TFLOPS is real: the 36.3168-ms real-model attention
would have to reach 27.328057344 ms with the same FLOP count. Further work should
measure the remaining matrix-operand transactions, six-spill/fifty-three-fill
candidate code and score/probability handoff rather than extrapolating the best
sample or assuming that more subgroups monotonically improve throughput.
