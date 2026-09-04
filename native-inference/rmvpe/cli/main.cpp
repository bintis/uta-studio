#include "uta_studio/rmvpe_runtime.h"
#include "uta_studio/audio.h"
#include "uta_studio/diagnostics.h"
#include <vulkan/vulkan.h>
#include <iostream>
#include <filesystem>
#include <fstream>
#include <string>
#include <chrono>
#include <cstdlib>
#include <sstream>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif

// The Vulkan environment-toggle and device-enumeration machinery below is
// identical to native-inference/roformer/cli/main.cpp's -- it is generic
// GGML/Vulkan CLI plumbing, not RoFormer-specific, and this crate is built as
// its own standalone CMake project (matching this repo's existing per-model
// native-inference layout), so it is duplicated here rather than shared.
namespace {

const std::vector<const char*> kVulkanDisableVariables = {
    "GGML_VK_DISABLE_ASYNC",
    "GGML_VK_DISABLE_BFLOAT16",
    "GGML_VK_DISABLE_COOPMAT",
    "GGML_VK_DISABLE_COOPMAT2",
    "GGML_VK_DISABLE_COOPMAT2_DECODE_VECTOR",
    "GGML_VK_DISABLE_DOT2",
    "GGML_VK_DISABLE_F16",
    "GGML_VK_DISABLE_FUSION",
    "GGML_VK_DISABLE_GRAPH_OPTIMIZE",
    "GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM",
    "GGML_VK_DISABLE_INTEGER_DOT_PRODUCT",
    "GGML_VK_DISABLE_MMVQ",
    "GGML_VK_DISABLE_MULTI_ADD",
    "GGML_VK_DISABLE_OCP_FP4",
};

void EnableAllVulkanFeatures() {
    uta_diagnostics::UnsetEnvironment("UTA_STUDIO_RMVPE_FORCE_CPU");
    for (const char* name : kVulkanDisableVariables) {
        uta_diagnostics::UnsetEnvironment(name);
    }
}

void EnableVulkanDiagnostics() {
    uta_diagnostics::SetEnvironment("GGML_VK_DISABLE_ASYNC", "1");
    uta_diagnostics::SetEnvironment("GGML_VK_DEBUG_MARKERS", "1");
    uta_diagnostics::SetEnvironment("GGML_VK_MEMORY_LOGGER", "1");
    uta_diagnostics::SetEnvironment("GGML_VK_SERIALIZE_SUBMISSIONS", "1");
    uta_diagnostics::SetEnvironment("GGML_VK_SUBMIT_LOGGER", "1");
}

void DisableVulkanDiagnostics() {
    for (const char* name : {
             "GGML_VK_DEBUG_MARKERS",
             "GGML_VK_MEMORY_LOGGER",
             "GGML_VK_SERIALIZE_SUBMISSIONS",
             "GGML_VK_SUBMIT_LOGGER",
             "GGML_VK_PERF_LOGGER",
             "GGML_VK_SYNC_LOGGER",
         }) {
        uta_diagnostics::UnsetEnvironment(name);
    }
}

void EnableVulkanWithoutAsync() {
    DisableVulkanDiagnostics();
    uta_diagnostics::SetEnvironment("GGML_VK_DISABLE_ASYNC", "1");
}

void EnableFastVulkan() {
    EnableAllVulkanFeatures();
    DisableVulkanDiagnostics();
}

void LogVulkanEnvironment() {
    for (const char* name : kVulkanDisableVariables) {
        uta_diagnostics::Log("cli", "environment", std::string("name=") + name +
                            " value=" + uta_diagnostics::GetEnvironment(name));
    }
    for (const char* name : {
             "UTA_STUDIO_RMVPE_FORCE_CPU",
             "UTA_STUDIO_VULKAN_DEVICE",
             "GGML_VK_DEBUG_MARKERS",
             "GGML_VK_MEMORY_LOGGER",
             "GGML_VK_SERIALIZE_SUBMISSIONS",
             "GGML_VK_SUBMIT_LOGGER",
             "GGML_VK_PERF_LOGGER",
             "GGML_VK_SYNC_LOGGER",
             "GGML_VK_VISIBLE_DEVICES",
         }) {
        uta_diagnostics::Log("cli", "environment", std::string("name=") + name +
                            " value=" + uta_diagnostics::GetEnvironment(name));
    }
}

std::string SecondsSince(std::chrono::steady_clock::time_point start) {
    std::ostringstream stream;
    stream << "duration_s=" << std::chrono::duration<double>(
        std::chrono::steady_clock::now() - start).count();
    return stream.str();
}

std::string JsonEscape(const std::string& value) {
    std::string escaped;
    escaped.reserve(value.size());
    for (char c : value) {
        if (c == '"' || c == '\\') {
            escaped += '\\';
        }
        escaped += c;
    }
    return escaped;
}

const char* VulkanDeviceKind(VkPhysicalDeviceType type) {
    switch (type) {
        case VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: return "gpu";
        case VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU: return "integrated_gpu";
        case VK_PHYSICAL_DEVICE_TYPE_CPU: return "cpu";
        default: return "other";
    }
}

#if defined(_WIN32)
using LibraryHandle = HMODULE;
LibraryHandle LoadVulkanLoader() { return ::LoadLibraryA("vulkan-1.dll"); }
void* LoadSymbol(LibraryHandle handle, const char* name) {
    return reinterpret_cast<void*>(::GetProcAddress(handle, name));
}
void UnloadVulkanLoader(LibraryHandle handle) { ::FreeLibrary(handle); }
#else
using LibraryHandle = void*;
LibraryHandle LoadVulkanLoader() { return ::dlopen("libvulkan.so.1", RTLD_NOW); }
void* LoadSymbol(LibraryHandle handle, const char* name) { return ::dlsym(handle, name); }
void UnloadVulkanLoader(LibraryHandle handle) { ::dlclose(handle); }
#endif

int ListVulkanDevices() {
    using PFN_vkCreateInstance = VkResult (VKAPI_PTR *)(
        const VkInstanceCreateInfo*, const VkAllocationCallbacks*, VkInstance*);
    using PFN_vkDestroyInstance = void (VKAPI_PTR *)(VkInstance, const VkAllocationCallbacks*);
    using PFN_vkEnumeratePhysicalDevices = VkResult (VKAPI_PTR *)(
        VkInstance, uint32_t*, VkPhysicalDevice*);
    using PFN_vkGetPhysicalDeviceProperties = void (VKAPI_PTR *)(
        VkPhysicalDevice, VkPhysicalDeviceProperties*);

    LibraryHandle loader = LoadVulkanLoader();
    if (!loader) {
        std::cerr << "Error: could not load the Vulkan loader for device enumeration" << std::endl;
        return 1;
    }
    auto create_instance = reinterpret_cast<PFN_vkCreateInstance>(
        LoadSymbol(loader, "vkCreateInstance"));
    auto destroy_instance = reinterpret_cast<PFN_vkDestroyInstance>(
        LoadSymbol(loader, "vkDestroyInstance"));
    auto enumerate_physical_devices = reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(
        LoadSymbol(loader, "vkEnumeratePhysicalDevices"));
    auto get_physical_device_properties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(
        LoadSymbol(loader, "vkGetPhysicalDeviceProperties"));
    if (!create_instance || !destroy_instance || !enumerate_physical_devices
        || !get_physical_device_properties) {
        std::cerr << "Error: the Vulkan loader is missing a required entry point" << std::endl;
        UnloadVulkanLoader(loader);
        return 1;
    }

    VkApplicationInfo app_info{};
    app_info.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    app_info.pApplicationName = "uta-rmvpe-runtime";
    app_info.apiVersion = VK_API_VERSION_1_1;

    VkInstanceCreateInfo instance_info{};
    instance_info.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    instance_info.pApplicationInfo = &app_info;

    VkInstance instance = VK_NULL_HANDLE;
    if (create_instance(&instance_info, nullptr, &instance) != VK_SUCCESS) {
        std::cerr << "Error: failed to create a Vulkan instance for device enumeration" << std::endl;
        UnloadVulkanLoader(loader);
        return 1;
    }

    uint32_t device_count = 0;
    enumerate_physical_devices(instance, &device_count, nullptr);
    std::vector<VkPhysicalDevice> devices(device_count);
    if (device_count > 0) {
        enumerate_physical_devices(instance, &device_count, devices.data());
    }

    std::ostringstream out;
    out << "[";
    for (uint32_t i = 0; i < device_count; ++i) {
        VkPhysicalDeviceProperties properties{};
        get_physical_device_properties(devices[i], &properties);
        if (i > 0) {
            out << ",";
        }
        out << "{\"index\":" << i
            << ",\"name\":\"" << JsonEscape(properties.deviceName) << "\""
            << ",\"kind\":\"" << VulkanDeviceKind(properties.deviceType) << "\"}";
    }
    out << "]";
    std::cout << out.str() << std::endl;

    destroy_instance(instance, nullptr);
    UnloadVulkanLoader(loader);
    return 0;
}

void WritePitchEvidenceFrames(const std::string& path, const std::vector<rmvpe::PitchFrame>& frames) {
    const std::string temporary = path + ".tmp";
    if (std::filesystem::exists(path) || std::filesystem::exists(temporary)) {
        throw std::runtime_error("refusing to overwrite RMVPE evidence output");
    }
    {
        std::ofstream file(temporary, std::ios::binary | std::ios::trunc);
        if (!file) throw std::runtime_error("could not open " + temporary + " for writing");
        file << "{\"frames\":[";
        file.precision(9);
        for (size_t i = 0; i < frames.size(); ++i) {
            const auto& frame = frames[i];
            if (i > 0) file << ",";
            file << "{\"time\":" << frame.time
                 << ",\"hz\":" << frame.hz
                 << ",\"confidence\":" << frame.confidence
                 << ",\"voiced\":" << (frame.voiced ? "true" : "false") << "}";
        }
        file << "]}\n";
        if (!file) {
            file.close();
            std::filesystem::remove(temporary);
            throw std::runtime_error("failed writing " + temporary);
        }
    }
    std::error_code error;
    std::filesystem::create_hard_link(temporary, path, error);
    if (error) {
        std::filesystem::remove(temporary);
        throw std::runtime_error("could not atomically publish " + path + ": " + error.message());
    }
    std::filesystem::remove(temporary);
}

} // namespace

