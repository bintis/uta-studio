//! Compact encoder output ownership, independent of the encoder graph arena.

use std::sync::Arc;

use super::weights::tensor_ref;
use crate::ffi::{
    BufferPtr, ContextPtr, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GgmlInitParams, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

pub(super) struct ResidentAudio {
    runtime: Arc<GgmlRuntime>,
    context: ContextPtr,
    buffer: BufferPtr,
    tensor: TensorPtr,
    width: usize,
    rows: usize,
}

impl std::fmt::Debug for ResidentAudio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResidentAudio")
            .field("width", &self.width)
            .field("rows", &self.rows)
            .finish()
    }
}

impl ResidentAudio {
    pub(super) fn capture(
        backend: &GgmlBackendHandle,
        source: TensorPtr,
        width: usize,
        rows: usize,
    ) -> Result<Self, String> {
        let api = &backend.runtime.model_api;
        // SAFETY: every pointer is owned by this backend or the caller's live
        // encoder graph; all transfers finish before those owners are released.
        let context = unsafe {
            (api.ggml_init)(GgmlInitParams {
                mem_size: 64 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        };
        if context.is_null() {
            return Err("could not allocate resident audio context".to_string());
        }
        let mut result = Self {
            runtime: Arc::clone(&backend.runtime),
            context,
            buffer: std::ptr::null_mut(),
            tensor: std::ptr::null_mut(),
            width,
            rows,
        };
        // SAFETY: `context` and the selected backend remain live through result.
        unsafe {
            result.tensor =
                (api.ggml_new_tensor_2d)(context, GGML_TYPE_F32, width as i64, rows as i64);
            let buffer_type = (api.ggml_backend_get_default_buffer_type)(backend.raw);
            result.buffer = (api.ggml_backend_alloc_ctx_tensors_from_buft)(context, buffer_type);
        }
        if result.buffer.is_null() {
            return Err("device memory unavailable for resident audio".to_string());
        }
        // SAFETY: both tensors are allocated, identically shaped F32 matrices.
        unsafe { (api.ggml_backend_tensor_copy)(source, result.tensor) };
        eprintln!(
            "[super acceleration] retained {} encoder audio bytes on device",
            width * rows * 4
        );
        Ok(result)
    }

    pub(super) fn inject(
        &self,
        context: ContextPtr,
        target: TensorPtr,
        offset: usize,
    ) -> Result<(), String> {
        let api = &self.runtime.model_api;
        let target_layout = tensor_ref(target)?;
        // SAFETY: the decoder owns context/target; this object owns the source.
        // The view is initialized after the target's allocation and is used
        // only for this synchronous copy, not as an untracked graph dependency.
        let view = unsafe {
            (api.ggml_view_2d)(
                context,
                target,
                self.width as i64,
                self.rows as i64,
                target_layout.nb[1],
                offset * self.width * std::mem::size_of::<f32>(),
            )
        };
        // SAFETY: the view references the live, allocated decoder input.
        let status = unsafe { (api.ggml_backend_view_init)(view) };
        if status != GGML_STATUS_SUCCESS {
            return Err(format!(
                "resident audio view initialization failed: {status}"
            ));
        }
        // SAFETY: synchronous backend copy retains both owners until completion.
        unsafe { (api.ggml_backend_tensor_copy)(self.tensor, view) };
        eprintln!(
            "[super acceleration] reused {} resident encoder audio bytes without host round trip",
            self.width * self.rows * 4
        );
        Ok(())
    }
}

impl Drop for ResidentAudio {
    fn drop(&mut self) {
        let api = &self.runtime.model_api;
        // SAFETY: transfers completed synchronously; these are uniquely owned
        // allocations, with the runtime retained until their release finishes.
        unsafe {
            if !self.buffer.is_null() {
                (api.ggml_backend_buffer_free)(self.buffer);
            }
            if !self.context.is_null() {
                (api.ggml_free)(self.context);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;

    #[test]
    #[ignore = "requires an explicit packaged GGML library directory; uses only the CPU reference device"]
    fn native_resident_audio_survives_source_release_and_injects_at_the_right_offset() {
        let directory = std::env::var("UTA_STUDIO_GGML_TEST_LIBRARY_DIR").unwrap();
        let runtime = GgmlRuntime::load(std::path::Path::new(&directory)).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| device.kind == DeviceKind::Cpu)
            .unwrap();
        let backend = runtime.create_backend(&device).unwrap();
        let api = &runtime.model_api;
        // SAFETY: isolated CPU tensors and explicit owners for this test only.
        unsafe {
            let context = (api.ggml_init)(GgmlInitParams {
                mem_size: 64 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            });
            let source = (api.ggml_new_tensor_2d)(context, GGML_TYPE_F32, 2, 2);
            let buffer_type = (api.ggml_backend_get_default_buffer_type)(backend.raw);
            let source_buffer =
                (api.ggml_backend_alloc_ctx_tensors_from_buft)(context, buffer_type);
            let values = [0.25_f32, -0.5, 0.75, 1.0];
            (api.ggml_backend_tensor_set)(
                source,
                values.as_ptr().cast(),
                0,
                std::mem::size_of_val(&values),
            );
            let retained = ResidentAudio::capture(&backend, source, 2, 2).unwrap();
            (api.ggml_backend_buffer_free)(source_buffer);
            (api.ggml_free)(context);
            let target_context = (api.ggml_init)(GgmlInitParams {
                mem_size: 64 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            });
            let target = (api.ggml_new_tensor_2d)(target_context, GGML_TYPE_F32, 2, 4);
            let target_buffer =
                (api.ggml_backend_alloc_ctx_tensors_from_buft)(target_context, buffer_type);
            let mut actual = [0.0_f32; 8];
            (api.ggml_backend_tensor_set)(
                target,
                actual.as_ptr().cast(),
                0,
                std::mem::size_of_val(&actual),
            );
            retained.inject(target_context, target, 1).unwrap();
            drop(retained);
            (api.ggml_backend_tensor_get)(
                target,
                actual.as_mut_ptr().cast(),
                0,
                std::mem::size_of_val(&actual),
            );
            (api.ggml_backend_buffer_free)(target_buffer);
            (api.ggml_free)(target_context);
            assert_eq!(actual, [0.0, 0.0, 0.25, -0.5, 0.75, 1.0, 0.0, 0.0]);
        }
    }
}
