# B580 attention: installed workgroups and continued throughput probes

## Installed result

The accepted nine-patch recipe, including compact eight-subgroup workgroups
and floating K/V storage, is installed in:

```text
/home/bintis/.local/share/uta-studio/runtime/ggml-vulkan
```

The existing application's `ggml-vulkan-v1` entry resolves to that directory.
The previous runtime is preserved at:

```text
/home/bintis/.local/share/uta-studio/runtime-backups/before-attention-workgroups-20260910T051459Z-c4d36855/ggml-vulkan
```

Evidence root: `test-artifacts/attention-installed-throughput/`.
The build is recorded by operation `20260910T050921-277fc860791c`, with exit
zero and all nine patches listed in its output. The later overlapping build
entry `20260910T051126-2d4941f1154a` rejected the already-modified source; it is
not a second successful build. `install-receipt.json` records the installation.
No model, source media, application settings, or launcher was changed.

The source recipe subsequently gained `0010-vulkan-attention-compact-row-scales.patch`.
That later ten-patch recipe is not this installation or this experiment's base.
Its independent acceptance is documented in `ROFORMER_B580_COMPACT_RESCALE.md`;
do not relabel the installed nine-patch results as a test of that later snapshot.

The installed Worker was run without `UTA_STUDIO_GGML_RUNTIME_DIR`. Its own
loader record, separated from its codec subprocesses, lists the four expected
GGML libraries under the active directory. Its actual XE90 run reports:

```text
ggml_vulkan: query-owned attention Br=64 Bc=32 subgroups=8 lanes=32 K=f16 V=f16
```

The Worker returned `status: ok`. Both six-second stereo output FLAC files
contain 529,200 finite interleaved samples and are byte-identical to the
corresponding pre-installation Worker outputs. This does not imply exact mix
reconstruction: the previously documented residual FLAC saturation still
exists. The output/publication code was not changed.

The installed library set passed 61 explicit numerical cases: 30 query-owned,
24 floating/mixed K/V, and seven original short-query/head-size cases. The
maximum reported NMSE is `3.1445150974831876e-7`. These are six Rust test
functions, not 61 independent test functions. Raw records are `installed-query`,
`installed-floating`, and `installed-fallback`; loader/audio details are in
`installation-verification.json`, and consolidated status is in `phase-status.json`.

## Throughput target and interpretation

The prior accepted kernel's approximately 12.7 TFLOPS remains the reference
result documented in `ROFORMER_B580_FRAGMENT_HANDOFF.md` and
`ROFORMER_B580_FLOATING_STORAGE.md`. It is not a fresh 20-TFLOPS result.
For the unchanged XE90 time-attention shape, QK plus PV contain
546,561,146,880 FLOPs. Achieving 20 TFLOPS under the same whole-fused-kernel
GPU timing convention requires approximately 27.328057344 ms.

The installed-study records retain negative probes of tile-major handoff,
shared-only subgroup synchronization, and one-time Q staging through probability
scratch. They are separate experimental libraries, not installed changes.
`measurement-summary.json` records their numerical and timing observations.
In particular, the shared-only synchronization probe measured 73.793 ms on
the time axis while a subsequent control measured 43.549 ms; merely weakening
the scope of a synchronization primitive did not establish an optimization.

### Key-block width

Experimental patch `attention-query-owned-key-width.patch` changes the key
block to 16 or 64 and adjusts dispatch, address calculations, and scratch
accounting together. Both widths passed the query-owned numerical suite in
`key-narrow-numeric` and `key-wide-numeric` before the later timing series.

Operation `20260910T060833-3581fdd61825` ran the following bounded comparison.
Each shape retains eight warmup and eight measured calls. Values below are
mean GPU milliseconds; these are diagnostic observations, not an uncontended
performance acceptance.

| Configuration | Time axis | Frequency axis | Time-axis effective TFLOPS |
| --- | ---: | ---: | ---: |
| 32-key control, first | 53.100 | 6.007 | 10.293 |
| 16-key candidate | 46.448 | 4.262 | 11.767 |
| 64-key candidate | 103.435 | 16.236 | 5.284 |
| 32-key control, return | 52.547 | 5.926 | 10.401 |

Both control runs contain another `attention-tests` compute client in the host
samples: peak observed compute-engine activity was approximately 66.95% and
37.39%, respectively. The samples have not been removed or labeled isolated.
The 16-key frequency result is a lead, not proof of a fair matched improvement.
The 64-key configuration is not selected. Neither key-width experiment is
installed. Complete samples and competing clients are in
`key-width-comparison.json`; the summary operation is
`20260910T060946-7e4157b18291`.

### Disjoint K/V within existing operand scratch

Commit `80135be` adds only the experimental patch:

```text
native-inference/ggml-worker/experiments/attention-dual-operand-tiles.patch
```

The isolated source/runtime is in `test-artifacts/attention-dual-operand-study/`.
With eight subgroups, initial Q staging already reserves 8 KiB, while one
32-by-64 half-precision K or V tile uses 4 KiB. The candidate puts K and V into
disjoint halves after Q fragments have been loaded. It loads both operands
before QK, replaces the K-to-V workgroup exchange with subgroup score
publication, and removes the separate V-publication workgroup barrier. The
end-of-block workgroup barrier still protects both operands before reuse.
The four-subgroup path retains the prior alternating tile.

This changes four workgroup barriers per key block to two on the eight-group
path without increasing declared shared memory. It preserves full context,
operand/probability rounding, output accumulation order, masks, and tail guards
in the source. These are inspected code properties, not numerical proof or a
measured speedup.

The resumed library build completed with exit zero in operation
`20260910T061047-3a126111a845`. Its shader passed `spirv-val --target-env vulkan1.3`
in operation `20260910T061258-1652c570910e`. The requested subsequent GPU numerical
invocation was blocked by the tool; no numerical or timing pass is claimed for
this candidate. It has not been added to the recipe or installed.

## Interruption and acceptance boundary

The host boot ID changed from `ad1bdfc3-25a9-4f60-818f-d480ac877e37` to
`d47a2a86-dd02-411d-9ea8-6cade35e10b2`; the new journal begins at
2026-09-10 15:03:54 JST. The earlier dual-operand build records
`20260910T060226-3cb7d2a05559` and `20260910T060432-0776a1083123` lack completion
records. Their outcomes remain unknown; later successful completion does not
retroactively repair them. The previous journal was not persistent, so the
restart cause is unknown. No attribution to the candidate kernel is established.

Operation `20260910T060756-75d050d52d01` confirms that the installed entry still
resolves to the accepted runtime after that restart. A user process was not
stopped to obtain timings. Already-running Workers must restart to load changed
shared libraries; the installation does not hot-replace their mapped libraries.

This is local runtime integration plus bounded experiments, not whole-song,
cross-vendor, Windows, formal Nix release, or long-term host-stability acceptance.
The 20-TFLOPS target is not yet established. The next unresolved experiment is
GPU numerical validation of the disjoint-operand shader, followed by a matched
control/candidate/control measurement only after that passes. Keep it outside
the application's installed runtime until those observations exist.
