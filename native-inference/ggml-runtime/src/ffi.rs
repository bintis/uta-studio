use std::ffi::{c_char, c_int, c_void};

use libloading::Library;

use crate::BackendPtr;

pub(crate) const GGML_TYPE_F32: GgmlType = 0;
pub(crate) const GGML_TYPE_F16: GgmlType = 1;
pub(crate) const GGML_TYPE_I32: GgmlType = 26;
pub(crate) const GGML_PREC_F32: GgmlPrec = 10;
pub(crate) const GGML_STATUS_SUCCESS: GgmlStatus = 0;
pub(crate) const GGUF_TYPE_UINT32: GgufType = 4;
pub(crate) const GGUF_TYPE_INT32: GgufType = 5;
pub(crate) const GGUF_TYPE_FLOAT32: GgufType = 6;
pub(crate) const GGUF_TYPE_STRING: GgufType = 8;
pub(crate) const GGUF_TYPE_ARRAY: GgufType = 9;
pub(crate) const GGUF_TYPE_UINT64: GgufType = 10;
pub(crate) const GGUF_TYPE_INT64: GgufType = 11;
pub(crate) const GGUF_TYPE_FLOAT64: GgufType = 12;

pub(crate) type GgmlType = u32;
pub(crate) type GgmlOp = u32;
pub(crate) type GgmlPrec = u32;
pub(crate) type GgmlStatus = c_int;
pub(crate) type GgufType = u32;

