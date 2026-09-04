//! wgpu/Vulkan `Tensor` implementation for the native GAME engine.
//!
//! Design mirrors `CpuTensor` exactly: a tensor is a view (`shape`,
//! `strides`, `offset`) over a shared backing buffer (`Arc<wgpu::Buffer>`
//! here instead of `Arc<Vec<f32>>`). Reshape/transpose/slice are metadata-
//! only, zero GPU work, byte-for-byte the same logic as `cpu/layout.rs`.
//!
//! Compute-heavy ops (matmul, elementwise, softmax, rms_norm, conv1d_dw,
//! rope) run as real WGSL compute shaders on GPU buffers. Structural /
//! data-movement ops with negligible FLOP cost (concat, embedding, repeat,
//! and the "materialize a non-contiguous view" path shared by all of them)
//! go through a CPU round trip: download, reindex with the same algorithm
//! `cpu/{layout,indexing}.rs` uses, re-upload. This is a deliberate scope
//! decision, not a shortcut on correctness: the actual GPU risk/benefit
//! surface on this hardware is the heavy compute dispatches (see
//! `native-inference/roformer/README.md`'s Arc-B580 async-submission
//! notes), not small index-shuffle buffers.
//!
//! Every dispatch is followed by an explicit `queue.submit` +
//! `device.poll(PollType::Wait)` before control returns to the caller —
//! this codebase's established `GGML_VK_DISABLE_ASYNC=1` /
//! `--vulkan-no-async` Arc-B580 stability default, applied here at the
//! wgpu level since there is no equivalent env var for wgpu itself.

mod pipelines;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::{Error, Result};

use super::Tensor;

const MAX_RANK: usize = 4;

#[derive(Clone, Default)]
pub struct GpuAdapterSelector {
    pub device_index: Option<usize>,
    pub name_contains: Option<String>,
}

#[derive(Clone)]
pub struct GpuDevice {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
    pipelines: Arc<pipelines::Pipelines>,
}

impl GpuDevice {
    pub fn new_with_selector(selector: Option<&GpuAdapterSelector>) -> Result<Self> {
        pollster::block_on(Self::new_with_selector_async(selector))
    }

    async fn new_with_selector_async(selector: Option<&GpuAdapterSelector>) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN).await;
        if adapters.is_empty() {
            return Err(Error::message(
                "no Vulkan-capable GPU adapter is available",
            ));
        }

        let chosen = if let Some(name) = selector.and_then(|s| s.name_contains.as_deref()) {
            adapters
                .iter()
                .find(|adapter| adapter.get_info().name.contains(name))
                .cloned()
                .ok_or_else(|| {
                    Error::message(format!("no Vulkan adapter name contains `{name}`"))
                })?
        } else {
            let index = selector.and_then(|s| s.device_index).unwrap_or(0);
            adapters.get(index).cloned().ok_or_else(|| {
                Error::message(format!(
                    "Vulkan adapter index {index} is unavailable ({} adapter(s) found)",
                    adapters.len()
                ))
            })?
        };
        let adapter_info = chosen.get_info();

        let (device, queue) = chosen
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("uta-game-worker"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|error| Error::message(format!("GPU device request failed: {error:?}")))?;

        let compiled = pipelines::Pipelines::new(&device);
        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            pipelines: Arc::new(compiled),
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    fn submit_and_wait(&self, encoder: wgpu::CommandEncoder) -> Result<()> {
        self.queue.submit(Some(encoder.finish()));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| Error::message(format!("GPU poll failed: {error:?}")))?;
        Ok(())
    }

    fn run_kernel(
        &self,
        kernel: &pipelines::Kernel,
        label: &str,
        uniform_bytes: &[u8],
        storage_buffers: &[&wgpu::Buffer],
        workgroups: (u32, u32, u32),
    ) -> Result<()> {
        let uniform = pipelines::uniform_buffer(&self.device, label, uniform_bytes);
        let mut entries = Vec::with_capacity(storage_buffers.len() + 1);
        entries.push(wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        });
        for (index, buffer) in storage_buffers.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: (index + 1) as u32,
                resource: buffer.as_entire_binding(),
            });
        }
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &kernel.layout,
            entries: &entries,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: None,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups.0, workgroups.1, workgroups.2);
        }
        self.submit_and_wait(encoder)
    }
}

#[derive(Clone)]
pub struct GpuTensor {
    buffer: Arc<wgpu::Buffer>,
    shape: Vec<usize>,
    strides: Vec<usize>,
    offset: usize,
    device: GpuDevice,
}

fn invalid_arg(message: impl Into<String>) -> Error {
    Error::message(message.into())
}

fn checked_num_elements(shape: &[usize]) -> Result<usize> {
    shape.iter().try_fold(1usize, |acc, &dim| {
        acc.checked_mul(dim)
            .ok_or_else(|| invalid_arg(format!("tensor shape {:?} is too large", shape)))
    })
}

