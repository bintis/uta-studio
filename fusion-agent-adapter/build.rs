use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=manifests");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    // Cargo places build-script output at target/<profile>/build/<package>/out.
    // The sibling adapter manifests are intentionally emitted beside the
    // binaries so Runtime Manager can discover a development build exactly as
    // it discovers the packaged release.
    let profile_dir = out_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("Cargo OUT_DIR has the standard profile/build/package layout");
    for name in [
        "uta-fusion-agent-adapter",
        "uta-fusion-agent-pi",
        "uta-fusion-agent-codex",
        "uta-fusion-agent-claude",
    ] {
        let source = PathBuf::from("manifests").join(format!("{name}.uta-fusion-adapter.json"));
        let target = profile_dir.join(format!("{name}.uta-fusion-adapter.json"));
        fs::copy(&source, &target).unwrap_or_else(|error| {
            panic!(
                "could not place adapter manifest {}: {error}",
                target.display()
            )
        });
    }
}
