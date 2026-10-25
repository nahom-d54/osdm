use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use download_engine::{
    BasicCredentials, Config, DownloadProperties, DownloadRequest, DownloadState, Engine,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn basic_credentials_are_session_only_and_never_forwarded_across_origins() {
    let leaked = Arc::new(AtomicUsize::new(0));
    let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_url = format!("http://{}/file", target_listener.local_addr().unwrap());
    let observed = leaked.clone();
    let target = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let observed = observed.clone();
            async move {
                if headers.contains_key("authorization") {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
                b"download bytes".as_slice()
            }
        }),
    );
    let target_task =
        tokio::spawn(async move { axum::serve(target_listener, target).await.unwrap() });
    let origin = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let target_url = target_url.clone();
            async move {
                if headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    != Some("Basic dGVzdDpzZWNyZXQ=")
                {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                Ok(axum::response::Redirect::temporary(&target_url))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, origin).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("downloads.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .enqueue_with_properties(
            DownloadRequest {
                url,
                destination: dir.path().join("file.bin"),
                expected_sha256: None,
            },
            DownloadProperties::default(),
            true,
            Some(BasicCredentials {
                username: "test".into(),
                password: "secret".into(),
            }),
            None,
        )
        .await
        .unwrap();
    let snapshot = serde_json::to_string(&engine.snapshot().await).unwrap();
    assert!(!snapshot.contains("secret"));
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    // Reopening must not recover the secret. The first attempt gets a 401.
    engine.resume(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while engine.snapshot().await[0].state != DownloadState::Failed {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        engine.snapshot().await[0]
            .error
            .as_ref()
            .unwrap()
            .contains("401")
    );
    engine
        .set_credentials(
            id,
            Some(BasicCredentials {
                username: "test".into(),
                password: "secret".into(),
            }),
        )
        .await
        .unwrap();
    engine.resume(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let download = engine.snapshot().await.remove(0);
            assert_ne!(
                download.state,
                DownloadState::Failed,
                "{:?}",
                download.error
            );
            if download.state == DownloadState::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(leaked.load(Ordering::SeqCst), 0);
    engine.shutdown().await.unwrap();
    let bytes = tokio::fs::read(&db).await.unwrap();
    assert!(!bytes.windows(6).any(|window| window == b"secret"));
    task.abort();
    target_task.abort();
}

#[tokio::test]
async fn captured_headers_are_session_only_and_origin_scoped() {
    use download_engine::CapturedHeaders;
    let leaks = Arc::new(AtomicUsize::new(0));
    let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_url = format!("http://{}/file", target_listener.local_addr().unwrap());
    let observed = leaks.clone();
    let target = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let observed = observed.clone();
            async move {
                if headers.contains_key("cookie") || headers.contains_key("authorization") {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
                b"captured fixture bytes".as_slice()
            }
        }),
    );
    let target_task = tokio::spawn(async move {
        axum::serve(target_listener, target).await.unwrap();
    });
    let origin = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let target_url = target_url.clone();
            async move {
                if headers.get("cookie").and_then(|v| v.to_str().ok())
                    != Some("session=capture-only")
                    || headers.get("authorization").and_then(|v| v.to_str().ok())
                        != Some("Bearer capture-only")
                {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                Ok(axum::response::Redirect::temporary(&target_url))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let origin_task = tokio::spawn(async move {
        axum::serve(listener, origin).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("queue.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let id = engine
        .enqueue_paused(DownloadRequest {
            url,
            destination: dir.path().join("file.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    let headers = || CapturedHeaders {
        cookie: Some("session=capture-only".into()),
        authorization: Some("Bearer capture-only".into()),
        user_agent: None,
    };
    engine.set_captured_headers(id, headers()).await.unwrap();
    assert!(
        !serde_json::to_string(&engine.snapshot().await)
            .unwrap()
            .contains("capture-only")
    );
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    engine.resume(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.snapshot().await[0].state != DownloadState::Failed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Secrets were not recovered; explicitly hand them off again on a paused ID.
    engine.pause(id).await.unwrap();
    engine.set_captured_headers(id, headers()).await.unwrap();
    engine.resume(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let d = engine.snapshot().await.remove(0);
            assert_ne!(d.state, DownloadState::Failed);
            if d.state == DownloadState::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(leaks.load(Ordering::SeqCst), 0);
    engine.shutdown().await.unwrap();
    assert!(
        !std::fs::read(database)
            .unwrap()
            .windows(12)
            .any(|bytes| bytes == b"capture-only")
    );
    target_task.abort();
    origin_task.abort();
}
