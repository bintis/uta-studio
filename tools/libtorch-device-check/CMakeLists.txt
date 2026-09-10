cmake_minimum_required(VERSION 3.20)
project(uta_libtorch_device_check LANGUAGES CXX)

set(TORCH_ROOT "" CACHE PATH "Private ROCm torch directory containing include and lib")
set(ROCM_ROOT "" CACHE PATH "Private expanded ROCm SDK directory")
set(TORCH_CXX_ABI "1" CACHE STRING "LibTorch libstdc++ ABI choice")
if(NOT EXISTS "${TORCH_ROOT}/include/ATen/ATen.h" OR NOT EXISTS "${ROCM_ROOT}/include/hip/hip_runtime_api.h")
  message(FATAL_ERROR "Set TORCH_ROOT and ROCM_ROOT to the explicit private native dependencies")
endif()
add_executable(uta-libtorch-device-check main.cpp)
target_compile_features(uta-libtorch-device-check PRIVATE cxx_std_20)
target_compile_options(uta-libtorch-device-check PRIVATE -Wall -Wextra)
target_compile_definitions(uta-libtorch-device-check PRIVATE
  _GLIBCXX_USE_CXX11_ABI=${TORCH_CXX_ABI} __HIP_PLATFORM_AMD__=1)
target_include_directories(uta-libtorch-device-check SYSTEM PRIVATE
  "${TORCH_ROOT}/include" "${TORCH_ROOT}/include/torch/csrc/api/include" "${ROCM_ROOT}/include")
foreach(component torch torch_cpu torch_hip c10 c10_hip)
  find_library(native_${component} NAMES ${component} PATHS "${TORCH_ROOT}/lib" NO_DEFAULT_PATH REQUIRED)
  # Keep registration libraries even when no direct unresolved symbol names them.
  target_link_libraries(uta-libtorch-device-check PRIVATE "-Wl,--no-as-needed" "${native_${component}}" "-Wl,--as-needed")
endforeach()
find_library(native_hip NAMES amdhip64 PATHS "${ROCM_ROOT}/lib" NO_DEFAULT_PATH REQUIRED)
target_link_libraries(uta-libtorch-device-check PRIVATE "${native_hip}")
set_target_properties(uta-libtorch-device-check PROPERTIES
  BUILD_RPATH "${TORCH_ROOT}/lib;${ROCM_ROOT}/lib;${ROCM_ROOT}/lib/rocm_sysdeps/lib")
