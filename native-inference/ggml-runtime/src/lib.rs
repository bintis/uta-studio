//! Rust-owned bindings for the packaged GGML shared libraries.
//!
//! This crate is the only product inference boundary below the Rust workers.
//! It loads upstream GGML shared libraries at runtime and calls their C ABI
//! directly. It does not build, link, or launch a model-specific C/C++ helper.

#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::all)]

use std::ffi::{CStr, CString, c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;

#[cfg(test)]
mod attention_breakdown_tests;
#[cfg(test)]
mod attention_model_tests;
#[cfg(test)]
mod attention_tests;

pub mod basic_pitch;
pub mod fcpe;
mod ffi;
pub mod firered;
pub mod game;
pub mod jbm555;
pub mod qwen;
pub mod rmvpe;
pub mod roformer;
pub mod rosvot;
mod stage_profile;
pub mod stars;
mod stft;
mod wav;

const DEVICE_TYPE_CPU: c_int = 0;
const DEVICE_TYPE_GPU: c_int = 1;
const DEVICE_TYPE_IGPU: c_int = 2;

#[repr(C)]
pub(crate) struct GgmlBackendDevice {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgmlBackend {
    _private: [u8; 0],
}

pub(crate) type BackendDevicePtr = *mut GgmlBackendDevice;
pub(crate) type BackendPtr = *mut GgmlBackend;

type BackendLoadAllFromPath = unsafe extern "C" fn(*const c_char);
type BackendDeviceCount = unsafe extern "C" fn() -> usize;
type BackendDeviceGet = unsafe extern "C" fn(usize) -> BackendDevicePtr;
type BackendDeviceName = unsafe extern "C" fn(BackendDevicePtr) -> *const c_char;
type BackendDeviceDescription = unsafe extern "C" fn(BackendDevicePtr) -> *const c_char;
type BackendDeviceType = unsafe extern "C" fn(BackendDevicePtr) -> c_int;
type BackendDeviceInit = unsafe extern "C" fn(BackendDevicePtr, *const c_char) -> BackendPtr;
type BackendFree = unsafe extern "C" fn(BackendPtr);
pub(crate) struct GgmlBackendReg {
    _private: [u8; 0],
}
type BackendRegPtr = *mut GgmlBackendReg;
type BackendDeviceReg = unsafe extern "C" fn(BackendDevicePtr) -> BackendRegPtr;
type BackendRegProcAddress =
    unsafe extern "C" fn(BackendRegPtr, *const c_char) -> *mut std::ffi::c_void;
/// `ggml_backend_set_n_threads_t` from `ggml-backend.h`. It is reached through
/// the device's registry rather than by linking a backend-specific symbol,
/// because the packaged runtime loads its backends as plugins.
type BackendSetThreads = unsafe extern "C" fn(BackendPtr, c_int);

pub(crate) struct Api {
    backend_load_all_from_path: BackendLoadAllFromPath,
    backend_device_count: BackendDeviceCount,
    backend_device_get: BackendDeviceGet,
    backend_device_name: BackendDeviceName,
    backend_device_description: BackendDeviceDescription,
    backend_device_type: BackendDeviceType,
    backend_device_init: BackendDeviceInit,
    backend_device_reg: BackendDeviceReg,
    backend_reg_proc_address: BackendRegProcAddress,
    backend_free: BackendFree,
}

impl Api {
    unsafe fn load(library: &Library) -> Result<Self, String> {
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                // SAFETY: every requested symbol and function signature is
                // copied from ggml commit 8c63e709's public C headers. The
                // runtime manifest pins that ABI before this module is used.
                let symbol = unsafe { library.get::<$ty>(concat!($name, "\0").as_bytes()) }
                    .map_err(|error| {
                        format!("GGML shared library is missing {}: {error}", $name)
                    })?;
                *symbol
            }};
        }
        Ok(Self {
            backend_load_all_from_path: symbol!(
                "ggml_backend_load_all_from_path",
                BackendLoadAllFromPath
            ),
            backend_device_count: symbol!("ggml_backend_dev_count", BackendDeviceCount),
            backend_device_get: symbol!("ggml_backend_dev_get", BackendDeviceGet),
            backend_device_name: symbol!("ggml_backend_dev_name", BackendDeviceName),
            backend_device_description: symbol!(
                "ggml_backend_dev_description",
                BackendDeviceDescription
            ),
            backend_device_type: symbol!("ggml_backend_dev_type", BackendDeviceType),
            backend_device_init: symbol!("ggml_backend_dev_init", BackendDeviceInit),
            backend_device_reg: symbol!("ggml_backend_dev_backend_reg", BackendDeviceReg),
            backend_reg_proc_address: symbol!(
                "ggml_backend_reg_get_proc_address",
                BackendRegProcAddress
            ),
            backend_free: symbol!("ggml_backend_free", BackendFree),
        })
    }
}

/// Device classes exposed by the packaged GGML backend plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// Explicit experimental reference lane. It is never selected as a
    /// fallback for a failed GPU request.
    Cpu,
    DiscreteGpu,
    IntegratedGpu,
}

