# Uta Analysis Engine — Audio Analysis Coverage Checklist v1.0

**目的**：确保音频分析设计交接没有遗漏。
**权威主文档**：`UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md`
**分离专文**：`UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md`

本清单不是另一个算法规范；它用于 agent/reviewer 做覆盖性审计。

---

# A. Contract / Timeline

- [x] `AnalyzeRequestV1`
- [x] caller identity vs Engine-decoded facts
- [x] exactly one primary source
- [x] local-file-only v1
- [x] mandatory SHA-256
- [x] explicit audio roles
- [x] canonical integer timebase = 1,000,000 units/s
- [x] `source_start`
- [x] v1 1:1 elapsed-time transform
- [x] no hidden time stretch/warp
- [x] `DecodedAudioFacts`
- [x] requested artifacts
- [x] analyze/export separation
- [x] production/benchmark/experimental runtime policy

主文档：§11–23，§69–74。

---

# B. Audio semantic routing

- [x] `original_mix`
- [x] `vocal_stem`
- [x] `guide_vocals`
- [x] `lead_vocal`
- [x] `clean_lead_vocal`
- [x] `instrumental`
- [x] backing/harmony secondary semantics
- [x] no filename-based role inference
- [x] no unnecessary separation for already-prepared inputs

主文档：§13–15，§24–29。
分离专文：§5–7。

---

# C. Separation / restoration

- [x] independent production instrumental path
- [x] vocal extraction
- [x] lead/support isolation
- [x] denoise
- [x] dereverb
- [x] raw `lead_vocal` vs `clean_lead_vocal`
- [x] `vocal_residual`
- [x] residual is not automatically backing/harmony
- [x] current RoFormer recipe mapping
- [x] one generic native RoFormer runtime direction
- [x] timeline preservation
- [x] finite/clipping/silence/energy gates
- [x] lead purity
- [x] vocal leakage
- [x] musical damage
- [x] cleanup consistency
- [x] cleanup-damage fallback

主文档：§24–32。
分离专文：§3–18。

---

# D. Vocal topology / duet

- [x] `single_lead`
- [x] `alternating_multi_lead`
- [x] `overlapping_multi_lead`
- [x] `lead_with_support`
- [x] `unknown`
- [x] overlap regions
- [x] support regions
- [x] alternating duet support
- [x] simultaneous duet uncertainty
- [x] no fake Singer A/B assignment
- [x] VocalChart track/part/role separation
- [x] `audio.lead_isolate`
- [x] `audio.lead_partition`
- [x] `lead_partition` non-blocking/future

主文档：§33–37。
分离专文：§8–12。

---

# E. Speech / lyrics / alignment

- [x] lyric mode `none`
- [x] lyric mode `reference`
- [x] lyric mode `canonical`
- [x] canonical text cannot be silently replaced
- [x] reference text reconciliation
- [x] Qwen3-ASR-1.7B baseline
- [x] Qwen3 ForcedAligner 0.6B baseline
- [x] FireRed optional challenger
- [x] transcript remains independent artifact
- [x] alignment remains independent artifact
- [x] phrase/word/syllable/phoneme constraints
- [x] soft/hard authority
- [x] hard boundary = structural barrier, not forced note
- [x] melisma survives lyric alignment

主文档：§16–19，§38–41。

---

# F. Continuous pitch

- [x] RMVPE as primary continuous F0
- [x] `f0_hz`
- [x] voicing
- [x] confidence
- [x] 10ms-ish canonical evidence timeline
- [x] continuous MIDI formula
- [x] no frame-wise direct MIDI note finalization
- [x] PitchEvidence 0.3
- [x] FCPE secondary F0
- [x] octave disagreement
- [x] dirty-separation disagreement

主文档：§42–43，§47，§49。

---

# G. Note/boundary experts

- [x] GAME primary note/boundary expert
- [x] GAME official ONNX → OpenVINO route
- [x] known timing/boundary conditioning
- [x] Basic Pitch independent onset/note cross-check
- [x] ROSVOT future secondary expert
- [x] STARS CKPT → ONNX subgraphs → OpenVINO candidate
- [x] STARS dynamic Python postprocess kept native-side
- [x] no fake placeholder evidence
- [x] disagreement-window selective execution

主文档：§44–48.1，§65–66。

---

# H. Technique / expressive evidence

- [x] DSP baseline
- [x] vibrato
- [x] glissando
- [x] portamento
- [x] ornament/melisma
- [x] breath/voicing transitions
- [x] STARS optional technique expert
- [x] no mandatory TechniqueStudent roadmap
- [x] vibrato does not fragment into semitone chatter
- [x] glissando traversed semitones are not automatic notes
- [x] melisma supports one token to many notes

