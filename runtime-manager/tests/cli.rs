use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_uta-runtime")
}

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uta-runtime-cli-{label}-{}-{stamp}",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn run_with_proxy(args: &[&str], proxy: &str) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .env("HTTP_PROXY", proxy)
        .env("HTTPS_PROXY", proxy)
        .env("ALL_PROXY", proxy)
        .env("http_proxy", proxy)
        .env("https_proxy", proxy)
        .env("all_proxy", proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn json_stdout(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn read_only_json_commands_do_not_create_the_store() {
    let store = temp_path("read-only");
    let store_arg = store.to_string_lossy().into_owned();
    for command in [
        vec!["list", "--output", "json", "--store", &store_arg],
        vec!["status", "--output", "json", "--store", &store_arg],
        vec!["paths", "--output", "json", "--store", &store_arg],
        vec!["verify", "--output", "json", "--store", &store_arg],
        vec!["doctor", "--output", "json", "--store", &store_arg],
    ] {
        let output = run(&command);
        assert!(
            output.status.success(),
            "{:?}: {}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(value["schema"], "uta.runtime.result");
        assert!(!store.exists(), "{:?} mutated the store", command);
    }
}

#[test]
fn every_read_only_command_makes_zero_proxy_connections() {
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let connections = Arc::new(AtomicUsize::new(0));
    let server_stop = Arc::clone(&stop);
    let server_connections = Arc::clone(&connections);
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    server_connections.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(
                        b"HTTP/1.1 502 Network access forbidden in read test\r\nContent-Length: 0\r\n\r\n",
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("proxy audit failed: {error}"),
            }
        }
    });
    let store = temp_path("network-isolation");
    let store_arg = store.to_string_lossy().into_owned();
    for command in [
        vec!["list", "--output", "json", "--store", &store_arg],
        vec![
            "show",
            "model:qwen3_asr_1_7b",
            "--output",
            "json",
            "--store",
            &store_arg,
        ],
        vec!["status", "--output", "json", "--store", &store_arg],
        vec!["paths", "--output", "json", "--store", &store_arg],
        vec![
            "plan",
            "model:qwen3_asr_1_7b",
            "--output",
            "json",
            "--store",
            &store_arg,
        ],
        vec!["verify", "--output", "json", "--store", &store_arg],
        vec!["doctor", "--output", "json", "--store", &store_arg],
        vec![
            "resolve",
            "model:qwen3_asr_1_7b",
            "--output",
            "json",
            "--store",
            &store_arg,
        ],
        vec![
            "smoke",
            "model:qwen3_asr_1_7b",
            "--output",
            "json",
            "--store",
            &store_arg,
        ],
    ] {
        let _ = run_with_proxy(&command, &proxy);
    }
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    assert_eq!(
        connections.load(Ordering::SeqCst),
        0,
        "read commands attempted network access"
    );
    assert!(!store.exists());
}

#[test]
fn status_check_and_resolve_use_readiness_exit_codes_and_clean_json() {
    let store = temp_path("readiness");
    let store_arg = store.to_string_lossy().into_owned();
    let status = run(&[
        "status",
        "model:rmvpe",
        "--check",
        "--output",
        "json",
        "--store",
        &store_arg,
    ]);
    assert_eq!(status.status.code(), Some(10));
    assert_eq!(json_stdout(&status)["status"], "not_ready");

    let resolve = run(&[
        "resolve",
        "model:rmvpe",
        "--policy",
        "production",
        "--output",
        "json",
        "--store",
        &store_arg,
    ]);
    assert_eq!(resolve.status.code(), Some(10));
    let error = json_stdout(&resolve);
    assert_eq!(error["schema"], "uta.runtime.error");
    assert_eq!(error["code"], "resource_missing");

    let benchmark = run(&[
        "resolve",
        "model:qwen3_asr_1_7b",
        "--policy",
        "benchmark",
        "--output",
        "json",
        "--store",
        &store_arg,
    ]);
    assert_eq!(benchmark.status.code(), Some(10));
    assert_eq!(json_stdout(&benchmark)["code"], "resource_missing");
    assert!(!store.exists());
}

