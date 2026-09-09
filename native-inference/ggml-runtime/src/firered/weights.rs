use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{BufferPtr, ContextPtr, GgmlTensor, GgufInitParams, GgufPtr, ModelApi, TensorPtr};
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all pointers belong to this live FireRed model and GGUF load.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

/// FireRedASR2-AED weights resident on one explicitly selected GGML backend.
pub struct FireRed {
    pub(crate) backend: GgmlBackendHandle,
    pub(crate) weight_context: ContextPtr,
    weight_buffer: BufferPtr,
}

impl Drop for FireRed {
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

impl FireRed {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
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
        let encoded =
            CString::new(name).map_err(|_| "FireRed tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("FireRed GGUF tensor is missing: {name}"))
        } else {
            Ok(tensor)
        }
    }

    /// Returns the tensor when the GGUF defines it. FireRed's reference
    /// modules load a `Linear` as weight plus optional bias, so a projection
    /// whose checkpoint carries no bias simply has no tensor to look up.
    pub(crate) fn optional_weight(&self, name: &str) -> Result<Option<TensorPtr>, String> {
        let encoded =
            CString::new(name).map_err(|_| "FireRed tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        Ok((!tensor.is_null()).then_some(tensor))
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "FireRed GGUF path")?;
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
            if !gguf.is_null() {
                ggml!(self.api(), gguf_free(gguf));
            }
            if !context.is_null() {
                ggml!(self.api(), ggml_free(context));
            }
            return Err("could not open FireRed GGUF weights through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        self.validate_architecture(gguf)?;
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
            return Err("could not allocate FireRed GGML weight buffer".to_string());
        }
        self.upload_tensors(path, gguf)
    }

    fn validate_architecture(&self, gguf: GgufPtr) -> Result<(), String> {
        let key = CString::new("general.architecture").unwrap();
        let index = ggml!(self.api(), gguf_find_key(gguf, key.as_ptr()));
        if index < 0 {
            return Err("FireRed GGUF has no general.architecture".to_string());
        }
        let value = ggml!(self.api(), gguf_get_val_str(gguf, index));
        if value.is_null() {
            return Err("FireRed GGUF architecture is not a string".to_string());
        }
        // SAFETY: GGUF owns this NUL-terminated string until `gguf_free`.
        let architecture = unsafe { CStr::from_ptr(value) }
            .to_str()
            .map_err(|_| "FireRed GGUF architecture is not UTF-8".to_string())?;
        if architecture != "firered_asr2_aed" {
            return Err(format!(
                "FireRed GGUF architecture is incompatible: {architecture}"
            ));
        }
        Ok(())
    }

    fn validate_anchor_shapes(&self) -> Result<(), String> {
        self.require_shape("encoder.input_preprocessor.conv.0.weight", &[3, 3, 1, 32])?;
        self.require_shape("encoder.input_preprocessor.conv.2.weight", &[3, 3, 32, 32])?;
        self.require_shape("encoder.input_preprocessor.out.weight", &[608, 1_280])?;
        self.require_shape("encoder.layer_stack.0.ffn1.net.1.weight", &[1_280, 5_120])?;
        self.require_shape("decoder.tgt_word_emb.weight", &[1_280, 8_667])?;
        self.require_shape(
            "decoder.layer_stack.0.self_attn.w_qs.weight",
            &[1_280, 1_280],
        )?;
        self.require_shape("decoder.tgt_word_prj.weight", &[1_280, 8_667])?;
        Ok(())
    }

    fn require_shape(&self, name: &str, expected: &[i64]) -> Result<(), String> {
        let tensor = tensor_ref(self.weight(name)?)?;
        let dimensions = tensor
            .ne
            .iter()
            .rposition(|dimension| *dimension != 1)
            .map_or(1, |last| last + 1);
        if &tensor.ne[..dimensions] != expected {
            Err(format!(
                "FireRed GGUF tensor shape mismatch: {name}; expected {expected:?}, found {:?}",
                &tensor.ne[..dimensions]
            ))
        } else {
            Ok(())
        }
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let data_offset = ggml!(self.api(), gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen FireRed GGUF: {error}"))?;
        let mut tensor = ggml!(self.api(), ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "FireRed tensor name contains NUL".to_string())?;
            let tensor_index = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_index < 0 {
                return Err(format!("FireRed tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(self.api(), ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(
                    self.api(),
                    gguf_get_tensor_offset(gguf, tensor_index)
                ))
                .ok_or_else(|| "FireRed tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek FireRed tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read FireRed tensor {name}: {error}"))?;
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
}

pub(crate) fn tensor_ref(tensor: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers pass a live GGML-owned tensor and keep its context alive.
    unsafe { tensor.as_ref() }.ok_or_else(|| "FireRed tensor pointer is null".to_string())
}

fn tensor_name(tensor: TensorPtr) -> Result<String, String> {
    let descriptor = tensor_ref(tensor)?;
    // SAFETY: GGML tensor names are inline NUL-terminated C strings.
    unsafe { CStr::from_ptr(descriptor.name.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "FireRed tensor name is not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("set {name}"))
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and rewritten FireRed F32 GGUF"]
    fn actual_firered_model_loads_on_selected_backend() {
        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let requested_kind =
            std::env::var("UTA_TEST_GGML_DEVICE_KIND").expect("set UTA_TEST_GGML_DEVICE_KIND");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            "discrete_gpu" => DeviceKind::DiscreteGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested FireRed test device is unavailable");
        let model = FireRed::load(runtime, &device, &path("UTA_TEST_FIRERED_GGUF")).unwrap();
        assert!(
            !model
                .weight("decoder.tgt_word_prj.weight")
                .unwrap()
                .is_null()
        );
    }
}