主文档：§48–55。

---

# I. Fusion

- [x] canonical evidence timeline
- [x] versioned confidence calibration
- [x] raw scores not directly comparable
- [x] correlation discounting
- [x] dependency provenance
- [x] context-aware weights
- [x] vibrato context
- [x] glissando context
- [x] fast-melisma context
- [x] dirty-separation context
- [x] candidate boundary generation
- [x] segment pitch candidates
- [x] octave alternatives
- [x] candidate graph
- [x] global segment decoding
- [x] HSMM/Viterbi
- [x] observation/transition/duration/constraint terms
- [x] rhythm quantization after semantic note inference

主文档：§49–65。

---

# J. Profiles / escalation

- [x] Fast
- [x] Balanced
- [x] Maximum
- [x] quality/cost policy rather than frozen model list
- [x] baseline required experts
- [x] disagreement triggers
- [x] selective secondary experts
- [x] Maximum != run everything across full song

主文档：§65–66。

---

# K. Output artifacts

- [x] Candidate VocalChart 0.3
- [x] PitchEvidence 0.3
- [x] `singing-analysis/0.3`
- [x] Transcript
- [x] Alignment
- [x] requested stems
- [x] diagnostics
- [x] provenance
- [x] fingerprint
- [x] degraded reasons
- [x] stable ID references from evidence to chart
- [x] no duplicate authoritative note geometry
- [x] Candidate vs Authored separation
- [x] neutral scoring defaults

主文档：§67–74，§92–94。

---

# L. Result / execution behavior

- [x] `ok`
- [x] `ok_degraded`
- [x] `failed`
- [x] `cancelled`
- [x] required expert failure fails closed
- [x] optional expert failure can degrade
- [x] worker NDJSON
- [x] stderr logs
- [x] no HTTP
- [x] no hidden model downloads
- [x] Engine writes authorized run-temp only
- [x] Studio validates/commits artifacts
- [x] cancellation
- [x] output confinement

主文档：§69–77，§95–96。

---

# M. Fingerprint / reproducibility

- [x] input SHA
- [x] request contract/version
- [x] role/timeline
- [x] lyric/constraint hash
- [x] model IDs
- [x] immutable model generations/content hashes
- [x] runtime generations/recipe digests
- [x] backend/device
- [x] calibration/fusion/HSMM/postprocess versions
- [x] separation/cleanup recipes
- [x] no self-modifying weights

主文档：§74–84。

---

# N. Model lifecycle

- [x] immutable external models
- [x] no self-training
- [x] no pseudo-label auto-promotion
- [x] new upstream model benchmark
- [x] Gold regression
- [x] conversion/parity
- [x] runtime validation
- [x] explicit catalog release
- [x] explicit install/reinstall
- [x] pitch-shift as regression test
- [x] time-stretch as regression test
- [x] gain/EQ/noise/codec robustness as regression tests

主文档：§78–84。

---

# O. Standalone exports / UTZ

- [x] Analyze vs Export split
- [x] standalone USTX
- [x] standalone MIDI
- [x] UTZ relationship
- [x] UTZ requires instrumental
- [x] no fake silence instrumental
- [x] representations are lossy/derived
- [x] faithful/editable USTX concepts

主文档：§85–88。

---

# P. Validation / production gates

- [x] separation metrics
- [x] ASR/alignment metrics
- [x] pitch metrics
- [x] note metrics
- [x] confidence calibration
- [x] duet acceptance
- [x] full-song
- [x] repeat
- [x] cancellation
- [x] device contention
- [x] Intel Arc/Xe validation
- [x] Gold Set
- [x] Windows/Linux runtime checks
- [x] no short-smoke-only promotion

主文档：§99–107。

---

# Q. Studio reintegration

- [x] Runtime Manager first
- [x] standalone Engine real-audio closure second
- [x] Engine contract freeze third
- [x] Studio reintegration only after standalone gate
- [x] Studio product DAG -> Engine request adapter
- [x] Engine does not understand Studio DB
- [x] model UI is Runtime Manager frontend
- [x] existing product API names preserved where possible

主文档：§89–98。
Agent Guide：§121–127，§139。

---

# 结论

只要本清单每项仍由权威主文档/分离专文覆盖，就可以认为当前音频分析设计没有因为组件拆分而丢失关键语义。

若后续新增模型或算法：

1. 先判断它替换/增强哪个稳定 capability；
2. 不先修改 Studio product API；
3. 不直接修改 UTZ core；
4. 通过 Runtime Manager resource recipe 管理模型；
5. 通过 Analysis Engine evidence/fusion 接入；
6. 增加 Gold regression 和 parity gate。
