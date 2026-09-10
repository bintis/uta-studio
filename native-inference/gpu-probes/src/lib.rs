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
    pub budget_bytes: Option<u64>,
    pub usage_bytes: Option<u64>,
}

impl VulkanDeviceProbe {
    /// Unknown budgets must not be mistaken for all installed VRAM being free.
    pub fn available_device_local_bytes(&self) -> Option<u64> {
        device_local_available(&self.memory_heaps)
    }
}

fn device_local_available(heaps: &[MemoryHeapProbe]) -> Option<u64> {
    let mut found = false;
    let mut available = 0_u64;
    for heap in heaps
        .iter()
        .filter(|heap| heap.flags & vk::MemoryHeapFlags::DEVICE_LOCAL.as_raw() != 0)
    {
        found = true;
        available = available.saturating_add(heap.budget_bytes?.saturating_sub(heap.usage_bytes?));
    }
    found.then_some(available)
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
            let mut heaps: Vec<MemoryHeapProbe> = memory.memory_heaps
                [..memory.memory_heap_count as usize]
                .iter()
                .enumerate()
                .map(|(heap_index, heap)| MemoryHeapProbe {
                    index: heap_index,
                    bytes: heap.size,
                    flags: heap.flags.as_raw(),
                    budget_bytes: None,
                    usage_bytes: None,
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
            if extensions
                .iter()
                .any(|extension| extension == "VK_EXT_memory_budget")
            {
                let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
                let mut memory_properties =
                    vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
                // SAFETY: this physical device reports memory-budget support;
                // both chained output structures remain live during the query.
                unsafe {
                    instance
                        .get_physical_device_memory_properties2(physical, &mut memory_properties)
                };
                for heap in &mut heaps {
                    heap.budget_bytes = Some(budget.heap_budget[heap.index]);
                    heap.usage_bytes = Some(budget.heap_usage[heap.index]);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn heap(budget: Option<u64>, usage: Option<u64>) -> MemoryHeapProbe {
        MemoryHeapProbe {
            index: 0,
            bytes: 10_000,
            flags: vk::MemoryHeapFlags::DEVICE_LOCAL.as_raw(),
            budget_bytes: budget,
            usage_bytes: usage,
        }
    }

    #[test]
    fn unknown_budget_does_not_use_heap_capacity() {
        assert_eq!(device_local_available(&[heap(None, None)]), None);
        assert_eq!(device_local_available(&[]), None);
        assert_eq!(
            device_local_available(&[heap(Some(1_000), Some(200)), heap(None, None)]),
            None
        );
    }

    #[test]
    fn usage_is_subtracted_from_observed_budget() {
        assert_eq!(
            device_local_available(&[heap(Some(1_000), Some(200)), heap(Some(2_000), Some(1_500))]),
            Some(1_300)
        );
        assert_eq!(
            device_local_available(&[heap(Some(1_000), Some(1_200))]),
            Some(0)
        );
    }
}

fn format_api_version(version: u32) -> String {
    format!(
        "{}.{}.{}",
        vk::api_version_major(version),
        vk::api_version_minor(version),
        vk::api_version_patch(version)
    )
}
