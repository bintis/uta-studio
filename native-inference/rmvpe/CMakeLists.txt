cmake_minimum_required(VERSION 3.20)

project(uta_studio_rmvpe_runtime VERSION 0.1.0 LANGUAGES C CXX)

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
    message(FATAL_ERROR "Uta! Studio RMVPE runtime requires -DGGML_VULKAN=ON")
endif()

add_library(uta_rmvpe_runtime STATIC
    src/diagnostics.cpp
    src/graph.cpp
    src/rmvpe_runtime.cpp
)
target_compile_definitions(uta_rmvpe_runtime PUBLIC HAVE_GGML_VULKAN=1)
target_include_directories(uta_rmvpe_runtime
    PUBLIC include
    PRIVATE src
)
target_link_libraries(uta_rmvpe_runtime PUBLIC ggml ggml-vulkan)

if(MSVC)
    target_compile_options(uta_rmvpe_runtime PRIVATE /W3 /utf-8)
else()
    target_compile_options(uta_rmvpe_runtime PRIVATE -Wall -Wextra)
endif()

add_executable(uta-rmvpe-runtime
    cli/main.cpp
    src/audio.cpp
)
target_include_directories(uta-rmvpe-runtime PRIVATE src third_party)
target_link_libraries(uta-rmvpe-runtime PRIVATE uta_rmvpe_runtime)

# --list-vulkan-devices enumerates physical devices directly (ggml-vulkan.h
# exposes a device count and name but not VkPhysicalDeviceType). It resolves
# the loader with dlopen/LoadLibrary at runtime rather than linking it, so
# this only needs the Vulkan headers at build time -- not a library to link,
# and not a dependency on ggml-vulkan's internal, unexported find_package
# target.
find_package(Vulkan REQUIRED)
target_include_directories(uta-rmvpe-runtime PRIVATE ${Vulkan_INCLUDE_DIRS})
target_link_libraries(uta-rmvpe-runtime PRIVATE ${CMAKE_DL_LIBS})
