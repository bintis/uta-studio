# 21C — Uta! Studio Identity and Audit-Link Closure

**State:** `READY`
**Parent:** card 21 final-v1 design parity audit
**Task class:** static identity/i18n/docs/package-metadata closure; no model inference

## Gap

Repository rules require the product identity **Uta! Studio**, but audit revision 1 found the punctuation-free display variant across 84 non-generated files while the canonical form appeared in only one. Product copy, i18n source keys, package descriptions and current documentation therefore did not use the canonical display name consistently.

Card 21 also named two absent historical agent-task audit inputs.

The active repository policy is available through `AGENTS.md` and `docs/engineering-constraints.md`; deleted historical logs must not be recreated. The audit card/current design links should name only current authoritative inputs, while the separately reserved final repository acceptance remains deferred.

## Scope

1. Replace punctuation-free product-display variants with `Uta! Studio` / `UTA! STUDIO` in current code comments/copy, i18n catalogues, user guides, package metadata and current task/design/research documents.
2. Preserve stable machine identities required by current contracts: crate/binary/path slugs such as `uta-studio`, protocol/resource IDs, repository URLs and canonical environment variables such as `UTA_STUDIO_*` do not gain punctuation.
3. Keep EN/zh-CN/ja key sets and placeholders identical while migrating source-language keys/copy.
4. Add a cheap case-insensitive static test/gate that rejects plain display-name variants outside Git metadata and generated/dependency/build trees, with explicit allowlisting only for stable machine identities.
5. Repair active card/design cross-references to absent agent-task files by pointing at current authoritative policy, without recreating deleted historical logs or starting the deferred release pass.
6. Verify desktop/package metadata and About/close/install/export copy use the canonical display identity.

## Focused acceptance

- The display-name scan is empty for disallowed plain variants.
- EN/zh-CN/ja parity and placeholder tests pass.
- Package/crate metadata and current docs use `Uta! Studio` while machine IDs remain stable.
- No model inference, download, accelerator command, whole-workspace suite or Nix build.
- Rerun card 21 static identity/i18n/docs/packaging audit.

## Result

**Result:** `READY`

Current product copy, comments, EN/zh-CN/ja catalog keys and values, user guides, active design/research/task documents, desktop metadata, package descriptions, release metadata, About/close/install/export copy and generated documentation now use **Uta! Studio** / **UTA! STUDIO** consistently. Stable machine identities—including `uta-studio`, protocol/resource IDs, repository URLs, filesystem paths and `UTA_STUDIO_*` variables—were left unchanged.

`tools/check-product-identity.sh` rejects the punctuation-free display variant case-insensitively while excluding only Git metadata, generated/dependency/build trees and binary ZIP input. It is invoked by `build.sh` and the release workflow metadata job. The gate passes with no disallowed match. EN/zh-CN/ja key and placeholder parity tests pass (5 focused Desktop i18n tests), and `cargo xtask docs check` confirms `docs/USER_GUIDE.md` plus `desktop/assets/docs/docs.bundle.json` were regenerated from current localized sources.

Active card/design references now point to `AGENTS.md`, `docs/engineering-constraints.md`, `tasks/remaining-models/STATE.md`, `docs/KEY_CONCLUSIONS.md` and the current parity/process documents. No deleted historical agent-task log or checklist was recreated; the later explicit release pass remains reserved by `AGENTS.md`. Package metadata and the Linux desktop entry use the canonical display identity. `cargo fmt --all -- --check` passes. No model inference, download, accelerator command, whole-workspace suite or Nix build was used.
