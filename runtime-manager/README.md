# Uta Runtime Manager

`uta-runtime-manager` owns Uta! Studio's audited runtime/model catalog, managed
store, integrity verification, policy-aware resolution, and generation leases.
`uta-runtime` is its local process frontend.

## Process protocol

Studio-facing integrations launch `uta-runtime` directly with argv values; they
do not invoke a shell and do not link this crate. Use `--output ndjson` for the
process boundary or `--output json` for one final document.

Every NDJSON stdout line is a versioned machine frame:

- final result: `schema=uta.runtime.result`, `schema_version=1`, `type=result`;
- error: `schema=uta.runtime.error`, `schema_version=1`, `type=error`;
- mutation event: `schema=uta.runtime.event`, `schema_version=1`, with a typed
  event such as `operation_started` or `resource_started`.

Human/debug output belongs on stderr. Callers must preserve domain error codes
and must not parse stderr to infer lifecycle state.

Supported commands include `list`, `show`, `status`, `paths`, `plan`, `setup`,
`install`, `import`, `verify`, `repair`, `reinstall`, `remove`, `doctor`,
`smoke`, `resolve`, and the read-only `fusion-providers` discovery query.
`configure-fusion-provider --provider <pi|codex|claude>` and
`clear-fusion-provider` persist only the provider identity; non-interactive
mutations require `--yes`. Provider rows report PATH and native-adapter
availability, never authentication readiness. A selected provider may contact
an external AI service and incur provider charges; credentials remain owned by
the provider CLI.

## RMVPE audited import

RMVPE conversion is an explicit two-step install action. First run
`native-inference/openvino-worker/convert-rmvpe-to-ir.sh` with the pinned source
ONNX and source-built OpenVINO 2026.3 converter. The script verifies source SHA
`5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd`
and conversion recipe
`ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876`,
then atomically creates the bucketed IR directory without replacing source or
existing output.

Import that completed directory through the machine boundary:

```sh
uta-runtime import model:rmvpe \
  --from /path/to/openvino-ir-2026.3.0-bucketed \
  --yes --policy production --output ndjson --store /authorized/runtime/store
```

The catalog and generated receipt keep three identities separate: the
Dream-High/RMVPE algorithm lineage, the exact lj1995 `rmvpe.onnx` source
artifact, and the converted bucketed OpenVINO IR manifest/runtime recipe.
Runtime Manager verifies the pinned IR manifest and every XML/weights digest,
stages all files into an immutable generation, verifies the generated install
manifest, and only then atomically publishes `current.json`.

## Safety invariants

- Read commands are offline and do not create or modify the store.
- Network acquisition is limited to explicitly confirmed setup/install/repair/
  reinstall operations.
- Downloaded and imported payloads must match catalog-pinned SHA-256 identities.
- A generation is published and verified before `current.json` changes.
- Existing and leased generations are not overwritten.
- Legacy user directories are detected but never silently adopted or deleted.
- Production resolution fails closed unless every required backend and worker
  capability is validated.
