use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{DeviceClass, GpuSafetyConfig};

const WORKGROUP_SIZE: u64 = 64;
const MAX_WORKGROUPS_PER_DIM: u64 = 65_535;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterIdentity {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub device_type: String,
    pub backend: &'static str,
}

pub struct GpuBuffer {
    raw: wgpu::Buffer,
    len: usize,
}

impl GpuBuffer {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

struct DeviceInner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: AdapterIdentity,
    pipelines: Pipelines,
    uncaptured_errors: Arc<Mutex<Vec<String>>>,
}

impl Drop for DeviceInner {
    fn drop(&mut self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.device.destroy();
    }
}

#[derive(Clone)]
pub struct GpuDevice {
    inner: Arc<DeviceInner>,
}

impl GpuDevice {
    /// Creates an explicitly Vulkan-only device. Callers must validate the
    /// safety profile before reaching this function.
    pub fn new(safety: GpuSafetyConfig, label: &str) -> Result<Self, String> {
        pollster::block_on(Self::new_async(safety, label))
    }

    async fn new_async(safety: GpuSafetyConfig, label: &str) -> Result<Self, String> {
        if safety.batch_size != 1 || !safety.vulkan_no_async || !safety.serial_pipeline {
            return Err("refusing to create Vulkan device for an unsafe profile".to_string());
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN).await;
        let adapter = adapters
            .into_iter()
            .find(|candidate| {
                matches_device_class(candidate.get_info().device_type, safety.device_class)
            })
            .ok_or_else(|| format!("no Vulkan adapter matches {:?}", safety.device_class))?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Vulkan {
            return Err(format!(
                "selected adapter unexpectedly reports backend {:?}, not Vulkan",
                info.backend
            ));
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(label),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("Vulkan device request failed: {error:?}"))?;

        let uncaptured_errors = Arc::new(Mutex::new(Vec::new()));
        let callback_errors = Arc::clone(&uncaptured_errors);
        device.on_uncaptured_error(Arc::new(move |error| {
            if let Ok(mut errors) = callback_errors.lock() {
                errors.push(format!("{error:?}"));
            }
        }));

        let pipelines = Pipelines::new(&device);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("Vulkan poll after pipeline creation failed: {error:?}"))?;

