use download_engine::{Config, DownloadRequest, DownloadState, Engine};
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: download <http(s)-url> <absolute-destination-path>");
        std::process::exit(2);
    }
    let engine = Engine::open("osdm.db", Config::default()).await?;
    let destination = PathBuf::from(&args[2]);
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err("destination must be an absolute filename".into());
    }
    let destination = tokio::fs::canonicalize(destination.parent().unwrap())
        .await?
        .join(destination.file_name().unwrap());
    let url = reqwest::Url::parse(&args[1])?.to_string();
    let existing = engine
        .snapshot()
        .await
        .into_iter()
        .find(|d| d.destination == destination && d.url == url);
    let id = if let Some(download) = existing {
        engine.resume(download.id).await?;
        download.id
    } else {
        engine
            .enqueue(DownloadRequest {
                url,
                destination,
                expected_sha256: None,
            })
            .await?
    };
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let mut failure = None;
    loop {
        tokio::select! {
            _ = &mut interrupted => { eprintln!("Pausing and saving progress..."); break; }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let download = engine.snapshot().await.into_iter().find(|d| d.id == id).unwrap();
                eprintln!("{:?}: {} / {:?} bytes, {} B/s", download.state, download.downloaded_bytes(), download.total_bytes, download.bytes_per_second);
                if download.state == DownloadState::Failed { failure = download.error; break; }
                if download.state == DownloadState::Completed { break; }
            }
        }
    }
    engine.shutdown().await?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    Ok(())
}