#[test]
fn plan_is_offline_and_ndjson_is_one_structured_result() {
    let store = temp_path("plan");
    let store_arg = store.to_string_lossy().into_owned();
    let output = run(&[
        "plan",
        "model:qwen3_asr_1_7b",
        "--policy",
        "benchmark",
        "--output",
        "ndjson",
        "--store",
        &store_arg,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = String::from_utf8(output.stdout).unwrap();
    assert_eq!(lines.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
    assert_eq!(value["type"], "result");
    let json_output = run(&[
        "plan",
        "model:qwen3_asr_1_7b",
        "--policy",
        "benchmark",
        "--output",
        "json",
        "--store",
        &store_arg,
    ]);
    assert!(json_output.status.success());
    assert_eq!(json_stdout(&json_output)["command"], "plan");
    assert!(!store.exists());
}

#[test]
fn ndjson_mutation_uses_structured_start_resource_and_error_events() {
    let store = temp_path("ndjson-mutation");
    let store_arg = store.to_string_lossy().into_owned();
    let output = run(&[
        "install",
        "model:bs_roformer_vocals_ep317",
        "--yes",
        "--output",
        "ndjson",
        "--store",
        &store_arg,
    ]);
    assert_eq!(output.status.code(), Some(14));
    assert!(output.stderr.is_empty());
    let events = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["type"], "operation_started");
    assert_eq!(events[1]["type"], "resource_started");
    assert_eq!(events[2]["type"], "error");
    assert!(!store.exists());
}

#[test]
fn non_tty_mutation_requires_yes_without_network_or_store_changes() {
    let store = temp_path("confirmation");
    let store_arg = store.to_string_lossy().into_owned();
    let output = run(&[
        "install",
        "model:qwen3_asr_1_7b",
        "--output",
        "json",
        "--store",
        &store_arg,
    ]);
    assert_eq!(output.status.code(), Some(16));
    let value = json_stdout(&output);
    assert_eq!(value["code"], "confirmation_required");
    assert!(!store.exists());
}

#[test]
fn unknown_options_are_rejected_instead_of_silently_ignored() {
    let output = run(&["list", "--ouptut", "json", "--output", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_stdout(&output)["code"], "invalid_cli_usage");
}

#[test]
fn ndjson_result_and_unknown_resource_error_frames_are_versioned_and_typed() {
    let store = temp_path("ndjson-contract");
    let store_arg = store.to_string_lossy().into_owned();
    let result = run(&[
        "status",
        "model:game",
        "--policy",
        "production",
        "--output",
        "ndjson",
        "--store",
        &store_arg,
    ]);
    assert!(result.status.success());
    let result_frame: serde_json::Value =
        serde_json::from_slice(result.stdout.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(result_frame["schema"], "uta.runtime.result");
    assert_eq!(result_frame["schema_version"], 1);
    assert_eq!(result_frame["type"], "result");
    assert_eq!(result_frame["command"], "status");
    assert_eq!(result_frame["data"][0]["usable"], false);

    let error = run(&[
        "show",
        "model:not_in_catalog",
        "--output",
        "ndjson",
        "--store",
        &store_arg,
    ]);
    assert_eq!(error.status.code(), Some(10));
    let error_frame: serde_json::Value =
        serde_json::from_slice(error.stdout.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(error_frame["schema"], "uta.runtime.error");
    assert_eq!(error_frame["schema_version"], 1);
    assert_eq!(error_frame["type"], "error");
    assert_eq!(error_frame["code"], "unknown_resource");
    assert!(error.stderr.is_empty());
    assert!(!store.exists());
}

#[test]
fn invalid_cli_usage_has_stable_exit_and_machine_error() {
    let output = run(&["show", "--output", "json"]);
    assert_eq!(output.status.code(), Some(2));
    let value = json_stdout(&output);
    assert_eq!(value["code"], "invalid_cli_usage");
}
