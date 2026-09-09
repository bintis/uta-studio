//! Read-only Vulkan capability probes retained for future GGML tuning.
//! This crate is not an inference backend and never creates a logical device.

use ash::{Entry, vk};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct VulkanProbe {
    pub schema_version: u32,
    pub devices: Vec<VulkanDeviceProbe>,
}

#[derive(Debug, Serialize)]
pub struct VulkanDeviceProbe {
    pub index: usize,
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: String,
    pub api_version: String,
    pub driver_version: u32,
    pub subgroup_size: u32,
    pub subgroup_supported_stages: u32,
    pub subgroup_supported_operations: u32,
    pub queue_families: Vec<QueueFamilyProbe>,
    pub memory_heaps: Vec<MemoryHeapProbe>,
    pub extensions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct QueueFamilyProbe {
    pub index: usize,
    pub queue_count: u32,
    pub flags: u32,
}

#[derive(Debug, Serialize)]
pub struct MemoryHeapProbe {
    pub index: usize,
    pub bytes: u64,
    pub flags: u32,
}

pub fn probe_vulkan() -> Result<VulkanProbe, String> {
    // SAFETY: loading the system Vulkan loader does not create a device or
    // submit work. Every Vulkan handle below is destroyed before returning.
    let entry =
        unsafe { Entry::load() }.map_err(|error| format!("could not load Vulkan: {error}"))?;
    let application_name = c"Uta! Studio GGML probe";
    let application = vk::ApplicationInfo::default()
        .application_name(application_name)
        .application_version(1)
        .engine_name(application_name)
        .engine_version(1)
        .api_version(vk::API_VERSION_1_1);
    let create = vk::InstanceCreateInfo::default().application_info(&application);
    // SAFETY: `create` points to live stack data and has no extension chains.
    let instance = unsafe { entry.create_instance(&create, None) }
        .map_err(|error| format!("could not create Vulkan probe instance: {error}"))?;
    let result = (|| {
        // SAFETY: `instance` is live and no mutation races this local probe.
        let physical_devices = unsafe { instance.enumerate_physical_devices() }
            .map_err(|error| format!("could not enumerate Vulkan devices: {error}"))?;
        let mut devices = Vec::with_capacity(physical_devices.len());
        for (index, physical) in physical_devices.into_iter().enumerate() {
            let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
            let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
            // SAFETY: both output structs remain live for the duration of the call.
            unsafe { instance.get_physical_device_properties2(physical, &mut properties2) };
            let properties = properties2.properties;
            // SAFETY: Vulkan guarantees a NUL-terminated device_name array.
            let name = unsafe { std::ffi::CStr::from_ptr(properties.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: physical handle belongs to this live instance.
            let queues = unsafe { instance.get_physical_device_queue_family_properties(physical) }
                .into_iter()
                .enumerate()
                .map(|(queue_index, queue)| QueueFamilyProbe {
                    index: queue_index,
                    queue_count: queue.queue_count,
                    flags: queue.queue_flags.as_raw(),
                })
                .collect();
            // SAFETY: physical handle belongs to this live instance.
            let memory = unsafe { instance.get_physical_device_memory_properties(physical) };
            let heaps = memory.memory_heaps[..memory.memory_heap_count as usize]
                .iter()
                .enumerate()
                .map(|(heap_index, heap)| MemoryHeapProbe {
                    index: heap_index,
                    bytes: heap.size,
                    flags: heap.flags.as_raw(),
                })
                .collect();
            // SAFETY: physical handle belongs to this live instance.
            let mut extensions =
                unsafe { instance.enumerate_device_extension_properties(physical) }
                    .map_err(|error| {
                        format!("could not enumerate Vulkan device extensions: {error}")
                    })?
                    .into_iter()
                    .map(|extension| {
                        // SAFETY: Vulkan guarantees a NUL-terminated extension_name array.
                        unsafe { std::ffi::CStr::from_ptr(extension.extension_name.as_ptr()) }
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>();
            extensions.sort();
            devices.push(VulkanDeviceProbe {
                index,
                name,
                vendor_id: properties.vendor_id,
                device_id: properties.device_id,
                device_type: format!("{:?}", properties.device_type),
                api_version: format_api_version(properties.api_version),
                driver_version: properties.driver_version,
                subgroup_size: subgroup.subgroup_size,
                subgroup_supported_stages: subgroup.supported_stages.as_raw(),
                subgroup_supported_operations: subgroup.supported_operations.as_raw(),
                queue_families: queues,
                memory_heaps: heaps,
                extensions,
            });
        }
        Ok(VulkanProbe {
            schema_version: 1,
            devices,
        })
    })();
    // SAFETY: all physical-device queries are complete and no child objects exist.
    unsafe { instance.destroy_instance(None) };
    result
}

fn format_api_version(version: u32) -> String {
    format!(
        "{}.{}.{}",
        vk::api_version_major(version),
        vk::api_version_minor(version),
        vk::api_version_patch(version)
    )
}