/// A GGML device descriptor. Merely enumerating descriptors never creates a
/// logical device or submits work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub ggml_index: usize,
    pub name: String,
    pub description: String,
    pub kind: DeviceKind,
}

/// Loaded upstream GGML ABI and its backend plugins.
///
/// The shared-library handle remains alive until every backend created from it
/// has been dropped.
pub struct GgmlRuntime {
    _library: Library,
    api: Api,
    model_api: ffi::ModelApi,
    library_dir: PathBuf,
}

impl GgmlRuntime {
    pub fn load(library_dir: &Path) -> Result<Arc<Self>, String> {
        if !library_dir.is_dir() {
            return Err("GGML shared-library directory is unavailable".to_string());
        }
        let core = library_dir.join(core_library_name());
        if !core.is_file() {
            return Err(format!(
                "packaged GGML core shared library is unavailable: {}",
                core.display()
            ));
        }
        // SAFETY: loading the manifest-selected upstream GGML shared library
        // runs its loader initializers but does not initialize a model backend.
        let library = unsafe { Library::new(&core) }
            .map_err(|error| format!("could not load packaged GGML: {error}"))?;
        // SAFETY: `library` remains owned by the returned runtime.
        let api = unsafe { Api::load(&library) }?;
        // SAFETY: loading every function pointer verifies the stable ABI
        // capabilities used by the Rust graphs, and the library remains live
        // for every copied pointer.
        let model_api = unsafe { ffi::ModelApi::load(&library) }?;
        let encoded_dir = path_c_string(library_dir, "GGML shared-library directory")?;
        // SAFETY: the path is NUL-terminated and live for this call. GGML owns
        // any plugin handles it registers. This enumerates plugin metadata but
        // does not initialize a logical device.
        unsafe { (api.backend_load_all_from_path)(encoded_dir.as_ptr()) };
        Ok(Arc::new(Self {
            _library: library,
            api,
            model_api,
            library_dir: library_dir.to_path_buf(),
        }))
    }

    pub fn library_dir(&self) -> &Path {
        &self.library_dir
    }

    pub fn devices(&self) -> Result<Vec<DeviceDescriptor>, String> {
        // SAFETY: GGML's process-global backend registry is initialized while
        // this runtime keeps the core library loaded.
        let count = unsafe { (self.api.backend_device_count)() };
        let mut devices = Vec::new();
        for index in 0..count {
            // SAFETY: `index < count` for the unchanged registry traversal.
            let raw = unsafe { (self.api.backend_device_get)(index) };
            if raw.is_null() {
                return Err("GGML returned a null backend device".to_string());
            }
            // SAFETY: GGML owns these descriptor strings for the registry's
            // lifetime and promises NUL-terminated names.
            let device_type = unsafe { (self.api.backend_device_type)(raw) };
            let kind = match device_type {
                DEVICE_TYPE_CPU => DeviceKind::Cpu,
                DEVICE_TYPE_GPU => DeviceKind::DiscreteGpu,
                DEVICE_TYPE_IGPU => DeviceKind::IntegratedGpu,
                _ => continue,
            };
            // SAFETY: `raw` is a live registry entry.
            let name = unsafe { copy_ggml_string((self.api.backend_device_name)(raw)) }?;
            // SAFETY: `raw` is a live registry entry.
            let description =
                unsafe { copy_ggml_string((self.api.backend_device_description)(raw)) }?;
            devices.push(DeviceDescriptor {
                ggml_index: index,
                name,
                description,
                kind,
            });
        }
        Ok(devices)
    }

    pub fn vulkan_devices(&self) -> Result<Vec<DeviceDescriptor>, String> {
        Ok(self
            .devices()?
            .into_iter()
            .filter(|device| device.kind != DeviceKind::Cpu)
            .collect())
    }

    pub fn create_backend(
        self: &Arc<Self>,
        descriptor: &DeviceDescriptor,
    ) -> Result<GgmlBackendHandle, String> {
        // Re-resolve by the retained GGML registry index so callers cannot
        // manufacture an unenumerated device pointer.
        // SAFETY: the registry remains loaded through `self`.
        let count = unsafe { (self.api.backend_device_count)() };
        if descriptor.ggml_index >= count {
            return Err("selected GGML device is no longer available".to_string());
        }
        // SAFETY: the index was bounded immediately above.
        let device = unsafe { (self.api.backend_device_get)(descriptor.ggml_index) };
        if device.is_null() {
            return Err("selected GGML device is unavailable".to_string());
        }
        // SAFETY: `device` is a live registry entry. Rechecking both the type
        // and requested descriptor prevents a caller-constructed value from
        // changing execution class or turning a GPU failure into CPU work.
        let actual_type = unsafe { (self.api.backend_device_type)(device) };
        let type_matches = matches!(
            (descriptor.kind, actual_type),
            (DeviceKind::Cpu, DEVICE_TYPE_CPU)
                | (DeviceKind::DiscreteGpu, DEVICE_TYPE_GPU)
                | (DeviceKind::IntegratedGpu, DEVICE_TYPE_IGPU)
        );
        if !type_matches {
            return Err("selected GGML device class changed before initialization".to_string());
        }
        // SAFETY: a null parameter string requests backend defaults. The
        // descriptor above fixes the exact backend device that may initialize.
        let backend = unsafe { (self.api.backend_device_init)(device, std::ptr::null()) };
        if backend.is_null() {
            return Err(format!(
                "failed to initialize GGML {:?} device {}",
                descriptor.kind, descriptor.description
            ));
        }
        let handle = GgmlBackendHandle {
            raw: backend,
            runtime: Arc::clone(self),
        };
        if descriptor.kind == DeviceKind::Cpu {
            handle.apply_cpu_thread_count(device);
        }
        Ok(handle)
    }
}

