use std::{
    fmt,
    time::{Duration, Instant},
};

use bevy::prelude::Resource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiDirtyRegion {
    Library,
    Analysis,
    Editor,
    Dialog,
    Settings,
    Documentation,
    Chrome,
    All,
}

impl UiDirtyRegion {
    const fn mask(self) -> u8 {
        match self {
            Self::Library => 1 << 0,
            Self::Analysis => 1 << 1,
            Self::Editor => 1 << 2,
            Self::Dialog => 1 << 3,
            Self::Settings => 1 << 4,
            Self::Documentation => 1 << 5,
            Self::Chrome => 1 << 6,
            Self::All => u8::MAX,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct UiDirtyRegions(u8);

impl UiDirtyRegions {
    fn insert(&mut self, region: UiDirtyRegion) {
        self.0 |= region.mask();
    }

    pub(crate) fn contains(self, region: UiDirtyRegion) -> bool {
        if region == UiDirtyRegion::All {
            self.0 == u8::MAX
        } else {
            self.0 & region.mask() != 0
        }
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn requires_full_rebuild(self) -> bool {
        self.contains(UiDirtyRegion::All) || self.contains(UiDirtyRegion::Chrome)
    }
}

impl fmt::Debug for UiDirtyRegions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.contains(UiDirtyRegion::All) {
            return formatter.write_str("all");
        }
        let mut list = formatter.debug_list();
        for (region, name) in [
            (UiDirtyRegion::Library, "library"),
            (UiDirtyRegion::Analysis, "analysis"),
            (UiDirtyRegion::Editor, "editor"),
            (UiDirtyRegion::Dialog, "dialog"),
            (UiDirtyRegion::Settings, "settings"),
            (UiDirtyRegion::Documentation, "documentation"),
            (UiDirtyRegion::Chrome, "chrome"),
        ] {
            if self.contains(region) {
                list.entry(&name);
            }
        }
        list.finish()
    }
}

/// Transitional scoped dirty state. Existing call sites that set `.0`
/// directly still request a safe full rebuild; migrated call sites identify
/// their region so rebuild traces expose where incremental rendering has the
/// highest payoff.
#[derive(Resource, Default)]
pub(crate) struct UiInvalidated(pub(crate) bool, UiDirtyRegions);

impl UiInvalidated {
    pub(crate) fn invalidate(&mut self, region: UiDirtyRegion) {
        self.0 = true;
        self.1.insert(region);
    }

    pub(crate) fn take(&mut self) -> Option<UiDirtyRegions> {
        if !self.0 {
            return None;
        }
        self.0 = false;
        let mut regions = std::mem::take(&mut self.1);
        if regions.is_empty() {
            regions.insert(UiDirtyRegion::All);
        }
        Some(regions)
    }
}

pub(crate) struct PendingUiRebuild {
    pub(crate) sequence: u64,
    pub(crate) started: Instant,
    pub(crate) render_elapsed: Duration,
    pub(crate) old_entities: usize,
    pub(crate) regions: UiDirtyRegions,
}

#[derive(Resource, Default)]
pub(crate) struct UiRebuildMetrics {
    sequence: u64,
    pending: Option<PendingUiRebuild>,
}

impl UiRebuildMetrics {
    pub(crate) fn begin(
        &mut self,
        started: Instant,
        render_elapsed: Duration,
        old_entities: usize,
        regions: UiDirtyRegions,
    ) {
        self.sequence += 1;
        self.pending = Some(PendingUiRebuild {
            sequence: self.sequence,
            started,
            render_elapsed,
            old_entities,
            regions,
        });
    }

    pub(crate) fn finish(&mut self) -> Option<PendingUiRebuild> {
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_invalidation_is_reported_as_full() {
        let mut invalidated = UiInvalidated {
            0: true,
            ..UiInvalidated::default()
        };
        assert_eq!(invalidated.take(), Some(UiDirtyRegions(u8::MAX)));
        assert!(invalidated.take().is_none());
    }

    #[test]
    fn scoped_invalidations_are_coalesced() {
        let mut invalidated = UiInvalidated::default();
        invalidated.invalidate(UiDirtyRegion::Analysis);
        invalidated.invalidate(UiDirtyRegion::Chrome);
        let regions = invalidated.take().unwrap();
        assert!(regions.contains(UiDirtyRegion::Analysis));
        assert!(regions.contains(UiDirtyRegion::Chrome));
        assert!(!regions.contains(UiDirtyRegion::Editor));
    }

    #[test]
    fn content_regions_do_not_force_the_shell_to_rebuild() {
        for region in [
            UiDirtyRegion::Library,
            UiDirtyRegion::Analysis,
            UiDirtyRegion::Editor,
            UiDirtyRegion::Dialog,
            UiDirtyRegion::Settings,
            UiDirtyRegion::Documentation,
        ] {
            let mut invalidated = UiInvalidated::default();
            invalidated.invalidate(region);
            assert!(!invalidated.take().unwrap().requires_full_rebuild());
        }
    }

    #[test]
    fn chrome_and_legacy_invalidations_still_request_a_safe_full_rebuild() {
        let mut chrome = UiInvalidated::default();
        chrome.invalidate(UiDirtyRegion::Chrome);
        assert!(chrome.take().unwrap().requires_full_rebuild());

        let mut legacy = UiInvalidated {
            0: true,
            ..UiInvalidated::default()
        };
        assert!(legacy.take().unwrap().requires_full_rebuild());
    }
}
