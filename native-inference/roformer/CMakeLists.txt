cmake_minimum_required(VERSION 3.20)

project(uta_studio_roformer_runtime VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(UTA_GGML_SOURCE_DIR "" CACHE PATH "Pinned ggml source checkout")
if(NOT UTA_GGML_SOURCE_DIR)
    message(FATAL_ERROR
        "UTA_GGML_SOURCE_DIR is required. Point it at the audited ggml checkout; "
        "the build never downloads dependencies implicitly.")
endif()
if(NOT EXISTS "${UTA_GGML_SOURCE_DIR}/CMakeLists.txt")
    message(FATAL_ERROR "UTA_GGML_SOURCE_DIR does not contain ggml: ${UTA_GGML_SOURCE_DIR}")
endif()

# The helper uses GGML directly. The caller selects the compiled backend, for example:
#   -DGGML_VULKAN=ON -DGGML_CUDA=OFF -DGGML_SYCL=OFF
add_subdirectory("${UTA_GGML_SOURCE_DIR}" "${CMAKE_BINARY_DIR}/ggml" EXCLUDE_FROM_ALL)
if(NOT TARGET ggml-vulkan)
    message(FATAL_ERROR "Uta! Studio RoFormer runtime requires -DGGML_VULKAN=ON")
endif()

add_library(uta_roformer_runtime STATIC
    src/diagnostics.cpp
    src/graph.cpp
    src/roformer_runtime.cpp
    src/utils.cpp
)
target_include_directories(uta_roformer_runtime
    PUBLIC include
    PRIVATE src
)
target_link_libraries(uta_roformer_runtime PUBLIC ggml ggml-vulkan)

find_package(OpenMP)
if(OpenMP_CXX_FOUND)
    target_link_libraries(uta_roformer_runtime PUBLIC OpenMP::OpenMP_CXX)
    target_compile_definitions(uta_roformer_runtime PUBLIC USE_OPENMP)
endif()

if(MSVC)
    target_compile_options(uta_roformer_runtime PRIVATE /W3 /utf-8)
else()
    target_compile_options(uta_roformer_runtime PRIVATE -Wall -Wextra)
endif()

add_executable(uta-roformer-runtime
    cli/main.cpp
    src/audio.cpp
)
target_include_directories(uta-roformer-runtime PRIVATE src third_party)
target_link_libraries(uta-roformer-runtime PRIVATE uta_roformer_runtime)
