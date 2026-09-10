//! Request-owned independent work. Join before output/cache ownership ends.
use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct Owner {
    pub cancellation: CancellationToken,
    failure: Arc<Mutex<Option<EngineError>>>,
}
impl Owner {
    pub fn new(caller: &CancellationToken) -> Self {
        Self {
            cancellation: caller.child(),
            failure: Arc::new(Mutex::new(None)),
        }
    }
    fn failed(&self, error: &EngineError) {
        if error.code != EngineErrorCode::Cancelled {
            self.failure
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get_or_insert_with(|| error.clone());
        }
        self.cancellation.cancel();
    }
    pub fn finish<T>(&self, result: EngineResult<T>) -> EngineResult<T> {
        if (result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.code == EngineErrorCode::Cancelled))
            && let Some(error) = self
                .failure
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
        {
            return Err(error);
        }
        result
    }
    pub fn start<T: Send + 'static>(
        &self,
        parallel: bool,
        execute: impl FnOnce() -> EngineResult<T> + Send + 'static,
    ) -> EngineResult<Task<T>> {
        if !parallel {
            return Ok(Task {
                owner: self.clone(),
                handle: None,
                ready: Some(execute()?),
            });
        }
        let audio = crate::audio::reuse::Snapshot::capture();
        let events = crate::events::EventSnapshot::capture();
        let acceleration = crate::execution::AccelerationSnapshot::capture();
        let work = Arc::new(Mutex::new(Some(execute)));
        let child_work = Arc::clone(&work);
        let owner = self.clone();
        let handle = std::thread::Builder::new()
            .name("uta-analysis-task".into())
            .spawn(move || {
                let _audio = audio.enter();
                let _events = events.enter();
                let _acceleration = acceleration.enter();
                let execute = child_work
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("owned task has one execution");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute))
                    .unwrap_or_else(|_| Err(task_panic()));
                if let Err(error) = &result {
                    owner.failed(error);
                }
                result
            });
        match handle {
            Ok(handle) => Ok(Task {
                owner: self.clone(),
                handle: Some(handle),
                ready: None,
            }),
            Err(error) => {
                crate::debug_log::record("optional_task_overlap_unavailable", &error.to_string());
                let execute = work
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("unstarted task retains its execution");
                Ok(Task {
                    owner: self.clone(),
                    handle: None,
                    ready: Some(execute()?),
                })
            }
        }
    }
}
fn task_panic() -> EngineError {
    EngineError::new(
        EngineErrorCode::InternalError,
        "owned analysis task panicked",
    )
}

pub(super) struct Task<T> {
    owner: Owner,
    handle: Option<std::thread::JoinHandle<EngineResult<T>>>,
    ready: Option<T>,
}
impl<T> Task<T> {
    pub fn join(mut self) -> EngineResult<T> {
        if let Some(value) = self.ready.take() {
            return Ok(value);
        }
        let result = self
            .handle
            .take()
            .expect("task joins once")
            .join()
            .unwrap_or_else(|_| Err(task_panic()));
        if let Err(error) = &result {
            self.owner.failed(error);
        }
        result
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.owner.cancellation.cancel();
            // The worker owns its FFmpeg reader/reaping. Never detach it while
            // output rollback or shared-cache cleanup could remove its inputs.
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    #[test]
    fn independent_work_overlaps_and_preserves_request_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&events);
        crate::events::with_event_sink(
            "owned-request",
            None,
            Vec::new(),
            Arc::new(move |event| {
                received.lock().unwrap().push(event);
            }),
            || {
                let owner = Owner::new(&CancellationToken::default());
                let (started, observed) = mpsc::channel();
                let (release, wait) = mpsc::channel();
                let task = owner
                    .start(true, move || {
                        let lifecycle = crate::events::begin_node(
                            "dsp",
                            "analysis.acoustic_dsp",
                            None,
                            "fixture",
                        );
                        started.send(()).unwrap();
                        wait.recv().unwrap();
                        lifecycle.complete();
                        Ok("evidence")
                    })
                    .unwrap();
                observed.recv_timeout(Duration::from_secs(2)).unwrap();
                // Other ready work can execute while this task is still alive.
                release.send(()).unwrap();
                assert_eq!(task.join().unwrap(), "evidence");
            },
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| event.request_id == "owned-request")
        );
    }
    #[test]
    fn background_failure_keeps_its_cause_instead_of_becoming_user_cancellation() {
        let caller = CancellationToken::default();
        let owner = Owner::new(&caller);
        let task = owner
            .start(true, || {
                Err::<(), _>(EngineError::new(
                    EngineErrorCode::DecodeFailed,
                    "actual decode failure",
                ))
            })
            .unwrap();
        assert!(task.join().is_err());
        assert!(owner.cancellation.is_cancelled());
        assert!(!caller.is_cancelled());
        let error = owner
            .finish::<()>(Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "sibling stopped",
            )))
            .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::DecodeFailed);
        assert_eq!(error.message, "actual decode failure");
    }
    #[test]
    fn early_return_cancels_and_joins_without_detaching_work() {
        let owner = Owner::new(&CancellationToken::default());
        let cancellation = owner.cancellation.clone();
        let (finished, observed) = mpsc::channel();
        let task = owner
            .start(true, move || {
                while !cancellation.is_cancelled() {
                    std::thread::yield_now();
                }
                finished.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        drop(task);
        observed.try_recv().unwrap();
    }
    #[test]
    fn ordinary_work_finishes_inline() {
        let owner = Owner::new(&CancellationToken::default());
        let caller_thread = std::thread::current().id();
        let task = owner
            .start(false, move || {
                assert_eq!(std::thread::current().id(), caller_thread);
                Ok(7)
            })
            .unwrap();
        assert_eq!(task.join().unwrap(), 7);
    }
}
