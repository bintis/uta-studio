use std::io::{Read, Write};
use std::path::Path;

use crate::error::{RuntimeManagerError, RuntimeManagerResult};

/// Explicit acquisition boundary used only by confirmed mutation operations.
/// Read-only Runtime Manager APIs never receive or call a transport.
pub trait AcquisitionTransport {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        maximum_bytes: Option<u64>,
    ) -> RuntimeManagerResult<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HttpAcquisitionTransport;

impl AcquisitionTransport for HttpAcquisitionTransport {
    fn download(
        &self,
        url: &str,
        destination: &Path,
        maximum_bytes: Option<u64>,
    ) -> RuntimeManagerResult<()> {
        let response = ureq::get(url).call().map_err(|error| {
            RuntimeManagerError::new("network_failed", format!("download failed: {error}"))
                .retryable()
        })?;
        let declared_bytes = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if maximum_bytes
            .zip(declared_bytes)
            .is_some_and(|(maximum, declared)| declared > maximum)
        {
            return Err(RuntimeManagerError::new(
                "integrity_mismatch",
                "remote artifact exceeds the pinned size",
            ));
        }
        let mut body = response.into_body();
        let mut input = body.as_reader();
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|error| RuntimeManagerError::new("publish_failed", error.to_string()))?;
        let mut buffer = [0_u8; 128 * 1024];
        let mut downloaded = 0_u64;
        loop {
            let count = input.read(&mut buffer).map_err(|error| {
                RuntimeManagerError::new("network_failed", error.to_string()).retryable()
            })?;
            if count == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(count as u64);
            if maximum_bytes.is_some_and(|maximum| downloaded > maximum) {
                return Err(RuntimeManagerError::new(
                    "integrity_mismatch",
                    "download exceeded the pinned artifact size",
                ));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| RuntimeManagerError::new("publish_failed", error.to_string()))?;
        }
        output
            .sync_all()
            .map_err(|error| RuntimeManagerError::new("publish_failed", error.to_string()))
    }
}
