# Qwen Vulkan worker validation

Date: 2026-08-22

## Locked identities

- GGML/Vulkan revision: `8c63e70982c95ceb862e3a1073a2c1beef75d60a`
- ASR engine revision: `ea077b87590bcfb090d7c38c03ab36cd1c7005d3`
- ASR runtime recipe: `53083b7b39dd2a805f441453ae07c797`
- ASR canonical source revision: `7278e1e70fe206f11671096ffdd38061171dd6e5`
- ASR converted repository revision: `92282af1610a2db19d66f2bef1e260f5deca782d`
- ASR model SHA-256: `b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e`
- Forced-aligner engine revision: `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c`
- Forced-aligner recipe: `3ec367aaf3f723079851e2fbdbd375f8`
- Forced-aligner canonical source revision: `c07281df297b9905d24a508279258cccf987a064`
- Forced-aligner source `model.safetensors` SHA-256: `00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86`
- Forced-aligner model SHA-256: `c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b`
- Forced-aligner converter adaptation SHA-256: `ffd8a575238c81823509e2a7bf645bf9bb5d38db2903bc3306648afd619b42d6`
- Runtime manifest SHA-256: `1ac5c3d6a36689ddecfda110034b1cb9021467e04539940fd5bc75c0ef8fe4ec`
- Backend: Vulkan device 0, Intel Arc B580, Mesa open-source Intel driver

The source repositories, revisions, model filenames, integration patch, and
model acquisition identities are pinned by `../runtime-lock.json`.

## ASR language contract closure

Static CPU-only contract tests close language contract version 1 without
executing Vulkan. Explicit `config.language` hints are rejected before engine
launch because the pinned ASR runtime supports automatic detection rather than
that hint contract. Transcript evidence schema 2 records only runtime-detected
language, with `explicit_hint_policy=reject` and
`evidence_source=runtime_detected`; missing or conflicting detected-language
metadata fails closed. Worker stdout remains protocol-1 NDJSON.

Analysis Engine now omits the rejected ASR language hint, strictly consumes the
worker's schema-2 language contract, and preserves the runtime-detected language
through transcript fusion without fabricating confidence. The worker recognizes
the pinned runtime's observed `detected-language:` spelling, rejects conflicting
signals within or across log streams, and no longer enables quiet mode that
suppresses required language evidence. A protocol-1 Engine fixture exercises the
schema-2 worker boundary and rejects any Engine-generated ASR config containing a
`language` key.

The exact manifest-pinned ASR Vulkan route is policy-admitted as
`ProductionPinned`. These repairs were verified by CPU-only contract/integration
tests and do not add broader Vulkan quality/stability evidence; that limitation
remains an explicit advisory caveat rather than an alternate route or fallback.

## Forced Aligner static contract closure

Static CPU-only tests pin alignment evidence schema 2 without executing Vulkan.
Text profile `qwen-align-text-preserve-v1` canonicalizes line endings and outer
whitespace while preserving inner Unicode and punctuation. Language profile
`qwen-align-language-v1` accepts only the optional supported codes
`zh,en,yue,fr,de,it,ja,ko,pt,ru,es`, maps them to the pinned runtime tokenizer
names, and rejects unsupported values before engine launch. Alignment semantics
profile `qwen-align-token-word-80ms-v1` identifies the runtime's 80 ms timestamp
classes and Uta! Studio's no-invented-timestamps zero-piece merge behavior.

Analysis Engine now strictly consumes the schema-2 alignment evidence, validates
the three profiles, canonical/runtime language pairing, exact compact transcript
preservation, 80 ms measured timing grid, ordering, positive duration, and source
bounds. The protocol-1 Engine fixture uses the same schema-2 shape as the worker.

The known representative long-form quality blocker is not cleared by this static
schema repair: retained evidence still records 109/357 measured units and a last
measured boundary at 149.36 seconds for the 305.813375-second fixture. Static
inspection of the pinned runtime rules out two simplistic explanations: its
5,000 timestamp classes cover 400 seconds at 80 ms/class, and its audio encoder
already chunks long mel input. The runtime's separate 30-second transcribe-align
mode aligns ASR-generated text per chunk, so it cannot be reused for canonical
complete lyrics without changing text authority. A measured canonical-text
segmentation strategy still requires a separately authorized runtime-quality
repair; proportional timestamps or line-to-audio guesses were not introduced.
No Vulkan rerun was authorized or performed, and no timestamp interpolation was
added.

The exact current-flat-HF/classifier converter adaptation is vendored and hashed.
Runtime Manager independently records the canonical safetensors, converted GGUF,
converter patch, model recipe, and runtime recipe in LocalImport receipts.

The historical runtime manifest/recipe remains an exact native-engine identity.
No new runtime quality or safety evidence is inferred from static closure. The
exact manifest-pinned aligner Vulkan route is nevertheless owner-policy-admitted
as `ProductionPinned`; the retained long-form quality evidence remains an
explicit advisory caveat, not an automatic fallback or second backend.

## Historical real worker smoke

Both Rust worker executables were launched through their NDJSON stdio boundary
against a read-only local model installation and a 12-second, 16 kHz mono PCM
view of representative local singing audio. The compatibility audio was created
in a unique temporary directory; source media and installed models were not
modified.

ASR emitted a protocol-1 ready frame with the exact ASR recipe, progress frames,
a typed `transcript_evidence` output, and `done/ok`. The evidence identified the
pinned model, `backend=vulkan`, the runtime-manifest digest above, selected
Japanese, and contained non-empty decoded text. Engine timings reported model
load, mel, encoder, and decoder work on Vulkan device 0.

Forced alignment emitted the independent aligner recipe, progress frames, typed
`alignment_evidence`, and `done/ok`. The worker output identified the pinned
model and Vulkan runtime and preserved the supplied Japanese transcript. The
native aligner can return zero-duration Unicode pieces at measured segment
boundaries; the worker now joins those pieces to an adjacent measured segment
without inventing or interpolating timestamps. Its focused contract tests prove
that all text is retained and every emitted range is positive, finite,
non-overlapping, and acceptable to Analysis Engine's canonical timeline parser.

No network request, Python process, script runtime, CPU inference fallback, or
user-data mutation participated in either smoke.

This forced-aligner evidence remains bounded historical implementation evidence;
it does not establish broad quality/stability coverage. Qwen3 Forced Aligner and
Qwen3-ASR-1.7B are owner-policy-admitted as `ProductionPinned` on their exact
manifest-pinned Vulkan routes. Further Vulkan quality/stability work remains
advisory, and the separately reserved release/package acceptance still applies.
