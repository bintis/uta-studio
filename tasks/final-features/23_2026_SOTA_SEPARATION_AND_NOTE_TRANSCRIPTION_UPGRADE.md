# 23 — 2026 SOTA Separation + Singing Note Transcription Upgrade

**State:** `READY` (`integration_ready=yes`; broader real-song qualification remains advisory)

**Repository:** `/home/bintis/Code/uta-studio`

**Task class:** Model/runtime integration + Analysis Engine evidence/fusion upgrade

**Baseline date:** 2026-08-30

**Scope:**

- replace the aging default vocal-separation specialist with the strongest practical public BS-RoFormer checkpoint currently suitable for local integration;
- replace the aging independent instrumental specialist with the strongest practical public PolarFormer route currently suitable for local integration;
- track and gate the newer MVSep-only 124-band SOTA family without fabricating unavailable checkpoints;
- add a Japanese-pop-specific singing-note expert based on JBM555 CE+CTC;
- prepare a truthful T3MS timed-score integration path, but fail closed if upstream executable artifacts remain unavailable;
- add VocalParse-1.7B as the Mandarin symbolic-score expert when a native runtime can be proved;
- preserve Uta! Studio's current evidence-first Candidate Pool / melody-aware global-path architecture instead of replacing it with a single-model pipeline.

---

# 0. Mission

Upgrade the current pipeline from:

```text
Original Mix
    |
    +-> BS-RoFormer EP317 ----------------> Vocal
    |
    +-> MelBand-RoFormer Inst V2 ---------> Instrumental

Prepared Vocal
    |
    +-> RMVPE
    +-> FCPE
    +-> GAME
    +-> Basic Pitch
    +-> STARS
    +-> ROSVOT
    +-> Acoustic DSP
    |
    v
Candidate Pool
    |
    v
fusion-v16 / hsmm-v15
    |
    v
CanonicalSingingTrack
```

toward:

```text
                                +------------------------------+
                                |                              |
Original Mix                    |                              |
    |                           |                              |
    +-> Leap XE 90 vocals ------+--> Guide / Vocal ----------+ |
    |                                                         | |
    +-> PolarFormer public ----------> Instrumental           | |
    |                                                         | |
    +-> future BS-RoFormer 124-band --------------------------|-+
    |    only when exact public artifacts exist               |
    |                                                         |
    +-> future PolarFormer 124-band --------------------------|-+
         only when exact public artifacts exist               |
                                                              |
Prepared / analysis-ready vocal                               |
    |                                                         |
    +-> RMVPE ---------------- continuous F0 -----------------+|
    +-> FCPE ----------------- F0 challenger ----------------+|
    +-> GAME ----------------- boundary/pitch evidence ------+|
    +-> Basic Pitch ---------- onset/contour evidence -------+|
    +-> Acoustic DSP --------- onset/vibrato/glide ----------+|
    +-> STARS ---------------- conditioned note evidence ----+|
    +-> ROSVOT --------------- conditioned note evidence ----+|
    +-> JBM555 CE+CTC -------- Japanese note evidence -------+|
    +-> VocalParse-1.7B ------ Mandarin symbolic score ------+|
                                                              |
Original Mix -------------------------------------------------+|
    +-> future T3MS --------- timed note + rhythm evidence ---+|
                                                              |
                                                              v
                                            normalized evidence universe
                                                      |
                                +---------------------+--------------------+
                                |                     |                    |
                                v                     v                    v
                     segmentation hypotheses   pitch hypotheses   rhythm hypotheses
                                \                     |                    /
                                 \                    |                   /
                                  +-------------------+------------------+
                                                      |
                                                      v
                                             bounded Candidate Pool
                                                      |
                                   +------------------+------------------+
                                   |                                     |
                                   v                                     v
                           Algorithm selector                    AI judgment selector
                                   |                                     |
                                   +------------------+------------------+
                                                      |
                                                      v
                                            validated melody path
                                                      |
                                                      v
                                          CanonicalSingingTrack
                                         /                      \
                                        /                        \
                                discrete MIDI/score        continuous F0/pitch bend
```

The product objective is not "run more models". The objective is:

```text
better vocal isolation
+ better instrumental isolation
+ fewer false micro-notes
+ fewer vibrato/octave errors
+ better Japanese-pop note boundaries
+ stronger score-level pitch/rhythm evidence
+ preserved provenance and uncertainty
```

---

# 1. Mandatory repository rules

Before editing anything, read:

```text
AGENTS.md
tasks/remaining-models/STATE.md
docs/KEY_CONCLUSIONS.md
docs/engineering-constraints.md
```

Also inspect the current working tree and current source, especially:

```text
analysis-engine/src/candidate_pipeline.rs
analysis-engine/src/fusion/**
analysis-engine/src/planner/plan.rs
analysis-engine/src/artifact/advanced_notes.rs
analysis-engine/src/contract/capability.rs
analysis-engine/src/workflow.rs
runtime-manager/src/catalog.rs
runtime-manager/src/resolver.rs
native-inference/roformer/**
native-inference/openvino-worker/**
native-inference/qwen-worker/**
native-inference/runtime-lock.json
app-core/src/audio_model.rs
app-core/src/workflow/default_definition.rs
```

The working tree is already dirty. Preserve all unrelated user work.

Forbidden:

```text
git reset
git checkout -- .
git restore .
git clean -fd
git stash without explicit user request
replacing current files from HEAD
reverting unrelated changes
```

Current source and focused tests override stale historical task conclusions.

Do not run the reserved whole-workspace / final Nix / formal release acceptance pass unless explicitly requested.

---

# 2. Architectural invariants that must remain true

## 2.1 Production is native-only

Production must not depend on:

```text
Python inference
PyTorch inference
a localhost Python service
remote model APIs
arbitrary user scripts as hidden fallbacks
```

Python is allowed for isolated reference/conversion/research phases only:

```text
checkpoint inspection
reference inference
ONNX export
parity capture
conversion
research reproduction
```

Production routes must remain native and supervised by the existing Runtime Manager / worker process architecture.

---

## 2.2 Studio does not own backend implementation details

Keep the existing boundary:

```text
Studio/app-core
    |
    +-> uta-analyze machine protocol
    +-> uta-runtime machine protocol

Analysis Engine + Runtime Manager
    |
    +-> concrete model/runtime/backend resolution
```

Do not import backend crates into `app-core/**` or `desktop/**`.

Do not expose arbitrary checkpoint paths in normal Analysis settings.

---

## 2.3 Independent separation specialists remain valid

The current default workflow deliberately uses:

```text
SeparationStrategyV1::IndependentSpecialists
```

Preserve this design.

Do not force one separator to be both the vocal and instrumental product truth just because it can mathematically emit both stems.

The default product may choose:

```text
best practical vocal specialist
+
best practical instrumental specialist
```

and publish the two outputs independently with exact provenance.

---

## 2.4 Continuous F0 is not discrete MIDI truth

Preserve the existing conclusion:

```text
RMVPE / FCPE continuous F0
!=
authoritative target MIDI notes
```

Vibrato/glissando/portamento remain continuous performance evidence.

Semantic notes remain score-level hypotheses selected from measured evidence.

---

## 2.5 No hidden post-hoc MIDI smoothing

Forbidden shortcuts:

```text
final MIDI -> median filter -> overwrite pitches
```

```text
if duration < 100 ms: delete note
```

```text
if jump > N semitones: clamp to previous note
```

Preserve the existing 21J direction:

```text
persistent F0 transitions
hysteresis
robust cents-domain pitch centers
F0Consolidation
octave-return prior
vibrato/glissando continuity
short-note evidence cost
phrase-local melody reasoning
Candidate provenance
```

Any new expert must enter this evidence universe rather than bypass it.

---

# 3. Current source facts relevant to this task

At task creation time the repository currently has:

```text
DEFAULT_VOCAL_MODEL_ID = "bs_roformer_vocals_ep317"
DEFAULT_BGM_MODEL_ID   = "melband_roformer_inst_v2"
```

Current default note/evidence family includes:

```text
RMVPE
FCPE
GAME
Basic Pitch
STARS
ROSVOT
Acoustic DSP
```

Current Candidate Pipeline already accepts:

```text
continuous pitch evidence
advanced note evidence
technique evidence
boundary constraints
GAME evidence
Basic Pitch evidence
Acoustic evidence
```

