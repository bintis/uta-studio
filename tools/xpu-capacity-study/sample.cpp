// Read-only global Level Zero counters. No workload execution or system setting changes.
#include "common.hpp"
#include <level_zero/ze_api.h>
#include <level_zero/zet_api.h>
#include <csignal>
#include <fstream>
#include <limits>
#include <poll.h>
#include <sstream>
#include <unistd.h>

static volatile std::sig_atomic_t stopped = 0;
static void stop(int) { stopped = 1; }
static int64_t epoch() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
}
static void check(ze_result_t result, const char* name) {
    if (result != ZE_RESULT_SUCCESS) {
        std::ostringstream message;
        message << name << " 0x" << std::hex << static_cast<unsigned>(result);
        throw std::runtime_error(message.str());
    }
}
struct Session {
    ze_context_handle_t context{};
    ze_device_handle_t device{};
    zet_metric_streamer_handle_t streamer{};
    ~Session() {
        if (streamer) zetMetricStreamerClose(streamer);
        if (context && device) zetContextActivateMetricGroups(context, device, 0, nullptr);
        if (context) zeContextDestroy(context);
    }
};
static void scalar(const zet_typed_value_t& item) {
    switch (item.type) {
        case ZET_VALUE_TYPE_UINT32: std::cout << item.value.ui32; break;
        case ZET_VALUE_TYPE_UINT64: std::cout << item.value.ui64; break;
        case ZET_VALUE_TYPE_FLOAT32:
            if (std::isfinite(item.value.fp32)) std::cout << item.value.fp32;
            else std::cout << "null";
            break;
        case ZET_VALUE_TYPE_FLOAT64:
            if (std::isfinite(item.value.fp64)) std::cout << item.value.fp64;
            else std::cout << "null";
            break;
        case ZET_VALUE_TYPE_BOOL8: std::cout << (item.value.b8 ? 1 : 0); break;
        default: throw std::runtime_error("unsupported metric value type");
    }
}
int main(int argc, char** argv) {
    try {
        if (argc != 3) throw std::invalid_argument("usage: sample GROUP RAW_OUTPUT; close stdin to stop");
        const std::string requested = argv[1];
        std::ofstream raw(argv[2], std::ios::binary | std::ios::out);
        if (!raw) throw std::runtime_error("cannot create raw output");
        std::signal(SIGINT, stop);
        std::signal(SIGTERM, stop);
        std::cout << std::setprecision(17);
        check(zeInit(ZE_INIT_FLAG_GPU_ONLY), "zeInit");
        uint32_t count = 0;
        check(zeDriverGet(&count, nullptr), "zeDriverGet count");
        std::vector<ze_driver_handle_t> drivers(count);
        check(zeDriverGet(&count, drivers.data()), "zeDriverGet");
        Session session;
        zet_metric_group_handle_t selected{};
        zet_metric_group_properties_t properties{ZET_STRUCTURE_TYPE_METRIC_GROUP_PROPERTIES};
        ze_device_properties_t device_properties{ZE_STRUCTURE_TYPE_DEVICE_PROPERTIES};
        for (auto driver : drivers) {
            uint32_t device_count = 0;
            check(zeDeviceGet(driver, &device_count, nullptr), "zeDeviceGet count");
            std::vector<ze_device_handle_t> devices(device_count);
            check(zeDeviceGet(driver, &device_count, devices.data()), "zeDeviceGet");
            for (auto device : devices) {
                check(zeDeviceGetProperties(device, &device_properties), "zeDeviceGetProperties");
                if (device_properties.deviceId != 0xe20b) continue;
                uint32_t group_count = 0;
                check(zetMetricGroupGet(device, &group_count, nullptr), "zetMetricGroupGet count");
                std::vector<zet_metric_group_handle_t> groups(group_count);
                check(zetMetricGroupGet(device, &group_count, groups.data()), "zetMetricGroupGet");
                for (auto group : groups) {
                    check(zetMetricGroupGetProperties(group, &properties), "zetMetricGroupGetProperties");
                    if (requested == properties.name &&
                        (properties.samplingType & ZET_METRIC_GROUP_SAMPLING_TYPE_FLAG_TIME_BASED)) {
                        selected = group;
                        session.device = device;
                        ze_context_desc_t descriptor{ZE_STRUCTURE_TYPE_CONTEXT_DESC};
                        check(zeContextCreate(driver, &descriptor, &session.context), "zeContextCreate");
                        break;
                    }
                }
                if (selected) break;
            }
            if (selected) break;
        }
        if (!selected) throw std::runtime_error("requested time-based group not found on B580");
        zet_metric_global_timestamps_resolution_exp_t resolution{
            ZET_STRUCTURE_TYPE_METRIC_GLOBAL_TIMESTAMPS_RESOLUTION_EXP};
        properties.pNext = &resolution;
        check(zetMetricGroupGetProperties(selected, &properties), "metric timestamp resolution");
        uint32_t metric_count = 0;
        check(zetMetricGet(selected, &metric_count, nullptr), "zetMetricGet count");
        std::vector<zet_metric_handle_t> metrics(metric_count);
        check(zetMetricGet(selected, &metric_count, metrics.data()), "zetMetricGet");
        std::cout << "{\"type\":\"metadata\",\"group\":" << probe::quote(requested)
            << ",\"device\":" << probe::quote(device_properties.name)
            << ",\"uid\":" << getuid() << ",\"timer_hz\":" << resolution.timerResolution
            << ",\"timestamp_bits\":" << resolution.timestampValidBits
            << ",\"scope\":\"device-wide time-based counters; not process filtered\",\"metrics\":[";
        for (uint32_t index = 0; index < metric_count; ++index) {
            zet_metric_properties_t description{ZET_STRUCTURE_TYPE_METRIC_PROPERTIES};
            check(zetMetricGetProperties(metrics[index], &description), "zetMetricGetProperties");
            if (index) std::cout << ',';
            std::cout << "{\"name\":" << probe::quote(description.name)
                << ",\"unit\":" << probe::quote(description.resultUnits)
                << ",\"description\":" << probe::quote(description.description) << '}';
        }
        std::cout << "]}\n";
        auto calibrate = [&] {
            uint64_t host = 0, ticks = 0;
            const auto begin = epoch();
            check(zetMetricGroupGetGlobalTimestampsExp(selected, true, &host, &ticks), "metric timestamps");
            const auto end = epoch();
            std::cout << "{\"type\":\"calibration\",\"epoch_before_ns\":" << begin
                << ",\"epoch_after_ns\":" << end << ",\"driver_host_ns\":" << host
                << ",\"metric_ticks\":" << ticks << "}\n" << std::flush;
        };
        check(zetContextActivateMetricGroups(session.context, session.device, 1, &selected), "activate");
        zet_metric_streamer_desc_t descriptor{ZET_STRUCTURE_TYPE_METRIC_STREAMER_DESC};
        descriptor.notifyEveryNReports = 100;
        descriptor.samplingPeriod = 1000000;
        check(zetMetricStreamerOpen(session.context, session.device, selected, &descriptor, nullptr,
                                  &session.streamer), "stream open");
        std::cout << "{\"type\":\"stream\",\"period_ns\":" << descriptor.samplingPeriod << "}\n";
        calibrate();
        std::cerr << "READY\n" << std::flush;
        uint64_t batches = 0, rows = 0, warnings = 0;
        const auto started = std::chrono::steady_clock::now();
        bool done = false;
        do {
            pollfd input{STDIN_FILENO, POLLIN | POLLHUP, 0};
            if (poll(&input, 1, 10) > 0) done = true;
            size_t size = 0;
            auto status = zetMetricStreamerReadData(session.streamer, UINT32_MAX, &size, nullptr);
            if (status == ZE_RESULT_WARNING_DROPPED_DATA) ++warnings;
            else check(status, "stream read size");
            if (size) {
                std::vector<uint8_t> buffer(size);
                status = zetMetricStreamerReadData(session.streamer, UINT32_MAX, &size, buffer.data());
                if (status == ZE_RESULT_WARNING_DROPPED_DATA) ++warnings;
                else check(status, "stream read");
                // File format: native little-endian uint64 byte length, followed by exact driver bytes.
                const uint64_t length = size;
                raw.write(reinterpret_cast<const char*>(&length), sizeof(length));
                raw.write(reinterpret_cast<const char*>(buffer.data()), size);
                if (!raw) throw std::runtime_error("raw stream write failed");
                uint32_t value_count = 0;
                status = zetMetricGroupCalculateMetricValues(selected,
                    ZET_METRIC_GROUP_CALCULATION_TYPE_METRIC_VALUES, size, buffer.data(), &value_count, nullptr);
                if (status == ZE_RESULT_WARNING_DROPPED_DATA) ++warnings;
                else check(status, "calculate count");
                std::vector<zet_typed_value_t> values(value_count);
                status = zetMetricGroupCalculateMetricValues(selected,
                    ZET_METRIC_GROUP_CALCULATION_TYPE_METRIC_VALUES, size, buffer.data(), &value_count, values.data());
                if (status == ZE_RESULT_WARNING_DROPPED_DATA) ++warnings;
                else check(status, "calculate values");
                if (value_count % metric_count) throw std::runtime_error("metric tuple length mismatch");
                const auto read_epoch = epoch();
                for (uint32_t offset = 0; offset < value_count; offset += metric_count) {
                    std::cout << "{\"type\":\"sample\",\"batch\":" << batches
                        << ",\"read_epoch_ns\":" << read_epoch << ",\"values\":[";
                    for (uint32_t index = 0; index < metric_count; ++index) {
                        if (index) std::cout << ',';
                        scalar(values[offset + index]);
                    }
                    std::cout << "]}\n";
                    ++rows;
                }
                ++batches;
            }
            if (std::chrono::steady_clock::now() - started > std::chrono::seconds(120)) done = true;
            std::cout.flush();
        } while (!done && !stopped);
        calibrate();
        std::cout << "{\"type\":\"completion\",\"rows\":" << rows << ",\"batches\":" << batches
            << ",\"dropped_data_warnings\":" << warnings << "}\n" << std::flush;
        return warnings ? 4 : 0;
    } catch (const std::exception& error) {
        std::cerr << "METRIC_ERROR " << error.what() << '\n';
        return 2;
    }
}
