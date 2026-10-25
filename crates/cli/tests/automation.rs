use serde_json::Value;
use std::process::{Command, Output};

fn cli(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_osdm-cli"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn records(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn queue_only_persists_and_repeated_batch_reuses_ids() {
    let root = tempfile::tempdir().unwrap();
    // No listening server: queue-only must not probe or transfer.
    std::fs::write(
        root.path().join("links.txt"),
        "\u{feff}http://127.0.0.1:1/a.bin\n\nhttp://127.0.0.1:1/b.bin\nhttp://127.0.0.1:1/a.bin\n",
    )
    .unwrap();
    let args = [
        "add",
        "--database",
        "queue.db",
        "--directory",
        ".",
        "--input",
        "links.txt",
        "--queue-only",
        "--silent",
    ];
    let first = cli(root.path(), &args);
    assert!(first.status.success(), "{:?}", records(&first));
    let first = records(&first);
    assert_eq!(first.len(), 3);
    assert_eq!(first[0]["id"], first[2]["id"]);
    assert_eq!(first[2]["duplicate"], true);
    let again = cli(root.path(), &args);
    assert!(again.status.success());
    for (a, b) in first.iter().zip(records(&again)) {
        assert_eq!(a["id"], b["id"]);
        assert_eq!(b["duplicate"], true);
    }
    assert!(!root.path().join("a.bin").exists());
    assert!(
        !std::fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part"))
    );
}

#[test]
fn preflight_rejects_entire_invalid_batch_and_redacts_input() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [
        "https://user:secret@example.invalid/private",
        "file:///private",
        "https://example.invalid/%2e%2e",
        "https://example.invalid/CON.txt",
        "https://example.invalid/a%2fb",
    ] {
        let output = cli(
            root.path(),
            &[
                "add",
                "--database",
                "queue.db",
                "--directory",
                ".",
                "--queue-only",
                "http://127.0.0.1:1/good.bin",
                invalid,
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(records(&output)[0]["code"], "invalid_input");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(invalid));
        assert!(output.stderr.is_empty());
        assert!(!root.path().join("queue.db").exists());
    }
}

