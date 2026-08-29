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
        self.0 & region.mask() != 0
    }

    pub(crate) fn requires_full_rebuild(self) -> bool {
        self.contains(UiDirtyRegion::Chrome)
    }
}

impl fmt::Debug for UiDirtyRegions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

#[derive(Resource, Default)]
pub(crate) struct UiInvalidated {
    dirty: bool,
    regions: UiDirtyRegions,
}

impl UiInvalidated {
    pub(crate) fn invalidate(&mut self, region: UiDirtyRegion) {
        self.dirty = true;
        self.regions.insert(region);
    }

    pub(crate) fn take(&mut self) -> Option<UiDirtyRegions> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(std::mem::take(&mut self.regions))
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
    fn chrome_invalidations_request_a_full_rebuild() {
        let mut chrome = UiInvalidated::default();
        chrome.invalidate(UiDirtyRegion::Chrome);
        assert!(chrome.take().unwrap().requires_full_rebuild());
    }
}
