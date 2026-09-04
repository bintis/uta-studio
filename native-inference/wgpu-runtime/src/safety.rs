use serde::{Deserialize, Serialize};

pub const SAFETY_PROFILE: &str = "wgpu-vulkan-serial-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Gpu,
    DiscreteGpu,
    IntegratedGpu,
}

impl DeviceClass {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("gpu") {
            "gpu" => Ok(Self::Gpu),
            "discrete_gpu" => Ok(Self::DiscreteGpu),
            "integrated_gpu" => Ok(Self::IntegratedGpu),
            value => Err(format!(
                "unsupported gpu_device_class `{value}`; expected gpu, discrete_gpu, or integrated_gpu"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuSafetyConfig {
    pub batch_size: u32,
    pub vulkan_no_async: bool,
    pub serial_pipeline: bool,
    pub device_class: DeviceClass,
}

impl GpuSafetyConfig {
    /// Validates the production safety profile without touching Vulkan.
    pub fn from_worker_config(config: &serde_json::Value) -> Result<Self, String> {
        let profile = config
            .get("gpu_safety_profile")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "GPU execution requires gpu_safety_profile".to_string())?;
        if profile != SAFETY_PROFILE {
            return Err(format!(
                "unsupported gpu_safety_profile `{profile}`; expected `{SAFETY_PROFILE}`"
            ));
        }

        let batch_size = config
            .get("batch_size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "GPU execution requires integer batch_size=1".to_string())?;
        if batch_size != 1 {
            return Err(format!(
                "GPU safety profile requires batch_size=1, got {batch_size}"
            ));
        }

        let vulkan_no_async = config
            .get("vulkan_no_async")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| "GPU execution requires vulkan_no_async=true".to_string())?;
        if !vulkan_no_async {
            return Err("GPU safety profile requires vulkan_no_async=true".to_string());
        }

        let serial_pipeline = config
            .get("serial_pipeline")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| "GPU execution requires serial_pipeline=true".to_string())?;
        if !serial_pipeline {
            return Err("GPU safety profile requires serial_pipeline=true".to_string());
        }

        let device_class = DeviceClass::parse(
            config
                .get("gpu_device_class")
                .and_then(serde_json::Value::as_str),
        )?;

        Ok(Self {
            batch_size: 1,
            vulkan_no_async: true,
            serial_pipeline: true,
            device_class,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionDevice {
    Cpu,
    Gpu(GpuSafetyConfig),
}

impl ExecutionDevice {
    /// Parses and validates execution intent. This function is deliberately
    /// GPU-independent so a malformed or unsafe request fails before any
    /// Vulkan loader or driver is touched.
    pub fn from_worker_config(config: &serde_json::Value) -> Result<Self, String> {
        match config
            .get("device")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("cpu")
        {
            "cpu" => Ok(Self::Cpu),
            "gpu" => GpuSafetyConfig::from_worker_config(config).map(Self::Gpu),
            value => Err(format!(
                "unsupported native worker device `{value}`; expected cpu or gpu"
            )),
        }
    }

    pub fn backend_label(self) -> &'static str {
        match self {
            Self::Cpu => "gguf_native_cpu",
            Self::Gpu(_) => "wgpu_vulkan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_gpu() -> serde_json::Value {
        serde_json::json!({
            "device": "gpu",
            "gpu_safety_profile": SAFETY_PROFILE,
            "batch_size": 1,
            "vulkan_no_async": true,
            "serial_pipeline": true,
            "gpu_device_class": "discrete_gpu"
        })
    }

    #[test]
    fn cpu_is_the_non_gpu_default() {
        assert_eq!(
            ExecutionDevice::from_worker_config(&serde_json::json!({})).unwrap(),
            ExecutionDevice::Cpu
        );
    }

    #[test]
    fn exact_serial_gpu_profile_is_accepted_without_creating_a_device() {
        assert_eq!(
            ExecutionDevice::from_worker_config(&valid_gpu()).unwrap(),
            ExecutionDevice::Gpu(GpuSafetyConfig {
                batch_size: 1,
                vulkan_no_async: true,
                serial_pipeline: true,
                device_class: DeviceClass::DiscreteGpu,
            })
        );
    }

    #[test]
    fn each_unsafe_gpu_value_fails_closed() {
        for (key, value) in [
            ("batch_size", serde_json::json!(2)),
            ("vulkan_no_async", serde_json::json!(false)),
            ("serial_pipeline", serde_json::json!(false)),
        ] {
            let mut config = valid_gpu();
            config[key] = value;
            assert!(
                ExecutionDevice::from_worker_config(&config).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn missing_profile_fails_closed() {
        let mut config = valid_gpu();
        config.as_object_mut().unwrap().remove("gpu_safety_profile");
        assert!(ExecutionDevice::from_worker_config(&config).is_err());
    }
}