fn contiguous_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![0; shape.len()];
    let mut stride = 1usize;
    for axis in (0..shape.len()).rev() {
        strides[axis] = stride;
        stride = stride.saturating_mul(shape[axis]);
    }
    strides
}

fn validate_axis(axis: usize, rank: usize, op_name: &str) -> Result<()> {
    if axis >= rank {
        return Err(invalid_arg(format!(
            "{op_name} axis {axis} is out of bounds for rank {rank}"
        )));
    }
    Ok(())
}

fn normalize_axis(axis: isize, rank: usize, op_name: &str) -> Result<usize> {
    if rank == 0 {
        return Err(invalid_arg(format!(
            "{op_name} requires a tensor with at least one dimension"
        )));
    }
    let rank_isize = rank as isize;
    let normalized = if axis < 0 { rank_isize + axis } else { axis };
    if normalized < 0 || normalized >= rank_isize {
        return Err(invalid_arg(format!(
            "{op_name} axis {axis} is out of bounds for rank {rank}"
        )));
    }
    Ok(normalized as usize)
}

fn broadcast_shape(lhs: &[usize], rhs: &[usize]) -> Result<Vec<usize>> {
    let rank = lhs.len().max(rhs.len());
    let mut out = vec![1usize; rank];
    for axis in 0..rank {
        let lhs_dim = lhs
            .len()
            .checked_sub(rank - axis)
            .and_then(|index| lhs.get(index))
            .copied()
            .unwrap_or(1);
        let rhs_dim = rhs
            .len()
            .checked_sub(rank - axis)
            .and_then(|index| rhs.get(index))
            .copied()
            .unwrap_or(1);
        if lhs_dim != rhs_dim && lhs_dim != 1 && rhs_dim != 1 {
            return Err(invalid_arg(format!(
                "cannot broadcast shapes {:?} and {:?}",
                lhs, rhs
            )));
        }
        out[axis] = lhs_dim.max(rhs_dim);
    }
    Ok(out)
}

fn pad4(shape: &[usize], strides: &[usize]) -> Result<([u32; 4], [u32; 4])> {
    if shape.len() > MAX_RANK {
        return Err(invalid_arg(format!(
            "GPU tensor rank {} exceeds the supported maximum of {MAX_RANK}",
            shape.len()
        )));
    }
    let mut padded_shape = [1u32; 4];
    let mut padded_strides = [0u32; 4];
    let pad = MAX_RANK - shape.len();
    for index in 0..shape.len() {
        padded_shape[pad + index] = shape[index] as u32;
        padded_strides[pad + index] = strides[index] as u32;
    }
    Ok((padded_shape, padded_strides))
}

fn precompute_inv_freqs(dims: usize, theta: f32) -> Vec<f32> {
    (0..dims)
        .step_by(2)
        .map(|local_offset| 1.0 / theta.powf(local_offset as f32 / dims as f32))
        .collect()
}

fn normalize_rope_dims(head_dim: usize, rope_dims: usize, op_name: &str, mixed: bool) -> Result<usize> {
    if head_dim == 0 {
        return Err(invalid_arg(format!("{op_name} requires head_dim > 0")));
    }
    let dims = if rope_dims == 0 { head_dim } else { rope_dims };
    if dims > head_dim {
        return Err(invalid_arg(format!(
            "{op_name} rope_dims {dims} exceeds head_dim {head_dim}"
        )));
    }
    if mixed {
        if dims % 4 != 0 {
            return Err(invalid_arg(format!(
                "{op_name} requires rope_dims divisible by 4 for mixed RoPE, got {dims}"
            )));
        }
    } else if dims % 2 != 0 {
        return Err(invalid_arg(format!(
            "{op_name} requires an even rope_dims, got {dims}"
        )));
    }
    Ok(dims)
}

