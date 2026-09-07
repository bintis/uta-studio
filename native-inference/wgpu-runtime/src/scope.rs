use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::GpuDevice;

struct State {
    device: GpuDevice,
    error: Option<String>,
}

thread_local! {
    static ACTIVE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Activates one synchronous GPU device on the current worker thread.
/// Nested scopes are rejected and dropping the guard always releases the
/// thread-local device reference.
pub struct GpuScope {
    active: bool,
    _not_send: PhantomData<Rc<()>>,
}

impl GpuScope {
    pub fn enter(device: GpuDevice) -> Result<Self, String> {
        ACTIVE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                return Err("a WGPU inference scope is already active on this thread".to_string());
            }
            *slot = Some(State {
                device,
                error: None,
            });
            Ok(Self {
                active: true,
                _not_send: PhantomData,
            })
        })
    }

    /// Ends the scope and reports the first GPU failure. Model code using the
    /// scoped host-vector helpers never falls back to CPU after such a
    /// failure; zero-shaped placeholders only preserve control flow until
    /// this boundary returns the recorded error.
    pub fn finish(mut self) -> Result<(), String> {
        let state = ACTIVE.with(|slot| slot.borrow_mut().take());
        self.active = false;
        let state = state.ok_or_else(|| "WGPU inference scope disappeared".to_string())?;
        match state.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for GpuScope {
    fn drop(&mut self) {
        if self.active {
            ACTIVE.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
}

/// Returns `None` when no GPU scope is active. When active, performs a real
/// synchronous Vulkan linear operation and returns `Some`, including after a
/// previously recorded error (in which case a correctly-sized zero buffer is
/// returned until `GpuScope::finish` fails the task).
#[allow(clippy::too_many_arguments)]
pub fn scoped_linear(
    label: &str,
    input: &[f32],
    rows: usize,
    in_dim: usize,
    weight: &[f32],
    bias: Option<&[f32]>,
    out_dim: usize,
    weight_transposed: bool,
) -> Option<Vec<f32>> {
    let expected = rows.saturating_mul(out_dim);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.linear(
            label,
            &input,
            rows,
            in_dim,
            weight,
            bias,
            out_dim,
            weight_transposed,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_bidirectional_gru(
    label: &str,
    input: &[f32],
    frames: usize,
    input_size: usize,
    hidden: usize,
    input_weight: &[f32],
    recurrent_weight: &[f32],
    bias: &[f32],
) -> Option<Vec<f32>> {
    let expected = 2usize.saturating_mul(frames).saturating_mul(hidden);
    run_scoped(expected, |device| {
        let output = device.bidirectional_gru(
            label,
            input,
            frames,
            input_size,
            hidden,
            input_weight,
            recurrent_weight,
            bias,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_conv1d_pytorch(
    label: &str,
    input: &[f32],
    frames: usize,
    in_channels: usize,
    weight: &[f32],
    bias: Option<&[f32]>,
    out_channels: usize,
    kernel: usize,
    groups: usize,
    dilation: usize,
) -> Option<Vec<f32>> {
    let expected = frames.saturating_mul(out_channels);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.conv1d_pytorch(
            label,
            &input,
            frames,
            in_channels,
            weight,
            bias,
            out_channels,
            kernel,
            groups,
            dilation,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_conv_transpose1d(
    label: &str,
    input: &[f32],
    input_frames: usize,
    in_channels: usize,
    weight: &[f32],
    bias: &[f32],
    out_channels: usize,
    kernel: usize,
    stride: usize,
    padding: usize,
    output_frames: usize,
) -> Option<Vec<f32>> {
    let expected = output_frames.saturating_mul(out_channels);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.conv_transpose1d(
            label,
            &input,
            input_frames,
            in_channels,
            weight,
            bias,
            out_channels,
            kernel,
            stride,
            padding,
            output_frames,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_conv_transpose2d_nchw(
    label: &str,
    input: &[f32],
    in_channels: usize,
    in_height: usize,
    in_width: usize,
    weight: &[f32],
    out_channels: usize,
    kernel_height: usize,
    kernel_width: usize,
    stride_height: usize,
    stride_width: usize,
    pad_height: usize,
    pad_width: usize,
    out_height: usize,
    out_width: usize,
) -> Option<Vec<f32>> {
    let expected = out_channels
        .saturating_mul(out_height)
        .saturating_mul(out_width);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.conv_transpose2d_nchw(
            label,
            &input,
            in_channels,
            in_height,
            in_width,
            weight,
            out_channels,
            kernel_height,
            kernel_width,
            stride_height,
            stride_width,
            pad_height,
            pad_width,
            out_height,
            out_width,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_conv2d_nchw(
    label: &str,
    input: &[f32],
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
) -> Option<(Vec<f32>, usize, usize)> {
    let padded_height = in_height.saturating_add(pad_height.saturating_mul(2));
    let padded_width = in_width.saturating_add(pad_width.saturating_mul(2));
    let out_height = padded_height
        .saturating_sub(kernel_height)
        .checked_div(stride_height.max(1))
        .unwrap_or(0)
        .saturating_add(1);
    let out_width = padded_width
        .saturating_sub(kernel_width)
        .checked_div(stride_width.max(1))
        .unwrap_or(0)
        .saturating_add(1);
    let expected = out_channels
        .saturating_mul(out_height)
        .saturating_mul(out_width);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let (output, actual_height, actual_width) = device.conv2d_nchw(
            label,
            &input,
            in_channels,
            in_height,
            in_width,
            weight,
            bias,
            out_channels,
            kernel_height,
            kernel_width,
            stride_height,
            stride_width,
            pad_height,
            pad_width,
        )?;
        if (actual_height, actual_width) != (out_height, out_width) {
            return Err(format!(
                "{label}: GPU convolution returned an unexpected shape"
            ));
        }
        device.download(&format!("{label}.download"), &output)
    })
    .map(|values| (values, out_height, out_width))
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_attention(
    label: &str,
    query: &[f32],
    key: &[f32],
    value: &[f32],
    nonpadding: &[bool],
    query_frames: usize,
    key_frames: usize,
    hidden: usize,
    heads: usize,
) -> Option<Vec<f32>> {
    let expected = query_frames.saturating_mul(hidden);
    run_scoped(expected, |device| {
        let output = device.attention_context(
            label,
            query,
            key,
            value,
            nonpadding,
            query_frames,
            key_frames,
            hidden,
            heads,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn scoped_relative_attention(
    label: &str,
    query: &[f32],
    key: &[f32],
    value: &[f32],
    position: &[f32],
    bias_u: &[f32],
    bias_v: &[f32],
    nonpadding: &[bool],
    frames: usize,
    position_frames: usize,
    hidden: usize,
    heads: usize,
) -> Option<Vec<f32>> {
    let expected = frames.saturating_mul(hidden);
    run_scoped(expected, |device| {
        let output = device.relative_attention_context(
            label,
            query,
            key,
            value,
            position,
            bias_u,
            bias_v,
            nonpadding,
            frames,
            position_frames,
            hidden,
            heads,
        )?;
        device.download(&format!("{label}.download"), &output)
    })
}

pub fn scoped_silu(label: &str, input: &[f32]) -> Option<Vec<f32>> {
    run_scoped(input.len(), |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.silu(label, &input)?;
        device.download(&format!("{label}.download"), &output)
    })
}

pub fn scoped_glu(
    label: &str,
    input: &[f32],
    frames: usize,
    input_channels: usize,
) -> Option<Vec<f32>> {
    let expected = frames.saturating_mul(input_channels / 2);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.glu(label, &input, frames, input_channels)?;
        device.download(&format!("{label}.download"), &output)
    })
}

pub fn scoped_layer_norm(
    label: &str,
    input: &[f32],
    rows: usize,
    channels: usize,
    weight: &[f32],
    bias: &[f32],
    epsilon: f32,
) -> Option<Vec<f32>> {
    let expected = rows.saturating_mul(channels);
    run_scoped(expected, |device| {
        let input = device.upload(&format!("{label}.input"), input)?;
        let output = device.layer_norm(label, &input, rows, channels, weight, bias, epsilon)?;
        device.download(&format!("{label}.download"), &output)
    })
}

fn run_scoped(
    expected_output: usize,
    operation: impl FnOnce(&GpuDevice) -> Result<Vec<f32>, String>,
) -> Option<Vec<f32>> {
    let (device, failed) = ACTIVE.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref()
            .map(|state| (state.device.clone(), state.error.is_some()))
    })?;
    if failed {
        return Some(vec![0.0; expected_output]);
    }
    match operation(&device) {
        Ok(values) => Some(values),
        Err(error) => {
            ACTIVE.with(|slot| {
                if let Some(state) = slot.borrow_mut().as_mut()
                    && state.error.is_none()
                {
                    state.error = Some(error);
                }
            });
            Some(vec![0.0; expected_output])
        }
    }
}
