//! Compact same-backend matrices, with synchronous copies and explicit final
//! consumers. Copy views have short-lived metadata, not an ever-growing arena.
use crate::GgmlBackendHandle;
use crate::ffi::{
    BufferPtr, ContextPtr, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GgmlInitParams, TensorPtr,
};

pub(crate) struct Matrix<'backend> {
    backend: &'backend GgmlBackendHandle,
    context: ContextPtr,
    buffer: BufferPtr,
    pub tensor: TensorPtr,
    width: usize,
    rows: usize,
}
impl<'backend> Matrix<'backend> {
    pub fn new(
        backend: &'backend GgmlBackendHandle,
        width: usize,
        rows: usize,
    ) -> Result<Self, String> {
        let columns = i64::try_from(width).map_err(|_| "resident matrix width overflows")?;
        let height = i64::try_from(rows).map_err(|_| "resident matrix rows overflow")?;
        width
            .checked_mul(rows)
            .and_then(|count| count.checked_mul(4))
            .ok_or("resident matrix bytes overflow")?;
        let api = &backend.runtime.model_api;
        // SAFETY: this context/buffer are owned together, and the borrowed
        // backend cannot be dropped before this matrix.
        let context = unsafe {
            (api.ggml_init)(GgmlInitParams {
                mem_size: 64 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        };
        if context.is_null() {
            return Err("resident matrix context allocation failed".into());
        }
        let mut result = Self {
            backend,
            context,
            buffer: std::ptr::null_mut(),
            tensor: std::ptr::null_mut(),
            width,
            rows,
        };
        unsafe {
            result.tensor = (api.ggml_new_tensor_2d)(context, GGML_TYPE_F32, columns, height);
            if result.tensor.is_null() {
                return Err("resident matrix tensor allocation failed".into());
            }
            (api.ggml_set_input)(result.tensor);
            let kind = (api.ggml_backend_get_default_buffer_type)(backend.raw);
            result.buffer = (api.ggml_backend_alloc_ctx_tensors_from_buft)(context, kind);
        }
        if result.buffer.is_null() {
            return Err("resident matrix device allocation unavailable".into());
        }
        Ok(result)
    }

    pub fn copy_from(&self, source: TensorPtr, start: usize, rows: usize) -> Result<(), String> {
        let view = self.view(start, rows)?;
        copy(self.backend, source, view.tensor)
    }
    pub fn copy_to(&self, target: TensorPtr, start: usize, rows: usize) -> Result<(), String> {
        let view = self.view(start, rows)?;
        copy(self.backend, view.tensor, target)
    }
    pub fn write(&self, values: &[f32]) -> Result<(), String> {
        if values.len() != self.width * self.rows {
            return Err("resident matrix input shape mismatch".into());
        }
        // SAFETY: the full allocated matrix is overwritten before consumption.
        unsafe {
            (self.backend.runtime.model_api.ggml_backend_tensor_set)(
                self.tensor,
                values.as_ptr().cast(),
                0,
                std::mem::size_of_val(values),
            )
        };
        Ok(())
    }
    fn view(&self, start: usize, rows: usize) -> Result<View<'_>, String> {
        let offset = row_range(self.width, self.rows, start, rows)?;
        let api = &self.backend.runtime.model_api;
        // SAFETY: metadata is scoped to this synchronous copy; matrix storage
        // remains owned by self until the copy and View drop complete.
        let context = unsafe {
            (api.ggml_init)(GgmlInitParams {
                mem_size: 64 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        };
        if context.is_null() {
            return Err("resident copy view context allocation failed".into());
        }
        let mut view = View {
            backend: self.backend,
            context,
            tensor: std::ptr::null_mut(),
        };
        unsafe {
            view.tensor = (api.ggml_view_2d)(
                context,
                self.tensor,
                self.width as i64,
                rows as i64,
                self.width * 4,
                offset,
            );
            if view.tensor.is_null() {
                return Err("resident copy view allocation failed".into());
            }
            let status = (api.ggml_backend_view_init)(view.tensor);
            if status != GGML_STATUS_SUCCESS {
                return Err(format!(
                    "resident copy view initialization failed: {status}"
                ));
            }
        }
        Ok(view)
    }
}
fn row_range(width: usize, height: usize, start: usize, rows: usize) -> Result<usize, String> {
    if rows == 0 || start.checked_add(rows).is_none_or(|end| end > height) {
        return Err("resident matrix copy range is outside its source".into());
    }
    start
        .checked_mul(width)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| "resident matrix offset overflows".into())
}
fn copy(backend: &GgmlBackendHandle, source: TensorPtr, target: TensorPtr) -> Result<(), String> {
    // SAFETY: private model callers retain both contexts through this copy.
    // These metadata borrows never escape the synchronous operation.
    let left = unsafe { source.as_ref() }.ok_or("resident copy source is null")?;
    let right = unsafe { target.as_ref() }.ok_or("resident copy target is null")?;
    // ggml_backend_tensor_copy requires the same layout, not merely equal
    // element count. Never pass a strided concatenation view to that API.
    if left.type_ != GGML_TYPE_F32
        || right.type_ != GGML_TYPE_F32
        || left.ne != right.ne
        || left.nb != right.nb
    {
        return Err("resident copy requires identically laid out float matrices".into());
    }
    unsafe { (backend.runtime.model_api.ggml_backend_tensor_copy)(source, target) };
    let bytes = unsafe { (backend.runtime.model_api.ggml_nbytes)(source) };
    crate::acceleration::record_resident_copy(bytes);
    Ok(())
}
impl Drop for Matrix<'_> {
    fn drop(&mut self) {
        let api = &self.backend.runtime.model_api;
        // SAFETY: all reads/copies have completed and the backend is still live.
        unsafe {
            if !self.buffer.is_null() {
                (api.ggml_backend_buffer_free)(self.buffer);
            }
            (api.ggml_free)(self.context);
        }
    }
}
struct View<'backend> {
    backend: &'backend GgmlBackendHandle,
    context: ContextPtr,
    tensor: TensorPtr,
}
impl Drop for View<'_> {
    fn drop(&mut self) {
        unsafe { (self.backend.runtime.model_api.ggml_free)(self.context) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_views_are_checked_in_bytes_without_crossing_the_last_consumer_range() {
        assert_eq!(row_range(256, 96, 64, 32).unwrap(), 64 * 256 * 4);
        assert!(row_range(256, 96, 65, 32).is_err());
        assert!(row_range(256, 96, 0, 0).is_err());
        assert!(row_range(usize::MAX, 96, 64, 1).is_err());
    }
    #[test]
    #[ignore = "requires an explicit packaged GGML library directory; native CPU reference tensors only"]
    fn native_matrix_copies_slices_after_source_release_and_resets_hidden_state() {
        let directory = std::env::var("UTA_STUDIO_GGML_TEST_LIBRARY_DIR").unwrap();
        let runtime = crate::GgmlRuntime::load(std::path::Path::new(&directory)).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| device.kind == crate::DeviceKind::Cpu)
            .unwrap();
        let backend = runtime.create_backend(&device).unwrap();
        let scope = crate::acceleration::Scope::enter(true);
        let source = Matrix::new(&backend, 4, 3).unwrap();
        source
            .write(&(0..12).map(|value| value as f32).collect::<Vec<_>>())
            .unwrap();
        let retained = Matrix::new(&backend, 4, 3).unwrap();
        retained.copy_from(source.tensor, 0, 3).unwrap();
        drop(source);
        let chunk = Matrix::new(&backend, 4, 2).unwrap();
        retained.copy_to(chunk.tensor, 1, 2).unwrap();
        let mut result = vec![0.0_f32; 8];
        unsafe {
            (runtime.model_api.ggml_backend_tensor_get)(
                chunk.tensor,
                result.as_mut_ptr().cast(),
                0,
                result.len() * 4,
            )
        };
        assert_eq!(result, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
        let hidden = Matrix::new(&backend, 4, 1).unwrap();
        retained.copy_to(hidden.tensor, 2, 1).unwrap();
        hidden.write(&[0.0; 4]).unwrap();
        let mut result = [1.0_f32; 4];
        unsafe {
            (runtime.model_api.ggml_backend_tensor_get)(
                hidden.tensor,
                result.as_mut_ptr().cast(),
                0,
                16,
            )
        };
        assert_eq!(result, [0.0; 4]);
        assert_eq!(scope.resident_copy_bytes(), 96);
    }
}