fn validate_rope_shape(
    shape: &[usize],
    positions_len: usize,
    head_dim: usize,
    num_heads: usize,
    op_name: &str,
) -> Result<()> {
    if shape.len() != 3 {
        return Err(invalid_arg(format!(
            "{op_name} expects a rank-3 tensor shaped [num_heads, seq_len, head_dim], got {:?}",
            shape
        )));
    }
    if shape[0] != num_heads || shape[1] != positions_len || shape[2] != head_dim {
        return Err(invalid_arg(format!(
            "{op_name} expected [{num_heads}, {positions_len}, {head_dim}], got {:?}",
            shape
        )));
    }
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BinaryParams {
    lhs_shape: [u32; 4],
    lhs_strides: [u32; 4],
    rhs_shape: [u32; 4],
    rhs_strides: [u32; 4],
    out_shape: [u32; 4],
    out_strides: [u32; 4],
    op_code: u32,
    total: u32,
    width: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UnaryParams {
    op_code: u32,
    total: u32,
    scale: f32,
    width: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SoftmaxParams {
    outer: u32,
    axis_len: u32,
    width: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RmsNormParams {
    rows: u32,
    feature_dim: u32,
    eps: f32,
    width: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatmulParams {
    batch: u32,
    m: u32,
    k: u32,
    n: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Conv1dParams {
    time: u32,
    channels: u32,
    kernel_size: u32,
    out_time: u32,
    stride: u32,
    padding: u32,
    has_bias: u32,
    width: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RopeParams {
    num_heads: u32,
    seq_len: u32,
    head_dim: u32,
    start: u32,
    dims: u32,
    width: u32,
    _pad1: u32,
    _pad2: u32,
}

const WORKGROUP_SIZE: u64 = 64;
const MAX_WORKGROUPS_PER_DIM: u64 = 65_535;

/// A flat 1D index space of `total` threads dispatched as `workgroup_size(64)`
/// needs `ceil(total/64)` workgroups — but Vulkan caps workgroups-per-dimension
/// at 65535, which a real attention-score-sized tensor exceeds (a production
/// GAME run hit this: `[heads, seq_len, seq_len]` needed 96000 workgroups on
/// one dimension). Spread across a 2D grid instead: `(wg_x, wg_y, 1)`
/// workgroups, with every shader reconstructing the flat index as
/// `gid.y * width + gid.x` where `width = wg_x * 64` (the `width` field on
/// every Params struct here). Returns `(wg_x, wg_y, width)`.
fn dispatch_grid_1d(total: usize) -> (u32, u32, u32) {
    let total_workgroups = ((total as u64 + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE).max(1);
    if total_workgroups <= MAX_WORKGROUPS_PER_DIM {
        return (
            total_workgroups as u32,
            1,
            (total_workgroups * WORKGROUP_SIZE) as u32,
        );
    }
    let wg_x = MAX_WORKGROUPS_PER_DIM;
    let wg_y = (total_workgroups + wg_x - 1) / wg_x;
    (wg_x as u32, wg_y as u32, (wg_x * WORKGROUP_SIZE) as u32)
}

impl GpuTensor {
    fn num_elements(&self) -> usize {
        self.shape.iter().product()
    }

    fn is_contiguous(&self) -> bool {
        if self.shape.is_empty() {
            return true;
        }
        let mut expected = 1usize;
        for i in (0..self.shape.len()).rev() {
            if self.strides[i] != expected {
                return false;
            }
            expected *= self.shape[i];
        }
        true
    }

    fn upload_contiguous(device: &GpuDevice, data: &[f32], shape: &[usize]) -> Result<Self> {
        let n = checked_num_elements(shape)?;
        if data.len() != n {
            return Err(invalid_arg(format!(
                "upload: data length {} does not match shape {:?} ({n} elements)",
                data.len(),
                shape
            )));
        }
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let buffer = pipelines::storage_buffer_init(&device.device, "game.gpu.tensor", bytes);
        Ok(Self {
            buffer: Arc::new(buffer),
            shape: shape.to_vec(),
            strides: contiguous_strides(shape),
            offset: 0,
            device: device.clone(),
        })
    }

    /// Returns a tensor whose buffer holds exactly this tensor's own values,
    /// packed row-major starting at byte 0 — the layout every compute
    /// shader assumes. A no-op when already true; otherwise downloads the
    /// full backing buffer once and re-walks it with the exact same
    /// offset+stride algorithm `CpuTensor::to_vec` uses, then re-uploads.
    fn materialize_contiguous(&self) -> Result<Self> {
        if self.offset == 0 && self.is_contiguous() {
            return Ok(self.clone());
        }
        let full = pipelines::download_all(&self.device.device, &self.device.queue, &self.buffer)?;
        let n = self.num_elements();
        let mut out = vec![0.0f32; n];
        let ndims = self.shape.len();
        let mut indices = vec![0usize; ndims];
        for slot in out.iter_mut() {
            let mut index = self.offset;
            for d in 0..ndims {
                index += indices[d] * self.strides[d];
            }
            *slot = full[index];
            for d in (0..ndims).rev() {
                indices[d] += 1;
                if indices[d] < self.shape[d] {
                    break;
                }
                indices[d] = 0;
            }
        }
        Self::upload_contiguous(&self.device, &out, &self.shape)
    }

    fn download_own(&self) -> Result<Vec<f32>> {
        let contiguous = self.materialize_contiguous()?;
        let full = pipelines::download_all(
            &contiguous.device.device,
            &contiguous.device.queue,
            &contiguous.buffer,
        )?;
        let n = contiguous.num_elements();
        Ok(full[..n].to_vec())
    }

    fn dispatch_binary(&self, rhs: &Self, op_code: u32) -> Result<Self> {
        let out_shape = broadcast_shape(&self.shape, &rhs.shape)?;
        let lhs_c = self.materialize_contiguous()?;
        let rhs_c = rhs.materialize_contiguous()?;
        let (lhs_shape, lhs_strides) = pad4(&lhs_c.shape, &lhs_c.strides)?;
        let (rhs_shape, rhs_strides) = pad4(&rhs_c.shape, &rhs_c.strides)?;
        let (out_shape_padded, out_strides_padded) =
            pad4(&out_shape, &contiguous_strides(&out_shape))?;
        let total = checked_num_elements(&out_shape)?;
        let (wg_x, wg_y, width) = dispatch_grid_1d(total);
        let params = BinaryParams {
            lhs_shape,
            lhs_strides,
            rhs_shape,
            rhs_strides,
            out_shape: out_shape_padded,
            out_strides: out_strides_padded,
            op_code,
            total: total as u32,
            width,
            _pad1: 0,
        };
        let out_buffer = pipelines::storage_buffer_zeroed(
            &self.device.device,
            "game.gpu.binary.out",
            (total * 4) as u64,
        );
        self.device.run_kernel(
            &self.device.pipelines.binary,
            "game.gpu.binary",
            bytemuck::bytes_of(&params),
            &[&lhs_c.buffer, &rhs_c.buffer, &out_buffer],
            (wg_x, wg_y, 1),
        )?;
        Ok(Self {
            buffer: Arc::new(out_buffer),
            shape: out_shape.clone(),
            strides: contiguous_strides(&out_shape),
            offset: 0,
            device: self.device.clone(),
        })
    }

    fn dispatch_unary(&self, op_code: u32, scale: f32) -> Result<Self> {
        let input_c = self.materialize_contiguous()?;
        let total = input_c.num_elements();
        let (wg_x, wg_y, width) = dispatch_grid_1d(total);
        let params = UnaryParams {
            op_code,
            total: total as u32,
            scale,
            width,
        };
        let out_buffer = pipelines::storage_buffer_zeroed(
            &self.device.device,
            "game.gpu.unary.out",
            (total * 4) as u64,
        );
        self.device.run_kernel(
            &self.device.pipelines.unary,
            "game.gpu.unary",
            bytemuck::bytes_of(&params),
            &[&input_c.buffer, &out_buffer],
            (wg_x, wg_y, 1),
        )?;
        Ok(Self {
            buffer: Arc::new(out_buffer),
            shape: input_c.shape.clone(),
            strides: input_c.strides.clone(),
            offset: 0,
            device: self.device.clone(),
        })
    }

    fn dispatch_rope_range(
        &self,
        inv_freqs: &[f32],
        positions: &[f32],
        num_heads: usize,
        seq_len: usize,
        head_dim: usize,
        start: usize,
        dims: usize,
    ) -> Result<()> {
        let pairs = dims / 2;
        let total = num_heads * seq_len * pairs;
        if total == 0 {
            return Ok(());
        }
        let (wg_x, wg_y, width) = dispatch_grid_1d(total);
        let params = RopeParams {
            num_heads: num_heads as u32,
            seq_len: seq_len as u32,
            head_dim: head_dim as u32,
            start: start as u32,
            dims: dims as u32,
            width,
            _pad1: 0,
            _pad2: 0,
        };
        let inv_freqs_buffer = pipelines::storage_buffer_init(
            &self.device.device,
            "game.gpu.rope.inv_freqs",
            bytemuck::cast_slice(inv_freqs),
        );
        let positions_buffer = pipelines::storage_buffer_init(
            &self.device.device,
            "game.gpu.rope.positions",
            bytemuck::cast_slice(positions),
        );
        self.device.run_kernel(
            &self.device.pipelines.rope,
            "game.gpu.rope",
            bytemuck::bytes_of(&params),
            &[&inv_freqs_buffer, &positions_buffer, &self.buffer],
            (wg_x, wg_y, 1),
        )
    }
}

impl Tensor for GpuTensor {
    type Device = GpuDevice;

    fn from_data(data: &[f32], shape: &[usize], device: &Self::Device) -> Result<Self> {
        Self::upload_contiguous(device, data, shape)
    }

    fn zeros(shape: &[usize], device: &Self::Device) -> Result<Self> {
        let n = checked_num_elements(shape)?;
        let data = vec![0.0f32; n];
        Self::upload_contiguous(device, &data, shape)
    }

    fn device(&self) -> &Self::Device {
        &self.device
    }

    fn shape(&self) -> &[usize] {
        &self.shape
    }

    fn export(&self, buf: &mut [f32]) -> Result<()> {
        let n = self.num_elements();
        if buf.len() != n {
            return Err(invalid_arg(format!(
                "export buffer length {} does not match tensor shape {:?} ({n} elements)",
                buf.len(),
                self.shape
            )));
        }
        buf.copy_from_slice(&self.download_own()?);
        Ok(())
    }

    fn reshape(self, shape: &[usize]) -> Result<Self> {
        let new_n = checked_num_elements(shape)?;
        let old_n = self.num_elements();
        if new_n != old_n {
            return Err(invalid_arg(format!(
                "reshape: cannot reshape {:?} ({old_n} elements) to {:?} ({new_n} elements)",
                self.shape, shape
            )));
        }
        if self.offset == 0 && self.is_contiguous() {
            Ok(Self {
                buffer: self.buffer,
                shape: shape.to_vec(),
                strides: contiguous_strides(shape),
                offset: 0,
                device: self.device,
            })
        } else {
            self.materialize_contiguous()?.reshape(shape)
        }
    }

    fn transpose(self, dim0: usize, dim1: usize) -> Result<Self> {
        let rank = self.shape.len();
        if dim0 >= rank || dim1 >= rank {
            return Err(invalid_arg(format!(
                "transpose: dimensions ({dim0}, {dim1}) out of range for rank {rank}"
            )));
        }
        let mut shape = self.shape;
        let mut strides = self.strides;
        shape.swap(dim0, dim1);
        strides.swap(dim0, dim1);
        Ok(Self {
            buffer: self.buffer,
            shape,
            strides,
            offset: self.offset,
            device: self.device,
        })
    }

    fn contiguous(self) -> Result<Self> {
        if self.offset == 0 && self.is_contiguous() {
            Ok(self)
        } else {
            self.materialize_contiguous()
        }
    }

    fn slice(self, axis: usize, start: usize, end: usize) -> Result<Self> {
        validate_axis(axis, self.shape.len(), "slice")?;
        if end < start || end > self.shape[axis] {
            return Err(invalid_arg(format!(
                "slice [{start},{end}) is invalid for dimension size {} on axis {axis}",
                self.shape[axis]
            )));
        }
        let new_offset = self.offset + start * self.strides[axis];
        let mut shape = self.shape;
        shape[axis] = end - start;
        Ok(Self {
            buffer: self.buffer,
            shape,
            strides: self.strides,
            offset: new_offset,
            device: self.device,
        })
    }

    fn concat(parts: &[&Self], axis: usize) -> Result<Self> {
        let first = parts
            .first()
            .ok_or_else(|| invalid_arg("concat requires at least one tensor"))?;
        let rank = first.shape.len();
        validate_axis(axis, rank, "concat")?;
        let mut out_shape = first.shape.clone();
        out_shape[axis] = 0;
        for part in parts {
            if part.shape.len() != rank {
                return Err(invalid_arg(format!(
                    "concat rank mismatch: expected rank {rank}, got shape {:?}",
                    part.shape
                )));
            }
            for dim in 0..rank {
                if dim != axis && part.shape[dim] != first.shape[dim] {
                    return Err(invalid_arg(format!(
                        "concat shape mismatch on axis {axis}: expected non-concat dims {:?}, got {:?}",
                        first.shape, part.shape
                    )));
                }
            }
            out_shape[axis] += part.shape[axis];
        }

        let out_len = checked_num_elements(&out_shape)?;
        let mut out = vec![0.0f32; out_len];
        let outer: usize = out_shape[..axis].iter().product();
        let inner: usize = out_shape[axis + 1..].iter().product();
        let out_axis_span = out_shape[axis] * inner;
        let mut axis_offset = 0usize;
        for part in parts {
            let data = part.download_own()?;
            let part_block = part.shape[axis] * inner;
            for outer_index in 0..outer {
                let dst_start = outer_index * out_axis_span + axis_offset * inner;
                let src_start = outer_index * part_block;
                out[dst_start..dst_start + part_block]
                    .copy_from_slice(&data[src_start..src_start + part_block]);
            }
            axis_offset += part.shape[axis];
        }

        Self::upload_contiguous(&first.device, &out, &out_shape)
    }

    fn add(self, rhs: &Self) -> Result<Self> {
        self.dispatch_binary(rhs, 0)
    }

    fn mul(self, rhs: &Self) -> Result<Self> {
        self.dispatch_binary(rhs, 1)
    }

    fn scale(self, s: f32) -> Result<Self> {
        self.dispatch_unary(0, s)
    }

    fn sigmoid(self) -> Result<Self> {
        self.dispatch_unary(1, 0.0)
    }

    fn matmul(&self, rhs: &Self) -> Result<Self> {
        let lhs_shape = self.shape.clone();
        let rhs_shape = rhs.shape.clone();
        let (batch, m, k, n) = match (lhs_shape.len(), rhs_shape.len()) {
            (2, 2) => {
                let (m, k) = (lhs_shape[0], lhs_shape[1]);
                let (rhs_k, n) = (rhs_shape[0], rhs_shape[1]);
                if k != rhs_k {
                    return Err(invalid_arg(format!(
                        "matmul shape mismatch: {:?} @ {:?}",
                        lhs_shape, rhs_shape
                    )));
                }
                (1, m, k, n)
            }
            (3, 3) => {
                let (batch, m, k) = (lhs_shape[0], lhs_shape[1], lhs_shape[2]);
                let (rhs_batch, rhs_k, n) = (rhs_shape[0], rhs_shape[1], rhs_shape[2]);
                if batch != rhs_batch || k != rhs_k {
                    return Err(invalid_arg(format!(
                        "batched matmul shape mismatch: {:?} @ {:?}",
                        lhs_shape, rhs_shape
                    )));
                }
                (batch, m, k, n)
            }
            _ => {
                return Err(invalid_arg(format!(
                    "matmul expects rank-2 or rank-3 tensors, got {:?} and {:?}",
                    lhs_shape, rhs_shape
                )));
            }
        };

        let lhs_c = self.materialize_contiguous()?;
        let rhs_c = rhs.materialize_contiguous()?;
        let params = MatmulParams {
            batch: batch as u32,
            m: m as u32,
            k: k as u32,
            n: n as u32,
        };
        let out_len = batch * m * n;
        let out_buffer = pipelines::storage_buffer_zeroed(
            &self.device.device,
            "game.gpu.matmul.out",
            (out_len * 4) as u64,
        );
        self.device.run_kernel(
            &self.device.pipelines.matmul,
            "game.gpu.matmul",
            bytemuck::bytes_of(&params),
            &[&lhs_c.buffer, &rhs_c.buffer, &out_buffer],
            (
                ((n + 7) / 8).max(1) as u32,
                ((m + 7) / 8).max(1) as u32,
                batch.max(1) as u32,
            ),
        )?;
        let out_shape = if lhs_shape.len() == 2 {
            vec![m, n]
        } else {
            vec![batch, m, n]
        };
        Ok(Self {
            buffer: Arc::new(out_buffer),
            shape: out_shape.clone(),
            strides: contiguous_strides(&out_shape),
            offset: 0,
            device: self.device.clone(),
        })
    }

    fn linear(&self, weight: &Self, bias: Option<&Self>) -> Result<Self> {
        let input_shape = self.shape.clone();
        if input_shape.is_empty() {
            return Err(invalid_arg(
                "linear expects an input tensor with at least one dimension",
            ));
        }
        if weight.shape.len() != 2 {
            return Err(invalid_arg(format!(
                "linear weight must be rank-2 [out_dim, in_dim], got {:?}",
                weight.shape
            )));
        }
        let in_dim = *input_shape.last().unwrap();
        let out_dim = weight.shape[0];
        if weight.shape[1] != in_dim {
            return Err(invalid_arg(format!(
                "linear shape mismatch: input {:?}, weight {:?}",
                input_shape, weight.shape
            )));
        }
        if let Some(bias) = bias
            && bias.shape != [out_dim]
        {
            return Err(invalid_arg(format!(
                "linear bias must have shape [{out_dim}], got {:?}",
                bias.shape
            )));
        }

        let rows: usize = input_shape[..input_shape.len() - 1].iter().product();
        let input_flat = self.clone().reshape(&[rows, in_dim])?;
        let weight_t = weight.clone().transpose(0, 1)?;
        let mut out = input_flat.matmul(&weight_t)?;
        if let Some(bias) = bias {
            out = out.add(bias)?;
        }
        let mut out_shape = input_shape[..input_shape.len() - 1].to_vec();
        out_shape.push(out_dim);
        out.reshape(&out_shape)
    }

    fn rms_norm(self, weight: &Self, eps: f32) -> Result<Self> {
        if self.shape.is_empty() {
            return Err(invalid_arg(
                "rms_norm expects an input tensor with at least one dimension",
            ));
        }
        let feature_dim = *self.shape.last().unwrap();
        if weight.shape != [feature_dim] {
            return Err(invalid_arg(format!(
                "rms_norm weight must have shape [{feature_dim}], got {:?}",
                weight.shape
            )));
        }
        let shape = self.shape.clone();
        let rows: usize = shape[..shape.len() - 1].iter().product();
        let input_c = self.materialize_contiguous()?;
        let weight_c = weight.materialize_contiguous()?;
        let (wg_x, wg_y, width) = dispatch_grid_1d(rows);
        let params = RmsNormParams {
            rows: rows as u32,
            feature_dim: feature_dim as u32,
            eps,
            width,
        };
        let total = rows * feature_dim;
        let out_buffer = pipelines::storage_buffer_zeroed(
            &self.device.device,
            "game.gpu.rms_norm.out",
            (total * 4) as u64,
        );
        self.device.run_kernel(
            &self.device.pipelines.rms_norm,
            "game.gpu.rms_norm",
            bytemuck::bytes_of(&params),
            &[&input_c.buffer, &weight_c.buffer, &out_buffer],
            (wg_x, wg_y, 1),
        )?;
        Ok(Self {
            buffer: Arc::new(out_buffer),
            shape: shape.clone(),
            strides: contiguous_strides(&shape),
            offset: 0,
            device: self.device.clone(),
        })
    }

    fn gelu(self) -> Result<Self> {
        self.dispatch_unary(2, 0.0)
    }

    fn softmax(self, axis: isize) -> Result<Self> {
        if self.shape.is_empty() {
            return Err(invalid_arg(
                "softmax expects a tensor with at least one dimension",
            ));
        }
        let rank = self.shape.len();
        let normalized = normalize_axis(axis, rank, "softmax")?;
        if normalized != rank - 1 {
            let last = rank - 1;
            let moved = self.transpose(normalized, last)?.softmax(-1)?;
            return moved.transpose(normalized, last);
        }

        let axis_len = self.shape[normalized];
        if axis_len == 0 {
            return Ok(self);
        }
        let outer: usize = self.shape[..normalized].iter().product();
        let contiguous = self.materialize_contiguous()?;
        if outer > 0 {
            let (wg_x, wg_y, width) = dispatch_grid_1d(outer);
            let params = SoftmaxParams {
                outer: outer as u32,
                axis_len: axis_len as u32,
                width,
                _pad1: 0,
            };
            contiguous.device.run_kernel(
                &contiguous.device.pipelines.softmax,
                "game.gpu.softmax",
                bytemuck::bytes_of(&params),
                &[&contiguous.buffer],
                (wg_x, wg_y, 1),
            )?;
        }
        Ok(contiguous)
    }

    fn rope(
        self,
        positions: &[i32],
        head_dim: usize,
        num_heads: usize,
        rope_dims: usize,
        theta: f32,
    ) -> Result<Self> {
        validate_rope_shape(&self.shape, positions.len(), head_dim, num_heads, "rope")?;
        let rope_dims = normalize_rope_dims(head_dim, rope_dims, "rope", false)?;
        let seq_len = self.shape[1];
        let contiguous = self.materialize_contiguous()?;
        let inv_freqs = precompute_inv_freqs(rope_dims, theta);
        let positions_f: Vec<f32> = positions.iter().map(|&p| p as f32).collect();
        contiguous.dispatch_rope_range(
            &inv_freqs,
            &positions_f,
            num_heads,
            seq_len,
            head_dim,
            0,
            rope_dims,
        )?;
        Ok(contiguous)
    }

    fn region_rope(
        self,
        global_pos: &[i32],
        region_ids: &[i32],
        head_dim: usize,
        num_heads: usize,
        rope_dims: usize,
        theta: f32,
    ) -> Result<Self> {
        validate_rope_shape(
            &self.shape,
            global_pos.len(),
            head_dim,
            num_heads,
            "region_rope",
        )?;
        if region_ids.len() != global_pos.len() {
            return Err(invalid_arg(format!(
                "region_rope expected {} region ids, got {}",
                global_pos.len(),
                region_ids.len()
            )));
        }
        let mixed_dims = normalize_rope_dims(head_dim, rope_dims, "region_rope", true)?;
        let half = mixed_dims / 2;
        let seq_len = self.shape[1];
        let contiguous = self.materialize_contiguous()?;
        let inv_freqs = precompute_inv_freqs(half, theta);
        let global_f: Vec<f32> = global_pos.iter().map(|&p| p as f32).collect();
        let region_f: Vec<f32> = region_ids.iter().map(|&p| p as f32).collect();
        contiguous.dispatch_rope_range(
            &inv_freqs,
            &global_f,
            num_heads,
            seq_len,
            head_dim,
            0,
            half,
        )?;
        contiguous.dispatch_rope_range(
            &inv_freqs,
            &region_f,
            num_heads,
            seq_len,
            head_dim,
            half,
            half,
        )?;
        Ok(contiguous)
    }

    fn conv1d_dw(
        self,
        kernel: &Self,
        bias: Option<&Self>,
        stride: usize,
        padding: usize,
    ) -> Result<Self> {
        if stride == 0 {
            return Err(invalid_arg("conv1d_dw requires stride > 0"));
        }
        if self.shape.len() != 2 {
            return Err(invalid_arg(format!(
                "conv1d_dw expects input shape [time, channels], got {:?}",
                self.shape
            )));
        }
        if kernel.shape.len() != 2 {
            return Err(invalid_arg(format!(
                "conv1d_dw kernel must have shape [channels, kernel_size], got {:?}",
                kernel.shape
            )));
        }
        let (time, channels) = (self.shape[0], self.shape[1]);
        let (kernel_channels, kernel_size) = (kernel.shape[0], kernel.shape[1]);
        if channels != kernel_channels {
            return Err(invalid_arg(format!(
                "conv1d_dw channel mismatch: input {:?}, kernel {:?}",
                self.shape, kernel.shape
            )));
        }
        if kernel_size == 0 {
            return Err(invalid_arg(
                "conv1d_dw kernel size must be greater than zero",
            ));
        }
        if let Some(bias) = bias
            && bias.shape != [channels]
        {
            return Err(invalid_arg(format!(
                "conv1d_dw bias must have shape [{channels}], got {:?}",
                bias.shape
            )));
        }

        let padded = time
            .checked_add(padding.checked_mul(2).ok_or_else(|| invalid_arg("conv1d_dw padding overflow"))?)
            .ok_or_else(|| invalid_arg("conv1d_dw padded size overflow"))?;
        let out_time = if padded < kernel_size {
            0
        } else {
            (padded - kernel_size) / stride + 1
        };

        let input_c = self.materialize_contiguous()?;
        let kernel_c = kernel.materialize_contiguous()?;
        let (bias_buffer, has_bias) = match bias {
            Some(bias) => (bias.materialize_contiguous()?.buffer, true),
            None => (
                Arc::new(pipelines::storage_buffer_zeroed(
                    &self.device.device,
                    "game.gpu.conv1d_dw.bias_dummy",
                    4,
                )),
                false,
            ),
        };
        let total = out_time * channels;
        let (wg_x, wg_y, width) = dispatch_grid_1d(total);
        let params = Conv1dParams {
            time: time as u32,
            channels: channels as u32,
            kernel_size: kernel_size as u32,
            out_time: out_time as u32,
            stride: stride as u32,
            padding: padding as u32,
            has_bias: has_bias as u32,
            width,
        };
        let out_buffer = pipelines::storage_buffer_zeroed(
            &self.device.device,
            "game.gpu.conv1d_dw.out",
            (total.max(1) * 4) as u64,
        );
        self.device.run_kernel(
            &self.device.pipelines.conv1d_dw,
            "game.gpu.conv1d_dw",
            bytemuck::bytes_of(&params),
            &[&input_c.buffer, &kernel_c.buffer, &bias_buffer, &out_buffer],
            (wg_x, wg_y, 1),
        )?;
        let out_shape = vec![out_time, channels];
        Ok(Self {
            buffer: Arc::new(out_buffer),
            shape: out_shape.clone(),
            strides: contiguous_strides(&out_shape),
            offset: 0,
            device: self.device.clone(),
        })
    }

    fn embedding(table: &Self, indices: &[i32]) -> Result<Self> {
        if table.shape.len() != 2 {
            return Err(invalid_arg(format!(
                "embedding table must have shape [rows, dim], got {:?}",
                table.shape
            )));
        }
        let rows = table.shape[0];
        let dim = table.shape[1];
        let full = table.download_own()?;
        let mut out = vec![0.0f32; indices.len() * dim];
        for (row_index, &index) in indices.iter().enumerate() {
            let source_row = usize::try_from(index)
                .map_err(|_| invalid_arg(format!("embedding index {index} is negative")))?;
            if source_row >= rows {
                return Err(invalid_arg(format!(
                    "embedding index {source_row} is out of bounds for {rows} rows"
                )));
            }
            let src_start = source_row * dim;
            let dst_start = row_index * dim;
            out[dst_start..dst_start + dim].copy_from_slice(&full[src_start..src_start + dim]);
        }
        Self::upload_contiguous(&table.device, &out, &[indices.len(), dim])
    }

    fn repeat(self, axis: usize, n: usize) -> Result<Self> {
        validate_axis(axis, self.shape.len(), "repeat")?;
        let shape = self.shape.clone();
        let data = self.download_own()?;

        let mut out_shape = shape.clone();
        out_shape[axis] = out_shape[axis]
            .checked_mul(n)
            .ok_or_else(|| invalid_arg("repeat axis size overflow"))?;
        let out_len = checked_num_elements(&out_shape)?;
        let mut out = vec![0.0f32; out_len];

        let outer: usize = shape[..axis].iter().product();
        let inner: usize = shape[axis + 1..].iter().product();
        let axis_block = shape[axis] * inner;
        let out_axis_block = out_shape[axis] * inner;

        for outer_index in 0..outer {
            let src_start = outer_index * axis_block;
            let src = &data[src_start..src_start + axis_block];
            for repeat_index in 0..n {
                let dst_start = outer_index * out_axis_block + repeat_index * axis_block;
                out[dst_start..dst_start + axis_block].copy_from_slice(src);
            }
        }

        Self::upload_contiguous(&self.device, &out, &out_shape)
    }
}
