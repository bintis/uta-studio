#include "uta_studio/roformer_runtime.h"
#include "uta_studio/audio.h"
#include "uta_studio/diagnostics.h"
#include <vulkan/vulkan.h>
#include <iostream>
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
    uta_diagnostics::UnsetEnvironment("UTA_STUDIO_ROFORMER_FORCE_CPU");
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
             "UTA_STUDIO_ROFORMER_FORCE_CPU",
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
// libggml-vulkan.so.0 already depends on this exact soname, so it is either
// already resident in this process (the common case: ggml-vulkan is always
// required by this CLI) or resolvable through the loader's normal search --
// this command never assumes or requires a specific installed path.
LibraryHandle LoadVulkanLoader() { return ::dlopen("libvulkan.so.1", RTLD_NOW); }
void* LoadSymbol(LibraryHandle handle, const char* name) { return ::dlsym(handle, name); }
void UnloadVulkanLoader(LibraryHandle handle) { ::dlclose(handle); }
#endif

// Enumerates physical devices through our own minimal Vulkan instance rather
// than ggml's, since ggml-vulkan.h only exposes a device count and a name
// string -- no VkPhysicalDeviceType. This relies on vkEnumeratePhysicalDevices
// returning the same ICD-defined order every call in this process, which is
// also what lets ggml_backend_vk_init(index) address the same physical device
// this command lists at that index. The loader is resolved dynamically
// (rather than linked) so this executable carries no build-time Vulkan
// library dependency beyond what ggml-vulkan already requires.
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
    app_info.pApplicationName = "uta-roformer-runtime";
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

} // namespace

void print_usage(const char* program_name) {
    std::cerr << "Usage: " << program_name << " <model.gguf> <input.wav> <output.wav> [options]" << std::endl;
    std::cerr << std::endl;
    std::cerr << "Options:" << std::endl;
    std::cerr << "  --chunk-size <N>   Chunk size in samples (default: from model, fallback 352800)" << std::endl;
    std::cerr << "  --overlap <N>      Number of overlaps for crossfade (default: from model, fallback 2)" << std::endl;
    std::cerr << "  --batch-size <N>   Chunks per GGML graph compute (must be 1)" << std::endl;
    std::cerr << "  --vulkan-device <N>  Vulkan device index (default: 0)" << std::endl;
    std::cerr << "  --diagnostic-log <path>       Append stdout/stderr to a durable, synced log" << std::endl;
    std::cerr << "  --enable-all-vulkan-features  Clear every GGML_VK_DISABLE_* override" << std::endl;
    std::cerr << "  --vulkan-diagnostics          Log memory and serialized submission boundaries" << std::endl;
    std::cerr << "  --vulkan-no-async             Disable async Vulkan without serialized submissions" << std::endl;
    std::cerr << "  --vulkan-fast                 Restore async Vulkan and keep durable stage logs" << std::endl;
    std::cerr << "  --serial-pipeline             Run CPU preprocess, GPU compute, and CPU postprocess in order" << std::endl;
    std::cerr << "  --machine-progress            Emit exact completed/total overlap-add chunk records" << std::endl;
    std::cerr << "  --list-vulkan-devices         Print JSON [{index,name,kind}] and exit; no model needed" << std::endl;
    std::cerr << "  --help, -h         Show this help message" << std::endl;
}

