use download_engine::{
    AutomationCommand as Command, BatchItem, Config, DownloadId, DownloadRequest, DownloadState,
    Engine, QueueSchedule,
};

async fn fixture() -> (tempfile::TempDir, Engine, DownloadId, DownloadId) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let queue = DownloadId::new_v4();
    engine
        .configure_automation(Command::SaveQueue {
            id: queue,
            name: "Night shift".into(),
        })
        .await
        .unwrap();
    let id = engine
        .enqueue_paused(DownloadRequest {
            url: "http://127.0.0.1:1/file.bin".into(),
            destination: dir.path().join("file.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    engine
        .configure_automation(Command::Assign {
            ids: vec![id],
            queue_id: Some(queue),
        })
        .await
        .unwrap();
    (dir, engine, queue, id)
}

fn schedule(queue: DownloadId, at: u64, catchup: bool) -> QueueSchedule {
    QueueSchedule {
        id: DownloadId::new_v4(),
        queue_id: queue,
        next_run: at,
        interval_seconds: Some(3600),
        stop_after_seconds: Some(60),
        run_once_on_startup: catchup,
        enabled: true,
    }
}

#[tokio::test]
async fn queue_membership_and_schedules_survive_restart_without_resuming() {
    let (dir, engine, queue, id) = fixture().await;
    engine
        .configure_automation(Command::SaveSchedule {
            schedule: schedule(queue, 4_100_000_000, false),
        })
        .await
        .unwrap();
    engine.shutdown().await.unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let state = engine.automation().await;
    assert_eq!(state.membership.get(&id), Some(&queue));
    assert_eq!(state.schedules.len(), 1);
    assert!(!state.queues[0].running);
    assert_eq!(engine.snapshot().await[0].state, DownloadState::Paused);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn missed_runs_skip_or_coalesce_and_clock_reversal_never_duplicates() {
    let (_dir, engine, queue, id) = fixture().await;
    let at = 4_100_000_000;
    let s = schedule(queue, at, false);
    engine
        .configure_automation(Command::SaveSchedule {
            schedule: s.clone(),
        })
        .await
        .unwrap();
    engine.automation_tick(at + 7201, true).await.unwrap();
    assert_eq!(engine.snapshot().await[0].state, DownloadState::Paused);
    assert_eq!(engine.automation().await.schedules[0].next_run, at + 10800);
    engine
        .configure_automation(Command::SaveSchedule {
            schedule: schedule(queue, at, true),
        })
        .await
        .unwrap();
    engine.automation_tick(at + 7201, true).await.unwrap();
    assert_eq!(engine.snapshot().await[0].state, DownloadState::Queued);
    engine
        .configure_automation(Command::Stop { id: queue })
        .await
        .unwrap();
    assert_eq!(engine.snapshot().await[0].state, DownloadState::Paused);
    engine.automation_tick(at, false).await.unwrap();
    assert_eq!(
        engine
            .snapshot()
            .await
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .state,
        DownloadState::Paused
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn due_window_stops_only_its_queue_and_consumes_slot_before_dispatch() {
    let (dir, engine, queue, id) = fixture().await;
    let other = engine
        .enqueue_paused(DownloadRequest {
            url: "http://127.0.0.1:1/other".into(),
            destination: dir.path().join("other.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    let at = 4_100_000_000;
    engine
        .configure_automation(Command::SaveSchedule {
            schedule: schedule(queue, at, false),
        })
        .await
        .unwrap();
    let state = engine.automation_tick(at, false).await.unwrap();
    assert_eq!(state.schedules[0].next_run, at + 3600);
    assert_eq!(state.queues[0].stop_at, Some(at + 60));
    engine.automation_tick(at + 60, false).await.unwrap();
    let snapshot = engine.snapshot().await;
    assert_eq!(
        snapshot.iter().find(|d| d.id == id).unwrap().state,
        DownloadState::Paused
    );
    assert_eq!(
        snapshot.iter().find(|d| d.id == other).unwrap().state,
        DownloadState::Paused
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn batch_duplicates_persist_and_partial_conflicts_never_overwrite() {
    let (dir, engine, queue, _) = fixture().await;
    let item = BatchItem {
        request: DownloadRequest {
            url: "https://example.invalid/new.bin#fragment".into(),
            destination: dir.path().join("new.bin"),
            expected_sha256: None,
        },
        queue_id: Some(queue),
    };
    let result = engine
        .enqueue_batch(vec![item.clone(), item.clone()], true)
        .await
        .unwrap();
    assert_eq!(result[0].id, result[1].id);
    assert!(result[1].duplicate);
    assert!(result.iter().all(|r| r.error.is_none()));
    let mut conflict = item.clone();
    conflict.request.url = "https://example.invalid/changed.bin".into();
    assert!(
        engine.enqueue_batch(vec![conflict], true).await.unwrap()[0]
            .error
            .is_some()
    );
    engine.shutdown().await.unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    assert!(engine.enqueue_batch(vec![item], true).await.unwrap()[0].duplicate);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn revision_four_migrates_transactionally_and_old_engine_rejects_five() {
    let (dir, engine, _, _) = fixture().await;
    engine.shutdown().await.unwrap();
    let db = dir.path().join("queue.db");
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    sqlx::query("DROP TABLE automation")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA user_version=4")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert_eq!(engine.snapshot().await.len(), 1);
    assert!(engine.automation().await.queues.is_empty());
    engine.shutdown().await.unwrap();
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 5);
    assert!(!(0..=4).contains(&version)); // Previous binary's admission guard.
    sqlx::query("PRAGMA user_version=6")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Engine::open(&db, Config::default()).await.is_err());
}

#[tokio::test]
async fn pending_browser_handoff_is_nonrunnable_durable_and_abortable() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let id = engine
        .prepare_capture(DownloadRequest {
            url: "http://127.0.0.1:1/file".into(),
            destination: dir.path().join("file.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    assert!(engine.resume(id).await.is_err());
    assert!(!engine.shutdown_if_idle().await.unwrap());
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert!(engine.automation().await.pending_captures.contains_key(&id));
    assert_eq!(engine.snapshot().await[0].state, DownloadState::Paused);
    engine.abort_capture(id).await.unwrap();
    engine.abort_capture(id).await.unwrap();
    assert!(engine.snapshot().await.is_empty());
    assert!(engine.commit_capture(id).await.is_err());
    assert!(!dir.path().join("file.bin").exists());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn idle_only_shutdown_refuses_runnable_work_and_blocks_later_admission() {
    let (dir, engine, queue, _) = fixture().await;
    engine
        .configure_automation(Command::Start { id: queue })
        .await
        .unwrap();
    assert!(!engine.shutdown_if_idle().await.unwrap());
    engine
        .configure_automation(Command::Stop { id: queue })
        .await
        .unwrap();
    assert!(engine.shutdown_if_idle().await.unwrap());
    assert!(
        engine
            .enqueue_paused(DownloadRequest {
                url: "http://127.0.0.1:1/later".into(),
                destination: dir.path().join("later.bin"),
                expected_sha256: None
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn browser_commit_is_idempotent_and_expired_leases_leave_no_phantom() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let request = |name: &str| DownloadRequest {
        url: "http://127.0.0.1:1/file".into(),
        destination: dir.path().join(name),
        expected_sha256: None,
    };
    let expired = engine
        .prepare_capture(request("expired.bin"))
        .await
        .unwrap();
    engine.automation_tick(4_100_000_000, false).await.unwrap();
    assert!(engine.snapshot().await.is_empty());
    assert!(engine.commit_capture(expired).await.is_err());
    let committed = engine
        .prepare_capture(request("committed.bin"))
        .await
        .unwrap();
    engine.commit_capture(committed).await.unwrap();
    engine.commit_capture(committed).await.unwrap();
    engine.abort_capture(committed).await.unwrap(); // Never removes committed work.
    assert_eq!(engine.snapshot().await.len(), 1);
    assert!(engine.automation().await.pending_captures.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn synchronization_keeps_original_and_publishes_only_changed_revisions() {
    use axum::{
        Router,
        body::Body,
        http::{HeaderMap, Response, StatusCode},
        routing::get,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let generation = Arc::new(AtomicUsize::new(1));
    let state = generation.clone();
    let app = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let generation = state.load(Ordering::SeqCst);
            async move {
                let mut response =
                    Response::builder().header("ETag", format!("\"generation-{generation}\""));
                if headers.get("range").is_some_and(|v| v == "bytes=0-0") {
                    response = response
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("Content-Range", "bytes 0-0/1024");
                    response.body(Body::from(vec![generation as u8])).unwrap()
                } else {
                    response
                        .body(Body::from(vec![generation as u8; 1024]))
                        .unwrap()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: format!("http://{address}/file"),
            destination: dir.path().join("original.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    async fn wait_complete(engine: &Engine, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let snapshot = engine.snapshot().await;
                if snapshot.len() == count
                    && snapshot.iter().all(|d| d.state == DownloadState::Completed)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    wait_complete(&engine, 1).await;
    let queue = DownloadId::new_v4();
    engine
        .configure_automation(Command::SaveQueue {
            id: queue,
            name: "Sync".into(),
        })
        .await
        .unwrap();
    engine
        .configure_automation(Command::Assign {
            ids: vec![id],
            queue_id: Some(queue),
        })
        .await
        .unwrap();
    engine
        .configure_automation(Command::SetSynchronization {
            id: queue,
            enabled: true,
        })
        .await
        .unwrap();
    engine
        .configure_automation(Command::Start { id: queue })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while engine.automation().await.queues[0].checking_sources {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(engine.snapshot().await.len(), 1); // Matching validator/length reuses the completed source.
    engine
        .configure_automation(Command::Stop { id: queue })
        .await
        .unwrap();
    generation.store(2, Ordering::SeqCst);
    engine
        .configure_automation(Command::Start { id: queue })
        .await
        .unwrap();
    wait_complete(&engine, 2).await;
    let snapshot = engine.snapshot().await;
    assert_eq!(
        std::fs::read(&snapshot[0].destination).unwrap(),
        vec![1u8; 1024]
    );
    assert_eq!(
        std::fs::read(&snapshot[1].destination).unwrap(),
        vec![2u8; 1024]
    );
    assert_ne!(snapshot[0].destination, snapshot[1].destination);
    assert!(engine.remove_download(id, false).await.is_err());
    engine
        .configure_automation(Command::Stop { id: queue })
        .await
        .unwrap();
    // A matching remote validator does not justify skipping a missing local copy.
    std::fs::remove_file(&snapshot[1].destination).unwrap();
    engine
        .configure_automation(Command::Start { id: queue })
        .await
        .unwrap();
    wait_complete(&engine, 3).await;
    assert_eq!(
        std::fs::read(&engine.snapshot().await[2].destination).unwrap(),
        vec![2u8; 1024]
    );
    engine.shutdown().await.unwrap();
    server.abort();
}