/// An initialized, explicitly selected GGML backend. GPU requests never fall
/// back to the experimental CPU device.
pub struct GgmlBackendHandle {
    pub(crate) raw: BackendPtr,
    pub(crate) runtime: Arc<GgmlRuntime>,
}

/// Environment name carrying the operator's CPU thread choice into the worker
/// process. Absent means the packaged GGML default, which is four threads
/// regardless of how many the machine has.
pub const CPU_THREAD_COUNT_ENV: &str = "UTA_STUDIO_GGML_CPU_THREADS";

impl GgmlBackendHandle {
    /// Applies the operator's CPU thread choice. The count is reached through
    /// `ggml_backend_reg_get_proc_address`, the plugin-safe path, because the
    /// CPU backend is a separately loaded shared library. A backend that does
    /// not publish the entry point keeps its own default.
    fn apply_cpu_thread_count(&self, device: BackendDevicePtr) {
        let Some(threads) = configured_cpu_threads() else {
            return;
        };
        // SAFETY: `device` is the live registry entry this backend came from.
        let registry = unsafe { (self.runtime.api.backend_device_reg)(device) };
        if registry.is_null() {
            return;
        }
        let Ok(name) = std::ffi::CString::new("ggml_backend_set_n_threads") else {
            return;
        };
        // SAFETY: the registry pointer is live and the name is NUL-terminated.
        let address =
            unsafe { (self.runtime.api.backend_reg_proc_address)(registry, name.as_ptr()) };
        if address.is_null() {
            return;
        }
        // SAFETY: the entry point's signature is `ggml_backend_set_n_threads_t`
        // from the pinned `ggml-backend.h`.
        let set_threads: BackendSetThreads = unsafe { std::mem::transmute(address) };
        // SAFETY: `self.raw` is this handle's live backend.
        unsafe { set_threads(self.raw, threads) };
    }
}

fn configured_cpu_threads() -> Option<c_int> {
    let value = std::env::var(CPU_THREAD_COUNT_ENV).ok()?;
    let threads = value.trim().parse::<i32>().ok()?;
    (1..=1024).contains(&threads).then_some(threads as c_int)
}

impl Drop for GgmlBackendHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: this handle uniquely owns `raw`, and `runtime` keeps the
            // function pointer and plugin code alive through this call.
            unsafe { (self.runtime.api.backend_free)(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

fn core_library_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "ggml.dll"
    }
    #[cfg(target_os = "macos")]
    {
        "libggml.dylib"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "libggml.so.0"
    }
}

fn path_c_string(path: &Path, label: &str) -> Result<CString, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("{label} is not valid UTF-8"))?;
    CString::new(value).map_err(|_| format!("{label} contains a NUL byte"))
}

unsafe fn copy_ggml_string(raw: *const c_char) -> Result<String, String> {
    if raw.is_null() {
        return Err("GGML returned a null device string".to_string());
    }
    // SAFETY: the caller established that `raw` points to a live
    // NUL-terminated GGML registry string.
    Ok(unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_core_name_is_a_shared_library() {
        let name = core_library_name();
        assert!(name.ends_with(".dll") || name.contains(".so") || name.ends_with(".dylib"));
    }

    #[test]
    fn path_encoding_rejects_nul() {
        assert!(path_c_string(Path::new("runtime/lib"), "path").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let invalid = std::ffi::OsStr::from_bytes(b"runtime\0lib");
            assert!(path_c_string(Path::new(invalid), "path").is_err());
        }
    }

    #[test]
    #[ignore = "requires an explicitly built pinned GGML runtime"]
    fn pinned_shared_runtime_loads_and_enumerates_without_initializing_a_device() {
        let root = std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR")
            .map(PathBuf::from)
            .expect("set UTA_TEST_GGML_RUNTIME_DIR");
        let runtime = GgmlRuntime::load(&root).unwrap();
        let devices = runtime.devices().unwrap();
        assert!(devices.iter().any(|device| device.kind == DeviceKind::Cpu));
        assert!(devices.iter().any(|device| device.kind != DeviceKind::Cpu));
    }
}
