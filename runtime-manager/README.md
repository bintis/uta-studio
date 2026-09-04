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

## RMVPE GGML/Vulkan import

RMVPE conversion is an explicit local action. Run
`native-inference/rmvpe/tools/convert_rmvpe_to_gguf.py` with the cataloged
`rmvpe.onnx` source to produce `rmvpe-f32.gguf`; the converter maps the native
ONNX Conv, BatchNorm, bidirectional-GRU, and output-head tensors into the
repository's `rmvpe` GGUF architecture without modifying the source file.

Import that GGUF through the machine boundary:

```sh
uta-runtime import model:rmvpe \
  --from /path/to/rmvpe-f32.gguf \
  --yes --policy benchmark --output ndjson --store /authorized/runtime/store
```

The catalog and generated receipt keep the Dream-High/RMVPE algorithm lineage,
the exact lj1995 `rmvpe.onnx` source identity, the GGUF conversion recipe, and
the GGML/Vulkan runtime recipe separate. Runtime Manager stages the file into
an immutable generation, verifies the generated install manifest, and only
then atomically publishes `current.json`. RMVPE remains a benchmark candidate
until its native Vulkan output and stability evidence are accepted.

## Safety invariants

- Read commands are offline and do not create or modify the store.
- Network acquisition is limited to explicitly confirmed setup/install/repair/
  reinstall operations.
- Cataloged digests are retained as provenance; acceptance also uses structural,
  manifest, type, and execution checks rather than a hash-only gate.
- A generation is published and verified before `current.json` changes.
- Existing and leased generations are not overwritten.
- Legacy user directories are detected but never silently adopted or deleted.
- Production resolution fails closed unless every required backend and worker
  capability is validated.
