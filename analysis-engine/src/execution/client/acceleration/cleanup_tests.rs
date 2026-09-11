use super::*;
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    directory: PathBuf,
    executable: PathBuf,
    input: PathBuf,
    cancellation: CancellationToken,
}
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "uta-studio-super-cleanup-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("worker");
        std::fs::write(&executable, r##"#!/usr/bin/env python3
import json, pathlib, sys, time
root = pathlib.Path(__file__).parent
def emit(frame): print(json.dumps(frame), flush=True)
emit({'type':'ready','component':'uta-ggml-worker'})
for line in sys.stdin:
    command = json.loads(line)
    if command['type'] == 'quit': break
    if command['type'] == 'prepare':
        if command['config'].get('hold'): time.sleep(60)
        emit({'type':'prepared','model_id':command['model_id'],'status':'loaded','message':'fixture weights prepared','device':'fixture GPU','free_bytes':1000000000})
    if command['type'] == 'run':
        with (root / 'executed').open('a') as output: output.write(command['model_id'] + '\n')
        emit({'type':'progress','task_id':command['task_id'],'fraction':1.0,'message':'fixture unit','work_units_completed':1,'work_units_total':1})
        if command['config'].get('fail'):
            emit({'type':'error','task_id':command['task_id'],'code':'fixture_failure','message':'fixture inference failed','retryable':False})
            continue
        target = pathlib.Path(command['output_dir']) / (command['task_id'] + '.json')
        target.write_text('{}')
        emit({'type':'output','task_id':command['task_id'],'artifact':'fixture','path':str(target),'media_type':'application/json'})
        emit({'type':'done','task_id':command['task_id'],'status':'ok'})
"##).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let input = directory.join("input.wav");
        std::fs::write(&input, b"untouched source fixture").unwrap();
        Self {
            directory,
            executable,
            input,
            cancellation: CancellationToken::default(),
        }
    }
    fn scope(&self, hold: bool) -> (AccelerationGuard, PathBuf) {
        let schedule = ["primary", "next", "actual"]
            .into_iter()
            .map(|model_id| PreloadSpec {
                model_id: model_id.to_string(),
                executable: self.executable.clone(),
                environment: BTreeMap::new(),
                config: serde_json::json!({"hold":hold}),
            })
            .collect();
        let scope = AccelerationGuard::enter(true, schedule, &self.directory, &self.cancellation);
        let cache = PathBuf::from(
            task_config(&serde_json::json!({}))["audio_cache_directory"]
                .as_str()
                .unwrap(),
        );
        (scope, cache)
    }
    fn run(&self, model_id: &str, fail: bool) -> EngineResult<Vec<NativeTaskOutput>> {
        let task = NativeTask {
            task_id: model_id.to_string(),
            node_id: "fixture".to_string(),
            presentation_node_id: None,
            model_id: model_id.to_string(),
            input_artifacts: vec![self.input.clone()],
            output_dir: self.directory.clone(),
            config: serde_json::json!({"fail":fail}),
            timeout: Duration::from_secs(5),
        };
        let expectation = WorkerExpectation {
            component: "uta-ggml-worker".to_string(),
            runtime_recipe_digest: None,
            environment: BTreeMap::new(),
        };
        SupervisedWorker::run(
            &self.executable,
            &expectation,
            &task,
            &self.cancellation,
            |_| {},
        )
    }
    fn source_unchanged(&self) {
        assert_eq!(
            std::fs::read(&self.input).unwrap(),
            b"untouched source fixture"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn pending_pid() -> u32 {
    with_context(|context| {
        context
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .process
            .as_ref()
            .unwrap()
            .child
            .id()
    })
}
fn assert_reaped(pid: u32) {
    let mut status = 0;
    // SAFETY: this is the PID of the isolated fixture child owned by this test.
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn cancellation_reaps_a_preparing_worker_before_removing_its_cache() {
    let fixture = Fixture::new();
    let (scope, cache) = fixture.scope(true);
    fixture.run("primary", false).unwrap();
    let pid = pending_pid();
    fixture.cancellation.cancel();
    let started = Instant::now();
    drop(scope);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(pid);
    assert!(!cache.exists());
    fixture.source_unchanged();
}

#[test]
fn current_failure_releases_the_unused_worker_and_cache_without_running_next_model() {
    let fixture = Fixture::new();
    let (scope, cache) = fixture.scope(false);
    assert_eq!(
        fixture.run("primary", true).unwrap_err().code,
        EngineErrorCode::WorkerFailed
    );
    let pid = pending_pid();
    drop(scope);
    assert_reaped(pid);
    assert!(!cache.exists());
    assert_eq!(
        std::fs::read_to_string(fixture.directory.join("executed")).unwrap(),
        "primary\n"
    );
    fixture.source_unchanged();
}

#[test]
fn skipped_planned_model_is_released_instead_of_being_executed() {
    let fixture = Fixture::new();
    let (scope, cache) = fixture.scope(false);
    fixture.run("primary", false).unwrap();
    let pid = pending_pid();
    fixture.run("actual", false).unwrap();
    assert_reaped(pid);
    assert_eq!(
        std::fs::read_to_string(fixture.directory.join("executed")).unwrap(),
        "primary\nactual\n"
    );
    drop(scope);
    assert!(!cache.exists());
    fixture.source_unchanged();
}
