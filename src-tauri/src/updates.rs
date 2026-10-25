//! Consent-based updates.
//!
//! Release builds check the public GitHub Releases API, download the matching
//! package, and verify it against the release's `SHA256SUMS.txt`. Windows
//! installers run with `/UPDATE /P /R`, which upgrades in place and restarts
//! OSDM instead of uninstalling the previous version. If a publisher configures
//! `OSDM_UPDATE_ENDPOINT` and `OSDM_UPDATE_PUBKEY`, the signed Tauri manifest
//! path is used instead.
use crate::{Shell, report};
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
#[cfg(not(windows))]
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

const GITHUB_REPO: &str = "nahom-d54/osdm";
const AUTO_CHECK_DELAY: Duration = Duration::from_secs(20);
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_RELEASE_METADATA: usize = 2 * 1024 * 1024;
const MAX_CHECKSUMS: usize = 64 * 1024;
const MAX_UPDATE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Linux,
}

fn verify_manifest(bytes: &[u8], signature: &str, key: &str) -> Result<serde_json::Value, String> {
    let key = base64::engine::general_purpose::STANDARD
        .decode(key)
        .map_err(|_| "invalid publisher public key")?;
    let key = std::str::from_utf8(&key).map_err(|_| "invalid publisher public key")?;
    let key =
        minisign_verify::PublicKey::decode(key).map_err(|_| "invalid publisher public key")?;
    let signature =
        minisign_verify::Signature::decode(signature).map_err(|_| "invalid manifest signature")?;
    key.verify(bytes, &signature, false)
        .map_err(|_| "manifest signature verification failed")?;
    serde_json::from_slice(bytes).map_err(|_| "invalid signed update manifest".into())
}

async fn fetch_limited(
    client: &reqwest::Client,
    url: reqwest::Url,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "could not fetch update metadata")?;
    if !response.status().is_success() {
        return Err(format!(
            "update metadata returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "could not read update metadata")?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("update metadata exceeds size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn perform_signed(app: &AppHandle, manual: bool) -> Result<(), String> {
    let (Some(endpoint), Some(key)) = (
        option_env!("OSDM_UPDATE_ENDPOINT"),
        option_env!("OSDM_UPDATE_PUBKEY"),
    ) else {
        return Err("signed updates are not configured for this build".into());
    };
    let endpoint: reqwest::Url = endpoint.parse().map_err(|_| "invalid update endpoint")?;
    if endpoint.scheme() != "https"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(
            "update endpoint must be an HTTPS manifest URL without credentials, query, or fragment"
                .into(),
        );
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "could not initialize update client")?;
    let manifest = fetch_limited(&client, endpoint.clone(), 1024 * 1024).await?;
    let signature_url = format!("{endpoint}.minisig")
        .parse()
        .map_err(|_| "invalid signature URL")?;
    let signature = fetch_limited(&client, signature_url, 8192).await?;
    let metadata = verify_manifest(
        &manifest,
        std::str::from_utf8(&signature).map_err(|_| "invalid signature encoding")?,
        key,
    )?;
    let version: semver::Version = metadata
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("manifest has no version")?
        .trim_start_matches('v')
        .parse()
        .map_err(|_| "manifest version is invalid")?;
    if version <= app.package_info().version {
        if manual {
            app.dialog()
                .message(format!(
                    "You are running the newest signed version (v{}).",
                    app.package_info().version
                ))
                .title("OSDM updates")
                .show(|_| {});
        }
        return Ok(());
    }
    let updater = app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![endpoint])
        .map_err(|_| "invalid update endpoint")?
        .timeout(Duration::from_secs(60))
        .version_comparator(|current, remote| remote.version > current)
        .build()
        .map_err(|_| "could not initialize updater")?;
    let Some(mut update) = updater
        .check()
        .await
        .map_err(|_| "could not check update manifest")?
    else {
        return Err("update metadata changed during verification; try again".into());
    };
    // Bind every install decision to the exact signed document we verified.
    if update.raw_json != metadata || update.version.trim_start_matches('v') != version.to_string()
    {
        return Err("update metadata changed during verification; try again".into());
    }
    if update.download_url.scheme() != "https" {
        return Err("update artifact requires HTTPS".into());
    }
    update.timeout = Some(Duration::from_secs(300));
    if !confirm_install(
        app,
        &format!("OSDM {} is available.", update.version),
        update
            .body
            .as_deref()
            .unwrap_or("No release notes supplied."),
        "Downloads will be paused and the application will restart.",
    )
    .await?
    {
        return Ok(());
    }
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|_| "update download or artifact signature verification failed")?;
    let shell = app.state::<Shell>();
    if shell.quitting.swap(true, Ordering::AcqRel) {
        return Err("application is already exiting".into());
    }
    if let Err(error) = shell.shutdown().await {
        shell.quitting.store(false, Ordering::Release);
        return Err(error);
    }
    let result = tokio::task::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|_| "update installer failed")?;
    if result.is_err() {
        shell.quitting.store(false, Ordering::Release);
        return Err("update installation failed; restart OSDM to reopen the saved queue".into());
    }
    shell.allow_exit.store(true, Ordering::Release);
    app.restart();
}

