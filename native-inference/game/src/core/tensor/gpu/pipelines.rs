use wgpu::util::DeviceExt;

use crate::{Error, Result};

/// One compiled pipeline plus the bind group layout it was built from, so
/// callers can build a matching bind group without re-deriving the layout.
pub(super) struct Kernel {
    pub(super) pipeline: wgpu::ComputePipeline,
    pub(super) layout: wgpu::BindGroupLayout,
}

pub(super) struct Pipelines {
    pub(super) binary: Kernel,
    pub(super) unary: Kernel,
    pub(super) softmax: Kernel,
    pub(super) rms_norm: Kernel,
    pub(super) matmul: Kernel,
    pub(super) conv1d_dw: Kernel,
    pub(super) rope: Kernel,
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn build_kernel(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
) -> Kernel {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries,
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

impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let binary = build_kernel(
            device,
            "game.gpu.binary",
            include_str!("shaders/binary.wgsl"),
            &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        );
        let unary = build_kernel(
            device,
            "game.gpu.unary",
            include_str!("shaders/unary.wgsl"),
            &[uniform_entry(0), storage_entry(1, true), storage_entry(2, false)],
        );
        let softmax = build_kernel(
            device,
            "game.gpu.softmax",
            include_str!("shaders/softmax.wgsl"),
            &[uniform_entry(0), storage_entry(1, false)],
        );
        let rms_norm = build_kernel(
            device,
            "game.gpu.rms_norm",
            include_str!("shaders/rms_norm.wgsl"),
            &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        );
        let matmul = build_kernel(
            device,
            "game.gpu.matmul",
            include_str!("shaders/matmul.wgsl"),
            &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        );
        let conv1d_dw = build_kernel(
            device,
            "game.gpu.conv1d_dw",
            include_str!("shaders/conv1d_dw.wgsl"),
            &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, true),
                storage_entry(4, false),
            ],
        );
        let rope = build_kernel(
            device,
            "game.gpu.rope",
            include_str!("shaders/rope.wgsl"),
            &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        );
        Self {
            binary,
            unary,
            softmax,
            rms_norm,
            matmul,
            conv1d_dw,
            rope,
        }
    }
}

pub(super) fn uniform_buffer(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

pub(super) fn storage_buffer_init(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let usage = wgpu::BufferUsages::STORAGE
        | wgpu::BufferUsages::COPY_DST
        | wgpu::BufferUsages::COPY_SRC;
    if bytes.is_empty() {
        return device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: 4,
            usage,
            mapped_at_creation: false,
        });
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage,
    })
}

pub(super) fn storage_buffer_zeroed(device: &wgpu::Device, label: &str, len_bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: len_bytes.max(4),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// Downloads the entire contents of `buffer` (its declared `size`, not a
/// logical tensor footprint) into a freshly allocated `Vec<f32>`, via a
/// staging copy — `STORAGE` buffers are not guaranteed host-mappable, so
/// reading one always goes through a `MAP_READ`-capable staging buffer.
pub(super) fn download_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
) -> Result<Vec<f32>> {
    let size = buffer.size();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("game.gpu.download_staging"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| Error::message(format!("GPU poll during download failed: {error:?}")))?;
    receiver
        .recv()
        .map_err(|error| Error::message(format!("GPU download channel closed: {error}")))?
        .map_err(|error| Error::message(format!("GPU buffer map failed: {error:?}")))?;

    let data = slice.get_mapped_range();
    let values: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    staging.unmap();
    Ok(values)
}
