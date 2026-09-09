use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{
    BufferPtr, ContextPtr, GGML_TYPE_F32, GgmlTensor, GgufInitParams, GgufPtr, ModelApi, TensorPtr,
};
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all handles are owned by this live ROSVOT model load.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

pub const HIDDEN_DIM: usize = 256;
pub const MEL_BINS: usize = 40;
pub const PITCH_CLASSES: usize = 89;

/// Loaded official ROSVOT weights on one explicitly selected GGML backend.
///
/// The migrated GGUF stores native PyTorch row-major bytes with dimensions in
/// GGML fastest-varying order, so graph operations consume weights directly.
pub struct Rosvot {
    pub(crate) backend: GgmlBackendHandle,
    pub(crate) weight_context: ContextPtr,
    weight_buffer: BufferPtr,
}

impl Drop for Rosvot {
    fn drop(&mut self) {
        let api = &self.backend.runtime.model_api;
        if !self.weight_buffer.is_null() {
            ggml!(api, ggml_backend_buffer_free(self.weight_buffer));
            self.weight_buffer = std::ptr::null_mut();
        }
        if !self.weight_context.is_null() {
            ggml!(api, ggml_free(self.weight_context));
            self.weight_context = std::ptr::null_mut();
        }
    }
}

impl Rosvot {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        if !model_path.is_file() {
            return Err("ROSVOT GGUF is unavailable".to_string());
        }
        let backend = runtime.create_backend(device)?;
        let mut model = Self {
            backend,
            weight_context: std::ptr::null_mut(),
            weight_buffer: std::ptr::null_mut(),
        };
        model.load_weights(model_path)?;
        Ok(model)
    }

    pub(crate) fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    pub(crate) fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        let canonical = canonical_tensor_name(name);
        let encoded = CString::new(canonical.as_str())
            .map_err(|_| "ROSVOT tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("ROSVOT GGUF tensor is missing: {name}"))
        } else {
            Ok(tensor)
        }
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "ROSVOT GGUF path")?;
        let mut context = std::ptr::null_mut();
        let gguf = ggml!(
            self.api(),
            gguf_init_from_file(
                encoded.as_ptr(),
                GgufInitParams {
                    no_alloc: true,
                    ctx: &mut context,
                }
            )
        );
        if gguf.is_null() || context.is_null() {
            return Err("could not open ROSVOT GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        if self.required_string(gguf, "general.architecture")? != "rosvot" {
            return Err("GGUF general.architecture is not rosvot".to_string());
        }
        self.validate_anchor_shapes()?;
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        self.weight_buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if self.weight_buffer.is_null() {
            return Err("could not allocate ROSVOT GGML weight buffer".to_string());
        }
        self.upload_tensors(path, gguf)
    }

    fn validate_anchor_shapes(&self) -> Result<(), String> {
        for (name, shape) in [
            ("mel_proj.weight", &[3, 40, 256][..]),
            ("mel_proj.bias", &[256][..]),
            ("pitch_embed.weight", &[256, 300][..]),
            ("uv_embed.weight", &[256, 3][..]),
            ("word_bd_embed.weight", &[256, 3][..]),
            ("cond_encoder.post_net1.weight", &[3, 256, 256][..]),
            (
                "net.net.mid.net.encoder_layers.0.feed_forward.w_1.weight",
                &[1, 256, 1024][..],
            ),
            ("note_bd_out.weight", &[256][..]),
            ("pitch_decoder.multihead_dot_attn.weight", &[256, 4][..]),
            ("pitch_decoder.pitch_out.weight", &[256, 89][..]),
        ] {
            self.require_shape(name, shape)?;
        }
        Ok(())
    }

    fn require_shape(&self, name: &str, expected: &[i64]) -> Result<(), String> {
        let tensor = tensor_ref(self.weight(name)?)?;
        let dimensions = tensor
            .ne
            .iter()
            .rposition(|dimension| *dimension != 1)
            .map_or(1, |last| last + 1);
        if tensor.type_ != GGML_TYPE_F32 || &tensor.ne[..dimensions] != expected {
            return Err(format!(
                "ROSVOT GGUF tensor shape mismatch: {name}; expected {expected:?}, found {:?}",
                &tensor.ne[..dimensions]
            ));
        }
        Ok(())
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let data_offset = ggml!(self.api(), gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen ROSVOT GGUF: {error}"))?;
        let mut tensor = ggml!(self.api(), ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            let name = tensor_name(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!("ROSVOT tensor is not F32: {name}"));
            }
            let encoded = CString::new(name.as_str())
                .map_err(|_| "ROSVOT tensor name contains NUL".to_string())?;
            let tensor_index = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_index < 0 {
                return Err(format!("ROSVOT tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(self.api(), ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(
                    self.api(),
                    gguf_get_tensor_offset(gguf, tensor_index)
                ))
                .ok_or_else(|| "ROSVOT tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek ROSVOT tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read ROSVOT tensor {name}: {error}"))?;
            ggml!(
                self.api(),
                ggml_backend_tensor_set(tensor, bytes.as_ptr().cast::<c_void>(), 0, size)
            );
            tensor = ggml!(
                self.api(),
                ggml_get_next_tensor(self.weight_context, tensor)
            );
        }
        Ok(())
    }

    fn required_string(&self, gguf: GgufPtr, key: &str) -> Result<String, String> {
        let raw = ggml!(
            self.api(),
            gguf_get_val_str(gguf, self.key_index(gguf, key)?)
        );
        if raw.is_null() {
            return Err(format!("ROSVOT GGUF string metadata is null: {key}"));
        }
        // SAFETY: GGML owns a NUL-terminated metadata string while `gguf` lives.
        unsafe { CStr::from_ptr(raw) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("ROSVOT GGUF string metadata is not UTF-8: {key}"))
    }

    fn key_index(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "ROSVOT GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api(), gguf_find_key(gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("ROSVOT GGUF is missing {key}"))
        } else {
            Ok(index)
        }
    }
}

