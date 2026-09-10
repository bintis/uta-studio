use super::*;

#[test]
fn shared_task_context_keeps_cache_alive_until_the_final_owner_finishes() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "uta-studio-shared-task-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let scope =
        AccelerationGuard::enter(true, Vec::new(), &directory, &CancellationToken::default());
    let cache = scope.audio_cache_directory().unwrap();
    let expected = cache.clone();
    let snapshot = AccelerationSnapshot::capture();
    let (started, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let child = std::thread::spawn(move || {
        let _scope = snapshot.enter();
        started.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let config = task_config(&serde_json::json!({"semantic_output": "fixture"}));
        assert_eq!(config["turbo_acceleration"], true);
        assert_eq!(config["audio_cache_directory"], serde_json::json!(expected));
        assert!(expected.is_dir());
        // A disabled nested request suppresses inheritance, then restores it.
        let disabled =
            AccelerationGuard::enter(false, Vec::new(), &expected, &CancellationToken::default());
        assert!(
            task_config(&serde_json::json!({}))
                .get("turbo_acceleration")
                .is_none()
        );
        drop(disabled);
        assert_eq!(
            task_config(&serde_json::json!({}))["turbo_acceleration"],
            true
        );
    });
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(scope);
    assert!(cache.is_dir());
    assert!(
        task_config(&serde_json::json!({}))
            .get("turbo_acceleration")
            .is_none()
    );
    release.send(()).unwrap();
    child.join().unwrap();
    assert!(!cache.exists());
    std::fs::remove_dir(&directory).unwrap();
}

#[test]
fn independently_entered_tasks_share_one_preparation_cursor_and_attempt_set() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "uta-studio-shared-cursor-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let scope =
        AccelerationGuard::enter(true, Vec::new(), &directory, &CancellationToken::default());
    let workers = (0..4)
        .map(|index| {
            let snapshot = AccelerationSnapshot::capture();
            std::thread::spawn(move || {
                let _scope = snapshot.enter();
                with_context(|context| {
                    context
                        .unwrap()
                        .attempted_tasks
                        .insert(format!("task-{index}"));
                });
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        with_context(|context| context.unwrap().attempted_tasks.len()),
        4
    );
    drop(scope);
    std::fs::remove_dir(&directory).unwrap();
}