fn github_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("OSDM/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "could not initialize GitHub update client".into())
}

async fn fetch_github(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
    what: &str,
) -> Result<Vec<u8>, String> {
    let request = client.get(url);
    let request = if url.starts_with("https://api.github.com/") {
        request
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    } else {
        request
    };
    let mut response = request
        .send()
        .await
        .map_err(|_| format!("could not reach GitHub to read {what}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            403 | 429 => {
                "GitHub rate limit reached while checking for updates; try again later".into()
            }
            code => format!("GitHub {what} returned HTTP {code}"),
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| format!("could not read {what} from GitHub"))?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(format!("{what} exceeds the size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches('v')).ok()
}

fn select_release(
    releases: Vec<GitHubRelease>,
    current: &semver::Version,
    include_prereleases: bool,
) -> Option<(semver::Version, GitHubRelease)> {
    let mut best: Option<(semver::Version, GitHubRelease)> = None;
    for release in releases {
        if release.draft || (release.prerelease && !include_prereleases) {
            continue;
        }
        let Some(version) = parse_version(&release.tag_name) else {
            continue;
        };
        if version <= *current {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_version, _)| version > *best_version)
        {
            best = Some((version, release));
        }
    }
    best
}

fn checksum_for(checksums: &str, name: &str) -> Option<String> {
    for line in checksums.lines() {
        let mut fields = line.split_whitespace();
        let (Some(hash), Some(file)) = (fields.next(), fields.next()) else {
            continue;
        };
        let file = file.strip_prefix('*').unwrap_or(file);
        if file == name && hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn digest_sha256(digest: &str) -> Option<String> {
    let value = digest.strip_prefix("sha256:").unwrap_or(digest);
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn asset_file_name(name: &str) -> Result<String, String> {
    let path = Path::new(name);
    if path.components().count() != 1 {
        return Err("update package has an unsafe filename".into());
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "update package has an unsafe filename".into())
}

fn host_platform() -> Option<Platform> {
    if cfg!(windows) {
        Some(Platform::Windows)
    } else if cfg!(target_os = "linux") {
        Some(Platform::Linux)
    } else {
        None
    }
}

fn update_asset(assets: &[GitHubAsset], platform: Platform) -> Option<&GitHubAsset> {
    assets.iter().find(|asset| match platform {
        Platform::Windows => asset.name.ends_with("_x64-setup.exe"),
        Platform::Linux => asset.name.ends_with("_amd64.deb"),
    })
}

fn update_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "invalid update download URL")?;
    if url.scheme() != "https" {
        return Err("update downloads must use HTTPS".into());
    }
    let host = url.host_str().ok_or("update download URL has no host")?;
    if host != "github.com" && !host.ends_with(".githubusercontent.com") {
        return Err("update download URL points to an unexpected host".into());
    }
    Ok(url)
}

fn short_notes(body: Option<&str>) -> String {
    let text = body.unwrap_or("No release notes supplied.").trim();
    let mut notes: String = text.chars().take(1400).collect();
    if text.chars().count() > 1400 {
        notes.push_str("\n\n…");
    }
    notes
}

async fn confirm_install(
    app: &AppHandle,
    heading: &str,
    notes: &str,
    action: &str,
) -> Result<bool, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!("{heading}\n\n{notes}\n\n{action}"))
        .title("OSDM update available")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Download update".into(),
            "Not now".into(),
        ))
        .show(move |consent| {
            let _ = send.send(consent);
        });
    Ok(receive.await.unwrap_or(false))
}