pub(crate) fn canonical_tensor_name(name: &str) -> String {
    [
        ("feed_forward_macaron", "ffm"),
        ("feed_forward", "ff"),
        ("encoder_layers", "el"),
        ("multihead_dot_attn", "mda"),
        ("multihead_attn", "mha"),
        ("conv_module", "cm"),
        ("pointwise_conv1", "pw1"),
        ("pointwise_conv2", "pw2"),
        ("depthwise_conv", "dw"),
        ("norm_ff_macaron", "nfm"),
    ]
    .into_iter()
    .fold(name.to_string(), |value, (from, to)| {
        value.replace(from, to)
    })
}

pub(crate) fn tensor_ref(tensor: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers use model/context-owned tensor pointers while those owners live.
    unsafe { tensor.as_ref() }.ok_or_else(|| "GGML returned a null ROSVOT tensor".to_string())
}

fn tensor_name(tensor: TensorPtr) -> Result<String, String> {
    let name = tensor_ref(tensor)?.name;
    // SAFETY: GGML tensor names are fixed NUL-terminated arrays.
    unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "ROSVOT tensor name is not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_rosvot_profile_has_expected_dimensions() {
        assert_eq!(HIDDEN_DIM, 256);
        assert_eq!(MEL_BINS, 40);
        assert_eq!(PITCH_CLASSES, 89);
    }

    #[test]
    fn tensor_names_match_the_rewritten_container() {
        let original = "net.net.mid.net.encoder_layers.0.feed_forward_macaron.w_1.weight";
        let canonical = "net.net.mid.net.el.0.ffm.w_1.weight";
        assert_eq!(canonical_tensor_name(original), canonical);
        assert!(canonical.len() < 64);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and ROSVOT GGUF"]
    fn rosvot_gguf_loads_on_explicit_device() {
        let runtime_path = std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_GGML_RUNTIME_DIR");
        let model_path = std::env::var_os("UTA_TEST_ROSVOT_GGUF")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_ROSVOT_GGUF");
        let requested_kind = std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND to cpu or integrated_gpu");
        let description_filter = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => crate::DeviceKind::Cpu,
            "integrated_gpu" => crate::DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description_filter)
            })
            .expect("requested GGML test device is unavailable");
        let _model = Rosvot::load(runtime, &device, &model_path).unwrap();
        eprintln!(
            "loaded ROSVOT on {} ({:?})",
            device.description, device.kind
        );
    }
}
