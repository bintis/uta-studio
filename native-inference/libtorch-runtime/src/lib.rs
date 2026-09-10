//! Independent native ATen model execution. Loading this Rust crate does not
//! link, locate, download or initialize a native runtime. The caller supplies
//! one explicit library, backend, device and precision for each model session.
mod ffi;
#[path = "../../ggml-runtime/src/acceleration.rs"]
pub mod acceleration;
#[path = "../../ggml-runtime/src/stft.rs"]
mod stft;
#[path = "../../ggml-runtime/src/wav.rs"]
mod wav;
#[path = "../../ggml-runtime/src/stage_profile.rs"]
mod stage_profile;
pub mod roformer;
pub mod game;
pub mod jbm555;
pub mod stars;
pub mod rosvot;
mod pitch;
pub use pitch::{basic_pitch, fcpe, rmvpe};

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend { LibtorchRocm, LibtorchXpu, LibtorchCpu }
impl Backend {
    pub fn name(self) -> &'static str {
        match self { Self::LibtorchRocm => "libtorch_rocm", Self::LibtorchXpu => "libtorch_xpu", Self::LibtorchCpu => "libtorch_cpu" }
    }
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "libtorch_rocm" => Ok(Self::LibtorchRocm), "libtorch_xpu" => Ok(Self::LibtorchXpu),
            "libtorch_cpu" => Ok(Self::LibtorchCpu), _ => Err(format!("unknown explicit LibTorch backend: {name}")),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision { Strict, MixedAttention }
