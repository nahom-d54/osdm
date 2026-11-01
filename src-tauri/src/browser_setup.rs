//! User-consented native-host setup. No browser profile inspection, installer
//! execution, silent extension installation, or arbitrary registry paths.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

const HOST: &str = "app.osdm.capture";
static SETUP: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Browser {
    Chrome,
    Edge,
    Firefox,
}
impl Browser {
    fn slug(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Firefox => "firefox",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Firefox => "Mozilla Firefox",
        }
    }
    #[cfg(windows)]
    fn registry_key(self) -> &'static str {
        match self {
            Self::Chrome => "Software\\Google\\Chrome\\NativeMessagingHosts\\app.osdm.capture",
            Self::Edge => "Software\\Microsoft\\Edge\\NativeMessagingHosts\\app.osdm.capture",
            Self::Firefox => "Software\\Mozilla\\NativeMessagingHosts\\app.osdm.capture",
        }
    }
    fn configured_id(self) -> Option<String> {
        let value = match self {
            Self::Chrome => option_env!("OSDM_CHROME_EXTENSION_ID"),
            Self::Edge => option_env!("OSDM_EDGE_EXTENSION_ID"),
            Self::Firefox => Some("osdm@osdm.app"),
        }?;
        valid_id(self, value).then(|| value.to_owned())
    }
    fn store_url(self) -> Option<String> {
        match self {
            Self::Chrome => self
                .configured_id()
                .map(|id| format!("https://chromewebstore.google.com/detail/{id}")),
            Self::Edge => self
                .configured_id()
                .map(|id| format!("https://microsoftedge.microsoft.com/addons/detail/{id}")),
            Self::Firefox => option_env!("OSDM_FIREFOX_ADDON_SLUG").and_then(|slug| {
                (!slug.is_empty()
                    && slug.len() <= 128
                    && slug.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    }))
                .then(|| format!("https://addons.mozilla.org/firefox/addon/{slug}/"))
            }),
        }
    }
}
fn valid_id(browser: Browser, id: &str) -> bool {
    if browser == Browser::Firefox {
        id == "osdm@osdm.app"
    } else {
        id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct HostManifest {
    name: String,
    description: String,
    path: PathBuf,
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    allowed_extensions: Option<Vec<String>>,
}
fn manifest(helper: &Path, browser: Browser, id: &str) -> Result<HostManifest, String> {
    if !valid_id(browser, id) {
        return Err(
            "Enter the installed extension ID: Chromium IDs have 32 lowercase letters a–p".into(),
        );
    }
    Ok(HostManifest {
        name: HOST.into(),
        description: "OSDM local download capture".into(),
        path: helper.to_owned(),
        kind: "stdio".into(),
        allowed_origins: (browser != Browser::Firefox)
            .then(|| vec![format!("chrome-extension://{id}/")]),
        allowed_extensions: (browser == Browser::Firefox).then(|| vec![id.into()]),
    })
}
fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|_| "Cannot read native-host registration")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 64 * 1024 {
        return Err("Native-host registration is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Cannot read native-host registration")?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read native-host registration")?;
    if bytes.len() > 64 * 1024 {
        return Err("Native-host registration is too large".into());
    }
    Ok(bytes)
}
fn owned_registration(root: &Path, path: &Path, helper: &Path, browser: Browser) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    if path.parent() != Some(root.as_path()) {
        return false;
    }
    let Ok(bytes) = read_bounded(path) else {
        return false;
    };
    let expected = format!("{}-{:x}.json", browser.slug(), Sha256::digest(&bytes));
    if path.file_name().and_then(|name| name.to_str()) != Some(&expected) {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<HostManifest>(&bytes) else {
        return false;
    };
    let id = if browser == Browser::Firefox {
        value
            .allowed_extensions
            .as_ref()
            .filter(|ids| ids.len() == 1)
            .map(|ids| ids[0].clone())
    } else {
        value
            .allowed_origins
            .as_ref()
            .filter(|ids| ids.len() == 1)
            .and_then(|ids| {
                ids[0]
                    .strip_prefix("chrome-extension://")
                    .and_then(|id| id.strip_suffix('/'))
                    .map(str::to_owned)
            })
    };
    id.and_then(|id| manifest(helper, browser, &id).ok())
        .is_some_and(|expected| value == expected)
}

trait Registry {
    fn read(&self, browser: Browser) -> Result<Option<PathBuf>, String>;
    fn write(&self, browser: Browser, path: &Path) -> Result<(), String>;
    fn remove(&self, browser: Browser) -> Result<(), String>;
}
struct SystemRegistry;
impl Registry for SystemRegistry {
    fn remove(&self, browser: Browser) -> Result<(), String> {
        #[cfg(windows)]
        {
            use winreg::{
                RegKey,
                enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
            };
            let key = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags(browser.registry_key(), KEY_SET_VALUE)
                .map_err(|_| "Cannot open per-user browser registration")?;
            key.delete_value("")
                .map_err(|_| "Cannot remove per-user browser registration".into())
        }
        #[cfg(not(windows))]
        {
            let _ = browser;
            Err("Guided native-host registration currently requires Windows".into())
        }
    }
    fn read(&self, browser: Browser) -> Result<Option<PathBuf>, String> {
        #[cfg(windows)]
        {
            use winreg::{RegKey, enums::HKEY_CURRENT_USER};
            let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey(browser.registry_key()) {
                Ok(key) => key,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("Cannot read per-user browser registration".into()),
            };
            match key.get_value::<String, _>("") {
                Ok(value) => Ok(Some(PathBuf::from(value))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err("Cannot read per-user browser registration".into()),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = browser;
            Ok(None)
        }
    }
    fn write(&self, browser: Browser, path: &Path) -> Result<(), String> {
        #[cfg(windows)]
        {
            use winreg::{RegKey, enums::HKEY_CURRENT_USER};
            let key = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(browser.registry_key())
                .map_err(|_| "Cannot create per-user browser registration")?
                .0;
            key.set_value("", &path.to_string_lossy().as_ref())
                .map_err(|_| "Cannot save per-user browser registration".into())
        }
        #[cfg(not(windows))]
        {
            let _ = (browser, path);
            Err("Guided native-host registration currently requires Windows".into())
        }
    }
}

fn unregister(
    root: &Path,
    helper: &Path,
    browser: Browser,
    confirmed: bool,
    registry: &impl Registry,
) -> Result<(), String> {
    if !confirmed {
        return Err("Confirm removal of this browser's OSDM helper registration".into());
    }
    let Some(current) = registry.read(browser)? else {
        return Ok(());
    };
    if !owned_registration(root, &current, helper, browser) {
        return Err("Refusing to remove a different or modified native-host registration".into());
    }
    if registry.read(browser)?.as_ref() != Some(&current) {
        return Err("Browser registration changed during removal; refresh and try again".into());
    }
    // Remove only our default value. Leave files, other values and subkeys alone.
    registry.remove(browser)
}
fn register(
    root: &Path,
    helper: &Path,
    browser: Browser,
    id: &str,
    confirmed: bool,
    registry: &impl Registry,
) -> Result<(), String> {
    if !confirmed {
        return Err("Confirm per-user browser registration before continuing".into());
    }
    let value = manifest(helper, browser, id)?;
    if !helper.is_absolute()
        || !std::fs::symlink_metadata(helper)
            .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
    {
        return Err(
            "The packaged native helper is missing; reinstall a bundle with browser integration"
                .into(),
        );
    }
    let previous = registry.read(browser)?;
    if previous
        .as_ref()
        .is_some_and(|path| !owned_registration(root, path, helper, browser))
    {
        return Err("A different native-host registration exists. OSDM will not overwrite it; remove or migrate it manually first".into());
    }
    std::fs::create_dir_all(root).map_err(|_| "Cannot create per-user native-host directory")?;
    let root = std::fs::canonicalize(root).map_err(|_| "Cannot resolve native-host directory")?;
    let bytes =
        serde_json::to_vec_pretty(&value).map_err(|_| "Cannot encode native-host manifest")?;
    let path = root.join(format!(
        "{}-{:x}.json",
        browser.slug(),
        Sha256::digest(&bytes)
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(&bytes)
                .and_then(|_| file.flush())
                .and_then(|_| file.sync_all())
                .map_err(|_| "Cannot persist native-host manifest")?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_bounded(&path)? != bytes {
                return Err("Refusing to replace an unrelated native-host manifest".into());
            }
        }
        Err(_) => return Err("Cannot create native-host manifest".into()),
    }
    // Recheck before changing the only registry value this operation owns.
    if registry.read(browser)? != previous {
        return Err("Browser registration changed during setup; refresh and try again".into());
    }
    registry.write(browser, &path)
}

#[derive(Serialize)]
pub(crate) struct BrowserStatus {
    browser: Browser,
    name: &'static str,
    store_url: Option<String>,
    extension_id: Option<String>,
    registered: bool,
    conflict: bool,
    unpacked_available: bool,
}
#[derive(Serialize)]
pub(crate) struct SetupStatus {
    registration_supported: bool,
    helper_available: bool,
    capture_enabled: bool,
    last_handshake: Option<u64>,
    browsers: Vec<BrowserStatus>,
}
fn paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(|_| "Cannot locate packaged browser resources")?
        .join("browser-integration");
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Cannot locate per-user OSDM data")?
        .join("native-hosts/guided");
    Ok((resources, root))
}
fn helper_path(resources: &Path) -> PathBuf {
    resources.join(if cfg!(windows) {
        "osdm-native-host.exe"
    } else {
        "osdm-native-host"
    })
}
fn extension_path(resources: &Path, browser: Browser) -> PathBuf {
    resources.join("extension").join(match browser {
        Browser::Chrome => "chromium",
        Browser::Edge => "edge",
        Browser::Firefox => "firefox",
    })
}
fn allow_native_setup() -> Result<(), String> {
    if osdm_browser_bridge::native_test_directory()
        .map_err(|_| "Invalid native test directory")?
        .is_some()
    {
        return Err(
            "Browser registration and external launches are disabled in isolated native tests"
                .into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn browser_setup_status(app: tauri::AppHandle) -> Result<SetupStatus, String> {
    let _guard = SETUP.lock().map_err(|_| "Browser setup is unavailable")?;
    let (resources, root) = paths(&app)?;
    let helper = helper_path(&resources);
    let isolated = osdm_browser_bridge::native_test_directory()
        .map_err(|_| "Invalid native test directory")?
        .is_some();
    let bridge = app.try_state::<std::sync::Arc<crate::capture::CaptureBridge>>();
    let enabled = bridge
        .as_ref()
        .is_some_and(|bridge| bridge.enabled.load(std::sync::atomic::Ordering::Acquire));
    let last_handshake = bridge.as_ref().and_then(|bridge| bridge.last_handshake());
    let mut browsers = Vec::new();
    for browser in [Browser::Chrome, Browser::Edge, Browser::Firefox] {
        let current = if isolated {
            None
        } else {
            SystemRegistry.read(browser)?
        };
        let owned = current
            .as_ref()
            .is_some_and(|path| owned_registration(&root, path, &helper, browser));
        let registered = owned
            && browser.configured_id().is_none_or(|id| {
                current
                    .as_ref()
                    .and_then(|path| read_bounded(path).ok())
                    .and_then(|bytes| serde_json::from_slice::<HostManifest>(&bytes).ok())
                    .is_some_and(|value| {
                        manifest(&helper, browser, &id).is_ok_and(|expected| value == expected)
                    })
            });
        browsers.push(BrowserStatus {
            browser,
            name: browser.name(),
            store_url: browser.store_url(),
            extension_id: browser.configured_id(),
            registered,
            conflict: current.is_some() && !owned,
            unpacked_available: extension_path(&resources, browser)
                .join("manifest.json")
                .is_file(),
        });
    }
    Ok(SetupStatus {
        registration_supported: cfg!(windows) && !isolated,
        helper_available: std::fs::symlink_metadata(&helper)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink()),
        capture_enabled: enabled,
        last_handshake,
        browsers,
    })
}
#[tauri::command]
pub(crate) fn browser_setup_register(
    app: tauri::AppHandle,
    browser: Browser,
    extension_id: String,
    confirmed: bool,
) -> Result<(), String> {
    allow_native_setup()?;
    if !cfg!(windows) {
        return Err("Guided native-host registration currently requires Windows".into());
    }
    let _guard = SETUP.lock().map_err(|_| "Browser setup is unavailable")?;
    if browser
        .configured_id()
        .is_some_and(|configured| configured != extension_id)
    {
        return Err("Use the extension ID configured for this release".into());
    }
    let (resources, root) = paths(&app)?;
    register(
        &root,
        &helper_path(&resources),
        browser,
        &extension_id,
        confirmed,
        &SystemRegistry,
    )
}
#[tauri::command]
pub(crate) fn browser_setup_unregister(
    app: tauri::AppHandle,
    browser: Browser,
    confirmed: bool,
) -> Result<(), String> {
    allow_native_setup()?;
    if !cfg!(windows) {
        return Err("Guided native-host registration currently requires Windows".into());
    }
    let _guard = SETUP.lock().map_err(|_| "Browser setup is unavailable")?;
    let (resources, root) = paths(&app)?;
    unregister(
        &root,
        &helper_path(&resources),
        browser,
        confirmed,
        &SystemRegistry,
    )
}
#[tauri::command]
pub(crate) fn browser_setup_open_store(
    app: tauri::AppHandle,
    browser: Browser,
) -> Result<(), String> {
    allow_native_setup()?;
    let url = browser.store_url().ok_or("No published store listing is configured for this browser; use the manual setup instructions")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Could not open the browser store".into())
}
#[tauri::command]
pub(crate) fn browser_setup_open_files(
    app: tauri::AppHandle,
    browser: Browser,
) -> Result<(), String> {
    allow_native_setup()?;
    let (resources, _) = paths(&app)?;
    let directory = extension_path(&resources, browser);
    if !directory.join("manifest.json").is_file() {
        return Err("This build has no packaged extension files. Use pnpm desktop:build or the source setup instructions".into());
    }
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|_| "Could not open packaged extension files".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";
    #[derive(Default)]
    struct MemoryRegistry {
        entries: RefCell<Vec<(Browser, PathBuf)>>,
        fail: Cell<bool>,
        writes: Cell<usize>,
    }
    impl Registry for MemoryRegistry {
        fn remove(&self, browser: Browser) -> Result<(), String> {
            self.entries
                .borrow_mut()
                .retain(|(target, _)| *target != browser);
            Ok(())
        }
        fn read(&self, browser: Browser) -> Result<Option<PathBuf>, String> {
            Ok(self
                .entries
                .borrow()
                .iter()
                .find(|(target, _)| *target == browser)
                .map(|(_, path)| path.clone()))
        }
        fn write(&self, browser: Browser, path: &Path) -> Result<(), String> {
            self.writes.set(self.writes.get() + 1);
            if self.fail.replace(false) {
                return Err("synthetic registry write failure".into());
            }
            self.entries
                .borrow_mut()
                .retain(|(target, _)| *target != browser);
            self.entries.borrow_mut().push((browser, path.to_owned()));
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("osdm-native-host.exe");
        std::fs::write(&helper, b"synthetic helper, never executed").unwrap();
        let root = dir.path().join("guided");
        (dir, root, helper)
    }
    #[test]
    fn registration_requires_consent_exact_identity_and_a_packaged_helper() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        assert!(register(&root, &helper, Browser::Chrome, ID, false, &registry).is_err());
        for id in [
            "",
            "invalid",
            "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP",
            "abcdefghijklmnopabcdefghijklmnop/../../",
        ] {
            assert!(register(&root, &helper, Browser::Chrome, id, true, &registry).is_err());
        }
        assert!(
            register(
                &root,
                &helper.with_file_name("missing.exe"),
                Browser::Chrome,
                ID,
                true,
                &registry
            )
            .is_err()
        );
        assert!(!root.exists());
        assert_eq!(registry.writes.get(), 0);
    }
    #[test]
    fn chrome_registration_has_one_origin_and_never_changes_another_browser() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        let path = registry.read(Browser::Chrome).unwrap().unwrap();
        assert!(owned_registration(&root, &path, &helper, Browser::Chrome));
        let value: HostManifest = serde_json::from_slice(&read_bounded(&path).unwrap()).unwrap();
        assert_eq!(
            value.allowed_origins,
            Some(vec![format!("chrome-extension://{ID}/")])
        );
        assert!(value.allowed_extensions.is_none());
        assert_eq!(value.path, helper);
        assert!(registry.read(Browser::Edge).unwrap().is_none());
        assert!(registry.read(Browser::Firefox).unwrap().is_none());
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        assert_eq!(registry.read(Browser::Chrome).unwrap(), Some(path));
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
    #[test]
    fn firefox_allowlists_only_its_fixed_identity() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        assert!(register(&root, &helper, Browser::Firefox, ID, true, &registry).is_err());
        register(
            &root,
            &helper,
            Browser::Firefox,
            "osdm@osdm.app",
            true,
            &registry,
        )
        .unwrap();
        let path = registry.read(Browser::Firefox).unwrap().unwrap();
        let value: HostManifest = serde_json::from_slice(&read_bounded(&path).unwrap()).unwrap();
        assert!(value.allowed_origins.is_none());
        assert_eq!(value.allowed_extensions, Some(vec!["osdm@osdm.app".into()]));
    }
    #[test]
    fn foreign_registration_and_tampered_owned_files_are_never_replaced() {
        let (dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        let foreign = dir.path().join("foreign.json");
        std::fs::write(&foreign, b"user work").unwrap();
        registry.write(Browser::Edge, &foreign).unwrap();
        assert!(register(&root, &helper, Browser::Edge, ID, true, &registry).is_err());
        assert_eq!(std::fs::read(&foreign).unwrap(), b"user work");
        assert!(!root.exists());
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        let path = registry.read(Browser::Chrome).unwrap().unwrap();
        std::fs::write(&path, b"modified manifest").unwrap();
        assert!(register(&root, &helper, Browser::Chrome, ID, true, &registry).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"modified manifest");
    }
    #[test]
    fn failed_registry_write_is_retryable_without_overwriting_a_manifest() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        registry.fail.set(true);
        assert!(register(&root, &helper, Browser::Chrome, ID, true, &registry).is_err());
        assert!(registry.read(Browser::Chrome).unwrap().is_none());
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
    #[test]
    fn preexisting_hash_path_collision_is_not_overwritten() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        std::fs::create_dir(&root).unwrap();
        let bytes =
            serde_json::to_vec_pretty(&manifest(&helper, Browser::Chrome, ID).unwrap()).unwrap();
        let path = root.join(format!("chrome-{:x}.json", Sha256::digest(bytes)));
        std::fs::write(&path, b"unrelated file").unwrap();
        assert!(register(&root, &helper, Browser::Chrome, ID, true, &registry).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"unrelated file");
        assert_eq!(registry.writes.get(), 0);
    }
    #[test]
    fn unregister_requires_confirmation_and_preserves_foreign_registrations_and_files() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        let owned = registry.read(Browser::Chrome).unwrap().unwrap();
        registry
            .write(Browser::Edge, &helper.with_file_name("foreign.json"))
            .unwrap();
        assert!(unregister(&root, &helper, Browser::Chrome, false, &registry).is_err());
        assert!(unregister(&root, &helper, Browser::Edge, true, &registry).is_err());
        unregister(&root, &helper, Browser::Chrome, true, &registry).unwrap();
        assert!(registry.read(Browser::Chrome).unwrap().is_none());
        assert!(registry.read(Browser::Edge).unwrap().is_some());
        assert!(owned.is_file());
        assert!(helper.is_file());
    }
}
