// Enumerate real Level Zero metric groups; test stream permission without modifying it.
#include <level_zero/ze_api.h>
#include <level_zero/zet_api.h>
#include <iomanip>
#include <iostream>
#include <vector>

static void status(const char* operation, ze_result_t result) {
    std::cout << "METRIC_STATUS " << operation << " 0x" << std::hex << static_cast<unsigned>(result) << std::dec << '\n';
}
int main() {
    std::cout.setf(std::ios::unitbuf);
    auto result = zeInit(ZE_INIT_FLAG_GPU_ONLY);
    status("zeInit", result);
    if (result != ZE_RESULT_SUCCESS) return 2;
    uint32_t count = 0;
    result = zeDriverGet(&count, nullptr); status("zeDriverGet", result);
    std::vector<ze_driver_handle_t> drivers(count);
    zeDriverGet(&count, drivers.data());
    for (auto driver : drivers) {
        ze_driver_properties_t driver_properties{ZE_STRUCTURE_TYPE_DRIVER_PROPERTIES};
        zeDriverGetProperties(driver, &driver_properties);
        std::cout << "DRIVER_VERSION " << driver_properties.driverVersion << '\n';
        ze_context_desc_t context_descriptor{ZE_STRUCTURE_TYPE_CONTEXT_DESC};
        ze_context_handle_t context{};
        result = zeContextCreate(driver, &context_descriptor, &context); status("zeContextCreate", result);
        if (result != ZE_RESULT_SUCCESS) continue;
        uint32_t device_count = 0;
        zeDeviceGet(driver, &device_count, nullptr);
        std::vector<ze_device_handle_t> devices(device_count);
        zeDeviceGet(driver, &device_count, devices.data());
        for (auto device : devices) {
            ze_device_properties_t properties{ZE_STRUCTURE_TYPE_DEVICE_PROPERTIES};
            zeDeviceGetProperties(device, &properties);
            std::cout << "DEVICE " << properties.name << " device_id=" << properties.deviceId
                << " slices=" << properties.numSlices << " subslices_per_slice=" << properties.numSubslicesPerSlice
                << " eus_per_subslice=" << properties.numEUsPerSubslice << " threads_per_eu=" << properties.numThreadsPerEU
                << " core_clock_mhz=" << properties.coreClockRate << '\n';
            uint32_t group_count = 0;
            result = zetMetricGroupGet(device, &group_count, nullptr); status("zetMetricGroupGet", result);
            std::cout << "METRIC_GROUP_COUNT " << group_count << '\n';
            if (result != ZE_RESULT_SUCCESS || !group_count) continue;
            std::vector<zet_metric_group_handle_t> groups(group_count);
            zetMetricGroupGet(device, &group_count, groups.data());
            bool attempted = false;
            for (auto group : groups) {
                zet_metric_group_properties_t group_properties{ZET_STRUCTURE_TYPE_METRIC_GROUP_PROPERTIES};
                zetMetricGroupGetProperties(group, &group_properties);
                std::cout << "GROUP " << group_properties.name << " sampling=" << group_properties.samplingType
                    << " domain=" << group_properties.domain << " count=" << group_properties.metricCount << '\n';
                uint32_t metric_count = 0;
                zetMetricGet(group, &metric_count, nullptr);
                std::vector<zet_metric_handle_t> metrics(metric_count);
                zetMetricGet(group, &metric_count, metrics.data());
                for (auto metric : metrics) {
                    zet_metric_properties_t metric_properties{ZET_STRUCTURE_TYPE_METRIC_PROPERTIES};
                    zetMetricGetProperties(metric, &metric_properties);
                    std::cout << "METRIC " << group_properties.name << '|' << metric_properties.name << '|'
                        << metric_properties.resultUnits << '|' << metric_properties.description << '\n';
                }
                if (!attempted && (group_properties.samplingType & ZET_METRIC_GROUP_SAMPLING_TYPE_FLAG_TIME_BASED)) {
                    attempted = true;
                    result = zetContextActivateMetricGroups(context, device, 1, &group);
                    status("zetContextActivateMetricGroups", result);
                    if (result == ZE_RESULT_SUCCESS) {
                        zet_metric_streamer_desc_t descriptor{ZET_STRUCTURE_TYPE_METRIC_STREAMER_DESC};
                        descriptor.notifyEveryNReports = 100;
                        descriptor.samplingPeriod = 1000000;
                        zet_metric_streamer_handle_t streamer{};
                        result = zetMetricStreamerOpen(context, device, group, &descriptor, nullptr, &streamer);
                        status("zetMetricStreamerOpen", result);
                        if (result == ZE_RESULT_SUCCESS) zetMetricStreamerClose(streamer);
                        zetContextActivateMetricGroups(context, device, 0, nullptr);
                    }
                }
            }
        }
        zeContextDestroy(context);
    }
}
