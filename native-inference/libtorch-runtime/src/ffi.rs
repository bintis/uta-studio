use std::ffi::{CStr, c_char, c_void};
use std::path::Path;

pub enum Runtime {}
pub enum Model {}
pub enum ResultHandle {}
#[derive(Default)]
#[repr(C)]
pub struct Tensor {
    pub name: *const c_char,
    pub data: *const c_void,
    pub dimensions: *const i64,
    pub elements: u64,
    pub rank: u32,
    pub kind: u32,
}

type BuildInfo = unsafe extern "C" fn() -> *const c_char;
type LayoutSize = unsafe extern "C" fn() -> usize;
type LastError = unsafe extern "C" fn() -> *const c_char;
type RuntimeCreate = unsafe extern "C" fn(*const c_char, i32, *const c_char) -> *mut Runtime;
type RuntimeFree = unsafe extern "C" fn(*mut Runtime);
type ModelOpen = unsafe extern "C" fn(*mut Runtime, *const c_char, *const c_char) -> *mut Model;
type ModelFree = unsafe extern "C" fn(*mut Model);
type ModelMetadata = unsafe extern "C" fn(*const Model) -> *const c_char;
type ModelCancel = unsafe extern "C" fn(*mut Model, i32);
type ModelForward =
    unsafe extern "C" fn(*mut Model, *const c_char, *const Tensor, usize) -> *mut ResultHandle;
type ResultCount = unsafe extern "C" fn(*const ResultHandle) -> usize;
type ResultTensor = unsafe extern "C" fn(*const ResultHandle, usize, *mut Tensor) -> i32;
type ResultTimings = unsafe extern "C" fn(*const ResultHandle, *mut crate::Timings) -> i32;
type ResultFree = unsafe extern "C" fn(*mut ResultHandle);

pub struct Api {
    pub build_info: BuildInfo,
    last_error: LastError,
    pub runtime_create: RuntimeCreate,
    pub runtime_free: RuntimeFree,
    pub model_open: ModelOpen,
    pub model_free: ModelFree,
    pub model_metadata: ModelMetadata,
    pub model_cancel: ModelCancel,
    pub model_forward: ModelForward,
    pub result_count: ResultCount,
    pub result_tensor: ResultTensor,
    pub result_timings: ResultTimings,
    pub result_free: ResultFree,
    // Dropped after all call sites have released their Arc<Api> ownership.
    _library: libloading::Library,
}
impl Api {
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: native code is loaded only from the caller's explicit local
        // runtime path. Symbols are declared by native/api.h, not inferred from
        // a filename or a backend of a different native library.
        unsafe {
            let library = libloading::Library::new(path).map_err(|error| {
                format!(
                    "cannot load native LibTorch runtime {}: {error}",
                    path.display()
                )
            })?;
            macro_rules! symbol {
                ($name:literal, $type:ty) => {
                    *library
                        .get::<$type>(concat!("uta_libtorch_", $name, "\0").as_bytes())
                        .map_err(|error| {
                            format!("native LibTorch runtime lacks {}: {error}", $name)
                        })?
                };
            }
            let size = symbol!("tensor_layout_size", LayoutSize);
            if size() != size_of::<Tensor>() {
                return Err("native LibTorch tensor ABI layout does not match Rust".to_string());
            }
            Ok(Self {
                build_info: symbol!("build_info", BuildInfo),
                last_error: symbol!("last_error", LastError),
                runtime_create: symbol!("runtime_create", RuntimeCreate),
                runtime_free: symbol!("runtime_free", RuntimeFree),
                model_open: symbol!("model_open", ModelOpen),
                model_free: symbol!("model_free", ModelFree),
                model_metadata: symbol!("model_metadata", ModelMetadata),
                model_cancel: symbol!("model_cancel", ModelCancel),
                model_forward: symbol!("model_forward", ModelForward),
                result_count: symbol!("result_count", ResultCount),
                result_tensor: symbol!("result_tensor", ResultTensor),
                result_timings: symbol!("result_timings", ResultTimings),
                result_free: symbol!("result_free", ResultFree),
                _library: library,
            })
        }
    }
    pub fn error(&self) -> String {
        // SAFETY: last_error returns a thread-local C string while this API is
        // loaded. Copy before any other native call can overwrite it.
        unsafe {
            let pointer = (self.last_error)();
            if pointer.is_null() {
                "native LibTorch returned an error without diagnostic text".to_string()
            } else {
                CStr::from_ptr(pointer).to_string_lossy().into_owned()
            }
        }
    }
}
