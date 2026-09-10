//! Per-analysis ownership for optional resident weights and shared PCM files.
use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct PreloadSpec {
    pub model_id: String,
    pub executable: PathBuf,
    pub config: serde_json::Value,
}

struct CacheDirectory(PathBuf);
impl Drop for CacheDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            crate::debug_log::record("acceleration_cache_cleanup_failed", &error.to_string());
        }
    }
}

struct Pending {
    spec: PreloadSpec,
    process: Option<WorkerProcess>,
    cancellation: CancellationToken,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.cancellation.is_cancelled()
            && let Some(process) = &mut self.process
        {
            let _ = process.send(&WorkerCommand::Quit);
            let _ = process.wait_for_exit(SHUTDOWN_TIMEOUT);
        }
        // WorkerProcess reaps/terminates its own child on cancellation or timeout.
    }
}

struct Context {
    // Drop the child before removing anything it may still be reading.
    pending: Option<Pending>,
    schedule: Vec<PreloadSpec>,
    cursor: usize,
    attempted_tasks: BTreeSet<String>,
    cancellation: CancellationToken,
    cache: Option<CacheDirectory>,
}
type SharedContext = Arc<Mutex<Context>>;
thread_local! { static CONTEXT: RefCell<Option<SharedContext>> = const { RefCell::new(None) }; }

#[derive(Clone)]
pub(crate) struct AccelerationSnapshot(Option<SharedContext>);
impl AccelerationSnapshot {
    pub fn capture() -> Self {
        Self(CONTEXT.with(|current| current.borrow().clone()))
    }
    pub fn enter(self) -> AccelerationGuard {
        AccelerationGuard {
            previous: CONTEXT.with(|current| current.replace(self.0)),
            thread: PhantomData,
        }
    }
}

fn with_context<T>(operation: impl FnOnce(Option<&mut Context>) -> T) -> T {
    let context = CONTEXT.with(|current| current.borrow().clone());
    let mut context = context
        .as_ref()
        .map(|context| context.lock().unwrap_or_else(|error| error.into_inner()));
    operation(context.as_deref_mut())
}

pub(crate) struct AccelerationGuard {
    previous: Option<SharedContext>,
    thread: PhantomData<Rc<()>>,
}
impl AccelerationGuard {
    pub(crate) fn audio_cache_directory(&self) -> Option<PathBuf> {
        with_context(|context| {
            context.and_then(|context| context.cache.as_ref().map(|cache| cache.0.clone()))
        })
    }

    pub fn enter(
        enabled: bool,
        schedule: Vec<PreloadSpec>,
        output: &Path,
        cancellation: &CancellationToken,
    ) -> Self {
        let context = enabled.then(|| {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let directory = output.join(format!(
                "analysis-audio-cache-{}-{nonce}",
                std::process::id()
            ));
            let cache = match std::fs::create_dir(&directory) {
                Ok(()) => Some(CacheDirectory(directory)),
                Err(error) => {
                    crate::debug_log::record("acceleration_cache_unavailable", &error.to_string());
                    None
                }
            };
            Arc::new(Mutex::new(Context {
                pending: None,
                schedule,
                cursor: 0,
                attempted_tasks: BTreeSet::new(),
                cancellation: cancellation.clone(),
                cache,
            }))
        });
        Self {
            previous: CONTEXT.with(|current| current.replace(context)),
            thread: PhantomData,
        }
    }
}
impl Drop for AccelerationGuard {
    fn drop(&mut self) {
        let context = CONTEXT.with(|current| current.replace(self.previous.take()));
        drop(context);
    }
}

pub(super) fn task_config(config: &serde_json::Value) -> serde_json::Value {
    let mut config = config.clone();
    with_context(|context| {
        if let Some(context) = context {
            config["turbo_acceleration"] = serde_json::Value::Bool(true);
            if let Some(cache) = &context.cache {
                config["audio_cache_directory"] = serde_json::json!(cache.0);
            }
        }
    });
    config
}

pub(super) fn take_for(executable: &Path, task: &NativeTask) -> Option<WorkerProcess> {
    with_context(|context| {
        let context = context?;
        if let Some(relative) = context.schedule[context.cursor..]
            .iter()
            .position(|spec| spec.model_id == task.model_id)
        {
            context.cursor += relative + 1;
        }
        let mut pending = context.pending.take()?;
        if pending.spec.model_id == task.model_id && pending.spec.executable == executable {
            pending.process.take()
        } else {
            crate::debug_log::record(
                "acceleration_unused_preload_released",
                &pending.spec.model_id,
            );
            None
        }
    })
}

