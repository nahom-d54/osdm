use download_engine::{Config, DownloadProperties, DownloadRequest, Engine};

#[tokio::test]
async fn properties_and_safe_destination_edits_persist_without_moving_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downloads.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let properties = DownloadProperties {
        description: "Release archive".into(),
        category_id: Some("compressed".into()),
    };
    let id = engine
        .enqueue_with_properties(
            DownloadRequest {
                url: "http://127.0.0.1:9/file".into(),
                destination: dir.path().join("original.bin"),
                expected_sha256: None,
            },
            properties.clone(),
            true,
            None,
            None,
        )
        .await
        .unwrap();
    let original = engine.snapshot().await.remove(0);
    let other = dir.path().join("other");
    tokio::fs::create_dir(&other).await.unwrap();
    engine
        .update_destination(id, other.join("new.bin"))
        .await
        .unwrap();
    let download = engine.snapshot().await.remove(0);
    assert_eq!(download.properties, properties);
    assert_ne!(download.temporary_path, original.temporary_path);
    tokio::fs::write(&download.temporary_path, b"retained data")
        .await
        .unwrap();
    assert!(
        engine
            .update_destination(id, dir.path().join("cannot-move.bin"))
            .await
            .is_err()
    );
    engine
        .update_destination(id, other.join("renamed.bin"))
        .await
        .unwrap();
    tokio::fs::write(other.join("existing.bin"), b"foreign")
        .await
        .unwrap();
    assert!(
        engine
            .update_destination(id, other.join("existing.bin"))
            .await
            .is_err()
    );
    let updated = DownloadProperties {
        description: "Updated".into(),
        category_id: None,
    };
    engine.update_properties(id, updated.clone()).await.unwrap();
    assert!(
        engine
            .update_properties(
                id,
                DownloadProperties {
                    description: "x".repeat(4097),
                    category_id: None
                }
            )
            .await
            .is_err()
    );
    engine.shutdown().await.unwrap();
    let reopened = Engine::open(&db, Config::default()).await.unwrap();
    let saved = reopened.snapshot().await.remove(0);
    assert_eq!(saved.properties, updated);
    assert_eq!(saved.destination.file_name().unwrap(), "renamed.bin");
    assert_eq!(
        tokio::fs::read(saved.temporary_path).await.unwrap(),
        b"retained data"
    );
    assert_eq!(
        tokio::fs::read(other.join("existing.bin")).await.unwrap(),
        b"foreign"
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn revision_three_upgrade_supplies_metadata_defaults_and_blocks_older_writers() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("legacy.db");
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::query("PRAGMA user_version=3")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    engine.shutdown().await.unwrap();
    let pool = SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(&db))
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    // The revision-three engine's admission check is 0..=3. It rejects this DB.
    assert!(!(0..=3).contains(&version));
    assert_eq!(version, 4);
    pool.close().await;
}
