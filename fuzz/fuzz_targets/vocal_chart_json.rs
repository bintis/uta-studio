#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(chart) = serde_json::from_slice::<utz::VocalChart>(bytes) {
        let _ = chart.validate();
    }
});