        Ok(Self {
            inner: Arc::new(DeviceInner {
                device,
                queue,
                adapter: AdapterIdentity {
                    name: info.name,
                    vendor: info.vendor,
                    device: info.device,
                    device_type: format!("{:?}", info.device_type),
                    backend: "vulkan",
                },
                pipelines,
                uncaptured_errors,
            }),
        })
    }

    pub fn adapter_identity(&self) -> &AdapterIdentity {
        &self.inner.adapter
    }

    pub fn upload(&self, label: &str, values: &[f32]) -> Result<GpuBuffer, String> {
        self.validate_buffer_len(values.len(), label)?;
        let bytes: &[u8] = bytemuck::cast_slice(values);
        let raw = if bytes.is_empty() {
            self.inner.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 4,
                usage: storage_usage(),
                mapped_at_creation: false,
            })
        } else {
            self.inner
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: storage_usage(),
                })
        };
        self.check_uncaptured_errors(label)?;
        Ok(GpuBuffer {
            raw,
            len: values.len(),
        })
    }

    pub fn download(&self, label: &str, buffer: &GpuBuffer) -> Result<Vec<f32>, String> {
        if buffer.is_empty() {
            return Ok(Vec::new());
        }
        let size = checked_byte_size(buffer.len, label)?;
        let staging = self.inner.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .inner
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        encoder.copy_buffer_to_buffer(&buffer.raw, 0, &staging, 0, size);
        self.inner.queue.submit(Some(encoder.finish()));
        self.wait(label)?;

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.wait(label)?;
        receiver
            .recv()
            .map_err(|error| format!("{label}: GPU map channel closed: {error}"))?
            .map_err(|error| format!("{label}: GPU map failed: {error:?}"))?;
        let mapped = slice.get_mapped_range();
        let values = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        Ok(values)
    }

    pub fn linear(
        &self,
        label: &str,
        input: &GpuBuffer,
        m: usize,
        k: usize,
        weight: &[f32],
        bias: Option<&[f32]>,
        n: usize,
        weight_transposed: bool,
    ) -> Result<GpuBuffer, String> {
        expect_len(input, checked_product(&[m, k], label)?, label, "input")?;
        expect_slice_len(weight, checked_product(&[k, n], label)?, label, "weight")?;
        if let Some(bias) = bias {
            expect_slice_len(bias, n, label, "bias")?;
        }
        let weight = self.upload(&format!("{label}.weight"), weight)?;
        let bias_values = bias.unwrap_or(&[0.0]);
        let bias = self.upload(&format!("{label}.bias"), bias_values)?;
        let total = checked_product(&[m, n], label)?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, width) = dispatch_grid(total)?;
        let params = LinearParams {
            m: checked_u32(m, label)?,
            k: checked_u32(k, label)?,
            n: checked_u32(n, label)?,
            weight_transposed: u32::from(weight_transposed),
            has_bias: u32::from(bias_values.len() == n),
            total: checked_u32(total, label)?,
            width,
            _pad: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.linear,
            label,
            bytemuck::bytes_of(&params),
            &[input, &weight, &bias, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    pub fn conv1d(
        &self,
        label: &str,
        input: &GpuBuffer,
        frames: usize,
        in_channels: usize,
        weight: &[f32],
        bias: &[f32],
        out_channels: usize,
        kernel: usize,
    ) -> Result<GpuBuffer, String> {
        expect_len(
            input,
            checked_product(&[frames, in_channels], label)?,
            label,
            "input",
        )?;
        expect_slice_len(
            weight,
            checked_product(&[kernel, in_channels, out_channels], label)?,
            label,
            "weight",
        )?;
        expect_slice_len(bias, out_channels, label, "bias")?;
        let weight = self.upload(&format!("{label}.weight"), weight)?;
        let bias = self.upload(&format!("{label}.bias"), bias)?;
        let total = checked_product(&[frames, out_channels], label)?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, width) = dispatch_grid(total)?;
        let params = Conv1dParams {
            frames: checked_u32(frames, label)?,
            in_channels: checked_u32(in_channels, label)?,
            out_channels: checked_u32(out_channels, label)?,
            kernel: checked_u32(kernel, label)?,
            padding: checked_u32(kernel / 2, label)?,
            total: checked_u32(total, label)?,
            width,
            _pad: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.conv1d,
            label,
            bytemuck::bytes_of(&params),
            &[input, &weight, &bias, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    pub fn depthwise_conv1d(
        &self,
        label: &str,
        input: &GpuBuffer,
        frames: usize,
        channels: usize,
        weight: &[f32],
        bias: &[f32],
        kernel: usize,
    ) -> Result<GpuBuffer, String> {
        expect_len(
            input,
            checked_product(&[frames, channels], label)?,
            label,
            "input",
        )?;
        expect_slice_len(
            weight,
            checked_product(&[kernel, channels], label)?,
            label,
            "weight",
        )?;
        expect_slice_len(bias, channels, label, "bias")?;
        let weight = self.upload(&format!("{label}.weight"), weight)?;
        let bias = self.upload(&format!("{label}.bias"), bias)?;
        let total = checked_product(&[frames, channels], label)?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, width) = dispatch_grid(total)?;
        let params = DepthwiseConv1dParams {
            frames: checked_u32(frames, label)?,
            channels: checked_u32(channels, label)?,
            kernel: checked_u32(kernel, label)?,
            padding: checked_u32(kernel / 2, label)?,
            total: checked_u32(total, label)?,
            width,
            _pad0: 0,
            _pad1: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.depthwise_conv1d,
            label,
            bytemuck::bytes_of(&params),
            &[input, &weight, &bias, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_nchw(
        &self,
        label: &str,
        input: &GpuBuffer,
        in_channels: usize,
        in_height: usize,
        in_width: usize,
        weight: &[f32],
        bias: &[f32],
        out_channels: usize,
        kernel_height: usize,
        kernel_width: usize,
        stride_height: usize,
        stride_width: usize,
        pad_height: usize,
        pad_width: usize,
    ) -> Result<(GpuBuffer, usize, usize), String> {
        if stride_height == 0 || stride_width == 0 || kernel_height == 0 || kernel_width == 0 {
            return Err(format!(
                "{label}: convolution kernel and strides must be non-zero"
            ));
        }
        expect_len(
            input,
            checked_product(&[in_channels, in_height, in_width], label)?,
            label,
            "input",
        )?;
        expect_slice_len(
            weight,
            checked_product(
                &[out_channels, in_channels, kernel_height, kernel_width],
                label,
            )?,
            label,
            "weight",
        )?;
        expect_slice_len(bias, out_channels, label, "bias")?;
        let padded_height = in_height
            .checked_add(
                pad_height
                    .checked_mul(2)
                    .ok_or_else(|| format!("{label}: height overflow"))?,
            )
            .ok_or_else(|| format!("{label}: height overflow"))?;
        let padded_width = in_width
            .checked_add(
                pad_width
                    .checked_mul(2)
                    .ok_or_else(|| format!("{label}: width overflow"))?,
            )
            .ok_or_else(|| format!("{label}: width overflow"))?;
        if padded_height < kernel_height || padded_width < kernel_width {
            return Err(format!("{label}: kernel exceeds padded input"));
        }
        let out_height = (padded_height - kernel_height) / stride_height + 1;
        let out_width = (padded_width - kernel_width) / stride_width + 1;
        let total = checked_product(&[out_channels, out_height, out_width], label)?;
        let weight = self.upload(&format!("{label}.weight"), weight)?;
        let bias = self.upload(&format!("{label}.bias"), bias)?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, width) = dispatch_grid(total)?;
        let params = Conv2dParams {
            in_channels: checked_u32(in_channels, label)?,
            in_height: checked_u32(in_height, label)?,
            in_width: checked_u32(in_width, label)?,
            out_channels: checked_u32(out_channels, label)?,
            out_height: checked_u32(out_height, label)?,
            out_width: checked_u32(out_width, label)?,
            kernel_height: checked_u32(kernel_height, label)?,
            kernel_width: checked_u32(kernel_width, label)?,
            stride_height: checked_u32(stride_height, label)?,
            stride_width: checked_u32(stride_width, label)?,
            pad_height: checked_u32(pad_height, label)?,
            pad_width: checked_u32(pad_width, label)?,
            total: checked_u32(total, label)?,
            width,
            _pad0: 0,
            _pad1: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.conv2d_nchw,
            label,
            bytemuck::bytes_of(&params),
            &[input, &weight, &bias, &output],
            (x, y, 1),
        )?;
        Ok((output, out_height, out_width))
    }

    pub fn relu(&self, label: &str, input: &GpuBuffer) -> Result<GpuBuffer, String> {
        self.unary(label, input, 0)
    }

    pub fn silu(&self, label: &str, input: &GpuBuffer) -> Result<GpuBuffer, String> {
        self.unary(label, input, 1)
    }

    pub fn sigmoid(&self, label: &str, input: &GpuBuffer) -> Result<GpuBuffer, String> {
        self.unary(label, input, 2)
    }

    pub fn glu(
        &self,
        label: &str,
        input: &GpuBuffer,
        frames: usize,
        input_channels: usize,
    ) -> Result<GpuBuffer, String> {
        if input_channels % 2 != 0 {
            return Err(format!("{label}: GLU input channels must be even"));
        }
        expect_len(
            input,
            checked_product(&[frames, input_channels], label)?,
            label,
            "input",
        )?;
        let output_channels = input_channels / 2;
        let total = checked_product(&[frames, output_channels], label)?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, width) = dispatch_grid(total)?;
        let params = GluParams {
            frames: checked_u32(frames, label)?,
            input_channels: checked_u32(input_channels, label)?,
            output_channels: checked_u32(output_channels, label)?,
            total: checked_u32(total, label)?,
            width,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.glu,
            label,
            bytemuck::bytes_of(&params),
            &[input, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    pub fn layer_norm(
        &self,
        label: &str,
        input: &GpuBuffer,
        rows: usize,
        channels: usize,
        weight: &[f32],
        bias: &[f32],
        epsilon: f32,
    ) -> Result<GpuBuffer, String> {
        expect_len(
            input,
            checked_product(&[rows, channels], label)?,
            label,
            "input",
        )?;
        expect_slice_len(weight, channels, label, "weight")?;
        expect_slice_len(bias, channels, label, "bias")?;
        if rows > MAX_WORKGROUPS_PER_DIM as usize {
            return Err(format!(
                "{label}: row count exceeds serial normalization dispatch limit"
            ));
        }
        let weight = self.upload(&format!("{label}.weight"), weight)?;
        let bias = self.upload(&format!("{label}.bias"), bias)?;
        let output = self.zeroed(&format!("{label}.output"), input.len)?;
        let params = LayerNormParams {
            rows: checked_u32(rows, label)?,
            channels: checked_u32(channels, label)?,
            epsilon,
            _pad: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.layer_norm,
            label,
            bytemuck::bytes_of(&params),
            &[input, &weight, &bias, &output],
            (rows.max(1) as u32, 1, 1),
        )?;
        Ok(output)
    }

    pub fn add(
        &self,
        label: &str,
        left: &GpuBuffer,
        right: &GpuBuffer,
    ) -> Result<GpuBuffer, String> {
        if left.len != right.len {
            return Err(format!("{label}: add operands have different lengths"));
        }
        let output = self.zeroed(&format!("{label}.output"), left.len)?;
        let (x, y, width) = dispatch_grid(left.len)?;
        let params = FlatParams {
            total: checked_u32(left.len, label)?,
            width,
            _pad0: 0,
            _pad1: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.add,
            label,
            bytemuck::bytes_of(&params),
            &[left, right, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    pub fn softmax_rows(
        &self,
        label: &str,
        input: &GpuBuffer,
        rows: usize,
        channels: usize,
    ) -> Result<GpuBuffer, String> {
        expect_len(
            input,
            checked_product(&[rows, channels], label)?,
            label,
            "input",
        )?;
        if rows > MAX_WORKGROUPS_PER_DIM as usize {
            return Err(format!(
                "{label}: row count exceeds serial softmax dispatch limit"
            ));
        }
        let output = self.zeroed(&format!("{label}.output"), input.len)?;
        let params = RowsParams {
            rows: checked_u32(rows, label)?,
            channels: checked_u32(channels, label)?,
            _pad0: 0,
            _pad1: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.softmax_rows,
            label,
            bytemuck::bytes_of(&params),
            &[input, &output],
            (rows.max(1) as u32, 1, 1),
        )?;
        Ok(output)
    }

    pub fn nchw_to_nhwc(
        &self,
        label: &str,
        input: &GpuBuffer,
        channels: usize,
        height: usize,
        width_in: usize,
    ) -> Result<GpuBuffer, String> {
        let total = checked_product(&[channels, height, width_in], label)?;
        expect_len(input, total, label, "input")?;
        let output = self.zeroed(&format!("{label}.output"), total)?;
        let (x, y, dispatch_width) = dispatch_grid(total)?;
        let params = NchwToNhwcParams {
            channels: checked_u32(channels, label)?,
            height: checked_u32(height, label)?,
            width_in: checked_u32(width_in, label)?,
            total: checked_u32(total, label)?,
            dispatch_width,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.nchw_to_nhwc,
            label,
            bytemuck::bytes_of(&params),
            &[input, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    fn unary(&self, label: &str, input: &GpuBuffer, operation: u32) -> Result<GpuBuffer, String> {
        let output = self.zeroed(&format!("{label}.output"), input.len)?;
        let (x, y, width) = dispatch_grid(input.len)?;
        let params = UnaryParams {
            total: checked_u32(input.len, label)?,
            operation,
            width,
            _pad: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.unary,
            label,
            bytemuck::bytes_of(&params),
            &[input, &output],
            (x, y, 1),
        )?;
        Ok(output)
    }

    fn zeroed(&self, label: &str, len: usize) -> Result<GpuBuffer, String> {
        self.validate_buffer_len(len, label)?;
        let raw = self.inner.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: checked_byte_size(len.max(1), label)?,
            usage: storage_usage(),
            mapped_at_creation: false,
        });
        self.check_uncaptured_errors(label)?;
        Ok(GpuBuffer { raw, len })
    }

    fn validate_buffer_len(&self, len: usize, label: &str) -> Result<(), String> {
        let bytes = checked_byte_size(len.max(1), label)?;
        let limits = self.inner.device.limits();
        if bytes > limits.max_buffer_size {
            return Err(format!(
                "{label}: requested {bytes}-byte buffer exceeds adapter max_buffer_size {}",
                limits.max_buffer_size
            ));
        }
        if bytes > u64::from(limits.max_storage_buffer_binding_size) {
            return Err(format!(
                "{label}: requested {bytes}-byte buffer exceeds max_storage_buffer_binding_size {}",
                limits.max_storage_buffer_binding_size
            ));
        }
        Ok(())
    }

    fn run_kernel(
        &self,
        kernel: &Kernel,
        label: &str,
        uniform_bytes: &[u8],
        buffers: &[&GpuBuffer],
        workgroups: (u32, u32, u32),
    ) -> Result<(), String> {
        self.check_uncaptured_errors(label)?;
        let validation = self
            .inner
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = self
            .inner
            .device
            .push_error_scope(wgpu::ErrorFilter::Internal);
        let out_of_memory = self
            .inner
            .device
            .push_error_scope(wgpu::ErrorFilter::OutOfMemory);

        let uniform = self
            .inner
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let mut entries = Vec::with_capacity(buffers.len() + 1);
        entries.push(wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        });
        for (index, buffer) in buffers.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: (index + 1) as u32,
                resource: buffer.raw.as_entire_binding(),
            });
        }
        let bind_group = self
            .inner
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &kernel.layout,
                entries: &entries,
            });
        let mut encoder = self
            .inner
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
        self.inner.queue.submit(Some(encoder.finish()));
        let wait_result = self.wait(label);

        let oom_error = pollster::block_on(out_of_memory.pop());
        let internal_error = pollster::block_on(internal.pop());
        let validation_error = pollster::block_on(validation.pop());
        wait_result?;
        if let Some(error) = oom_error.or(internal_error).or(validation_error) {
            return Err(format!("{label}: Vulkan kernel failed: {error:?}"));
        }
        self.check_uncaptured_errors(label)
    }

    fn wait(&self, label: &str) -> Result<(), String> {
        self.inner
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("{label}: synchronous Vulkan wait failed: {error:?}"))?;
        self.check_uncaptured_errors(label)
    }

    fn check_uncaptured_errors(&self, label: &str) -> Result<(), String> {
        let mut errors = self
            .inner
            .uncaptured_errors
            .lock()
            .map_err(|_| format!("{label}: GPU error state lock was poisoned"))?;
        if errors.is_empty() {
            return Ok(());
        }
        let joined = errors.join("; ");
        errors.clear();
        Err(format!("{label}: uncaptured Vulkan error: {joined}"))
    }
}

fn matches_device_class(device_type: wgpu::DeviceType, requested: DeviceClass) -> bool {
    match requested {
        DeviceClass::Gpu => matches!(
            device_type,
            wgpu::DeviceType::DiscreteGpu
                | wgpu::DeviceType::IntegratedGpu
                | wgpu::DeviceType::VirtualGpu
        ),
        DeviceClass::DiscreteGpu => device_type == wgpu::DeviceType::DiscreteGpu,
        DeviceClass::IntegratedGpu => device_type == wgpu::DeviceType::IntegratedGpu,
    }
}

fn storage_usage() -> wgpu::BufferUsages {
    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC
}

fn checked_product(values: &[usize], label: &str) -> Result<usize, String> {
    values.iter().try_fold(1usize, |product, value| {
        product
            .checked_mul(*value)
            .ok_or_else(|| format!("{label}: tensor element count overflow"))
    })
}

fn checked_byte_size(len: usize, label: &str) -> Result<u64, String> {
    len.checked_mul(std::mem::size_of::<f32>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| format!("{label}: tensor byte size overflow"))
}

fn checked_u32(value: usize, label: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{label}: dimension {value} exceeds u32"))
}

fn expect_len(buffer: &GpuBuffer, expected: usize, label: &str, role: &str) -> Result<(), String> {
    if buffer.len != expected {
        return Err(format!(
            "{label}: {role} has {} elements, expected {expected}",
            buffer.len
        ));
    }
    Ok(())
}

fn expect_slice_len(
    values: &[f32],
    expected: usize,
    label: &str,
    role: &str,
) -> Result<(), String> {
    if values.len() != expected {
        return Err(format!(
            "{label}: {role} has {} elements, expected {expected}",
            values.len()
        ));
    }
    Ok(())
}

fn dispatch_grid(total: usize) -> Result<(u32, u32, u32), String> {
    let total = u64::try_from(total).map_err(|_| "dispatch size exceeds u64".to_string())?;
    let groups = total.div_ceil(WORKGROUP_SIZE).max(1);
    let x = groups.min(MAX_WORKGROUPS_PER_DIM);
    let y = groups.div_ceil(x);
    if y > MAX_WORKGROUPS_PER_DIM {
        return Err("dispatch exceeds Vulkan two-dimensional workgroup limits".to_string());
    }
    let width = x
        .checked_mul(WORKGROUP_SIZE)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "dispatch width overflow".to_string())?;
    Ok((x as u32, y as u32, width))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LinearParams {
    m: u32,
    k: u32,
    n: u32,
    weight_transposed: u32,
    has_bias: u32,
    total: u32,
    width: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Conv1dParams {
    frames: u32,
    in_channels: u32,
    out_channels: u32,
    kernel: u32,
    padding: u32,
    total: u32,
    width: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DepthwiseConv1dParams {
    frames: u32,
    channels: u32,
    kernel: u32,
    padding: u32,
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Conv2dParams {
    in_channels: u32,
    in_height: u32,
    in_width: u32,
    out_channels: u32,
    out_height: u32,
    out_width: u32,
    kernel_height: u32,
    kernel_width: u32,
    stride_height: u32,
    stride_width: u32,
    pad_height: u32,
    pad_width: u32,
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct UnaryParams {
    total: u32,
    operation: u32,
    width: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GluParams {
    frames: u32,
    input_channels: u32,
    output_channels: u32,
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerNormParams {
    rows: u32,
    channels: u32,
    epsilon: f32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FlatParams {
    total: u32,
    width: u32,
    _pad0: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RowsParams {
    rows: u32,
    channels: u32,
    _pad0: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NchwToNhwcParams {
    channels: u32,
    height: u32,
    width_in: u32,
    total: u32,
    dispatch_width: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct Kernel {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

struct Pipelines {
    linear: Kernel,
    conv1d: Kernel,
    depthwise_conv1d: Kernel,
    conv2d_nchw: Kernel,
    unary: Kernel,
    glu: Kernel,
    layer_norm: Kernel,
    add: Kernel,
    softmax_rows: Kernel,
    nchw_to_nhwc: Kernel,
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            linear: build_kernel(
                device,
                "uta.wgpu.linear",
                include_str!("shaders/linear.wgsl"),
                4,
            ),
            conv1d: build_kernel(
                device,
                "uta.wgpu.conv1d",
                include_str!("shaders/conv1d.wgsl"),
                4,
            ),
            depthwise_conv1d: build_kernel(
                device,
                "uta.wgpu.depthwise_conv1d",
                include_str!("shaders/depthwise_conv1d.wgsl"),
                4,
            ),
            conv2d_nchw: build_kernel(
                device,
                "uta.wgpu.conv2d_nchw",
                include_str!("shaders/conv2d_nchw.wgsl"),
                4,
            ),
            unary: build_kernel(
                device,
                "uta.wgpu.unary",
                include_str!("shaders/unary.wgsl"),
                2,
            ),
            glu: build_kernel(device, "uta.wgpu.glu", include_str!("shaders/glu.wgsl"), 2),
            layer_norm: build_kernel(
                device,
                "uta.wgpu.layer_norm",
                include_str!("shaders/layer_norm.wgsl"),
                4,
            ),
            add: build_kernel(device, "uta.wgpu.add", include_str!("shaders/add.wgsl"), 3),
            softmax_rows: build_kernel(
                device,
                "uta.wgpu.softmax_rows",
                include_str!("shaders/softmax_rows.wgsl"),
                2,
            ),
            nchw_to_nhwc: build_kernel(
                device,
                "uta.wgpu.nchw_to_nhwc",
                include_str!("shaders/nchw_to_nhwc.wgsl"),
                2,
            ),
        }
    }
}

fn build_kernel(device: &wgpu::Device, label: &str, source: &str, storage_count: u32) -> Kernel {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let mut entries = Vec::with_capacity(storage_count as usize + 1);
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    for binding in 1..=storage_count {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding != storage_count,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    Kernel { pipeline, layout }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_wgsl_modules_parse_and_validate_without_a_gpu_context() {
        for (name, source) in [
            ("linear", include_str!("shaders/linear.wgsl")),
            ("conv1d", include_str!("shaders/conv1d.wgsl")),
            (
                "depthwise_conv1d",
                include_str!("shaders/depthwise_conv1d.wgsl"),
            ),
            ("conv2d_nchw", include_str!("shaders/conv2d_nchw.wgsl")),
            ("unary", include_str!("shaders/unary.wgsl")),
            ("glu", include_str!("shaders/glu.wgsl")),
            ("layer_norm", include_str!("shaders/layer_norm.wgsl")),
            ("add", include_str!("shaders/add.wgsl")),
            ("softmax_rows", include_str!("shaders/softmax_rows.wgsl")),
            ("nchw_to_nhwc", include_str!("shaders/nchw_to_nhwc.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|error| panic!("{name} WGSL parse failed: {error:?}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name} WGSL validation failed: {error:?}"));
        }
    }
}
