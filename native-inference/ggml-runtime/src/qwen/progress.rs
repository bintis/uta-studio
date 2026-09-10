//! Reports completed native windows, not estimated token counts or phases.
pub(super) struct Progress<'a> {
    completed: u64,
    total: u64,
    report: &'a mut dyn FnMut(u64, u64),
}
impl<'a> Progress<'a> {
    pub(super) fn new(total: usize, report: &'a mut dyn FnMut(u64, u64)) -> Self {
        let total = total as u64;
        if total > 0 {
            report(0, total);
        }
        Self {
            completed: 0,
            total,
            report,
        }
    }
    pub(super) fn run<T>(
        &mut self,
        execute: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let result = execute()?;
        self.completed += 1;
        (self.report)(self.completed, self.total);
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn reports_each_completed_window_before_the_next_and_never_counts_failure() {
        let events = RefCell::new(Vec::new());
        let mut report = |done, total| events.borrow_mut().push(format!("{done}/{total}"));
        let mut progress = Progress::new(3, &mut report);
        for name in ["first", "second"] {
            progress
                .run(|| {
                    events.borrow_mut().push(name.into());
                    Ok(())
                })
                .unwrap();
        }
        assert!(
            progress
                .run::<()>(|| Err("failed native window".into()))
                .is_err()
        );
        assert_eq!(*events.borrow(), ["0/3", "first", "1/3", "second", "2/3"]);
    }
    #[test]
    fn empty_alignment_has_no_invented_work_unit() {
        let _progress = Progress::new(0, &mut |_, _| panic!("no inference ran"));
    }
}
