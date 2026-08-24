# 17 — Lead Isolation / Future Lead Partition Contract

**State:** `READY`
**Precondition:** Phase A model cards 01–13 are terminal and card 05 reports `integration_ready=yes`.
**Task class:** semantic audio contract correction; no model execution
**Owner:** Analysis Engine separation semantics + Studio local Workflow/wire mapping

## Read

```text
AGENTS.md
docs/design/README.md
docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
docs/KEY_CONCLUSIONS.md
tasks/remaining-models/STATE.md
```

## Contract corrected by this card

The earlier card framing incorrectly treated `audio.lead_partition` as a
BackingVocal-versus-HarmonyVocal classifier. The authoritative separated
architecture defines two different capabilities:

```text
audio.lead_isolate
  complete/support-containing vocals -> foreground lead + vocal residual

audio.lead_partition
  multiple simultaneous foreground singers -> separate analysis streams
```

`audio.lead_partition` is optional/future work and is not a final-v1 baseline
prerequisite. It must not be claimed by a foreground/support separator.

Editor roles are a separate domain:

```text
Lead / Harmony / Backing / Adlib = chart-track authoring roles
```

Their existence does not imply that equivalent independently separated audio
stems exist.

## Accepted model semantics

The exact accepted Karaoke checkpoint remains the implementation recipe for
`audio.lead_isolate`. Its truthful product contract is:

```text
all vocals -> LeadVocal + VocalResidual
```

The model is one-target. The residual is deterministic subtraction and may
contain backing vocals, harmony, doubles, ad-libs, choir content and separation
error. It is never promoted to `BackingVocal` or `HarmonyVocal` by filename,
subtraction, UI copy or protocol naming.

Independent BackingVocal/HarmonyVocal stem requests remain typed
`MissingCapability(audio.lead_partition)` in v1. A future implementation must
select a source-verified multiple-foreground separator and an appropriate
identity/role contract; it must not reuse this residual as two aliases.

## Engine and Worker outcome

- `audio.lead_isolate` publishes LeadVocal and consumes the second Worker output
  only as typed `vocal_residual` validation/working data.
- GGML and OpenVINO routes use
  `semantic_output=lead_vocal+backing_vocal_residual` and publish the second
  output as `vocal_residual`.
- `audio.lead_partition implementation_exists=false` and retains its designed
  `partitioned_lead_vocals` output semantic.
- Backing/Harmony requested stems fail before model resolution or execution.
- No residual bytes are atomically published under a Backing/Harmony filename.

## Studio / Workflow outcome

- Processing Studio advertises only executable final-v1 transformations;
  `audio.lead_partition` is not offered and is not inserted into migrated/default
  Workflows.
- `audio.lead_isolate` exposes distinct `lead` and `residual` ports.
- Workflow schema 2 retains distinct local BackingVocal, HarmonyVocal and
  VocalResidual role identities for persistence/future compatibility. Legacy
  schema-1 `back_vocal` migrates to BackingVocal and never aliases Harmony.
- Editor Lead/Harmony/Backing/Adlib chart tracks remain unchanged.
- Studio continues through local DTOs and `AnalysisCliClient`; no backend crate
  dependency was added.

## Source and design evidence

- UVR's public vocal-split code defines Karaoke as lead removal and application
  output labels, but does not make the complement a pure harmony classifier.
- The exact checkpoint has one neural target; subtraction proves reconstruction,
  not Backing/Harmony taxonomy.
- MedleyVox exposes unison/duet/main-vs-rest/N-singing separation, not independent
  product BackingVocal/HarmonyVocal labels.
- The authoritative Uta separation design explicitly marks arbitrary perfect
  backing/harmony decomposition and simultaneous singer partition as future
  work.

External identities and immutable links remain in
`docs/research/non-game-model-readiness/SOURCE_LEDGER.md` (R10, O11, O12).
No model bytes, package, GPU or inference context were created for this card.

## Acceptance

Focused CPU/local tests cover:

```text
BackingVocal and HarmonyVocal requests fail closed
capability registry reports lead_partition unimplemented
Processing Studio does not advertise/insert lead_partition
schema-1 BackVocal migration does not alias Harmony
GGML rejects backing promotion and accepts lead+residual semantics
lead-isolation Worker output remains exact lead_vocal + vocal_residual
```

Card 17 is `READY` because final-v1 now matches the authoritative contract rather
than claiming an unsupported capability. This does not claim that future
multiple-foreground partitioning has been implemented.