Current 21J melody architecture is already evidence-first and should be preserved.

Do not rewrite this architecture merely to integrate new checkpoints.

---

# 4. Separation model strategy — three upgrade tracks

This task has three separation upgrade tracks. Track C contains two 124-band benchmark targets, because the practical public PolarFormer and the newer 124-band PolarFormer are not the same artifact generation:

```text
Track A: Leap XE 90 vocals
         strongest practical public vocal-specialist upgrade

Track B: PolarFormer public (current public 62-band generation)
         strongest practical public instrumental-specialist upgrade

Track C: 124-band SOTA family
         BS-RoFormer 124-band 2026.07
         + PolarFormer 124-band 2026.06
         benchmark-leading future targets gated on exact public artifacts
```

Track C is deliberately a strict upstream-artifact gate. Benchmark names are not downloadable model identities.

---

# 5. Separation benchmark facts — baseline 2026-08-30

Use MVSep Multisong numbers only as comparative benchmark evidence, not as proof that a checkpoint with a similar filename is identical.

Current relevant published Multisong results include:

| Model | Vocals SDR | Instrumental SDR | Local artifact status |
|---|---:|---:|---|
| BS-RoFormer 124 bands, MVSep 2026.07 | 12.3339 | 18.6414 | exact public checkpoint/config not verified at task creation |
| BS PolarFormer 124 bands, MVSep 2026.06 | 12.0230 | 18.3304 | exact public checkpoint/config not verified at task creation |
| BS-RoFormer Leap XE 90 bands | 11.7577 | 17.5303 | public checkpoint + config available |
| BS PolarFormer public model | 11.7575 | 18.0650 | public checkpoint lineage + ONNX/config available |
| BS-RoFormer EP317 / viperx generation | ~10.87 | ~17.17 | currently integrated legacy default |

MVSep also reports for BS-RoFormer 124-band 2026.07:

```text
50% overlap: vocals ~12.33, instrumental ~18.64
87% overlap: vocals ~12.39, instrumental ~18.70
```

Do not encode these numbers as product quality guarantees.

Authoritative benchmark references to re-check at execution time:

```text
https://mirror.mvsep.com/quality_checker/multisong_leaderboard?sort=vocals
https://mvsep.com/algorithms/34?lang=en
https://mvsep.com/quality_checker/entry/10009
```

---

# 6. Track A — BS-RoFormer Leap XE 90 vocals

## 6.1 Canonical public artifact

Candidate model ID:

```text
bs_roformer_leap_xe90_vocals
```

Display name:

```text
BS-RoFormer Leap XE 90 Vocals
```

Capability:

```text
audio.extract_vocals
```

Canonical public source currently available:

```text
repository:
    https://huggingface.co/pcunwa/BS-Roformer-Leap

subdirectory:
    Xe/

vocal checkpoint:
    Xe/bs_leap_xe_voc.ckpt

vocal config:
    Xe/leap_xe_config_voc.yaml
```

Observed checkpoint identity at task creation:

```text
size:   267,796,851 bytes
sha256: b739c1d2d87a81cd3dd3844ed9ad0bd678708c7a0a761a03a1aaff9af79a096d
```

Observed source commit containing XE files:

```text
4e47d6662ae82eaa8b4ac4329fe66099a843b48e
```

Re-check these facts before final catalog pinning.

---

## 6.2 Relevant Leap XE config facts

Current published XE config includes approximately:

```yaml
audio:
  chunk_size: 881559
  n_fft: 2048
  num_channels: 2
  sample_rate: 44100

model:
  dim: 256
  depth: 16
  stereo: true
  num_stems: 1
  time_transformer_depth: 1
  freq_transformer_depth: 1
  linear_transformer_depth: 0
  dim_head: 64
  heads: 8
  stft_n_fft: 2048
  stft_hop_length: 512
  stft_win_length: 2048
  stft_normalized: false
  mask_estimator_depth: 2
  mlp_expansion_factor: 4
  skip_connection: false

inference:
  num_overlap: 2
```

Note the source YAML also contains legacy/top-level audio fields whose values are not necessarily the neural model's effective STFT parameters. Treat `model.stft_*` and the authoritative upstream inference implementation as the semantic source of truth; do not mix top-level convenience fields with model-internal STFT settings.

The published `freqs_per_bands` list contains 90 bands.

Do not infer missing values from EP317.

Use the exact source config.

---

## 6.3 Preferred runtime path for Leap XE

The existing native RoFormer runtime already reads BS-RoFormer metadata such as:

```text
general.architecture
num_bands
dim
depth
num_stems
n_fft
hop_length
win_length
mask estimator depth
MLP expansion
skip connection
```

Therefore the preferred first integration attempt is:

```text
exact Leap XE checkpoint + YAML
    |
    v
source-verified CKPT -> GGUF conversion
    |
    v
existing native bs_roformer graph
    |
    v
GGML/Vulkan
```

Do not assume compatibility merely because both models are called BS-RoFormer.

Prove:

```text
weight names/shapes
band gathering
position semantics
STFT parameters
mask semantics
output target semantics
```

before reuse.

If graph parity fails, add model-specific graph handling rather than silently changing source semantics.

---

## 6.4 Leap XE integration must be side-by-side first

Do not immediately rename or overwrite:

```text
bs_roformer_vocals_ep317
```

Add Leap XE as a new exact model identity.

Qualification sequence:

```text
EP317 remains existing baseline
Leap XE becomes BenchmarkCandidate / Experimental
    |
    v
reference parity
native conversion parity
short real audio
full-song bounded run
quality A/B
    |
    v
only then change DEFAULT_VOCAL_MODEL_ID
```

This preserves reproducibility of existing artifacts.

---

## 6.5 Leap XE license metadata policy

The public Hugging Face repository is accessible, but at task creation no sufficiently explicit checkpoint license was verified from the model card.

Do not infer checkpoint license from:

```text
lucidrains implementation license
Music-Source-Separation-Training source license
GGML runtime license
```

Track separately:

```text
source-code license
checkpoint license
conversion tooling license
runtime license
```

If checkpoint license remains unresolved:

```text
technical integration may proceed under explicit LocalImport policy
License metadata remains advisory and does not block Production redistribution/readiness
```

Do not fabricate MIT/Apache/CC terms.

---

# 7. Track B — BS PolarFormer public instrumental specialist

## 7.1 Candidate identity

Recommended model ID:

```text
bs_polarformer_public_instrumental
```

Display name:

```text
BS PolarFormer Public
```

Primary product capability:

```text
audio.extract_instrumental
```

The model may internally produce vocals/other semantics, but the Uta product route should publish the semantic role it has actually qualified.

---

## 7.2 Canonical public artifacts

Current public conversion repository:

```text
https://huggingface.co/bgkb/bs_polarformer
```

Published files include:

```text
bs_polarformer.onnx
bs_polarformer_fp16.onnx
model_bs_polarformer_float16.yaml
convert_to_onnx.py
run_onnx_inference.py
```

Observed FP32 ONNX identity at task creation:

```text
size:   ~211 MB
sha256: 1c6857c34556c72d4094d4515c5725549bf987a63a1a8c37a7e7fc111b525c50
```

The public conversion script identifies its source checkpoint as:

```text
model_bs_polarformer_float16.ckpt
```

from the ZFTurbo Music-Source-Separation-Training release lineage.

The Hugging Face repository currently declares:

```text
license: MIT
```

Still record source checkpoint, conversion repository and converted ONNX identities separately.

---

## 7.3 PolarFormer is not ordinary BS-RoFormer

PolarFormer changes positional representation from RoPE-style behavior to PoPE / Polar Positional Embeddings.

Therefore:

```text
DO NOT simply relabel the existing bs_roformer GGML graph as PolarFormer.
```

Preferred integration path:

```text
exact public FP32 ONNX / exact canonical checkpoint
    |
    v
reference ONNX Runtime output
    |
    v
OpenVINO conversion
    |
    v
OpenVINO CPU parity
    |
    v
bounded GPU validation
    |
    v
native DSP / output contract
```

If direct OpenVINO conversion preserves semantics and is bounded, prefer it over writing a new GGML graph.

---

## 7.4 PolarFormer target semantics

MVSep public-model benchmark evidence:

