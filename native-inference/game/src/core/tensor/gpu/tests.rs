//! `wgsl_shaders_parse_and_validate` is pure CPU (naga's WGSL front end +
//! validator, no GPU/adapter contact) and always runs. Every other test
//! here creates a real `GpuDevice` and therefore dispatches real Vulkan
//! compute on whatever adapter is present — run these only when GPU
//! execution has been explicitly authorized for this host.

use super::{GpuDevice, GpuTensor};
use crate::tensor::tests;

const SHADER_SOURCES: &[(&str, &str)] = &[
    ("binary", include_str!("shaders/binary.wgsl")),
    ("unary", include_str!("shaders/unary.wgsl")),
    ("softmax", include_str!("shaders/softmax.wgsl")),
    ("rms_norm", include_str!("shaders/rms_norm.wgsl")),
    ("matmul", include_str!("shaders/matmul.wgsl")),
    ("conv1d_dw", include_str!("shaders/conv1d_dw.wgsl")),
    ("rope", include_str!("shaders/rope.wgsl")),
];

#[test]
fn wgsl_shaders_parse_and_validate() {
    for (name, source) in SHADER_SOURCES {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("shader `{name}` failed to parse: {error}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|error| panic!("shader `{name}` failed validation: {error}"));
    }
}

fn device() -> GpuDevice {
    let _ = env_logger::builder().is_test(true).try_init();
    GpuDevice::new_with_selector(None).expect("a Vulkan-capable GPU adapter is required")
}

#[test]
fn roundtrip_matches_uploaded_values() {
    tests::run_roundtrip::<GpuTensor>(&device());
}

#[test]
fn layout_ops_preserve_view_semantics() {
    tests::run_layout_ops_preserve_view_semantics::<GpuTensor>(&device());
}

#[test]
fn broadcast_add_and_mul_match_expected_values() {
    tests::run_broadcast_add_and_mul_match_expected_values::<GpuTensor>(&device());
}

#[test]
fn matmul_supports_2d_and_batched_3d_inputs() {
    tests::run_matmul_supports_2d_and_batched_3d_inputs::<GpuTensor>(&device());
}

#[test]
fn matmul_handles_views_and_rejects_unsupported_batch_shapes() {
    tests::run_matmul_handles_views_and_rejects_unsupported_batch_shapes::<GpuTensor>(&device());
}

#[test]
fn linear_applies_weight_rows_and_optional_bias() {
    tests::run_linear_applies_weight_rows_and_optional_bias::<GpuTensor>(&device());
}

#[test]
fn normalization_and_activation_ops_match_reference_values() {
    tests::run_normalization_and_activation_ops_match_reference_values::<GpuTensor>(&device());
}

#[test]
fn rope_rotates_each_head_using_global_positions() {
    tests::run_rope_rotates_each_head_using_global_positions::<GpuTensor>(&device());
}

#[test]
fn region_rope_splits_global_and_region_rotation_halves() {
    tests::run_region_rope_splits_global_and_region_rotation_halves::<GpuTensor>(&device());
}

#[test]
fn depthwise_conv_applies_per_channel_kernels() {
    tests::run_depthwise_conv_applies_per_channel_kernels::<GpuTensor>(&device());
}

#[test]
fn embedding_and_repeat_return_expected_rows() {
    tests::run_embedding_and_repeat_return_expected_rows::<GpuTensor>(&device());
}

#[test]
fn fused_attention_matches_reference() {
    tests::run_fused_attention_matches_reference::<GpuTensor>(&device());
}
