fn main() {
    println!("cargo:rerun-if-env-changed=GST_PLUGIN_SYSTEM_PATH_1_0");
    let paths = std::env::var("GST_PLUGIN_SYSTEM_PATH_1_0").unwrap_or_default();
    println!("cargo:rustc-env=UTA_STUDIO_PACKAGED_GST_PLUGIN_PATH={paths}");
}