void print_usage(const char* program_name) {
    std::cerr << "Usage: " << program_name << " <model.gguf> <input.wav> <output.json> [options]" << std::endl;
    std::cerr << std::endl;
    std::cerr << "input.wav must already be decoded to 16 kHz mono (see the Rust supervisor's" << std::endl;
    std::cerr << "audio::decode_mono_wav). output.json holds only {\"frames\":[...]}; the Rust" << std::endl;
    std::cerr << "supervisor wraps it with schema_version/model_id/hashes/backend." << std::endl;
    std::cerr << std::endl;
    std::cerr << "Options:" << std::endl;
    std::cerr << "  --batch-size <N>   Must be 1 (protocol parity with the RoFormer engines)" << std::endl;
    std::cerr << "  --vulkan-device <N>  Vulkan device index (default: 0)" << std::endl;
    std::cerr << "  --diagnostic-log <path>       Append stdout/stderr to a durable, synced log" << std::endl;
    std::cerr << "  --enable-all-vulkan-features  Clear every GGML_VK_DISABLE_* override" << std::endl;
    std::cerr << "  --vulkan-diagnostics          Log memory and serialized submission boundaries" << std::endl;
    std::cerr << "  --vulkan-no-async             Disable async Vulkan without serialized submissions" << std::endl;
    std::cerr << "  --vulkan-fast                 Restore async Vulkan and keep durable stage logs" << std::endl;
    std::cerr << "  --serial-pipeline             Accepted for protocol parity (this runtime has no" << std::endl;
    std::cerr << "                                concurrent pipeline stage to disable)" << std::endl;
    std::cerr << "  --machine-progress            Emit exact completed/total window records" << std::endl;
    std::cerr << "  --list-vulkan-devices         Print JSON [{index,name,kind}] and exit; no model needed" << std::endl;
    std::cerr << "  --help, -h         Show this help message" << std::endl;
}