```text
Vocals SDR:       11.7575
Instrumental SDR: 18.0650
```

This makes it especially attractive as the **instrumental specialist** in the existing independent-specialist design.

Do not choose it as instrumental truth solely because of one number.

A/B against current `melband_roformer_inst_v2` on representative songs and report at least:

```text
vocal leakage in instrumental
musical damage
high-frequency damage
transient preservation
stereo image
fullness
bleedless-like internal diagnostics where possible
```

Use existing `audio-quality-gates-v2` semantics rather than inventing fake SDR without ground truth.

---

## 7.5 PolarFormer replacement policy

Current:

```text
DEFAULT_BGM_MODEL_ID = "melband_roformer_inst_v2"
```

Target after qualification:

```text
DEFAULT_BGM_MODEL_ID = "bs_polarformer_public_instrumental"
```

Do not remove Inst V2.

Keep it as a selectable historical/alternative model while existing artifact identities remain valid.

---

# 8. Track C — 124-band SOTA family upstream-artifact gate

There are currently benchmark-leading 124-band models on MVSep that materially exceed the public practical models. This section deliberately includes both the 2026.07 BS-RoFormer 124-band model and the 2026.06 PolarFormer 124-band model so the execution agent does not confuse the public 62-band PolarFormer with the newer benchmark-only 124-band generation.

Two important targets are:

```text
BS-RoFormer 124 bands (2026.07)
BS PolarFormer 124 bands (2026.06)
```

Published Multisong metrics at task creation:

```text
BS-RoFormer 124:
    vocals       12.3339
    instrumental 18.6414

BS PolarFormer 124:
    vocals       12.0230
    instrumental 18.3304
```

The BS-RoFormer 124 route is currently the stronger benchmark target.

---

## 8.1 Critical 124-band rule

At task creation, an exact public checkpoint/config pair matching the MVSep 124-band SOTA entries has **not** been verified.

Therefore:

```text
benchmark entry
!=
locally available checkpoint
```

Forbidden:

```text
inventing a 124-band checkpoint URL
using an unrelated 124-band config with another checkpoint
calling a community file "MVSep 2026.07" without identity proof
copying weights from a similarly named model
creating a Production catalog entry that cannot resolve exact artifacts
```

---

## 8.2 124-band gate behavior

At execution time, re-check authoritative sources:

```text
MVSep algorithm pages
MVSep resource/download pages
ZFTurbo releases
model author repositories
verified Hugging Face repositories
```

If an exact public artifact appears, require all of:

```text
exact checkpoint
exact config
exact upstream source/revision
exact target semantics
license identity
reference inference
```

before creating a runnable model resource.

Otherwise record:

```text
BS-RoFormer 124 2026.07 = BLOCKED_UPSTREAM_ARTIFACT
PolarFormer 124 2026.06  = BLOCKED_UPSTREAM_ARTIFACT
```

Do not let those blockers prevent Leap XE / public PolarFormer completion.

---

## 8.3 Future 124-band adoption policy

If BS-RoFormer 124 exact artifacts become available and pass Uta qualification, evaluate it for both:

```text
vocal specialist
instrumental specialist
```

but do not automatically abandon `IndependentSpecialists`.

Possible future outcome:

```text
Vocal specialist:       BS-RoFormer 124
Instrumental specialist: BS-RoFormer 124
```

or:

```text
Vocal specialist:       BS-RoFormer 124
Instrumental specialist: PolarFormer 124
```

or another empirically stronger independent pair.

Choose by Uta's real product A/B evidence, not model-name prestige.

---

# 9. Separation runtime/catalog changes

Likely areas:

```text
runtime-manager/src/catalog.rs
runtime-manager/src/resolver.rs
runtime-manager/src/requirements.rs
native-inference/runtime-lock.json
native-inference/roformer/**
native-inference/openvino-worker/**
analysis-engine Engine separation route
app-core/src/audio_model.rs
app-core/src/workflow/default_definition.rs
```

Do not add a model to the UI until Runtime Manager can truthfully report its actual state.

---

# 10. Separation provenance requirements

Every generated stem must retain:

```text
source audio identity
model ID
source checkpoint identity
config identity
converted artifact identity
runtime recipe identity
backend
effective chunk size
effective overlap
precision
postprocess/DSP profile
semantic output role
```

Do not collapse:

```text
Leap XE vocal
PolarFormer instrumental
```

into a fake single "separator generation" identity when they are independently generated.

The combined `audio.separate_vocal_bgm` result may reference two child generations.

---

# 11. Separation acceptance metrics

For each model compare:

```text
current baseline
candidate public SOTA
```

Use at least:

```text
exact duration
channel count
sample rate
finite samples
non-silence
source unchanged
cancellation behavior
process cleanup
runtime
peak memory where measurable
```

Quality inspection should include:

```text
vocal leakage
instrumental leakage
musical damage
sibilance artifacts
transient smearing
low-frequency damage
stereo instability
reverb tails
backing-vocal behavior
```

If labeled stems are legally available, compute objective metrics.

Do not fabricate SDR on unlabeled real songs.

---

# 12. Recommended separation rollout order

```text
P0  Leap XE exact source audit
P0  Leap XE reference -> GGUF/native parity
P0  Leap XE side-by-side full-song qualification

P0  public PolarFormer exact source/ONNX audit
P0  PolarFormer OpenVINO parity
P0  PolarFormer side-by-side full-song qualification

P1  switch default independent specialists after acceptance

P2  re-check 124-band public artifacts
P2  integrate only if exact checkpoint/config/license exist
```

Do not block practical upgrades waiting for private/unavailable benchmark models.

---

# 13. Singing-note upgrade strategy

The note stack should evolve from model-role privilege toward latent-variable specialization.

Target responsibility model:

| Expert | Primary role |
|---|---|
| RMVPE | primary continuous performance F0 |
| FCPE | secondary F0/disagreement |
| GAME | fast physical segmentation + pitch evidence |
| Basic Pitch | onset/contour evidence |
| Acoustic DSP | articulation, voicing, vibrato, glide |
| STARS | conditioned physical note timing + technique |
| ROSVOT | conditioned note challenger |
| JBM555 CE+CTC | Japanese-pop physical onset/offset/pitch specialist |
| VocalParse-1.7B | Mandarin symbolic lyrics/pitch/note-value/BPM specialist |
| T3MS | future polyphonic timed-note + symbolic note-value expert |

No one model becomes universal truth.

---

# 14. Generic timed-note normalization layer

Current `AdvancedNoteEvidenceV1` contains STARS/ROSVOT-specific dependencies such as:

```text
shared singing frontend
annotation RMVPE
TimedTranscript
Chinese G2P
```

Do not fabricate these for JBM555 or T3MS.

Introduce a generic normalized representation conceptually similar to:

```rust
struct TimedNoteExpertEvidenceV1 {
    expert_id: String,
    model_generation: String,
    backend: String,
    notes: Vec<TimedNoteHypothesisV1>,
    provenance: EvidenceProvenance,
}

struct TimedNoteHypothesisV1 {
    source_id: String,
    range: TimeRange,
    midi: Option<u8>,

    source_local_boundary_score: Option<f32>,
    source_local_pitch_score: Option<f32>,

    calibrated_boundary_confidence: Option<f32>,
    calibrated_pitch_confidence: Option<f32>,
}
```

Exact names are implementation-owned.

Raw model contracts remain model-specific and truthful.

Normalization happens inside Analysis Engine before Candidate construction.

---

# 15. Japanese specialist — JBM555 CE+CTC

## 15.1 Canonical upstream

Repository:

```text
https://github.com/york135/CECTC_baseline_APSIPA25
```

Paper target:

```text
Singing MIDI Transcription with Music Language Models:
Formulation and Comparison
APSIPA ASC 2025
Yu Sugimoto et al.
```

Public inference checkpoint:

```text
models/jbm555_80
```

Public inference config:

```text
configs/inference_jbm.yaml
```

Model purpose:

```text
Japanese-pop singing-note onset/offset/pitch evidence
```

Recommended Uta model ID:

```text
jbm555_cectc_80
```

Capability:

```text
notes.jbm555
```

---

## 15.2 JBM555 benchmark context

Paper dataset:

```text
555 Japanese popular music songs
train       331
validation  112
test        112
```

Published JBM CE+CTC results:

```text
COn      88.44
COnP     81.18
COnPOff  64.37
```