#[test]
fn conflicts_are_partial_failures_and_never_overwrite() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("exists.bin"), b"keep").unwrap();
    let output = cli(
        root.path(),
        &[
            "add",
            "--database",
            "queue.db",
            "--directory",
            ".",
            "--queue-only",
            "http://127.0.0.1:1/exists.bin",
            "http://127.0.0.1:1/good.bin",
            "http://localhost:1/good.bin",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let rows = records(&output);
    assert_eq!(rows[0]["code"], "enqueue_failed");
    assert_eq!(rows[1]["type"], "accepted");
    assert_eq!(rows[2]["code"], "enqueue_failed");
    assert_eq!(
        std::fs::read(root.path().join("exists.bin")).unwrap(),
        b"keep"
    );
}

#[tokio::test]
async fn refuses_another_engine_owner() {
    let root = tempfile::tempdir().unwrap();
    let engine = download_engine::Engine::open(
        root.path().join("queue.db"),
        download_engine::Config::default(),
    )
    .await
    .unwrap();
    let output = cli(root.path(), &["start-queue", "--database", "queue.db"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(records(&output)[0]["code"], "engine_open_failed");
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn closed_output_pipe_still_shuts_down_and_preserves_admission() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_osdm-cli"))
        .current_dir(root.path())
        .args([
            "add",
            "--database",
            "queue.db",
            "--directory",
            ".",
            "--queue-only",
            "http://127.0.0.1:1/a.bin",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let engine = download_engine::Engine::open(
        root.path().join("queue.db"),
        download_engine::Config::default(),
    )
    .await
    .unwrap();
    let snapshot = engine.snapshot().await;
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].state, download_engine::DownloadState::Paused);
    engine.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_queue_and_direct_download_publish_exact_bytes() {
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = axum::Router::new().route(
        "/{name}",
        axum::routing::get(|| async { "exact fixture bytes" }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let url = format!("http://{address}/queued.bin");
    assert!(
        cli(
            root.path(),
            &[
                "add",
                "--database",
                "queue.db",
                "--directory",
                ".",
                "--queue-only",
                &url
            ]
        )
        .status
        .success()
    );
    let started = cli(root.path(), &["start-queue", "--database", "queue.db"]);
    assert!(started.status.success(), "{:?}", records(&started));
    assert_eq!(records(&started)[0]["state"], "Completed");
    assert_eq!(
        std::fs::read(root.path().join("queued.bin")).unwrap(),
        b"exact fixture bytes"
    );
    let direct = cli(
        root.path(),
        &[
            "add",
            "--database",
            "queue.db",
            "--directory",
            ".",
            "--filename",
            "chosen.bin",
            &url,
        ],
    );
    assert!(direct.status.success());
    assert_eq!(
        std::fs::read(root.path().join("chosen.bin")).unwrap(),
        b"exact fixture bytes"
    );
    let repeated = cli(
        root.path(),
        &[
            "add",
            "--database",
            "queue.db",
            "--directory",
            ".",
            "--queue-only",
            &url,
        ],
    );
    assert!(repeated.status.success());
    assert_eq!(records(&repeated)[0]["duplicate"], true);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transfer_failure_has_nonzero_exit_without_exposing_source() {
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server =
        tokio::spawn(async move { axum::serve(listener, axum::Router::new()).await.unwrap() });
    let url = format!("http://{address}/missing.bin");
    let output = cli(
        root.path(),
        &["add", "--database", "queue.db", "--directory", ".", &url],
    );
    assert_eq!(output.status.code(), Some(1));
    let rows = records(&output);
    assert_eq!(rows[0]["type"], "accepted");
    assert_eq!(rows[1]["state"], "Failed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&url));
    assert!(!root.path().join("missing.bin").exists());
    // start-queue does not silently retry historical failures.
    let start = cli(root.path(), &["start-queue", "--database", "queue.db"]);
    assert!(start.status.success());
    assert!(start.stdout.is_empty());
    server.abort();
}

#[test]
fn queue_commands_persist_assignment_and_do_not_dispatch_schedules_on_listing() {
    let root = tempfile::tempdir().unwrap();
    let queue = download_engine::DownloadId::new_v4();
    let command = |value: Value| {
        std::fs::write(
            root.path().join("command.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let output = cli(
            root.path(),
            &[
                "configure",
                "--database",
                "queue.db",
                "--command",
                "command.json",
            ],
        );
        assert!(output.status.success(), "{:?}", records(&output));
    };
    command(serde_json::json!({"action":"save_queue","id":queue,"name":"Automation"}));
    let queue_text = queue.to_string();
    let output = cli(
        root.path(),
        &[
            "add",
            "--database",
            "queue.db",
            "--directory",
            ".",
            "--queue",
            &queue_text,
            "--queue-only",
            "http://127.0.0.1:1/file.bin",
        ],
    );
    assert!(output.status.success());
    let id = records(&output)[0]["id"].as_str().unwrap().to_string();
    command(
        serde_json::json!({"action":"save_schedule","schedule":{"id":download_engine::DownloadId::new_v4(),"queue_id":queue,"next_run":1,"interval_seconds":null,"stop_after_seconds":null,"run_once_on_startup":true,"enabled":true}}),
    );
    let list = cli(root.path(), &["queues", "--database", "queue.db"]);
    assert!(list.status.success());
    let state = &records(&list)[0]["state"];
    assert_eq!(state["membership"][&id], queue_text);
    assert_eq!(state["schedules"][0]["enabled"], true); // Read-only ownership did not consume a due slot.
    assert!(!root.path().join("file.bin").exists());
    let stop = cli(
        root.path(),
        &[
            "stop-queue",
            "--database",
            "queue.db",
            "--queue",
            &queue_text,
        ],
    );
    assert!(stop.status.success());
}

#[test]
fn desktop_queue_dispatch_requires_explicit_target_and_never_opens_a_database() {
    let root = tempfile::tempdir().unwrap();
    let output = cli(root.path(), &["start-queue", "--desktop"]);
    assert_eq!(output.status.code(), Some(2));
    let output = cli(
        root.path(),
        &["queues", "--desktop", "--database", "should-not-exist.db"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!root.path().join("should-not-exist.db").exists());
}