async fn download_update(
    app: &AppHandle,
    client: &reqwest::Client,
    release: &GitHubRelease,
    asset: &GitHubAsset,
) -> Result<PathBuf, String> {
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt")
        .ok_or("release does not publish SHA256SUMS.txt; update was not downloaded")?;
    let checksum_url = update_url(&checksum_asset.browser_download_url)?;
    let checksum_bytes = fetch_github(
        client,
        checksum_url.as_str(),
        MAX_CHECKSUMS,
        "release checksums",
    )
    .await?;
    let checksum_text = String::from_utf8(checksum_bytes)
        .map_err(|_| "release checksum file is not UTF-8".to_string())?;
    let expected = checksum_for(&checksum_text, &asset.name)
        .ok_or_else(|| format!("release has no checksum for {}", asset.name))?;
    if let Some(digest) = asset.digest.as_deref().and_then(digest_sha256)
        && digest != expected
    {
        return Err("release checksum and GitHub digest disagree".into());
    }

    let url = update_url(&asset.browser_download_url)?;
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|_| "could not resolve the update cache directory")?
        .join("updates");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|_| "could not create the update cache directory")?;
    let file_name = asset_file_name(&asset.name)?;
    let final_path = directory.join(&file_name);
    let partial_path = directory.join(format!("{file_name}.part"));
    let _ = tokio::fs::remove_file(&partial_path).await;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "could not download the update package")?;
    if !response.status().is_success() {
        return Err(format!(
            "update download returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_UPDATE_BYTES)
    {
        return Err("update package exceeds the size limit".into());
    }

    let mut file = tokio::fs::File::create(&partial_path)
        .await
        .map_err(|_| "could not create the update package file")?;
    let mut response = response;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "could not read the update package")?
    {
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or("update package is too large")?;
        if total > MAX_UPDATE_BYTES {
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err("update package exceeds the size limit".into());
        }
        hasher.update(&chunk);
        if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err(format!("could not write the update package: {error}"));
        }
    }
    if let Err(error) = tokio::io::AsyncWriteExt::flush(&mut file).await {
        let _ = tokio::fs::remove_file(&partial_path).await;
        return Err(format!("could not finish the update package: {error}"));
    }
    drop(file);

    if asset.size > 0 && total != asset.size {
        let _ = tokio::fs::remove_file(&partial_path).await;
        return Err("update package size does not match GitHub metadata".into());
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = tokio::fs::remove_file(&partial_path).await;
        return Err("update package checksum mismatch; download rejected".into());
    }
    let _ = tokio::fs::remove_file(&final_path).await;
    tokio::fs::rename(&partial_path, &final_path)
        .await
        .map_err(|_| "could not finalize the update package")?;
    Ok(final_path)
}