Published MIR-ST500 cross-dataset result:

```text
COn      79.66
COnP     74.58
COnPOff  57.72
```

Treat these as paper benchmark facts, not guaranteed Uta scores.

---

# 16. JBM555 input semantics — dual input is mandatory

The upstream JBM feature path uses both:

```text
vocal signal
+
mixture signal
```

with multi-scale CQT features.

Do not reduce the product implementation to vocal-only input merely because Uta already has a high-quality vocal stem.

Desired Engine dataflow:

```text
OriginalMix -------------------------------> JBM mix input
     |
     v
selected vocal separation/preparation
     |
     v
AnalysisReadyLead / GuideVocals -----------> JBM vocal input
```

The JBM artifact must therefore depend on both exact upstream artifacts.

---

# 17. JBM555 CQT frontend

Reproduce the upstream CQT semantics before optimization.

Current public reference behavior is approximately:

```python
librosa.cqt(
    sr=44100,
    hop_length=1024,
    fmin=midi_to_hz(24),
    n_bins=384,
    bins_per_octave=48,
    filter_scale={0.5, 1.0, 2.0}
)
```

for both vocal and mixture.

This yields conceptually:

```text
3 vocal CQT channels
+
3 mixture CQT channels
=
6 model input channels
```

Do not substitute:

```text
RMVPE mel
STARS frontend
Basic Pitch frontend
ordinary STFT
```

without proven semantic parity.

---

# 18. JBM555 frontend parity

Required sequence:

```text
reference librosa CQT
    |
    v
native deterministic CQT
    |
    v
feature parity fixtures
```

Fixtures must include:

```text
silence
220 Hz sine
440 Hz sine
220 -> 440 transition
deterministic harmonic vocal-like signal
deterministic noise fixture
short real vocal + mix pair if repository fixture policy permits
```

Compare:

```text
shape
finite values
peak-bin locations
selected bins/frames
L1/L2 numeric error
```

Derive tolerance from measured reference behavior, not arbitrary constants.

---

# 19. JBM555 neural model + decoder

Treat upstream neural and decoder behavior as one reference contract before product Fusion reinterpretation.

Published inference parameters include:

```text
onset threshold: 0.32
offset threshold: 0.70
```

Do not initially retune them.

Reference sequence:

```text
PyTorch source model
    |
    v
raw logits
    |
    v
upstream Python decoder
    |
    v
reference note list
```

Native target:

```text
native CQT
    |
    v
OpenVINO
    |
    v
native decoder
    |
    v
Jbm555EvidenceV1
```

The two paths must agree on deterministic fixtures before Fusion integration.

Do not modify the JBM decoder to remove vibrato fragments; Uta's Candidate/HSMM layer owns that problem.

---

# 20. JBM555 raw artifact

Add a truthful model-specific contract conceptually like:

```rust
struct Jbm555EvidenceV1 {
    schema_version: u32,
    model_id: String,

    upstream_revision: String,
    checkpoint_identity: String,
    config_identity: String,
    conversion_recipe_identity: String,
    runtime_manifest_identity: String,

    backend: String,

    source_start: u64,
    source_duration: u64,

    mix_artifact_identity: String,
    vocal_artifact_identity: String,
    vocal_preparation_generation: String,

    frontend_profile: String,
    decode_profile: String,

    notes: Vec<Jbm555RawNoteV1>,
}
```

Do not persist huge dense logits unless they are needed for diagnostics and remain inside artifact bounds.

---

# 21. JBM555 runtime target

Preferred path:

```text
44.1 kHz mix + vocal
    |
    v
native normalization/CQT
    |
    v
OpenVINO neural graph
    |
    v
native exact decoder
    |
    v
typed evidence JSON
```

No Python fallback in Production.

No `AUTO` backend.

Follow current explicit CPU-reference / Production-backend policies.

---

# 22. JBM555 license metadata policy

Do not infer the checkpoint license from the paper or generic source code.

At execution time audit:

```text
repository LICENSE
checkpoint distribution terms
dataset restrictions
source-code terms
```

If exact checkpoint license remains unresolved:

```text
integration_ready may become yes
production_ready must truthfully retain licensing caveat/blocker according to repository policy
```

No fabricated MIT/Apache attribution.

---

# 23. Japanese planner route

Recommended policy after JBM integration:

## Fast / Japanese

```text
RMVPE        Always
GAME         Always
Acoustic DSP existing behavior
JBM555       Disabled
```

## Balanced / Japanese

```text
RMVPE        Always
GAME         Always
JBM555       conditional / opt-in until Production-qualified
FCPE         disagreement policy
Basic Pitch  disagreement policy
```

## Maximum / Japanese

When installed/allowed:

```text
RMVPE        Always
GAME         Always
JBM555       Always
FCPE         current Maximum policy
Basic Pitch  current Maximum policy
Acoustic DSP Always
ROSVOT       optional challenger
STARS        optional / not Japanese-authoritative
T3MS         only if real runnable artifacts exist
```

Language chooses expert participation, not expert truth.

---

# 24. JBM555 Candidate Pool behavior

Each JBM note contributes:

```text
boundary hypothesis
+
target-pitch hypothesis
```

but JBM pitch remains peer evidence.

Example:

```text
segment S

JBM555: MIDI 69
GAME:   MIDI 70
RMVPE:  69.08
FCPE:   69.02
```

Candidate construction should preserve plausible peer pitch states such as:

```text
S x JBM555 pitch
S x GAME pitch
S x RMVPE pitch
S x FCPE pitch
```

within current candidate bounds.

Do not hard-code:

```text
language == ja => JBM always wins
```

---

# 25. T3MS — future timed-score expert

Canonical research identity:

```text
KimLeekyung/T3MS
Note-Level Singing Melody Transcription for Time-Aligned Musical Score Generation
```

Target model role:

```text
polyphonic audio
    -> onset
    -> MIDI pitch
    -> offset
    -> symbolic note value
```

This is especially valuable because it provides a rhythm/score view that current Uta experts do not natively provide.

---

# 26. T3MS paper semantics to preserve

Paper-reported model/input details include approximately:

```text
sample rate: 16 kHz
STFT hop:    160 samples (~10 ms)
STFT window: 2048
segment:     5.12 s
overlap hop: 2.56 s
edge discard profile: ~1.28 s
```

Output tuple:

```text
(onset time, MIDI pitch, offset time, note value)
```

Paper token vocabulary:

```text
513 time tokens
128 pitch tokens
32 note-value tokens
3 special tokens
= 676 total
```

Published Transformer architecture is approximately:

```text
encoder layers: 12
decoder layers: 8
embedding dim:  512
heads:          8
dropout:        0.1
max output:     512
```

Note-value representation uses sixteenth-note units and has known meter/triplet limitations.

---

# 27. Critical T3MS upstream-artifact gate

At task creation, the official repository does not provide a complete production-ready implementation/checkpoint sufficient for exact Uta integration.

Therefore:

```text
DO NOT FABRICATE T3MS.
```

Forbidden:

```text
inventing checkpoint URLs
inventing hashes
inventing tensor names
inventing licenses
training an unrelated Transformer and calling it official T3MS
claiming a paper reproduction is the author's checkpoint
```

At execution time re-check:

```text
official GitHub
paper/project page
author release pages
official Hugging Face organization/accounts
```

If exact artifacts remain unavailable:

```text
T3MS = BLOCKED_UPSTREAM_ARTIFACT
```

This must not block JBM555 or separation completion.

---

# 28. T3MS reproduction policy

A from-paper reproduction is a distinct research resource and requires explicit authorization.

If ever performed, use a distinct identity such as:

```text
t3ms_reproduction_v1
```

Never label it `t3ms_official`.

A credible reproduction requires:

```text
training-data provenance
pseudo-label pipeline
architecture implementation
training recipe
evaluation protocol
checkpoint identity
comparison against paper metrics
```

Do not spend task time training such a reproduction unless explicitly asked.

---

# 29. T3MS evidence contract

If/when runnable, expose one correlated model invocation as:

```text
TimedNote evidence:
    onset
    offset
    MIDI pitch

Rhythm evidence:
    symbolic note value
```

Conceptually:

