use bytemuck::{Pod, Zeroable};

use super::{GpuBuffer, GpuDevice, checked_product, checked_u32, expect_slice_len};

const MAX_GRU_HIDDEN: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GruParams {
    frames: u32,
    hidden: u32,
    _pad0: u32,
    _pad1: u32,
}

impl GpuDevice {
    /// Runs a PyTorch-layout bidirectional GRU while keeping recurrent state
    /// on the device. Input weights are `[2, 3H, I]`, recurrent weights are
    /// `[2, 3H, H]`, biases are `[2, 6H]`, and output is `[2, T, H]`.
    #[allow(clippy::too_many_arguments)]
    pub fn bidirectional_gru(
        &self,
        label: &str,
        input: &[f32],
        frames: usize,
        input_size: usize,
        hidden: usize,
        input_weight: &[f32],
        recurrent_weight: &[f32],
        bias: &[f32],
    ) -> Result<GpuBuffer, String> {
        if frames == 0 || input_size == 0 || hidden == 0 || hidden > MAX_GRU_HIDDEN {
            return Err(format!(
                "{label}: GRU dimensions must be non-zero and hidden size must not exceed {MAX_GRU_HIDDEN}"
            ));
        }
        let gates = checked_product(&[3, hidden], label)?;
        let input_stage = checked_product(&[gates, input_size], label)?;
        let recurrent_stage = checked_product(&[gates, hidden], label)?;
        expect_slice_len(
            input,
            checked_product(&[frames, input_size], label)?,
            label,
            "input",
        )?;
        expect_slice_len(
            input_weight,
            checked_product(&[2, input_stage], label)?,
            label,
            "input weight",
        )?;
        expect_slice_len(
            recurrent_weight,
            checked_product(&[2, recurrent_stage], label)?,
            label,
            "recurrent weight",
        )?;
        expect_slice_len(
            bias,
            checked_product(&[2, 6, hidden], label)?,
            label,
            "bias",
        )?;

        let input = self.upload(&format!("{label}.input"), input)?;
        let projected_forward = self.linear(
            &format!("{label}.input_forward"),
            &input,
            frames,
            input_size,
            &input_weight[..input_stage],
            None,
            gates,
            true,
        )?;
        let projected_backward = self.linear(
            &format!("{label}.input_backward"),
            &input,
            frames,
            input_size,
            &input_weight[input_stage..],
            None,
            gates,
            true,
        )?;
        let recurrent_weight =
            self.upload(&format!("{label}.recurrent_weight"), recurrent_weight)?;
        let bias = self.upload(&format!("{label}.bias"), bias)?;
        let output = self.zeroed(
            &format!("{label}.output"),
            checked_product(&[2, frames, hidden], label)?,
        )?;
        let params = GruParams {
            frames: checked_u32(frames, label)?,
            hidden: checked_u32(hidden, label)?,
            _pad0: 0,
            _pad1: 0,
        };
        self.run_kernel(
            &self.inner.pipelines.bidirectional_gru,
            label,
            bytemuck::bytes_of(&params),
            &[
                &projected_forward,
                &projected_backward,
                &recurrent_weight,
                &bias,
                &output,
            ],
            (2, 1, 1),
        )?;
        Ok(output)
    }
}