async fn perform_github(app: &AppHandle, manual: bool) -> Result<(), String> {
    let settings = app
        .state::<Shell>()
        .settings
        .load()
        .await?
        .unwrap_or_default();
    let current = app.package_info().version.clone();
    let client = github_client(Duration::from_secs(30))?;
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=30");
    let metadata = fetch_github(&client, &url, MAX_RELEASE_METADATA, "release metadata").await?;
    let releases: Vec<GitHubRelease> =
        serde_json::from_slice(&metadata).map_err(|_| "invalid GitHub release metadata")?;
    let Some((version, release)) = select_release(releases, &current, settings.include_prereleases)
    else {
        if manual {
            app.dialog()
                .message(format!(
                    "You are running the newest available version (v{current})."
                ))
                .title("OSDM updates")
                .show(|_| {});
        }
        return Ok(());
    };
    let platform = host_platform().ok_or_else(|| {
        format!(
            "Automatic updates are not available on this platform. Download a package from {}.",
            release.html_url
        )
    })?;
    let asset = update_asset(&release.assets, platform).ok_or_else(|| {
        format!(
            "Release {} does not contain a package for this platform. See {}.",
            release.tag_name, release.html_url
        )
    })?;
    let action = if cfg!(windows) {
        "OSDM will close, install the update in place, and restart. Your downloads remain saved."
    } else {
        "The verified package will open in your software installer. Restart OSDM after installation."
    };
    let heading = format!(
        "OSDM {version} is available. You are running v{current}.\n\nPackage: {} ({:.1} MB)",
        asset.name,
        asset.size as f64 / (1024.0 * 1024.0)
    );
    if !confirm_install(app, &heading, &short_notes(release.body.as_deref()), action).await? {
        return Ok(());
    }
    let path = download_update(app, &client, &release, asset).await?;
    install_downloaded(app, &path).await
}

async fn install_downloaded(app: &AppHandle, path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let shell = app.state::<Shell>();
        if shell.quitting.swap(true, Ordering::AcqRel) {
            return Err("application is already exiting".into());
        }
        if let Err(error) = shell.shutdown().await {
            shell.quitting.store(false, Ordering::Release);
            return Err(error);
        }
        // /UPDATE keeps the existing installation and skips the uninstall page.
        // /P shows passive progress. /R restarts OSDM when installation finishes.
        let spawned = std::process::Command::new(path)
            .args(["/UPDATE", "/P", "/R"])
            .spawn();
        match spawned {
            // Exit immediately so the passive installer never sees OSDM still
            // running. The engine has already drained and saved its queue.
            Ok(_) => std::process::exit(0),
            Err(error) => {
                shell.quitting.store(false, Ordering::Release);
                shell.allow_exit.store(true, Ordering::Release);
                report(
                    app,
                    format!("Could not start the update installer: {error}"),
                );
                app.restart()
            }
        }
    }
    #[cfg(not(windows))]
    {
        app.opener()
            .open_path(path.to_string_lossy().as_ref(), None::<&str>)
            .map_err(|_| "could not open the downloaded update package")?;
        app.dialog()
            .message(
                "The verified update package was downloaded and opened. Finish installation with your software installer, then restart OSDM.",
            )
            .title("OSDM update ready")
            .show(|_| {});
        Ok(())
    }
}

async fn perform(app: &AppHandle, manual: bool) -> Result<(), String> {
    let signed = matches!(
        (
            option_env!("OSDM_UPDATE_ENDPOINT"),
            option_env!("OSDM_UPDATE_PUBKEY")
        ),
        (Some(endpoint), Some(key)) if !endpoint.is_empty() && !key.is_empty()
    );
    if signed {
        perform_signed(app, manual).await
    } else {
        perform_github(app, manual).await
    }
}

fn launch(app: AppHandle, manual: bool) {
    if app.state::<Shell>().updating.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let result = perform(&app, manual).await;
        if manual {
            if let Err(error) = result {
                report(&app, error);
            }
        } else if let Err(error) = result {
            eprintln!("OSDM: automatic update check failed: {error}");
        }
        app.state::<Shell>()
            .updating
            .store(false, Ordering::Release);
    });
}

pub fn check(app: AppHandle) {
    launch(app, true);
}