```rust
struct TimedScoreExpertEvidenceV1 {
    expert_id: String,
    notes: Vec<TimedScoreNoteHypothesisV1>,
    provenance: EvidenceProvenance,
}

struct TimedScoreNoteHypothesisV1 {
    range: TimeRange,
    midi: u8,
    note_value_units: Option<u8>,
    source_local_score: Option<f32>,
}
```

Do not count timing and rhythm heads from one T3MS invocation as independent experts.

---

# 30. T3MS input route

The paper is designed for polyphonic input.

Preserve reference-domain semantics:

```text
OriginalMix
    |
    v
16 kHz / STFT
    |
    v
T3MS
```

Do not silently switch to separated vocal input.

If a later experiment compares vocal input, store it as a separate execution/input profile.

This intentional source diversity is useful:

```text
JBM555   = mix + separated vocal
T3MS     = polyphonic mix
RMVPE    = prepared vocal
GAME     = prepared vocal
```

---

# 31. T3MS rhythm integration

Current Engine already has:

```text
rhythm-grid-dp-v1
```

T3MS note value should become rhythm evidence, not an unconditional timing rewrite.

Desired scoring concept:

```text
physical note candidate
+
BPM/grid
+
T3MS note-value evidence
    |
    v
rhythm utility
```

Do not treat T3MS's 4/4 / sixteenth-unit / non-triplet limitations as universal score truth.

For incompatible meter/rhythm contexts, attenuate symbolic rhythm authority while retaining physical timing/pitch evidence if valid.

---

# 32. Mandarin symbolic-score expert — VocalParse-1.7B

This task should also prepare/implement VocalParse because it fills a different evidence role than JBM/T3MS.

Canonical model:

```text
https://huggingface.co/pymaster/VocalParse
https://github.com/pymaster17/VocalParse
```

Current public model facts:

```text
base: Qwen3-ASR-1.7B
size: ~2B parameters
checkpoint format: safetensors
license: Apache-2.0
primary language: Mandarin Chinese
```

Current Hugging Face revision at task creation includes a 396-token AST-vocabulary-compatible checkpoint.

Re-check exact revision and file hashes before pinning.

---

# 33. VocalParse semantics

VocalParse outputs a structured autoregressive sequence containing:

```text
lyrics
MIDI pitch tokens
symbolic note-value tokens
global BPM
lyrics <-> note relation
```

Important limitation:

```text
current checkpoint does NOT directly predict physical note durations/onset-offset timestamps
```

Therefore do not force it into `TimedNoteExpertEvidenceV1` as though it measured physical note boundaries.

---

# 34. VocalParse symbolic evidence contract

Add a separate semantic contract conceptually:

```rust
struct SymbolicScoreEvidenceV1 {
    expert_id: String,
    source_segment: TimeRange,
    lyrics: Vec<SymbolicLyricUnitV1>,
    notes: Vec<SymbolicScoreNoteV1>,
    bpm: Option<f32>,
    provenance: EvidenceProvenance,
}

struct SymbolicScoreNoteV1 {
    lyric_index: Option<usize>,
    midi: u8,
    note_value: Option<SymbolicNoteValueV1>,
}
```

Do not fabricate onset/offset fields.

---

# 35. VocalParse physical-time projection

Use existing alignment evidence to project symbolic structure into physical context.

Conceptually:

```text
VocalParse symbolic score
    |
    +-> lyrics
    +-> pitch
    +-> note values
    |
    v
CanonicalLyrics / alignment relation
    |
    v
Qwen Forced Aligner physical ranges
    |
    v
projected score-time hypotheses
```

Provenance must distinguish:

```text
symbolic pitch source: VocalParse
symbolic rhythm source: VocalParse
physical timing source: Qwen Forced Aligner / other measured boundary expert
```

Never claim VocalParse measured physical onset/offset when it did not.

---

# 36. VocalParse native-runtime gate

Production remains native-only.

The official implementation is PyTorch/Qwen based, so do not package Python.

Preferred paths to investigate:

```text
extend existing Qwen native runtime with exact VocalParse vocabulary/model support
OR
add uta-vocalparse-worker using a proven GGUF/native conversion
```

But preserve distinct model/runtime identity from ordinary Qwen ASR.

Because VocalParse adds hundreds of AST tokens, prove tokenizer/vocabulary/model compatibility.

If exact native execution cannot be proved:

```text
VocalParse = Experimental / BLOCKED_NATIVE_RUNTIME
```

Do not weaken the native-only architecture to force integration.

---

# 37. Language-aware expert routing

Recommended high-level policy:

## Japanese (`ja`)

```text
JBM555      preferred specialist when available
GAME        strong physical boundary expert
RMVPE       continuous F0
FCPE        F0 challenger
Basic Pitch onset evidence
Acoustic    expressive/onset context
STARS       optional challenger, not Japanese-authoritative
ROSVOT      optional challenger
T3MS        future polyphonic timed-score expert
VocalParse  disabled by default
```

## Mandarin / Yue (`zh`, `yue`)

```text
VocalParse  symbolic-score expert when native route exists
STARS       conditioned physical note/technique expert
ROSVOT      conditioned note challenger
GAME        boundary expert
RMVPE/FCPE  continuous pitch evidence
Basic Pitch / Acoustic context
JBM555      disabled by default
```

## Other languages

Keep current language-general experts as the baseline until specific validation justifies specialist routing.

Language affects participation, not hard truth.

---

# 38. Candidate Pool must remain selector-independent

Preserve the approved invariant:

```text
measured evidence
    |
    v
one deterministic Candidate Pool
    |
    +-> Algorithm selector
    |
    +-> AI judgment selector
```

AI may select only real Engine candidates.

AI must not invent:

```text
new JBM note
new VocalParse pitch
new T3MS duration
smoothed MIDI
```

that is absent from the Candidate Pool.

---

# 39. New-model correlation rules

Do not double-count correlated evidence.

Examples:

```text
JBM555 depends on exact vocal separator + original mix
VocalParse may depend on the same prepared vocal as RMVPE/GAME
STARS/ROSVOT share conditioned frontend/timing dependencies
T3MS timing and note-value heads share one model invocation
```

Store explicit `depends_on` / correlation groups.

A model's separate output heads are not independent confirmations.

---

# 40. Fusion examples that must behave correctly

## 40.1 Vibrato

```text
JBM:       A4 | Bb4 | A4
GAME:      A4 | A4
RMVPE:     stable A4 center with vibrato
Basic:     no independent attack
Acoustic:  high vibrato
```

Expected semantic result:

```text
one coherent A4 note
```

Continuous F0 still retains vibrato.

---

## 40.2 Real fast note

```text
JBM:       A4 -> B4
GAME:      A4 -> B4
RMVPE:     sustained transition
Basic:     strong onset
alignment: new lyric/phoneme event
```

Expected:

```text
A4 | B4
```

Do not over-merge.

---

## 40.3 Short octave error

```text
JBM:   A5
GAME:  A5
RMVPE: A4
FCPE:  A4
```

Short duration, weak onset, immediate return.

Expected:

```text
A4 candidate remains selectable and may win
```

JBM/GAME disagreement evidence remains visible.

---

## 40.4 Symbolic VocalParse support

```text
VocalParse: one quarter-note A4
STARS:      one physical A4 region
GAME:       A4 | Bb4 | A4 fragments
RMVPE:      vibrato around A4
```

Candidate universe should include a coherent long A4 state backed by real measured timing and symbolic score support.

Do not let VocalParse fabricate physical boundaries.

---

# 41. Capability registry targets

Add only truthful implemented capabilities.

Planned:

```text
notes.jbm555
```

Future only when executable:

```text
score.t3ms
score.vocalparse
```

Outputs may conceptually map to:

```text
note_candidate_evidence
symbolic_score_evidence
rhythm_evidence
```

Do not advertise a capability before the real Engine route can execute it.

---

# 42. Workflow dual-input support for JBM555

JBM requires:

```text
mix_audio
vocal_audio
```

Use typed workflow bindings.

If current `AnalyzerBinding` supports multiple bindings for one analyzer, reuse it.

Otherwise extend minimally to permit:

```text
analyzer_input = "mix_audio"
analyzer_input = "vocal_audio"
```

Do not use implicit filesystem lookups or Studio-owned raw path parameters.

---

# 43. Model lifecycle/UI behavior

Keep:

```text
Models & runtime
```

as lifecycle/install/backend truth.

