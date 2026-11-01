use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use download_engine::{
    BasicCredentials, Config, DEFAULT_USER_AGENT, DownloadProperties, DownloadRequest,
    DownloadState, Engine,
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
    use download_engine::{CapturedHeader, CapturedHeaders};
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
                    || headers.get("accept-language").and_then(|v| v.to_str().ok()) != Some("en-GB")
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
        headers: vec![CapturedHeader {
            name: "accept-language".into(),
            value: "en-GB".into(),
        }],
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

#[tokio::test]
async fn configured_and_captured_identities_apply_only_valid_replay_headers() {
    use download_engine::{CapturedHeader, CapturedHeaders};
    use std::sync::Mutex;

    assert_eq!(
        DEFAULT_USER_AGENT,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let observed = seen.clone();
    let server = Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let observed = observed.clone();
            async move {
                let read = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                observed.lock().unwrap().push((
                    read("user-agent"),
                    read("accept-language"),
                    read("referer"),
                ));
                b"identity fixture".as_slice()
            }
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), Config::default())
        .await
        .unwrap();
    let complete = |engine: Engine, id| async move {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let download = engine
                    .snapshot()
                    .await
                    .into_iter()
                    .find(|download| download.id == id)
                    .unwrap();
                assert_ne!(download.state, DownloadState::Failed);
                if download.state == DownloadState::Completed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    };
    assert!(engine.set_user_agent("bad\r\nagent").await.is_err());
    engine.set_user_agent("OSDM-Test/1.0").await.unwrap();
    let id = engine
        .enqueue_paused(DownloadRequest {
            url: url.clone(),
            destination: dir.path().join("default.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    engine.resume(id).await.unwrap();
    complete(engine.clone(), id).await;

    let captured = engine
        .enqueue_paused(DownloadRequest {
            url: url.clone(),
            destination: dir.path().join("captured.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    engine
        .set_captured_headers(
            captured,
            CapturedHeaders {
                cookie: None,
                authorization: None,
                user_agent: Some("Browser/9.0".into()),
                headers: vec![CapturedHeader {
                    name: "accept-language".into(),
                    value: "en-GB".into(),
                }],
            },
        )
        .await
        .unwrap();
    engine.resume(captured).await.unwrap();
    complete(engine.clone(), captured).await;
    {
        let seen = seen.lock().unwrap();
        assert!(
            seen.iter()
                .any(|entry| entry.0.as_deref() == Some("OSDM-Test/1.0"))
        );
        assert!(
            seen.iter().any(|entry| {
                entry == &(Some("Browser/9.0".into()), Some("en-GB".into()), None)
            })
        );
    }

    let rejected = engine
        .enqueue_paused(DownloadRequest {
            url,
            destination: dir.path().join("rejected.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    for (name, value) in [
        ("host", "evil.example"),
        ("user-agent", "replayed-twice"),
        ("accept", "text/html\r\nX-Evil: 1"),
        ("cookie", "session=secret"),
        ("referer", "https://page.example/secret"),
    ] {
        assert!(
            engine
                .set_captured_headers(
                    rejected,
                    CapturedHeaders {
                        cookie: None,
                        authorization: None,
                        user_agent: None,
                        headers: vec![CapturedHeader {
                            name: name.into(),
                            value: value.into()
                        }],
                    }
                )
                .await
                .is_err()
        );
    }
    // Loopback HTTP is the documented test exception; other plain-HTTP origins
    // must still reject replayable secrets while accepting a non-secret identity.
    let insecure = engine
        .enqueue_paused(DownloadRequest {
            url: "http://example.invalid/file".into(),
            destination: dir.path().join("insecure.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    assert!(
        engine
            .set_captured_headers(
                insecure,
                CapturedHeaders {
                    cookie: Some("session=secret".into()),
                    authorization: None,
                    user_agent: Some("Browser/9.0".into()),
                    headers: vec![]
                }
            )
            .await
            .is_err(),
        "plain HTTP must reject replayable secrets"
    );
    assert!(
        engine
            .set_captured_headers(
                insecure,
                CapturedHeaders {
                    cookie: None,
                    authorization: None,
                    user_agent: Some("Browser/9.0".into()),
                    headers: vec![]
                }
            )
            .await
            .is_ok(),
        "a non-secret identity may be replayed over plain HTTP"
    );
    engine.cancel(insecure).await.unwrap();
    engine.cancel(rejected).await.unwrap();
    engine.shutdown().await.unwrap();
    task.abort();
}