pub fn start_auto_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_CHECK_DELAY).await;
        loop {
            let (enabled, quitting) = {
                let shell = app.state::<Shell>();
                let enabled = shell
                    .settings
                    .load()
                    .await
                    .ok()
                    .flatten()
                    .is_none_or(|settings| settings.auto_update_check);
                (enabled, shell.quitting.load(Ordering::Acquire))
            };
            if enabled && !quitting {
                launch(app.clone(), false);
            }
            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, draft: bool) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.into(),
            body: None,
            prerelease,
            draft,
            html_url: String::new(),
            assets: vec![],
        }
    }

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.into(),
            browser_download_url: "https://github.com/nahom-d54/osdm/releases/download/x/file"
                .into(),
            size: 1,
            digest: None,
        }
    }

    #[test]
    fn malformed_keys_or_unsigned_metadata_are_rejected() {
        assert!(verify_manifest(br#"{"version":"99.0.0"}"#, "invalid", "invalid").is_err());
    }

    #[test]
    fn selects_newest_allowed_release() {
        let current = semver::Version::parse("0.1.1-alpha.1").unwrap();
        let releases = vec![
            release("v0.2.0-alpha.1", true, false),
            release("v0.2.0", false, false),
            release("v0.3.0", false, true),
        ];
        let selected = select_release(releases.clone(), &current, true).unwrap();
        assert_eq!(selected.0.to_string(), "0.2.0");
        let selected = select_release(releases, &current, false).unwrap();
        assert_eq!(selected.0.to_string(), "0.2.0");
    }

    #[test]
    fn ignores_prereleases_when_disabled_and_downgrades() {
        let current = semver::Version::parse("0.1.1-alpha.1").unwrap();
        let releases = vec![
            release("v0.2.0-alpha.1", true, false),
            release("v0.1.0", false, false),
        ];
        assert!(select_release(releases, &current, false).is_none());
    }

    #[test]
    fn reads_exact_checksum_line() {
        let sums = "abc  other.exe\n\
                    0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  OSDM_x64-setup.exe\n";
        assert_eq!(
            checksum_for(sums, "OSDM_x64-setup.exe").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert!(checksum_for(sums, "missing.exe").is_none());
    }

    #[test]
    fn parses_github_release_payload() {
        let releases: Vec<GitHubRelease> = serde_json::from_str(
            r#"[
                {
                    "tag_name": "v0.2.0-alpha.1",
                    "body": "Notes",
                    "prerelease": true,
                    "draft": false,
                    "html_url": "https://github.com/nahom-d54/osdm/releases/tag/v0.2.0-alpha.1",
                    "assets": [
                        {
                            "name": "OSDM_0.2.0-alpha.1_x64-setup.exe",
                            "browser_download_url": "https://github.com/nahom-d54/osdm/releases/download/v0.2.0-alpha.1/OSDM_0.2.0-alpha.1_x64-setup.exe",
                            "size": 123,
                            "digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        }
                    ]
                }
            ]"#,
        )
        .unwrap();
        assert_eq!(releases[0].tag_name, "v0.2.0-alpha.1");
        assert_eq!(releases[0].assets[0].size, 123);
        assert_eq!(
            releases[0].assets[0].digest.as_deref(),
            Some("sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn selects_platform_packages() {
        let assets = vec![
            asset("SHA256SUMS.txt"),
            asset("OSDM_1.0.0_amd64.deb"),
            asset("OSDM_1.0.0_x64-setup.exe"),
        ];
        assert_eq!(
            update_asset(&assets, Platform::Windows).unwrap().name,
            "OSDM_1.0.0_x64-setup.exe"
        );
        assert_eq!(
            update_asset(&assets, Platform::Linux).unwrap().name,
            "OSDM_1.0.0_amd64.deb"
        );
    }

    #[test]
    fn rejects_unsafe_asset_names_and_foreign_hosts() {
        assert!(asset_file_name("../escape.exe").is_err());
        assert!(asset_file_name("folder/escape.exe").is_err());
        assert!(asset_file_name("OSDM_1.0.0_x64-setup.exe").is_ok());
        assert!(update_url("http://github.com/file").is_err());
        assert!(update_url("https://example.com/file").is_err());
        assert!(update_url("https://github.com/file").is_ok());
    }
}
