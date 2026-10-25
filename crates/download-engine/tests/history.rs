use download_engine::{Config, Download, DownloadId, DownloadRequest, DownloadState, Engine};
use tempfile::TempDir;

async fn stopped(engine: &Engine, dir: &TempDir, name: &str) -> Download {
    let id = engine
        .enqueue(DownloadRequest {
            url: "http://127.0.0.1:9/file".into(),
            destination: dir.path().join(name),
            expected_sha256: None,
        })
        .await
        .unwrap();
    engine.cancel(id).await.unwrap();
    engine
        .snapshot()
        .await
        .into_iter()
        .find(|d| d.id == id)
        .unwrap()
}

#[tokio::test]
async fn removal_persists_and_reorder_uses_only_remaining_ids() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let first = stopped(&engine, &dir, "first.bin").await;
    let second = stopped(&engine, &dir, "second.bin").await;
    let third = stopped(&engine, &dir, "third.bin").await;
    engine.remove_download(first.id, false).await.unwrap();
    let fourth = stopped(&engine, &dir, "fourth.bin").await;
    assert!(
        engine
            .reorder(&[first.id, second.id, third.id, fourth.id])
            .await
            .is_err()
    );
    engine
        .reorder(&[fourth.id, third.id, second.id])
        .await
        .unwrap();
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert_eq!(
        engine
            .snapshot()
            .await
            .iter()
            .map(|d| d.id)
            .collect::<Vec<_>>(),
        [fourth.id, third.id, second.id]
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_cleanup_is_explicit_owned_and_survives_history_removal() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let orphan = stopped(&engine, &dir, "orphan.bin").await;
    let retained = stopped(&engine, &dir, "retained.bin").await;
    tokio::fs::write(&orphan.temporary_path, b"orphan")
        .await
        .unwrap();
    tokio::fs::write(&retained.temporary_path, b"partial")
        .await
        .unwrap();
    tokio::fs::write(&retained.destination, b"user destination")
        .await
        .unwrap();
    let unrelated = dir.path().join(format!(
        ".osdm-{}.part",
        download_engine::DownloadId::new_v4()
    ));
    tokio::fs::write(&unrelated, b"not ours").await.unwrap();
    engine.remove_download(orphan.id, false).await.unwrap();
    assert!(orphan.temporary_path.exists());
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert_eq!(engine.cleanup_partial_data().await.unwrap(), 2);
    assert_eq!(engine.cleanup_partial_data().await.unwrap(), 0);
    assert!(!orphan.temporary_path.exists());
    assert!(!retained.temporary_path.exists());
    assert_eq!(
        tokio::fs::read(&retained.destination).await.unwrap(),
        b"user destination"
    );
    assert!(unrelated.exists());
    assert_eq!(engine.snapshot().await.len(), 1);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn removal_rejects_nonterminal_work_and_preserves_published_hard_link() {
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let download = stopped(&engine, &dir, "output.bin").await;
    engine.pause(download.id).await.unwrap();
    assert!(engine.remove_download(download.id, true).await.is_err());
    tokio::fs::write(&download.temporary_path, b"completed content")
        .await
        .unwrap();
    assert_eq!(engine.cleanup_partial_data().await.unwrap(), 0);
    assert!(download.temporary_path.exists());
    engine.cancel(download.id).await.unwrap();
    tokio::fs::hard_link(&download.temporary_path, &download.destination)
        .await
        .unwrap();
    engine.remove_download(download.id, true).await.unwrap();
    assert!(!download.temporary_path.exists());
    assert_eq!(
        tokio::fs::read(download.destination).await.unwrap(),
        b"completed content"
    );
    assert!(engine.snapshot().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn revision_one_migration_preserves_history_and_creates_orphan_ledger() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let download = stopped(&engine, &dir, "original.bin").await;
    engine.shutdown().await.unwrap();
    drop(engine);
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&database);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query("DROP TABLE orphaned_partials")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA user_version=1")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let restored = engine.snapshot().await.remove(0);
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&download).unwrap()
    );
    engine.remove_download(download.id, false).await.unwrap();
    assert_eq!(engine.cleanup_partial_data().await.unwrap(), 0);
    engine.shutdown().await.unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&database);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 3); // Revision-one releases reject this rather than silently discarding the ledger.
    pool.close().await;
}