Keep:

```text
Analysis / Processing Studio
```

as expert participation/policy.

Do not download models during:

```text
startup
page render
status query
plan preview
diagnostics
```

Install/import remains explicit user action.

---

# 44. Runtime state rules for this task

A model may validly finish as:

```text
integration_ready = yes
production_ready  = no
```

Reasons may include:

```text
checkpoint license unresolved
native runtime unavailable
upstream checkpoint unavailable
quality not yet qualified
```

Do not policy-promote missing artifacts into fake Production support.

---

# 45. Reference / conversion process isolation

For all heavy conversions:

```text
reference process -> exit
export process    -> fsync/hash -> exit
ORT process       -> exit
OpenVINO convert  -> fsync/hash -> exit
CPU parity        -> exit
GPU validation    -> only then
```

Do not keep PyTorch + ONNX external data + ORT + OpenVINO models resident in one process.

Follow existing RoFormer safety conclusions.

---

# 46. Accelerator authorization

Follow `AGENTS.md` exactly.

Non-Qwen Vulkan / Level Zero context creation requires explicit user authorization under current repository policy.

Do not interpret this task file itself as authorization to execute new GPU contexts.

Use CPU/reference conversion/testing where possible until authorization is explicit.

---

# 47. Separation deterministic tests

For Leap XE add tests for:

```text
exact model identity
GGUF metadata
90-band layout
weight shape contract
chunk/overlap config
output role
timeline preservation
cancellation
forced wrong-backend rejection
```

For PolarFormer add tests for:

```text
exact ONNX/source identity
PoPE model identity
input/output shape semantics
FP32 reference parity
OpenVINO conversion identity
no silent bs_roformer runtime substitution
instrumental output role
cancellation
```

For 124-band future resources add only gate tests if useful:

```text
no catalog implementation without exact artifacts
```

Do not create dummy models to make tests green.

---

# 48. JBM555 deterministic tests

Add tests for:

```text
raw parser identity
wrong model/config rejection
invalid timeline rejection
zero/negative duration rejection
MIDI range validation
NaN/Inf rejection
mix/vocal dependency identity
CQT parity
onset threshold behavior
offset threshold behavior
local onset maxima
same-pitch repeated attacks
tail clipping
native decoder parity
```

Cache identity must distinguish:

```text
mix A + vocal A
```

from:

```text
mix A + vocal B
```

---

# 49. Fusion regression tests

At minimum cover:

```text
stable note with vibrato
one-frame octave error
sustained true octave leap
short unsupported segmentation
strongly supported short grace-like note
glissando without attacks
same-pitch repeated attacks
large phrase-boundary leap
GAME/RMVPE/FCPE disagreement
JBM/RMVPE disagreement
JBM false vibrato split
JBM real short note
VocalParse symbolic support without physical-time fabrication
```

Existing 21J fixtures must remain green.

---

# 50. Real-song separation A/B

Use representative authorized songs with varied production characteristics.

Compare:

```text
EP317 vs Leap XE
Inst V2 vs PolarFormer public
```

If later available:

```text
Leap XE / PolarFormer public
vs
124-band candidates
```

Record exact source identity and do not overwrite existing evidence artifacts.

---

# 51. Real-song Japanese note A/B

Compare:

```text
current baseline
baseline + JBM555
Maximum experts + JBM555
```

Suggested song categories:

```text
clean J-pop female vocal
male J-pop
anisong high register
fast syllabic singing
heavy vibrato ballad
dense accompaniment
soft/whisper vocal
large leaps
rapid repeated same-pitch notes
```

Do not commit copyrighted audio.

---

# 52. Real-song note diagnostics

Report:

```text
total notes
pitched notes
notes per voiced second
median note duration
<80 ms count
<100 ms count
<150 ms count
large leaps >= 7 semitones
short octave returns
same-word micro-notes
pitch-source disagreement count
JBM-vs-RMVPE cents disagreement
JBM-vs-GAME boundary disagreement
review-region count
runtime
```

These are diagnostics, not automatic truth thresholds.

---

# 53. Ground-truth note metrics when available

If a legal labeled dataset is available, compute standard note metrics such as:

```text
COn
COnP
COnPOff
```

Use a documented evaluation protocol.

Do not claim paper benchmark values for Uta without actually running comparable evaluation.

---

# 54. Performance measurements

Separate runtime costs into:

For separation:

```text
decode
STFT/preprocess
neural inference
postprocess/iSTFT
overlap-add
encode/output
```

For JBM:

```text
decode/resample
CQT
OpenVINO inference
decoder
Fusion contribution
```

For VocalParse/T3MS when available:

```text
frontend
encoder
decoder generation
postprocess
projection/fusion
```

Do not optimize before parity.

---

# 55. Cache/fingerprint requirements

Separation artifact identity must include:

```text
source audio
checkpoint
config
converted artifact
runtime
backend
precision
chunk
overlap
DSP/postprocess profile
semantic role
```

JBM artifact identity must include:

```text
source mix
vocal artifact
separator/preparation generation
checkpoint
config
native CQT version
CQT parameters
conversion recipe
runtime
JBM decoder version
```

VocalParse/T3MS must include exact tokenizer/vocabulary/score-decoder identities.

If Candidate construction/scoring behavior materially changes, update the relevant fusion/selector version and fingerprint.

Do not bump versions for pure optional-model availability if deterministic scoring semantics are unchanged.

---

# 56. Suggested model IDs

Use exact current naming conventions after inspection.

Recommended identities:

```text
bs_roformer_leap_xe90_vocals
bs_polarformer_public_instrumental
jbm555_cectc_80
vocalparse_1_7b
```

Future only:

```text
bs_roformer_124b_2026_07
bs_polarformer_124b_2026_06
t3ms_official
```

Do not create the future IDs as runnable catalog resources unless exact artifacts exist.

---

# 57. Suggested version identities

Conceptually, if new contracts are introduced:

```text
jbm555-frontend-v1
jbm555-decoder-v1
jbm555-evidence-v1
timed-note-normalization-v1
symbolic-score-evidence-v1
timed-score-evidence-v1
```

Fusion versions change only when scoring/candidate semantics actually change.

---

# 58. Likely implementation files

Exact locations are implementation-owned after current-source inspection.

Likely areas:

```text
runtime-manager/src/catalog.rs
runtime-manager/src/resolver.rs
runtime-manager/src/requirements.rs
native-inference/runtime-lock.json

native-inference/roformer/**
native-inference/openvino-worker/**
native-inference/qwen-worker/**

analysis-engine/src/artifact/jbm555.rs
analysis-engine/src/artifact/timed_notes.rs
analysis-engine/src/artifact/symbolic_score.rs
analysis-engine/src/artifact/timed_score.rs

analysis-engine/src/contract/capability.rs
analysis-engine/src/planner/plan.rs
analysis-engine/src/candidate_pipeline.rs
analysis-engine/src/workflow.rs

analysis-engine/src/fusion/baseline.rs
analysis-engine/src/fusion/candidate_states.rs
analysis-engine/src/fusion/hsmm.rs
analysis-engine/src/fusion/canonical.rs

app-core/src/audio_model.rs
app-core/src/workflow/default_definition.rs
```

Do not create all files mechanically if existing modules already provide a better boundary.

Respect the 2000-line source-file limit.

---

# 59. Do not touch unrelated systems without necessity

Do not use this task to rewrite:

```text
library DB
editor architecture
export transaction model
AI adapter protocol
Qwen ASR behavior
Qwen alignment behavior
navigation
general UI theme
lead/backing role semantics
```

except where a narrow typed interface change is required.

---

# 60. Implementation phases

## Phase A — current-source audit

Confirm:

```text
working tree
current default model IDs
current RoFormer converter/runtime shape support
current OpenVINO worker contracts
current Candidate normalization path
current workflow binding limits
```

No destructive changes.

---

## Phase B — Leap XE source + license audit

Record:

```text
source repository/revision
vocal checkpoint identity
config identity
checkpoint license conclusion
reference inference profile
```

Do not download unrelated files.

---

## Phase C — Leap XE native integration

```text
source reference
-> GGUF conversion
-> exact metadata
-> CPU/reference parity if available
-> bounded Vulkan only with authorization
-> worker route
-> Runtime Manager
-> Engine separation
```

Side-by-side with EP317.

---

