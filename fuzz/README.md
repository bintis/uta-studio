# Parser fuzzing

Install `cargo-fuzz` explicitly, then run a bounded target locally:

```sh
cargo fuzz run utz_package -- -max_total_time=60
cargo fuzz run vocal_chart_json -- -max_total_time=60
cargo fuzz run ultrastar_text -- -max_total_time=60
```

The harnesses never read the library, settings, model cache, or source media.
Corpus and crash output under `fuzz/` are local developer artifacts and should
not contain user files.