#[tokio::test]
async fn cleanup_refuses_a_corrupted_path_record() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let download = stopped(&engine, &dir, "output.bin").await;
    engine.shutdown().await.unwrap();
    let protected = dir.path().join("user.txt");
    tokio::fs::write(&protected, b"keep me").await.unwrap();
    let mut payload = serde_json::to_value(&download).unwrap();
    payload["temporary_path"] = serde_json::to_value(&protected).unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&database);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query("UPDATE downloads SET payload=?")
        .bind(payload.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert!(engine.remove_download(download.id, true).await.is_err());
    assert!(engine.cleanup_partial_data().await.is_err());
    assert_eq!(tokio::fs::read(protected).await.unwrap(), b"keep me");
    engine.shutdown().await.unwrap();
}

async fn rewrite(
    database: &std::path::Path,
    id: DownloadId,
    update: impl Fn(&mut serde_json::Value),
) {
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(database);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload FROM downloads WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    update(&mut value);
    sqlx::query("UPDATE downloads SET payload=? WHERE id=?")
        .bind(value.to_string())
        .bind(id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// Simulate termination after the destination hard link exists but before completion
/// was persisted. Recovery must recognize its own published file, not overwrite or fail.
#[tokio::test]
async fn recovery_after_link_creation_before_completion_persistence() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let download = stopped(&engine, &dir, "linked.bin").await;
    engine.shutdown().await.unwrap();
    drop(engine);
    tokio::fs::write(&download.temporary_path, b"verified content")
        .await
        .unwrap();
    tokio::fs::hard_link(&download.temporary_path, &download.destination)
        .await
        .unwrap();
    rewrite(&database, download.id, |value| {
        value["state"] = serde_json::json!("Paused");
        value["ready_to_publish"] = serde_json::json!(true);
        value["total_bytes"] = serde_json::json!(16);
        value["segments"] = serde_json::json!([
            { "start": 0, "end": 16, "committed": 16, "downloaded": 16 }
        ]);
    })
    .await;
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    engine.resume(download.id).await.unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let current = engine.snapshot().await.remove(0);
            if current.state == DownloadState::Completed {
                return current;
            }
            assert_ne!(current.state, DownloadState::Failed, "{:?}", current.error);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        tokio::fs::read(&completed.destination).await.unwrap(),
        b"verified content"
    );
    assert!(!completed.temporary_path.exists());
    engine.shutdown().await.unwrap();
}

/// A different file at the destination is a conflict, never permission to replace it.
#[tokio::test]
async fn recovery_never_replaces_a_foreign_destination() {
    let dir = TempDir::new().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let download = stopped(&engine, &dir, "conflict.bin").await;
    engine.shutdown().await.unwrap();
    drop(engine);
    tokio::fs::write(&download.temporary_path, b"verified content")
        .await
        .unwrap();
    tokio::fs::write(&download.destination, b"someone else's file")
        .await
        .unwrap();
    rewrite(&database, download.id, |value| {
        value["state"] = serde_json::json!("Paused");
        value["ready_to_publish"] = serde_json::json!(true);
        value["total_bytes"] = serde_json::json!(16);
        value["segments"] = serde_json::json!([
            { "start": 0, "end": 16, "committed": 16, "downloaded": 16 }
        ]);
    })
    .await;
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    engine.resume(download.id).await.unwrap();
    let failed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let current = engine.snapshot().await.remove(0);
            if current.state == DownloadState::Failed {
                return current;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        tokio::fs::read(&failed.destination).await.unwrap(),
        b"someone else's file"
    );
    assert!(failed.temporary_path.exists());
    engine.shutdown().await.unwrap();
}