#[repr(C)]
pub(crate) struct GgmlContext {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgmlGraph {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgmlAllocator {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgmlBackendBufferType {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgmlBackendBuffer {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct GgufContext {
    _private: [u8; 0],
}

pub(crate) type ContextPtr = *mut GgmlContext;
pub(crate) type TensorPtr = *mut GgmlTensor;
pub(crate) type GraphPtr = *mut GgmlGraph;
pub(crate) type AllocatorPtr = *mut GgmlAllocator;
pub(crate) type BufferTypePtr = *mut GgmlBackendBufferType;
pub(crate) type BufferPtr = *mut GgmlBackendBuffer;
pub(crate) type GgufPtr = *mut GgufContext;

/// Layout from ggml.h at the pinned 0.20.2 commit. Model code reads only
/// `type_`, dimensions, strides, and name; GGML owns every pointer field.
#[repr(C)]
pub(crate) struct GgmlTensor {
    pub type_: GgmlType,
    pub buffer: BufferPtr,
    pub ne: [i64; 4],
    pub nb: [usize; 4],
    pub op: GgmlOp,
    pub op_params: [i32; 16],
    pub flags: i32,
    pub src: [TensorPtr; 10],
    pub view_src: TensorPtr,
    pub view_offs: usize,
    pub data: *mut c_void,
    pub name: [c_char; 64],
    pub extra: *mut c_void,
    pub padding: [c_char; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GgmlInitParams {
    pub mem_size: usize,
    pub mem_buffer: *mut c_void,
    pub no_alloc: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GgufInitParams {
    pub no_alloc: bool,
    pub ctx: *mut ContextPtr,
}

macro_rules! model_api {
    ($(fn $name:ident($($argument:ident: $argument_type:ty),* $(,)?) -> $return_type:ty;)+) => {
        pub(crate) struct ModelApi {
            $(pub(crate) $name: unsafe extern "C" fn($($argument_type),*) -> $return_type,)+
        }

        impl ModelApi {
            pub(crate) unsafe fn load(library: &Library) -> Result<Self, String> {
                Ok(Self {
                    $($name: *unsafe {
                        library.get::<unsafe extern "C" fn($($argument_type),*) -> $return_type>(
                            concat!(stringify!($name), "\0").as_bytes(),
                        )
                    }
                    .map_err(|error| format!(
                        "GGML shared library is missing {}: {error}",
                        stringify!($name)
                    ))?,)+
                })
            }
        }
    };
}

model_api! {
    fn ggml_init(params: GgmlInitParams) -> ContextPtr;
    fn ggml_free(ctx: ContextPtr) -> ();
    fn ggml_new_tensor_1d(ctx: ContextPtr, type_: GgmlType, ne0: i64) -> TensorPtr;
    fn ggml_new_tensor_2d(ctx: ContextPtr, type_: GgmlType, ne0: i64, ne1: i64) -> TensorPtr;
    fn ggml_new_tensor_3d(ctx: ContextPtr, type_: GgmlType, ne0: i64, ne1: i64, ne2: i64) -> TensorPtr;
    fn ggml_new_tensor_4d(ctx: ContextPtr, type_: GgmlType, ne0: i64, ne1: i64, ne2: i64, ne3: i64) -> TensorPtr;
    fn ggml_new_graph_custom(ctx: ContextPtr, size: usize, grads: bool) -> GraphPtr;
    fn ggml_build_forward_expand(graph: GraphPtr, tensor: TensorPtr) -> ();
    fn ggml_set_input(tensor: TensorPtr) -> ();
    fn ggml_set_output(tensor: TensorPtr) -> ();
    fn ggml_get_tensor(ctx: ContextPtr, name: *const c_char) -> TensorPtr;
    fn ggml_get_first_tensor(ctx: *const GgmlContext) -> TensorPtr;
    fn ggml_get_next_tensor(ctx: *const GgmlContext, tensor: TensorPtr) -> TensorPtr;
    fn ggml_nbytes(tensor: *const GgmlTensor) -> usize;
    fn ggml_nelements(tensor: *const GgmlTensor) -> i64;
    fn ggml_backend_alloc_ctx_tensors_from_buft(ctx: ContextPtr, buffer_type: BufferTypePtr) -> BufferPtr;
    fn ggml_backend_get_default_buffer_type(backend: BackendPtr) -> BufferTypePtr;
    fn ggml_backend_tensor_set(tensor: TensorPtr, data: *const c_void, offset: usize, size: usize) -> ();
    fn ggml_backend_tensor_get(tensor: *const GgmlTensor, data: *mut c_void, offset: usize, size: usize) -> ();
    fn ggml_backend_buffer_free(buffer: BufferPtr) -> ();
    fn ggml_gallocr_new(buffer_type: BufferTypePtr) -> AllocatorPtr;
    fn ggml_gallocr_free(allocator: AllocatorPtr) -> ();
    fn ggml_gallocr_reserve(allocator: AllocatorPtr, graph: GraphPtr) -> bool;
    fn ggml_gallocr_alloc_graph(allocator: AllocatorPtr, graph: GraphPtr) -> bool;
    fn ggml_backend_graph_compute(backend: BackendPtr, graph: GraphPtr) -> GgmlStatus;
    fn ggml_mul_mat(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_mul_mat_set_prec(tensor: TensorPtr, precision: GgmlPrec) -> ();
    fn ggml_get_rows(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_reshape_2d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64) -> TensorPtr;
    fn ggml_reshape_3d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64, ne2: i64) -> TensorPtr;
    fn ggml_reshape_4d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64, ne2: i64, ne3: i64) -> TensorPtr;
    fn ggml_view_1d(ctx: ContextPtr, a: TensorPtr, ne0: i64, offset: usize) -> TensorPtr;
    fn ggml_view_2d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64, nb1: usize, offset: usize) -> TensorPtr;
    fn ggml_view_3d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64, ne2: i64, nb1: usize, nb2: usize, offset: usize) -> TensorPtr;
    fn ggml_view_4d(ctx: ContextPtr, a: TensorPtr, ne0: i64, ne1: i64, ne2: i64, ne3: i64, nb1: usize, nb2: usize, nb3: usize, offset: usize) -> TensorPtr;
    fn ggml_permute(ctx: ContextPtr, a: TensorPtr, axis0: c_int, axis1: c_int, axis2: c_int, axis3: c_int) -> TensorPtr;
    fn ggml_cont(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_concat(ctx: ContextPtr, a: TensorPtr, b: TensorPtr, dim: c_int) -> TensorPtr;
    fn ggml_repeat(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_norm(ctx: ContextPtr, a: TensorPtr, epsilon: f32) -> TensorPtr;
    fn ggml_rms_norm(ctx: ContextPtr, a: TensorPtr, epsilon: f32) -> TensorPtr;
    fn ggml_rope_ext(ctx: ContextPtr, a: TensorPtr, positions: TensorPtr, frequency_factors: TensorPtr, dimensions: c_int, mode: c_int, original_context: c_int, frequency_base: f32, frequency_scale: f32, extension_factor: f32, attention_factor: f32, beta_fast: f32, beta_slow: f32) -> TensorPtr;
    fn ggml_flash_attn_ext(ctx: ContextPtr, query: TensorPtr, key: TensorPtr, value: TensorPtr, mask: TensorPtr, scale: f32, max_bias: f32, logit_softcap: f32) -> TensorPtr;
    fn ggml_flash_attn_ext_set_prec(tensor: TensorPtr, precision: GgmlPrec) -> ();
    fn ggml_cast(ctx: ContextPtr, a: TensorPtr, type_: GgmlType) -> TensorPtr;
    fn ggml_cpy(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_add(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_mul(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_gelu_erf(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_tanh(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_sigmoid(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_soft_max(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_silu(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_softplus(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_cos(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_sin(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_im2col(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride_0: c_int, stride_1: c_int, padding_0: c_int, padding_1: c_int, dilation_0: c_int, dilation_1: c_int, is_2d: bool, destination_type: GgmlType) -> TensorPtr;
    fn ggml_conv_1d(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride: c_int, padding: c_int, dilation: c_int) -> TensorPtr;
    fn ggml_conv_1d_dw(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride: c_int, padding: c_int, dilation: c_int) -> TensorPtr;
    fn ggml_conv_transpose_1d(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride: c_int, padding: c_int, dilation: c_int) -> TensorPtr;
    fn ggml_conv_2d(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride_0: c_int, stride_1: c_int, padding_0: c_int, padding_1: c_int, dilation_0: c_int, dilation_1: c_int) -> TensorPtr;
    fn ggml_conv_transpose_2d_p0(ctx: ContextPtr, weight: TensorPtr, input: TensorPtr, stride: c_int) -> TensorPtr;
    fn ggml_div(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_sub(ctx: ContextPtr, a: TensorPtr, b: TensorPtr) -> TensorPtr;
    fn ggml_sqrt(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_scale_bias(ctx: ContextPtr, a: TensorPtr, scale: f32, bias: f32) -> TensorPtr;
    fn ggml_relu(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_leaky_relu(ctx: ContextPtr, a: TensorPtr, negative_slope: f32, inplace: bool) -> TensorPtr;
    fn ggml_pool_1d(ctx: ContextPtr, a: TensorPtr, operation: u32, kernel: c_int, stride: c_int, padding: c_int) -> TensorPtr;
    fn ggml_pool_2d(ctx: ContextPtr, a: TensorPtr, operation: u32, kernel_0: c_int, kernel_1: c_int, stride_0: c_int, stride_1: c_int, padding_0: f32, padding_1: f32) -> TensorPtr;
    fn ggml_transpose(ctx: ContextPtr, a: TensorPtr) -> TensorPtr;
    fn ggml_element_size(tensor: *const GgmlTensor) -> usize;
    fn gguf_init_from_file(path: *const c_char, params: GgufInitParams) -> GgufPtr;
    fn gguf_free(ctx: GgufPtr) -> ();
    fn gguf_find_key(ctx: *const GgufContext, key: *const c_char) -> i64;
    fn gguf_get_val_str(ctx: *const GgufContext, key: i64) -> *const c_char;
    fn gguf_get_val_u32(ctx: *const GgufContext, key: i64) -> u32;
    fn gguf_get_val_i32(ctx: *const GgufContext, key: i64) -> i32;
    fn gguf_get_val_u64(ctx: *const GgufContext, key: i64) -> u64;
    fn gguf_get_val_i64(ctx: *const GgufContext, key: i64) -> i64;
    fn gguf_get_val_f32(ctx: *const GgufContext, key: i64) -> f32;
    fn gguf_get_val_f64(ctx: *const GgufContext, key: i64) -> f64;
    fn gguf_get_val_bool(ctx: *const GgufContext, key: i64) -> bool;
    fn gguf_get_kv_type(ctx: *const GgufContext, key: i64) -> GgufType;
    fn gguf_get_arr_type(ctx: *const GgufContext, key: i64) -> GgufType;
    fn gguf_get_arr_data(ctx: *const GgufContext, key: i64) -> *const c_void;
    fn gguf_get_arr_n(ctx: *const GgufContext, key: i64) -> usize;
    fn gguf_get_arr_str(ctx: *const GgufContext, key: i64, index: usize) -> *const c_char;
    fn gguf_get_data_offset(ctx: *const GgufContext) -> usize;
    fn gguf_find_tensor(ctx: *const GgufContext, name: *const c_char) -> i64;
    fn gguf_get_tensor_offset(ctx: *const GgufContext, tensor: i64) -> usize;
    fn gguf_get_n_tensors(ctx: *const GgufContext) -> i64;
}
