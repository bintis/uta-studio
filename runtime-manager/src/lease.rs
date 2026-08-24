use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use fs2::FileExt;

use crate::error::{RuntimeManagerError, RuntimeManagerResult};

#[derive(Debug)]
struct LeaseToken;

fn registry() -> &'static Mutex<BTreeMap<String, Weak<LeaseToken>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Weak<LeaseToken>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Process-local pins for immutable generations. The lease is side-effect free
/// so diagnostic resolution does not mutate the store. Engine tasks keep it
/// alive for their complete execution scope.
#[derive(Debug, Clone)]
pub struct ResourceLease {
    pins: Vec<(String, Arc<LeaseToken>)>,
    file_locks: Vec<Arc<std::fs::File>>,
}

impl ResourceLease {
    pub(crate) fn acquire(keys: impl IntoIterator<Item = String>) -> Self {
        let mut leases = registry().lock().unwrap_or_else(|error| error.into_inner());
        leases.retain(|_, token| token.strong_count() > 0);
        let pins = keys
            .into_iter()
            .map(|key| {
                let token = leases.get(&key).and_then(Weak::upgrade).unwrap_or_else(|| {
                    let token = Arc::new(LeaseToken);
                    leases.insert(key.clone(), Arc::downgrade(&token));
                    token
                });
                (key, token)
            })
            .collect();
        Self {
            pins,
            file_locks: Vec::new(),
        }
    }

    pub(crate) fn acquire_with_files(
        items: impl IntoIterator<Item = (String, Option<PathBuf>)>,
    ) -> RuntimeManagerResult<Self> {
        let items = items.into_iter().collect::<Vec<_>>();
        let mut lease = Self::acquire(items.iter().map(|(key, _)| key.clone()));
        for (_, path) in items {
            let Some(path) = path else { continue };
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(|error| {
                    RuntimeManagerError::new(
                        "resource_missing",
                        format!(
                            "could not open generation lease anchor {}: {error}",
                            path.display()
                        ),
                    )
                })?;
            FileExt::lock_shared(&file).map_err(|error| {
                RuntimeManagerError::new(
                    "resource_in_use",
                    format!("could not lease generation {}: {error}", path.display()),
                )
            })?;
            lease.file_locks.push(Arc::new(file));
        }
        Ok(lease)
    }

    pub(crate) fn path_is_locked(path: &Path) -> bool {
        let Ok(file) = std::fs::OpenOptions::new().read(true).open(path) else {
            return false;
        };
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => {
                let _ = FileExt::unlock(&file);
                false
            }
            Err(_) => true,
        }
    }

    pub(crate) fn merged(leases: impl IntoIterator<Item = ResourceLease>) -> Self {
        let mut merged = Self {
            pins: Vec::new(),
            file_locks: Vec::new(),
        };
        for lease in leases {
            merged.pins.extend(lease.pins);
            merged.file_locks.extend(lease.file_locks);
        }
        merged
    }

    pub fn generation_keys(&self) -> impl Iterator<Item = &str> {
        self.pins.iter().map(|(key, _)| key.as_str())
    }

    pub(crate) fn is_active(key: &str) -> bool {
        registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
            .is_some_and(|token| token.strong_count() > 0)
    }
}

impl PartialEq for ResourceLease {
    fn eq(&self, other: &Self) -> bool {
        self.pins.len() == other.pins.len()
            && self.file_locks.len() == other.file_locks.len()
            && self
                .pins
                .iter()
                .zip(&other.pins)
                .all(|((left_key, left), (right_key, right))| {
                    left_key == right_key && Arc::ptr_eq(left, right)
                })
    }
}

impl Eq for ResourceLease {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_anchor_uses_an_os_file_lock_without_writing_a_lease_file() {
        let path =
            std::env::temp_dir().join(format!("uta-runtime-lease-anchor-{}", std::process::id()));
        std::fs::write(&path, b"manifest").unwrap();
        let lease = ResourceLease::acquire_with_files([(
            "model:rmvpe:generation".to_string(),
            Some(path.clone()),
        )])
        .unwrap();
        assert!(ResourceLease::path_is_locked(&path));
        let merged = ResourceLease::merged([lease.clone()]);
        drop(lease);
        assert!(ResourceLease::path_is_locked(&path));
        drop(merged);
        assert!(!ResourceLease::path_is_locked(&path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn cloned_lease_keeps_generations_active() {
        let key = "model:rmvpe:generation".to_string();
        let lease = ResourceLease::acquire([key.clone()]);
        let cloned = lease.clone();
        drop(lease);
        assert!(ResourceLease::is_active(&key));
        drop(cloned);
        assert!(!ResourceLease::is_active(&key));
    }
}
