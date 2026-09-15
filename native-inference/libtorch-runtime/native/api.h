#ifndef UTA_LIBTORCH_API_H
#define UTA_LIBTORCH_API_H
#include <stddef.h>
#include <stdint.h>
#if defined(_WIN32)
#define UTA_LIBTORCH_EXPORT __declspec(dllexport)
#else
#define UTA_LIBTORCH_EXPORT __attribute__((visibility("default")))
#endif
#ifdef __cplusplus
extern "C" {
#define UTA_LIBTORCH_NOEXCEPT noexcept
#else
#define UTA_LIBTORCH_NOEXCEPT
#endif

typedef struct UtaLibtorchRuntime UtaLibtorchRuntime;
typedef struct UtaLibtorchModel UtaLibtorchModel;
typedef struct UtaLibtorchResult UtaLibtorchResult;

/* Borrowed contiguous row-major tensor. kind: 0=float32, 1=int64.
 * The caller retains all pointers through the synchronous call. */
typedef struct UtaLibtorchTensor {
    const char* name;
    const void* data;
    const int64_t* dimensions;
    uint64_t elements;
    uint32_t rank;
    uint32_t kind;
} UtaLibtorchTensor;

typedef struct UtaLibtorchTimings {
    double upload_seconds;
    double synchronized_compute_seconds;
    double readback_seconds;
} UtaLibtorchTimings;

/* Read-only build capabilities; this does not initialize any accelerator. */
UTA_LIBTORCH_EXPORT const char* uta_libtorch_build_info(void) UTA_LIBTORCH_NOEXCEPT;
/* Process-lifetime diagnostic callback. Both strings are borrowed for the
 * synchronous call. The callback must not throw, re-enter this library or call
 * accelerator APIs. NULL disables it; it never controls model eligibility. */
typedef void (*UtaLibtorchDiagnosticCallback)(const char* phase, const char* detail);
UTA_LIBTORCH_EXPORT void uta_libtorch_set_diagnostic_callback(UtaLibtorchDiagnosticCallback callback) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT size_t uta_libtorch_tensor_layout_size(void) UTA_LIBTORCH_NOEXCEPT;
/* Error text is thread-local and valid until the next call on that thread. */
UTA_LIBTORCH_EXPORT const char* uta_libtorch_last_error(void) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT UtaLibtorchRuntime* uta_libtorch_runtime_create(const char* backend, int device, const char* precision) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT void uta_libtorch_runtime_free(UtaLibtorchRuntime* runtime) UTA_LIBTORCH_NOEXCEPT;
/* Model holds shared runtime ownership after the caller releases its handle. */
UTA_LIBTORCH_EXPORT UtaLibtorchModel* uta_libtorch_model_open(UtaLibtorchRuntime* runtime, const char* resource, const char* path) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT void uta_libtorch_model_free(UtaLibtorchModel* model) UTA_LIBTORCH_NOEXCEPT;
/* Metadata is immutable and valid for model lifetime. */
UTA_LIBTORCH_EXPORT const char* uta_libtorch_model_metadata(const UtaLibtorchModel* model) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT void uta_libtorch_model_cancel(UtaLibtorchModel* model, int cancelled) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT UtaLibtorchResult* uta_libtorch_model_forward(UtaLibtorchModel* model, const char* operation,
    const UtaLibtorchTensor* inputs, size_t input_count) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT size_t uta_libtorch_result_count(const UtaLibtorchResult* result) UTA_LIBTORCH_NOEXCEPT;
/* Borrowed output view remains valid until result_free. Returns 0 on success. */
UTA_LIBTORCH_EXPORT int uta_libtorch_result_tensor(const UtaLibtorchResult* result, size_t index, UtaLibtorchTensor* output) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT int uta_libtorch_result_timings(const UtaLibtorchResult* result, UtaLibtorchTimings* output) UTA_LIBTORCH_NOEXCEPT;
UTA_LIBTORCH_EXPORT void uta_libtorch_result_free(UtaLibtorchResult* result) UTA_LIBTORCH_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#endif
