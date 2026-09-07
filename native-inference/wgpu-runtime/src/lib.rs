//! Shared, fail-closed WGPU/Vulkan execution support for Uta! Studio's
//! native inference workers.
//!
//! Merely depending on this crate does not create a graphics API instance.
//! A Vulkan instance and device are created only by [`GpuDevice::new`] when
//! the `gpu` feature is enabled and a worker has first validated the exact
//! serial safety profile.

mod safety;

pub use safety::{DeviceClass, ExecutionDevice, GpuSafetyConfig, SAFETY_PROFILE};

#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "gpu")]
mod scope;
#[cfg(feature = "gpu")]
pub use gpu::{AdapterIdentity, GpuBuffer, GpuDevice};
#[cfg(feature = "gpu")]
pub use scope::{
    GpuScope, scoped_attention, scoped_bidirectional_gru, scoped_conv_transpose1d,
    scoped_conv_transpose2d_nchw, scoped_conv1d_pytorch, scoped_conv2d_nchw, scoped_glu,
    scoped_layer_norm, scoped_linear, scoped_relative_attention, scoped_silu,
};