int main(int argc, char* argv[]) {
    // Default values (will be overridden by model defaults if not explicitly set)
    int chunk_size = -1;  // -1 means use model default
    int num_overlap = -1; // -1 means use model default
    int batch_size = 1;
    int vulkan_device = 0;
    bool chunk_size_set = false;
    bool num_overlap_set = false;
    bool enable_all_vulkan_features = false;
    bool vulkan_diagnostics = false;
    bool vulkan_no_async = false;
    bool vulkan_fast = false;
    bool serial_pipeline = false;
    bool machine_progress = false;
    std::string diagnostic_log_path;

    // Check for flags that need no model/input/output positional args first.
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

    // Parse optional arguments
    for (int i = 4; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "--chunk-size" && i + 1 < argc) {
            try {
                chunk_size = std::stoi(argv[++i]);
                if (chunk_size <= 0) {
                     std::cerr << "Error: chunk-size must be a positive integer" << std::endl;
                     return 1;
                }
                chunk_size_set = true;
            } catch (...) {
                std::cerr << "Error: invalid chunk-size" << std::endl;
                return 1;
            }
        } else if (arg == "--overlap" && i + 1 < argc) {
            try {
                num_overlap = std::stoi(argv[++i]);
                if (num_overlap < 1) {
                    std::cerr << "Error: overlap must be at least 1" << std::endl;
                    return 1;
                }
                num_overlap_set = true;
             } catch (...) {
                std::cerr << "Error: invalid overlap" << std::endl;
                return 1;
            }
        } else if (arg == "--batch-size" && i + 1 < argc) {
            try {
                batch_size = std::stoi(argv[++i]);
                if (batch_size != 1) {
                    std::cerr << "Error: batch-size must be 1; larger batches hard-reset the current Vulkan host" << std::endl;
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
            serial_pipeline = true;
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

    try {
        if (!diagnostic_log_path.empty()) {
            uta_diagnostics::RedirectToDurableLog(diagnostic_log_path);
        }
        uta_diagnostics::Log("cli", "process.start",
            "model=" + model_path + " input=" + input_path + " output=" + output_path);

        const std::string vulkan_device_value = std::to_string(vulkan_device);
        uta_diagnostics::SetEnvironment("UTA_STUDIO_VULKAN_DEVICE", vulkan_device_value.c_str());
        uta_diagnostics::Log("cli", "runtime.parameters",
            "batch_size=" + std::to_string(batch_size) +
            " vulkan_device=" + std::to_string(vulkan_device) +
            " serial_pipeline=" + std::string(serial_pipeline ? "true" : "false"));

        if (enable_all_vulkan_features) {
            EnableAllVulkanFeatures();
            uta_diagnostics::Log("cli", "vulkan.features.enabled",
                                 "all_disable_overrides_cleared=true");
        }
        if (vulkan_diagnostics) {
            EnableVulkanDiagnostics();
            uta_diagnostics::Log("cli", "vulkan.diagnostics.enabled",
                                 "async=false memory=true serialized_submissions=true submit_boundaries=true debug_markers=true");
        }
        if (vulkan_no_async) {
            EnableVulkanWithoutAsync();
            uta_diagnostics::Log("cli", "vulkan.no_async.enabled",
                                 "async=false serialized_submissions=false submit_boundaries=false");
        }
        if (vulkan_fast) {
            EnableFastVulkan();
            uta_diagnostics::Log("cli", "vulkan.fast.enabled",
                                 "async=true serialized_submissions=false durable_stage_logs=true all_disable_overrides_cleared=true");
        }
        LogVulkanEnvironment();

        std::cout << "Initializing UtaRoformerGraph..." << std::endl;
        const auto start_time = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "model.initialize.begin");

        RoformerRuntime engine(model_path);
        uta_diagnostics::Log("cli", "model.initialize.end", SecondsSince(start_time));

        // Use model defaults if not explicitly set by user
        if (!chunk_size_set) {
            chunk_size = engine.GetDefaultChunkSize();
        }
        if (!num_overlap_set) {
            num_overlap = engine.GetDefaultNumOverlap();
        }

        std::cout << "Loading audio: " << input_path << std::endl;
        const auto load_start = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "audio.load.begin", "path=" + input_path);
        AudioBuffer input_audio = AudioFile::Load(input_path);
        uta_diagnostics::Log("cli", "audio.load.end", SecondsSince(load_start));

        std::cout << "Audio loaded: " << input_audio.samples << " samples, "
                  << input_audio.channels << " channels, "
                  << input_audio.sampleRate << " Hz" << std::endl;

        // 1. Check Sample Rate
        int required_sr = engine.GetSampleRate();
        std::cout << "Model expects sample rate: " << required_sr << " Hz" << std::endl;

        if (input_audio.sampleRate != required_sr) {
            throw std::runtime_error("Input audio sample rate must be " + std::to_string(required_sr) +
                                     " Hz. Current: " + std::to_string(input_audio.sampleRate));
        }

        // 2. Check Channels & Auto-Expand Mono
        if (input_audio.channels == 1) {
             std::cout << "[Info] Input is Mono. Expanding to Stereo..." << std::endl;
             std::vector<float> stereo_data(input_audio.samples * 2);
             for(size_t i=0; i<input_audio.samples; ++i) {
                 stereo_data[i*2 + 0] = input_audio.data[i];
                 stereo_data[i*2 + 1] = input_audio.data[i];
             }
             input_audio.data = std::move(stereo_data);
             input_audio.channels = 2;
             input_audio.samples *= 2;
        } else if (input_audio.channels != 2) {
             // We can either reject or try to process first 2 channels?
             // Ideally reject to be safer, or warn.
             throw std::runtime_error("Input audio must be Stereo (2 channels) or Mono (1 channel). Current: " + std::to_string(input_audio.channels));
        }

        std::cout << "Processing with chunk_size=" << chunk_size
                  << ", overlap=" << num_overlap
                  << ", batch_size=" << batch_size << std::endl;
        auto process_start = std::chrono::steady_clock::now();
        uta_diagnostics::Log("cli", "audio.process.begin",
            "chunk_size=" + std::to_string(chunk_size) +
            " overlap=" + std::to_string(num_overlap) +
            " batch_size=" + std::to_string(batch_size) +
            " interleaved_samples=" + std::to_string(input_audio.data.size()));

        // Exact overlap-add work-unit callback. Machine records are consumed
        // only by the owned GGML protocol worker; human CLI mode keeps a bar.
        auto progress_callback = [&](int completed, int total) {
            if (machine_progress) {
                std::cout << "UTA_WORK_UNITS v1 " << completed << " " << total << std::endl;
                return;
            }
            if (!diagnostic_log_path.empty()) {
                uta_diagnostics::Log("cli", "audio.progress",
                                     "completed=" + std::to_string(completed) +
                                     " total=" + std::to_string(total));
                return;
            }
            const float progress = static_cast<float>(completed) / static_cast<float>(total);
            int barWidth = 50;
            std::cout << "[";
            int pos = barWidth * progress;
            for (int i = 0; i < barWidth; ++i) {
                if (i < pos) std::cout << "=";
                else if (i == pos) std::cout << ">";
                else std::cout << " ";
            }
            std::cout << "] " << int(progress * 100.0) << " %\r";
            std::cout.flush();
        };

        std::vector<std::vector<float>> output_stems = engine.Process(
            input_audio.data, chunk_size, num_overlap, progress_callback, nullptr, batch_size, serial_pipeline);

        // Clear progress line
        std::cout << std::string(70, ' ') << "\r";

        auto process_end = std::chrono::steady_clock::now();
        std::chrono::duration<double> diff = process_end - process_start;
        uta_diagnostics::Log("cli", "audio.process.end", "duration_s=" + std::to_string(diff.count()));
        std::cout << "Processed in " << diff.count() << " seconds." << std::endl;

        int num_stems = output_stems.size();
        std::cout << "Model returned " << num_stems << " stems." << std::endl;

        for (int i = 0; i < num_stems; ++i) {
            // Prepare output filename
            std::string current_output_path = output_path;
            if (num_stems > 1) {
                // Insert _stem_i before extension
                size_t dot_pos = output_path.find_last_of(".");
                if (dot_pos != std::string::npos) {
                    current_output_path = output_path.substr(0, dot_pos) + "_stem_" + std::to_string(i) + output_path.substr(dot_pos);
                } else {
                    current_output_path = output_path + "_stem_" + std::to_string(i);
                }
            }

            // Prepare AudioBuffer
            AudioBuffer output_audio_buf;
            output_audio_buf.data = std::move(output_stems[i]); // Move to avoid copy
            output_audio_buf.channels = 2; // Output is always stereo
            output_audio_buf.sampleRate = required_sr;
            output_audio_buf.samples = output_audio_buf.data.size();

            std::cout << "Saving output stem " << i << ": " << current_output_path << std::endl;
            uta_diagnostics::Log("cli", "audio.save.begin",
                                 "stem=" + std::to_string(i) + " path=" + current_output_path);
            AudioFile::Save(current_output_path, output_audio_buf);
            uta_diagnostics::Log("cli", "audio.save.end",
                                 "stem=" + std::to_string(i) + " path=" + current_output_path);
        }

        std::cout << "Done!" << std::endl;
        uta_diagnostics::Log("cli", "process.end", SecondsSince(start_time));

    } catch (const std::exception& e) {
        uta_diagnostics::Log("cli", "process.error", std::string("message=") + e.what());
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