int main(int argc, char* argv[]) {
    int batch_size = 1;
    int vulkan_device = 0;
    bool enable_all_vulkan_features = false;
    bool vulkan_diagnostics = false;
    bool vulkan_no_async = false;
    bool vulkan_fast = false;
    bool machine_progress = false;
    std::string diagnostic_log_path;

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--help" || arg == "-h") {
            print_usage(argv[0]);
            return 0;
        }
        if (arg == "--list-vulkan-devices") {
            return ListVulkanDevices();
        }
    }

    if (argc < 4) {
        print_usage(argv[0]);
        return 1;
    }

    std::string model_path = argv[1];
    std::string input_path = argv[2];
    std::string output_path = argv[3];

    for (int i = 4; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--batch-size" && i + 1 < argc) {
            try {
                batch_size = std::stoi(argv[++i]);
                if (batch_size != 1) {
                    std::cerr << "Error: batch-size must be 1" << std::endl;
                    return 1;
                }
            } catch (...) {
                std::cerr << "Error: invalid batch-size" << std::endl;
                return 1;
            }
        } else if (arg == "--vulkan-device" && i + 1 < argc) {
            try {
                vulkan_device = std::stoi(argv[++i]);
                if (vulkan_device < 0 || vulkan_device > 255) {
                    std::cerr << "Error: vulkan-device must be between 0 and 255" << std::endl;
                    return 1;
                }
            } catch (...) {
                std::cerr << "Error: invalid vulkan-device" << std::endl;
                return 1;
            }
        } else if (arg == "--diagnostic-log" && i + 1 < argc) {
            diagnostic_log_path = argv[++i];
            if (diagnostic_log_path.empty()) {
                std::cerr << "Error: diagnostic log path must not be empty" << std::endl;
                return 1;
            }
        } else if (arg == "--enable-all-vulkan-features") {
            enable_all_vulkan_features = true;
        } else if (arg == "--vulkan-diagnostics") {
            vulkan_diagnostics = true;
        } else if (arg == "--vulkan-no-async") {
            vulkan_no_async = true;
        } else if (arg == "--vulkan-fast") {
            vulkan_fast = true;
        } else if (arg == "--serial-pipeline") {
            // Accepted for CLI protocol parity with the RoFormer engines; this
            // runtime has no concurrent pipeline stage to disable.
        } else if (arg == "--machine-progress") {
            machine_progress = true;
        } else {
            std::cerr << "Unknown option: " << arg << std::endl;
            print_usage(argv[0]);
            return 1;
        }
    }

    const int vulkan_mode_count = static_cast<int>(vulkan_diagnostics) +
                                  static_cast<int>(vulkan_no_async) +
                                  static_cast<int>(vulkan_fast);
    if (vulkan_mode_count > 1) {
        std::cerr << "Error: --vulkan-diagnostics, --vulkan-no-async, and --vulkan-fast are mutually exclusive"
                  << std::endl;
        return 1;
    }
    (void)batch_size;

    try {
        if (!diagnostic_log_path.empty()) {
            uta_diagnostics::RedirectToDurableLog(diagnostic_log_path);
        }
        uta_diagnostics::Log("cli", "process.start",
            "model=" + model_path + " input=" + input_path + " output=" + output_path);

        const std::string vulkan_device_value = std::to_string(vulkan_device);
        uta_diagnostics::SetEnvironment("UTA_STUDIO_VULKAN_DEVICE", vulkan_device_value.c_str());

        if (enable_all_vulkan_features) {
            EnableAllVulkanFeatures();
        }
        if (vulkan_diagnostics) {
            EnableVulkanDiagnostics();
        }
        if (vulkan_no_async) {
            EnableVulkanWithoutAsync();
        }
        if (vulkan_fast) {
            EnableFastVulkan();
        }
        LogVulkanEnvironment();

        std::cout << "Initializing RmvpeRuntime..." << std::endl;
        const auto start_time = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "model.initialize.begin");

        RmvpeRuntime engine(model_path);
        uta_diagnostics::Log("cli", "model.initialize.end", SecondsSince(start_time));
        std::cout << "Backend: " << engine.BackendName() << std::endl;

        std::cout << "Loading audio: " << input_path << std::endl;
        const auto load_start = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "audio.load.begin", "path=" + input_path);
        MonoAudio input_audio = AudioFile::LoadMono16k(input_path);
        uta_diagnostics::Log("cli", "audio.load.end", SecondsSince(load_start));
        std::cout << "Audio loaded: " << input_audio.samples.size() << " samples at "
                  << input_audio.sample_rate << " Hz" << std::endl;

        auto process_start = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "audio.process.begin",
            "samples=" + std::to_string(input_audio.samples.size()));

        auto progress_callback = [&](int completed, int total) {
            if (machine_progress) {
                std::cout << "UTA_WORK_UNITS v1 " << completed << " " << total << std::endl;
                return;
            }
            if (!diagnostic_log_path.empty()) {
                uta_diagnostics::Log("cli", "audio.progress",
                                     "completed=" + std::to_string(completed) +
                                     " total=" + std::to_string(total));
            }
        };

        std::vector<rmvpe::PitchFrame> frames = engine.Process(input_audio.samples, progress_callback);

        auto process_end = std::chrono::steady_clock::now();
        std::chrono::duration<double> diff = process_end - process_start;
        uta_diagnostics::Log("cli", "audio.process.end",
            "duration_s=" + std::to_string(diff.count()) + " frames=" + std::to_string(frames.size()));
        std::cout << "Processed " << frames.size() << " frames in " << diff.count() << " seconds." << std::endl;

        std::cout << "Saving output: " << output_path << std::endl;
        uta_diagnostics::Log("cli", "output.save.begin", "path=" + output_path);
        WritePitchEvidenceFrames(output_path, frames);
        uta_diagnostics::Log("cli", "output.save.end", "path=" + output_path);

        std::cout << "Done!" << std::endl;
        uta_diagnostics::Log("cli", "process.end", SecondsSince(start_time));

    } catch (const std::exception& e) {
        uta_diagnostics::Log("cli", "process.error", std::string("message=") + e.what());
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