## Phase D — PolarFormer source/ONNX audit

Verify:

```text
canonical checkpoint lineage
public ONNX identity
config
license
reference run
```

---

## Phase E — PolarFormer OpenVINO integration

```text
ONNX Runtime reference
-> OpenVINO FP32
-> CPU parity
-> bounded GPU validation
-> native audio DSP
-> Runtime Manager
-> Engine instrumental route
```

Side-by-side with Inst V2.

---

## Phase F — separation default switch

Only after quality/runtime acceptance:

```text
DEFAULT_VOCAL_MODEL_ID -> Leap XE
DEFAULT_BGM_MODEL_ID   -> PolarFormer public
```

Update default workflow and truthful UI copy.

Preserve historical models.

---

## Phase G — 124-band upstream gate

Re-check exact public artifacts.

If unavailable:

```text
record blockers
stop that subtask
```

No fabrication.

---

## Phase H — generic timed-note normalization

Add a model-neutral Analysis Engine normalization contract.

Adapt STARS/ROSVOT consumption without changing their raw contracts.

All existing tests must remain green.

---

## Phase I — JBM reference harness

Capture:

```text
CQT
raw logits
decoded notes
```

from canonical upstream on deterministic inputs.

---

## Phase J — native JBM frontend/model

Implement:

```text
native CQT
OpenVINO graph
native decoder
```

Pass parity in that order.

---

## Phase K — JBM Runtime Manager + Engine

Add:

```text
model identity
notes.jbm555 capability
dual-input bindings
raw evidence parser
normalization
Candidate Pool contribution
```

---

## Phase L — Japanese planner + A/B

Add language-aware participation and real Japanese-song evaluation.

Do not globally enable JBM for all languages.

---

## Phase M — VocalParse source/native gate

Audit exact current VocalParse revision and tokenizer.

Investigate native Qwen compatibility.

If blocked:

```text
BLOCKED_NATIVE_RUNTIME
```

No Python Production fallback.

---

## Phase N — T3MS upstream gate

Re-check exact official artifacts.

If unavailable:

```text
BLOCKED_UPSTREAM_ARTIFACT
```

Prepare generic timed-score contract only if useful independently.

---

# 61. Focused test commands

Use repository-prescribed shell:

```bash
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-runtime-manager
bash dev.sh -c cargo test -p uta-studio-core
```

If modified:

```bash
bash dev.sh -c cargo test -p <actual-openvino-worker-package>
bash dev.sh -c cargo test -p <actual-ggml-worker-package>
bash dev.sh -c cargo test -p <actual-qwen-worker-package>
```

Also:

```bash
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
git diff --check
```

Do not run the reserved final whole-workspace/Nix release pass without explicit request.

---

# 62. Regression invariants

All must remain true:

```text
[ ] source media remains read-only
[ ] no silent CPU fallback
[ ] no Python Production fallback
[ ] existing EP317 artifacts remain reproducible
[ ] existing Inst V2 artifacts remain reproducible
[ ] existing RMVPE path remains valid
[ ] existing FCPE path remains valid
[ ] GAME remains usable
[ ] STARS provenance remains truthful
[ ] ROSVOT provenance remains truthful
[ ] Basic Pitch remains onset/contour evidence
[ ] continuous F0 never becomes direct target-note truth
[ ] vibrato does not become arbitrary note alternation
[ ] glissando does not become a chromatic staircase
[ ] repeated same-pitch real attacks remain possible
[ ] legitimate octave leaps remain selectable
[ ] Algorithm and AI consume the same Candidate Pool
[ ] AI cannot invent corrected notes
[ ] missing optional experts degrade explicitly
[ ] missing required experts fail closed
[ ] model directories are never deleted automatically
[ ] unrelated working-tree changes are preserved
```

---

# 63. Acceptance — Leap XE technical integration

Leap XE may be marked technically integrated only when:

```text
[ ] exact checkpoint/config/revision recorded
[ ] checkpoint license conclusion recorded truthfully
[ ] authoritative reference inference works
[ ] CKPT -> native artifact conversion is reproducible
[ ] native graph matches source semantics
[ ] short deterministic parity passes
[ ] real short-song semantic output passes
[ ] cancellation and cleanup pass
[ ] Runtime Manager resolves exact generation
[ ] Engine calls the real route
[ ] output is lossless, exact-duration, stereo 44.1 kHz
[ ] existing EP317 remains selectable
[ ] representative full-song bounded run passes
[ ] A/B quality does not regress product target
```

---

# 64. Acceptance — PolarFormer technical integration

```text
[ ] exact source/ONNX/config identities recorded
[ ] license recorded from authoritative source
[ ] ONNX Runtime reference passes
[ ] OpenVINO conversion passes
[ ] OpenVINO CPU parity passes
[ ] bounded selected-device validation passes
[ ] native audio wrapper preserves timeline/stereo semantics
[ ] instrumental output semantics are verified
[ ] cancellation/cleanup pass
[ ] Runtime Manager resolves exact generation
[ ] Engine real route passes
[ ] current Inst V2 remains selectable
[ ] representative full-song bounded run passes
[ ] A/B instrumental quality is accepted
```

---

# 65. Acceptance — default separation switch

Only switch defaults when:

```text
[ ] Leap XE integration accepted
[ ] PolarFormer integration accepted
[ ] model status/readiness is truthful in Runtime Manager
[ ] default workflow uses IndependentSpecialists
[ ] no fake combined model identity is introduced
[ ] focused Studio/app-core tests pass
[ ] existing saved workflows migrate safely
```

---

# 66. Acceptance — 124-band family

A 124-band model may become executable only when:

```text
[ ] exact public checkpoint exists
[ ] exact public config exists
[ ] exact model identity is linked to the intended benchmark lineage
[ ] exact license identity exists
[ ] authoritative reference inference works
[ ] native/OpenVINO conversion parity passes
```

Otherwise the correct state is:

```text
BLOCKED_UPSTREAM_ARTIFACT
```

---

# 67. Acceptance — JBM555

```text
[ ] exact checkpoint/config/source identities recorded
[ ] license conclusion recorded
[ ] upstream Python inference reproduced
[ ] dual-input semantics preserved
[ ] native CQT parity passes
[ ] ONNX parity passes
[ ] OpenVINO CPU parity passes
[ ] native decoder parity passes
[ ] typed raw evidence crosses worker boundary
[ ] generic timed-note normalization is truthful
[ ] Candidate Pool contains JBM-derived candidates
[ ] JBM pitch remains peer evidence
[ ] Japanese language routing is implemented
[ ] optional absence degrades explicitly
[ ] real Japanese-song A/B evidence exists
```

---

# 68. Acceptance — VocalParse

VocalParse may become a real model resource only when:

```text
[ ] exact current checkpoint/tokenizer revision pinned
[ ] Apache-2.0 identity recorded
[ ] native runtime executes the exact extended vocabulary/model
[ ] output parser preserves lyrics/pitch/note value/BPM
[ ] no physical onset/offset is fabricated
[ ] symbolic-score evidence enters Candidate/rhythm logic truthfully
[ ] Mandarin route passes representative real-song evaluation
```

If native runtime is not available:

```text
BLOCKED_NATIVE_RUNTIME
```

is correct.

---

# 69. Acceptance — T3MS

T3MS may become executable only when:

```text
[ ] official/authoritative implementation exists
[ ] exact checkpoint exists
[ ] exact config exists
[ ] artifact provenance is known
[ ] license identity is known
[ ] authoritative reference inference runs
[ ] overlap/window stitching semantics are reproduced
[ ] physical timing/pitch and note-value evidence are preserved
```

If not:

```text
BLOCKED_UPSTREAM_ARTIFACT
```

Do not create fake readiness.

---

# 70. Final product target

Preferred practical near-term production candidate after this task:

```text
Original Mix
    |
    +-> BS-RoFormer Leap XE 90 ----------> Vocal
    |
    +-> BS PolarFormer public -----------> Instrumental

Analysis-ready Vocal
    |
    +-> RMVPE
    +-> FCPE
    +-> GAME
    +-> Basic Pitch
    +-> Acoustic DSP
    +-> STARS / ROSVOT where language/policy applies
    +-> JBM555 for Japanese
    +-> VocalParse for Mandarin when native-ready

Original Mix
    +-> T3MS only when official runnable artifacts exist

All evidence
    |
    v
bounded Candidate Pool
    |
    v
melody-aware Algorithm / AI selection
    |
    v
CanonicalSingingTrack
```

