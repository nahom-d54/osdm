//! Explicit, consent-based updates. The manifest itself and installer must both
//! verify under the build-time publisher key; no key or endpoint is invented here.
use crate::{Shell, report};
use base64::Engine as _;
use std::{sync::atomic::Ordering, time::Duration};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

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

async fn perform(app: &AppHandle) -> Result<(), String> {
    let (Some(endpoint), Some(key)) = (
        option_env!("OSDM_UPDATE_ENDPOINT"),
        option_env!("OSDM_UPDATE_PUBKEY"),
    ) else {
        return Err("Updates are not configured for this build. A publisher HTTPS endpoint and public signing key are required.".into());
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
        app.dialog()
            .message("No newer signed update is available.")
            .title("OSDM updates")
            .show(|_| {});
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
    // The plugin fetches the manifest independently. Bind every install decision to
    // the exact signed document we verified, including version and artifact signature.
    if update.raw_json != metadata || update.version.trim_start_matches('v') != version.to_string()
    {
        return Err("update metadata changed during verification; try again".into());
    }
    if update.download_url.scheme() != "https" {
        return Err("update artifact requires HTTPS".into());
    }
    update.timeout = Some(Duration::from_secs(300));
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!(
            "Install OSDM {}? Downloads will be paused and the application will restart.\n\n{}",
            update.version,
            update
                .body
                .as_deref()
                .unwrap_or("No release notes supplied.")
        ))
        .title("OSDM update available")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install update".into(),
            "Not now".into(),
        ))
        .show(move |consent| {
            let _ = send.send(consent);
        });
    if !receive.await.unwrap_or(false) {
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

pub fn check(app: AppHandle) {
    if app.state::<Shell>().updating.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = perform(&app).await {
            report(&app, error);
        }
        app.state::<Shell>()
            .updating
            .store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn malformed_keys_or_unsigned_metadata_are_rejected() {
        assert!(super::verify_manifest(br#"{"version":"99.0.0"}"#, "invalid", "invalid").is_err());
    }
}