impl Precision {
    pub fn name(self) -> &'static str { match self { Self::Strict => "strict", Self::MixedAttention => "mixed_attention" } }
    pub fn parse(name: &str) -> Result<Self, String> {
        match name { "strict" => Ok(Self::Strict), "mixed_attention" => Ok(Self::MixedAttention),
            _ => Err(format!("unknown explicit LibTorch precision: {name}")) }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildInfo {
    pub torch_version: String,
    pub compiled_backend: String,
    pub roformer_projection_math: String,
    pub models: Vec<String>,
    pub qualification: String,
}
#[derive(Clone)]
pub struct Library { api: Arc<ffi::Api>, info: BuildInfo }
impl Library {
    /// Opens only the named native library and reads its build capabilities.
    /// This call does not select or initialize an accelerator device.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() { return Err(format!("native LibTorch library is not installed: {}", path.display())); }
        let api = Arc::new(ffi::Api::load(path)?);
        // SAFETY: symbols were loaded with their declared ABI, and the API owns
        // the library while its immutable build-info string is copied.
        let info = unsafe { copy_text((api.build_info)(), "native build capabilities")? };
        let info: BuildInfo = serde_json::from_str(&info).map_err(|error| format!("invalid LibTorch build capabilities: {error}"))?;
        Ok(Self { api, info })
    }
    pub fn build_info(&self) -> &BuildInfo { &self.info }
    pub fn open(&self, resource: &str, model_path: &Path, backend: Backend, device: u16, precision: Precision) -> Result<Model, String> {
        if !self.info.models.iter().any(|item| item == resource) {
            return Err(format!("native library has no executable model plan for {resource}"));
        }
        if backend != Backend::LibtorchCpu && self.info.compiled_backend != backend.name() {
            return Err(format!("requested {} but native library provides {}; no backend fallback", backend.name(), self.info.compiled_backend));
        }
        let backend_name = cstring(backend.name())?;
        let precision_name = cstring(precision.name())?;
        let resource_name = cstring(resource)?;
        let path = model_path.to_str().ok_or_else(|| "native model path is not UTF-8".to_string())?;
        let path = cstring(path)?;
        // SAFETY: all C strings are valid for these synchronous calls. Both
        // native handles retain ownership independently of caller strings.
        unsafe {
            let runtime = NonNull::new((self.api.runtime_create)(backend_name.as_ptr(), i32::from(device), precision_name.as_ptr()))
                .ok_or_else(|| self.api.error())?;
            let pointer = (self.api.model_open)(runtime.as_ptr(), resource_name.as_ptr(), path.as_ptr());
            // Copy the error before freeing another object can overwrite TLS.
            let error = if pointer.is_null() { Some(self.api.error()) } else { None };
            (self.api.runtime_free)(runtime.as_ptr());
            if let Some(error) = error { return Err(error); }
            let handle = NonNull::new(pointer).expect("null model returned a copied error");
            let inner = Arc::new(ModelInner { api: Arc::clone(&self.api), handle });
            let metadata = copy_text((self.api.model_metadata)(handle.as_ptr()), "native model metadata")?;
            let metadata = serde_json::from_str(&metadata).map_err(|error| format!("invalid native GGUF metadata JSON: {error}"))?;
            Ok(Model { inner, resource: resource.to_string(), backend, device, precision, metadata })
        }
    }
}

struct ModelInner { api: Arc<ffi::Api>, handle: NonNull<ffi::Model> }
// SAFETY: native forward calls are serialized by the model's mutex;
// cancellation is atomic. Arc prevents free until the last call/handle ends.
unsafe impl Send for ModelInner {}
unsafe impl Sync for ModelInner {}
impl Drop for ModelInner {
    fn drop(&mut self) {
        // SAFETY: this is the last owner; the retained API outlives destruction.
        unsafe { (self.api.model_free)(self.handle.as_ptr()); }
    }
}

#[derive(Clone)]
pub struct Model {
    inner: Arc<ModelInner>,
    pub resource: String,
    pub backend: Backend,
    pub device: u16,
    pub precision: Precision,
    pub metadata: serde_json::Value,
}
impl Model {
    /// Set cancellation without racing model destruction. Reset explicitly
    /// before starting another independent request on this session.
    pub fn cancel(&self, cancelled: bool) {
        // SAFETY: Arc owns the handle, and the native cancellation flag is atomic.
        unsafe { (self.inner.api.model_cancel)(self.inner.handle.as_ptr(), i32::from(cancelled)); }
    }
    pub fn forward(&self, operation: &str, inputs: &[Input<'_>]) -> Result<Output, String> {
        let operation = cstring(operation)?;
        let names = inputs.iter().map(|input| cstring(input.name)).collect::<Result<Vec<_>, _>>()?;
        let mut native = Vec::with_capacity(inputs.len());
        for (input, name) in inputs.iter().zip(&names) {
            let elements = element_count(input.shape)?;
            if elements != input.data.len() { return Err(format!("native input {} shape and buffer disagree", input.name)); }
            native.push(ffi::Tensor {
                name: name.as_ptr(), data: input.data.pointer(), dimensions: input.shape.as_ptr(),
                elements: elements as u64, rank: u32::try_from(input.shape.len()).map_err(|_| "native tensor rank exceeds ABI".to_string())?,
                kind: input.data.kind(),
            });
        }
        // SAFETY: names, dimensions and typed data live through the call;
        // native execution is synchronous and copies all retained inputs.
        let pointer = unsafe { (self.inner.api.model_forward)(self.inner.handle.as_ptr(), operation.as_ptr(), native.as_ptr(), native.len()) };
        let handle = NonNull::new(pointer).ok_or_else(|| self.inner.api.error())?;
        let result = ResultGuard { api: Arc::clone(&self.inner.api), handle };
        result.copy()
    }
    pub fn integer(&self, key: &str) -> Result<i64, String> {
        self.metadata.get(key).and_then(serde_json::Value::as_i64).ok_or_else(|| format!("missing integer GGUF metadata: {key}"))
    }
    pub fn number(&self, key: &str) -> Result<f64, String> {
        self.metadata.get(key).and_then(serde_json::Value::as_f64).ok_or_else(|| format!("missing numeric GGUF metadata: {key}"))
    }
}

pub enum Data<'a> { F32(&'a [f32]), I64(&'a [i64]) }
impl Data<'_> {
    fn len(&self) -> usize { match self { Self::F32(value) => value.len(), Self::I64(value) => value.len() } }
    fn pointer(&self) -> *const std::ffi::c_void { match self { Self::F32(value) => value.as_ptr().cast(), Self::I64(value) => value.as_ptr().cast() } }
    fn kind(&self) -> u32 { match self { Self::F32(_) => 0, Self::I64(_) => 1 } }
}
pub struct Input<'a> { pub name: &'a str, pub shape: &'a [i64], pub data: Data<'a> }
impl<'a> Input<'a> {
    pub fn f32(name: &'a str, shape: &'a [i64], data: &'a [f32]) -> Self { Self { name, shape, data: Data::F32(data) } }
    pub fn i64(name: &'a str, shape: &'a [i64], data: &'a [i64]) -> Self { Self { name, shape, data: Data::I64(data) } }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "dtype", content = "values", rename_all = "snake_case")]
pub enum Values { F32(Vec<f32>), I64(Vec<i64>) }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tensor { pub shape: Vec<i64>, pub data: Values }
impl Tensor {
    pub fn f32(&self) -> Result<&[f32], String> { match &self.data { Values::F32(value) => Ok(value), _ => Err("native output is not float32".to_string()) } }
    pub fn i64(&self) -> Result<&[i64], String> { match &self.data { Values::I64(value) => Ok(value), _ => Err("native output is not int64".to_string()) } }
    pub fn into_f32(self) -> Result<Vec<f32>, String> { match self.data { Values::F32(value) => Ok(value), _ => Err("native output is not float32".to_string()) } }
    pub fn input<'a>(&'a self, name: &'a str) -> Input<'a> {
        Input { name, shape: &self.shape, data: match &self.data { Values::F32(value) => Data::F32(value), Values::I64(value) => Data::I64(value) } }
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[repr(C)]
pub struct Timings { pub upload_seconds: f64, pub synchronized_compute_seconds: f64, pub readback_seconds: f64 }
#[derive(Debug, Serialize, Deserialize)]
pub struct Output { pub tensors: BTreeMap<String, Tensor>, pub timings: Timings }
impl Output {
    pub fn get(&self, name: &str) -> Result<&Tensor, String> { self.tensors.get(name).ok_or_else(|| format!("native output lacks {name}")) }
    pub fn take(&mut self, name: &str) -> Result<Tensor, String> { self.tensors.remove(name).ok_or_else(|| format!("native output lacks {name}")) }
}
struct ResultGuard { api: Arc<ffi::Api>, handle: NonNull<ffi::ResultHandle> }
impl Drop for ResultGuard {
    fn drop(&mut self) {
        // SAFETY: last result owner; API stays loaded through result destruction.
        unsafe { (self.api.result_free)(self.handle.as_ptr()); }
    }
}
impl ResultGuard {
    fn copy(&self) -> Result<Output, String> {
        let mut tensors = BTreeMap::new();
        // SAFETY: all views are borrowed from a live owned result. Shapes and
        // buffer counts are checked before converting native slices to owned Rust.
        unsafe {
            let count = (self.api.result_count)(self.handle.as_ptr());
            for index in 0..count {
                let mut view = ffi::Tensor::default();
                if (self.api.result_tensor)(self.handle.as_ptr(), index, &mut view) != 0 { return Err(self.api.error()); }
                let name = copy_text(view.name, "native tensor name")?;
                let shape = if view.rank == 0 { Vec::new() } else {
                    if view.dimensions.is_null() { return Err("native output dimensions are null".to_string()); }
                    std::slice::from_raw_parts(view.dimensions, view.rank as usize).to_vec()
                };
                let elements = element_count(&shape)?;
                if u64::try_from(elements).ok() != Some(view.elements) || (elements != 0 && view.data.is_null()) {
                    return Err("native output shape and buffer disagree".to_string());
                }
                let width = match view.kind { 0 => size_of::<f32>(), 1 => size_of::<i64>(), _ => return Err("native output dtype is unsupported".to_string()) };
                if elements.checked_mul(width).is_none_or(|bytes| bytes > isize::MAX as usize) { return Err("native output buffer exceeds address range".to_string()); }
                let data = match view.kind {
                    0 => Values::F32(if elements == 0 { Vec::new() } else { std::slice::from_raw_parts(view.data.cast(), elements).to_vec() }),
                    1 => Values::I64(if elements == 0 { Vec::new() } else { std::slice::from_raw_parts(view.data.cast(), elements).to_vec() }),
                    _ => unreachable!(),
                };
                if tensors.insert(name.clone(), Tensor { shape, data }).is_some() { return Err(format!("duplicate native result tensor: {name}")); }
            }
            let mut timings = Timings::default();
            if (self.api.result_timings)(self.handle.as_ptr(), &mut timings) != 0 { return Err("native result timings unavailable".to_string()); }
            Ok(Output { tensors, timings })
        }
    }
}
fn cstring(value: &str) -> Result<CString, String> { CString::new(value).map_err(|_| "native string contains an embedded NUL".to_string()) }
unsafe fn copy_text(pointer: *const std::ffi::c_char, description: &str) -> Result<String, String> {
    if pointer.is_null() { return Err(format!("{description} is null")); }
    // SAFETY: the caller guarantees the pointer is a live native NUL-terminated string.
    unsafe { CStr::from_ptr(pointer) }.to_str().map(str::to_string).map_err(|error| format!("{description} is not UTF-8: {error}"))
}
fn element_count(shape: &[i64]) -> Result<usize, String> {
    shape.iter().try_fold(1usize, |count, dimension| {
        let dimension = usize::try_from(*dimension).map_err(|_| "native tensor dimension is negative or too large".to_string())?;
        count.checked_mul(dimension).filter(|value| *value <= isize::MAX as usize)
            .ok_or_else(|| "native tensor shape exceeds address range".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn shapes_reject_negative_and_overflow() {
        assert_eq!(element_count(&[]).unwrap(), 1);
        assert_eq!(element_count(&[2, 0, 4]).unwrap(), 0);
        assert_eq!(element_count(&[2, 3]).unwrap(), 6);
        assert!(element_count(&[-1]).is_err());
        assert!(element_count(&[i64::MAX, 2]).is_err());
    }
    #[test] fn explicit_backend_has_no_cross_backend_alias() {
        assert!(Backend::parse("ggml_vulkan").is_err());
        assert!(Backend::parse("auto").is_err());
        assert_eq!(Backend::parse("libtorch_rocm").unwrap().name(), "libtorch_rocm");
        assert!(Precision::parse("fp16").is_err());
    }
    #[test] fn strings_reject_embedded_nul() { assert!(cstring("model\0other").is_err()); }
    #[test] fn build_info_preserves_projection_math_in_diagnostics() {
        for math in ["ieee", "tf32_qkv_ffn_single_model_diagnostic"] {
            let info: BuildInfo = serde_json::from_value(serde_json::json!({
                "torch_version": "fixture", "compiled_backend": "libtorch_xpu",
                "roformer_projection_math": math, "models": [], "qualification": "not_asserted"
            })).unwrap();
            assert_eq!(serde_json::to_value(info).unwrap()["roformer_projection_math"], math);
        }
    }
    #[test] fn missing_runtime_is_not_ready() {
        assert!(Library::load(Path::new("/uta-studio-no-such-native-runtime/libuta_libtorch.so")).is_err());
    }
    #[test] fn owned_outputs_are_typed() {
        let tensor = Tensor { shape: vec![2], data: Values::I64(vec![1, 2]) };
        assert!(tensor.f32().is_err());
        assert_eq!(tensor.i64().unwrap(), &[1, 2]);
    }
}