pub(super) fn start_next(task: &NativeTask) -> Option<String> {
    with_context(|context| {
        let context = context?;
        if context.cancellation.is_cancelled()
            || context.pending.is_some()
            || !context.attempted_tasks.insert(task.task_id.clone())
        {
            return None;
        }
        let spec = context.schedule[context.cursor..]
            .iter()
            .find(|spec| spec.model_id != task.model_id)?
            .clone();
        let result = (|| {
            let mut process = WorkerProcess::spawn(&spec.executable)?;
            process.send(&WorkerCommand::Prepare {
                model_id: &spec.model_id,
                config: &spec.config,
            })?;
            Ok::<_, EngineError>(process)
        })();
        match result {
            Ok(process) => {
                let message = format!(
                    "Requested memory-budgeted weight preload for {}",
                    spec.model_id
                );
                context.pending = Some(Pending {
                    spec,
                    process: Some(process),
                    cancellation: context.cancellation.clone(),
                });
                Some(message)
            }
            Err(error) => Some(format!("Optional preload unavailable: {}", error.message)),
        }
    })
}

#[cfg(all(test, unix))]
mod cleanup_tests;
#[cfg(test)]
mod ownership_tests;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn next_task_consumes_the_prepared_worker_and_cache_is_removed_at_scope_exit() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "uta-studio-super-coordinator-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("worker");
        std::fs::write(&executable, r##"#!/usr/bin/env python3
import json, pathlib, sys
root = pathlib.Path(__file__).parent
with (root / 'launches').open('a') as output: output.write('launch\n')
def emit(frame): print(json.dumps(frame), flush=True)
emit({'type':'ready','component':'uta-ggml-worker'})
for line in sys.stdin:
    command = json.loads(line)
    if command['type'] == 'quit': break
    if command['type'] == 'prepare':
        emit({'type':'prepared','model_id':command['model_id'],'status':'loaded','message':'fixture weights prepared','device':'fixture GPU','free_bytes':1000000000})
    if command['type'] == 'run':
        assert command['config']['turbo_acceleration'] is True
        assert pathlib.Path(command['config']['audio_cache_directory']).is_dir()
        target = pathlib.Path(command['output_dir']) / (command['task_id'] + '.json')
        target.write_text('{}')
        emit({'type':'progress','task_id':command['task_id'],'fraction':1.0,'message':'fixture unit completed','work_units_completed':1,'work_units_total':1})
        emit({'type':'diagnostic','task_id':command['task_id'],'message':'fixture acceleration diagnostic'})
        emit({'type':'output','task_id':command['task_id'],'artifact':'fixture','path':str(target),'media_type':'application/json'})
        emit({'type':'done','task_id':command['task_id'],'status':'ok'})
"##).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let input = directory.join("input.wav");
        std::fs::write(&input, b"isolated protocol fixture").unwrap();
        let cancellation = CancellationToken::default();
        let schedule = ["primary", "next"]
            .into_iter()
            .map(|model_id| PreloadSpec {
                model_id: model_id.to_string(),
                executable: executable.clone(),
                config: serde_json::json!({}),
            })
            .collect();
        let guard = AccelerationGuard::enter(true, schedule, &directory, &cancellation);
        let cache = PathBuf::from(
            task_config(&serde_json::json!({}))["audio_cache_directory"]
                .as_str()
                .unwrap(),
        );
        let expectation = WorkerExpectation {
            component: "uta-ggml-worker".to_string(),
            runtime_recipe_digest: None,
        };
        for model_id in ["primary", "next"] {
            let task = NativeTask {
                task_id: model_id.to_string(),
                node_id: "fixture".to_string(),
                presentation_node_id: None,
                model_id: model_id.to_string(),
                input_artifacts: vec![input.clone()],
                output_dir: directory.clone(),
                config: serde_json::json!({}),
                timeout: Duration::from_secs(5),
            };
            SupervisedWorker::run(&executable, &expectation, &task, &cancellation, |_| {}).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(directory.join("launches"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert!(cache.is_dir());
        drop(guard);
        assert!(!cache.exists());
        assert!(input.is_file());
        assert!(
            task_config(&serde_json::json!({}))
                .get("turbo_acceleration")
                .is_none()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn disabled_nested_request_does_not_inherit_acceleration() {
        let cancellation = CancellationToken::default();
        let outer = AccelerationGuard::enter(
            true,
            Vec::new(),
            Path::new("/nonexistent/uta-studio-fixture"),
            &cancellation,
        );
        assert_eq!(
            task_config(&serde_json::json!({}))["turbo_acceleration"],
            true
        );
        let inner = AccelerationGuard::enter(
            false,
            Vec::new(),
            Path::new("/nonexistent/uta-studio-fixture"),
            &cancellation,
        );
        assert!(
            task_config(&serde_json::json!({}))
                .get("turbo_acceleration")
                .is_none()
        );
        drop(inner);
        assert_eq!(
            task_config(&serde_json::json!({}))["turbo_acceleration"],
            true
        );
        drop(outer);
    }
}