Future target, only when exact artifacts become public and qualify:

```text
124-band BS-RoFormer / PolarFormer family
```

---

# 71. Priority order

Execute in this order:

```text
P0  Leap XE 90 source/license/reference audit
P0  Leap XE native conversion + side-by-side qualification

P0  PolarFormer public source/license/reference audit
P0  PolarFormer OpenVINO + side-by-side qualification

P1  switch default separation specialists after acceptance

P1  generic timed-note normalization
P1  JBM555 reference + native CQT + OpenVINO
P1  JBM555 Candidate/Planner integration
P1  Japanese real-song A/B

P2  VocalParse native-runtime investigation/integration

P2  re-check BS-RoFormer 124 + PolarFormer 124 public artifacts
P2  integrate only if exact artifacts exist

P2  re-check T3MS official artifacts
P2  integrate only if exact artifacts exist

P3  tune multi-expert scoring from multi-song evidence
P3  add rhythm evidence from VocalParse/T3MS where truthful
```

Do not start by tuning HSMM constants against unverified model output.

---

# 72. Required durable updates after completion

Update only current-state durable documents, following repository rules:

```text
tasks/remaining-models/STATE.md
docs/KEY_CONCLUSIONS.md
```

when a resource's current effective conclusion changes.

Do not create verbose historical execution logs under `docs/`.

Task-card status should truthfully reflect partial completion if some upstream resources remain blocked.

For example, valid final outcome:

```text
Leap XE:             READY
PolarFormer public:  READY
JBM555:              READY / BLOCKED_NATIVE depending exact native readiness
VocalParse:          BLOCKED_NATIVE_RUNTIME
BS-RoFormer 124:     BLOCKED_UPSTREAM_ARTIFACT
PolarFormer 124:     BLOCKED_UPSTREAM_ARTIFACT
T3MS:                BLOCKED_UPSTREAM_ARTIFACT
```

One blocked future model must not erase completed practical upgrades.

## 72.1 Current effective result — 2026-08-30 (supersedes the earlier blocked result)

- `bs_roformer_leap_xe90_vocals` is the default vocal specialist. Runtime Manager manages the original public 267,433,600-byte F32 GGUF conversion `bs_leap_xe_voc-F32.gguf` at revision `440487b8300dcd61453cc52ec244a38150b03456`; it is not requantized. The GGML graph now reads the public `bs-roformer` schema, 90 contiguous band widths, public tensor names and fused-QKV layout. A real 6.000 s strong-vocal passage completed on Intel Arc B580 in 26.65 s of compute and produced finite, non-silent 44.1 kHz stereo F32 audio. The command included `--batch-size 1 --vulkan-no-async --serial-pipeline`. The original EP317 resource, installer and OpenVINO executor were removed.
- `bs_polarformer_public_instrumental` is the default Instrumental specialist. Runtime Manager downloads the public MIT ONNX revision `9158719ee2173edd480a735764627526506fe4af`; the native worker implements the published 44.1 kHz stereo STFT/mask/residual/overlap path. A real FP32 ONNX CPU smoke produced lossless stereo FLAC through the machine protocol.
- The default remains `IndependentSpecialists`: Leap and PolarFormer run independently with their own provenance and progress.
- Workflow schema 3 migrates saved schema-1/2 EP317 selections to Leap, retains both independent separator invocations and restores a missing optional JBM555 Maximum card. Processing Studio's Stage 3 DAG and Models & runtime settings expose Leap, PolarFormer and JBM555 consistently.
- Analysis Engine now owns `uta.analysis-engine.timed-note-evidence` v1, a model-independent physical timed-note contract with separate source-local boundary/pitch scores and optional explicitly calibrated confidence. STARS/ROSVOT raw contracts normalize through it before bounded Candidate construction; their original correlated dependencies remain intact. Continuous F0, vibrato, and glissando semantics are unchanged.
- `jbm555_cectc_80` is an executable OpenVINO LocalImport resource from `york135/CECTC_baseline_APSIPA25` revision `d1352eda1ea69d94cf7b1b06bf0b003d874b389a`. The worker consumes exact mix plus prepared vocal, runs a native three-scale log-frequency frontend and the published CE-CTC graph, applies the upstream 0.32/0.70 decoder, and emits normalized timed-note evidence. Japanese Maximum plans schedule it and Candidate fusion preserves its dependency provenance. The 3,990,463-byte ONNX completed the current short machine-protocol smoke on explicit CPU; broader Japanese-song quality remains unqualified rather than blocking integration.
- Bitness-preserving GGUF diagnostics were produced for the other two new models. PolarFormer is 206,153,536 bytes with 728 F32 and 78 I64 tensors; its PoPE/Sin/Cos/Einsum graph is not implemented by the current GGML RoFormer backend, so it remains truthfully non-executable there. JBM555 is 3,981,280 bytes with 32 F32 tensors; a temporary GGML CPU probe executed its complete 10-convolution/6-linear/Softmax neural graph for seven frames in 37.463 ms and returned finite normalized output. Product routing remains OpenVINO until the corresponding GGML frontend/protocol integration exists.
- `vocalparse_1_7b`: Hugging Face revision `4c617b1a88c8e663351d9072c549d81d7f78a36f` and source revision `e7b3946c940a9216a5314f9ba11a19fd70a6befb` are Apache-2.0 and publish the checkpoint/tokenizer, but current native Qwen execution has no proved VocalParse extended-vocabulary path. Status is `BLOCKED_NATIVE_RUNTIME`; no `score.vocalparse` capability is advertised.
- `t3ms`: official source head `467dcd1b065c0dcb2b1b7d21712431bd1af7e4db` publishes neither an executable checkpoint nor a license. Status is `BLOCKED_UPSTREAM_ARTIFACT`; no `score.t3ms` capability is advertised.
- Public Hugging Face model searches for exact 124-band BS-RoFormer/PolarFormer artifacts returned no runnable model. Both future IDs remain `BLOCKED_UPSTREAM_ARTIFACT` and are not Catalog resources.
- Source-size compliance was restored by moving planner tests and native-worker helpers behind existing module boundaries. Focused Rust, native-runtime, CPU and explicitly authorized GGML/Vulkan checks pass. The explicitly requested `nix build .` also passes; wrapped-application smoke and the remaining formal release acceptance were not run.

---

# 73. Final execution report

At handoff, report concisely:

```text
1. files changed
2. exact model/source/config identities
3. license conclusions
4. conversion/runtime routes
5. reference/parity results
6. Runtime Manager states
7. separation A/B results
8. JBM Japanese note A/B results
9. Candidate/Fusion effects
10. VocalParse status
11. 124-band status
12. T3MS status
13. remaining blockers
14. focused test results
```

Do not claim SOTA integration for a benchmark-only model without exact runnable artifacts.

Do not claim Production when only reference/CPU/integration evidence exists.

---

# 74. Definition of Done

This task is complete when the practical upgrades and all upstream gates are represented truthfully:

```text
[x] Leap XE 90 has a real exact integration result or a concrete evidence-backed blocker
[x] public PolarFormer has a real exact integration result or a concrete evidence-backed blocker
[x] current defaults use the newly integrated independent specialists
[x] BS-RoFormer 124-band status is truthfully resolved
[x] PolarFormer 124-band status is truthfully resolved
[x] JBM555 has a real native evidence path or a concrete blocker
[x] generic timed-note normalization does not fabricate STARS/ROSVOT dependencies
[x] Japanese Maximum plans advertise and schedule the native JBM route
[x] VocalParse has a truthful native-runtime status
[x] T3MS has a truthful upstream-artifact status
[x] continuous F0 remains separate from discrete score truth
[x] vibrato/glissando remain performance evidence
[x] Candidate provenance is preserved
[x] Algorithm and AI share one deterministic Candidate Pool
[x] no hidden MIDI smoothing was added
[x] no Python Production fallback was added
[x] no checkpoint/license/runtime was invented
[x] existing user working-tree changes were preserved
[x] focused tests pass for every modified component
```

**Fail closed whenever exact provenance, semantics, licensing, or runtime parity cannot be established. Partial truthful completion is preferred over fabricated full completion.**
