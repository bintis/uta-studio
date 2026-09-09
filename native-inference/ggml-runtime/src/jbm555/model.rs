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
        // SAFETY: all handles are owned by this live JBM555 model load.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

/// Loaded JBM555 CE+CTC weights on one explicitly selected GGML backend.
pub struct Jbm555 {
    pub(crate) backend: GgmlBackendHandle,
    pub(crate) weight_context: ContextPtr,
    weight_buffer: BufferPtr,
}

impl Drop for Jbm555 {
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

impl Jbm555 {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        if !model_path.is_file() {
            return Err("JBM555 GGUF is unavailable".to_string());
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
        let encoded =
            CString::new(name).map_err(|_| "JBM555 tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("JBM555 GGUF tensor is missing: {name}"))
        } else {
            Ok(tensor)
        }
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "JBM555 GGUF path")?;
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
            return Err("could not open JBM555 GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        if self.required_string(gguf, "general.architecture")? != "jbm555" {
            return Err("GGUF general.architecture is not jbm555".to_string());
        }
        for (key, expected) in [
            ("sample_rate", 44_100),
            ("hop_size", 1_024),
            ("bins", 384),
            ("channels", 6),
        ] {
            if self.required_u32(gguf, key)? != expected {
                return Err(format!("JBM555 GGUF metadata mismatch: {key}"));
            }
        }
        if ggml!(self.api(), gguf_get_n_tensors(gguf)) != 32 {
            return Err("JBM555 GGUF tensor count is not 32".to_string());
        }
        self.validate_shapes()?;
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        self.weight_buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if self.weight_buffer.is_null() {
            return Err("could not allocate JBM555 GGML weight buffer".to_string());
        }
        self.upload_tensors(path, gguf)
    }

    fn validate_shapes(&self) -> Result<(), String> {
        for branch in ["onset_cnn", "pitch_cnn"] {
            for (layer, input, output) in [
                (1, 6, 16),
                (2, 16, 32),
                (3, 32, 32),
                (4, 32, 32),
                (5, 32, 32),
            ] {
                self.require_shape(
                    &format!("{branch}.conv{layer}.weight"),
                    &[9, 9, input, output],
                )?;
                self.require_shape(&format!("{branch}.conv{layer}.bias"), &[output])?;
            }
            let final_output = if branch == "onset_cnn" { 4 } else { 18 };
            for (layer, input, output) in [(1, 3_072, 64), (2, 64, 32), (3, 32, final_output)] {
                self.require_shape(&format!("{branch}.fc{layer}.weight"), &[input, output])?;
                self.require_shape(&format!("{branch}.fc{layer}.bias"), &[output])?;
            }
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
                "JBM555 GGUF tensor shape mismatch: {name}; expected {expected:?}, found {:?}",
                &tensor.ne[..dimensions]
            ));
        }
        Ok(())
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let data_offset = ggml!(self.api(), gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen JBM555 GGUF: {error}"))?;
        let mut tensor = ggml!(self.api(), ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            let name = tensor_name(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!("JBM555 tensor is not F32: {name}"));
            }
            let encoded = CString::new(name.as_str())
                .map_err(|_| "JBM555 tensor name contains NUL".to_string())?;
            let tensor_index = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_index < 0 {
                return Err(format!("JBM555 tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(self.api(), ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(
                    self.api(),
                    gguf_get_tensor_offset(gguf, tensor_index)
                ))
                .ok_or_else(|| "JBM555 tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek JBM555 tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read JBM555 tensor {name}: {error}"))?;
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
            return Err(format!("JBM555 GGUF string metadata is null: {key}"));
        }
        // SAFETY: GGML owns a NUL-terminated metadata string while `gguf` lives.
        unsafe { CStr::from_ptr(raw) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("JBM555 GGUF string metadata is not UTF-8: {key}"))
    }

    fn required_u32(&self, gguf: GgufPtr, key: &str) -> Result<u32, String> {
        Ok(ggml!(
            self.api(),
            gguf_get_val_u32(gguf, self.key_index(gguf, key)?)
        ))
    }

    fn key_index(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "JBM555 GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api(), gguf_find_key(gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("JBM555 GGUF is missing {key}"))
        } else {
            Ok(index)
        }
    }
}

pub(crate) fn tensor_ref(tensor: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers use model/context-owned tensor pointers while those owners live.
    unsafe { tensor.as_ref() }.ok_or_else(|| "GGML returned a null JBM555 tensor".to_string())
}

fn tensor_name(tensor: TensorPtr) -> Result<String, String> {
    let name = tensor_ref(tensor)?.name;
    // SAFETY: GGML tensor names are fixed NUL-terminated arrays.
    unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "JBM555 tensor name is not UTF-8".to_string())
}
