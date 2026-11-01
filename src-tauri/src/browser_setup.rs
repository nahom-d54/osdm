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
    Brave,
    Chromium,
    Firefox,
}
const BROWSERS: [Browser; 5] = [
    Browser::Chrome,
    Browser::Edge,
    Browser::Brave,
    Browser::Chromium,
    Browser::Firefox,
];
impl Browser {
    fn slug(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Brave => "brave",
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "Brave",
            Self::Chromium => "Chromium",
            Self::Firefox => "Mozilla Firefox",
        }
    }
    #[cfg(windows)]
    fn registry_key(self) -> &'static str {
        match self {
            Self::Chrome => "Software\\Google\\Chrome\\NativeMessagingHosts\\app.osdm.capture",
            Self::Edge => "Software\\Microsoft\\Edge\\NativeMessagingHosts\\app.osdm.capture",
            Self::Brave => {
                "Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts\\app.osdm.capture"
            }
            Self::Chromium => "Software\\Chromium\\NativeMessagingHosts\\app.osdm.capture",
            Self::Firefox => "Software\\Mozilla\\NativeMessagingHosts\\app.osdm.capture",
        }
    }
    fn configured_id(self) -> Option<String> {
        let value = match self {
            Self::Chrome | Self::Brave | Self::Chromium => option_env!("OSDM_BUNDLED_CHROME_ID"),
            Self::Edge => option_env!("OSDM_BUNDLED_EDGE_ID"),
            Self::Firefox => Some("osdm@osdm.app"),
        };
        if let Some(value) = value.filter(|value| valid_id(self, value)) {
            return Some(value.to_owned());
        }
        // Source builds share the packaged fallback identity. A public key is
        // not a signed store listing and must never imply a store URL.
        let identity: serde_json::Value =
            serde_json::from_str(include_str!("../../extension/identity.json")).ok()?;
        identity["chromium"]["id"]
            .as_str()
            .filter(|id| valid_id(self, id))
            .map(str::to_owned)
    }
    fn store_url(self) -> Option<String> {
        match self {
            Self::Chrome | Self::Brave | Self::Chromium => option_env!("OSDM_CHROME_EXTENSION_ID")
                .filter(|id| valid_id(self, id))
                .map(|id| format!("https://chromewebstore.google.com/detail/{id}")),
            Self::Edge => option_env!("OSDM_EDGE_EXTENSION_ID")
                .filter(|id| valid_id(self, id))
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
fn manifest_ids(value: &HostManifest, browser: Browser) -> Option<Vec<String>> {
    if browser == Browser::Firefox {
        return value
            .allowed_extensions
            .clone()
            .filter(|ids| ids == &["osdm@osdm.app"]);
    }
    let origins = value.allowed_origins.as_ref()?;
    if origins.is_empty() || origins.len() > 32 {
        return None;
    }
    let mut ids = Vec::new();
    for origin in origins {
        let id = origin
            .strip_prefix("chrome-extension://")?
            .strip_suffix('/')?;
        if !valid_id(browser, id) || ids.iter().any(|existing| existing == id) {
            return None;
        }
        ids.push(id.to_owned());
    }
    Some(ids)
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
    let parent = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok());
    let guided = parent.is_some() && parent == std::fs::canonicalize(root).ok();
    let legacy_name = if browser == Browser::Firefox {
        "firefox.json"
    } else {
        "chromium.json"
    };
    let legacy = parent.is_some()
        && parent
            == root
                .parent()
                .and_then(|parent| std::fs::canonicalize(parent).ok())
        && path.file_name().and_then(|name| name.to_str()) == Some(legacy_name);
    if !guided && !legacy {
        return false;
    }
    let Ok(bytes) = read_bounded(path) else {
        return false;
    };
    let expected = format!("{}-{:x}.json", browser.slug(), Sha256::digest(&bytes));
    if guided && path.file_name().and_then(|name| name.to_str()) != Some(&expected) {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<HostManifest>(&bytes) else {
        return false;
    };
    let Some(ids) = manifest_ids(&value, browser) else {
        return false;
    };
    // Installation moves/upgrades must not turn a genuine old registration
    // into a foreign one. Its owned content hash and schema still have to match.
    if !value.path.is_absolute() || value.path.file_name() != helper.file_name() {
        return false;
    }
    let Ok(mut expected) = manifest(&value.path, browser, &ids[0]) else {
        return false;
    };
    if browser != Browser::Firefox {
        expected.allowed_origins = Some(
            ids.iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
        );
    }
    value == expected
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
    register_with_ids(root, helper, browser, id, confirmed, registry, None)
}
fn register_with_ids(
    root: &Path,
    helper: &Path,
    browser: Browser,
    id: &str,
    confirmed: bool,
    registry: &impl Registry,
    replacements: Option<&[String]>,
) -> Result<(), String> {
    if !confirmed {
        return Err("Confirm per-user browser registration before continuing".into());
    }
    let mut value = manifest(helper, browser, id)?;
    if let Some(ids) = replacements
        && (ids.is_empty()
            || ids.len() > 32
            || ids.iter().any(|id| !valid_id(browser, id))
            || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
            || (browser == Browser::Firefox && ids.len() != 1))
    {
        return Err("Use 1–32 unique valid extension IDs; Firefox has one fixed identity".into());
    }
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
    if replacements.is_none()
        && browser != Browser::Firefox
        && let Some(path) = &previous
    {
        let existing: HostManifest = serde_json::from_slice(&read_bounded(path)?)
            .map_err(|_| "Cannot decode existing registration")?;
        let mut ids =
            manifest_ids(&existing, browser).ok_or("Invalid existing extension identities")?;
        if !ids.iter().any(|previous| previous == id) {
            if ids.len() >= 32 {
                return Err(
                    "Too many extension identities; remove unused helper registrations first"
                        .into(),
                );
            }
            ids.push(id.to_owned());
        }
        value.allowed_origins = Some(
            ids.iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
        );
    }
    if browser != Browser::Firefox
        && let Some(ids) = replacements
    {
        value.allowed_origins = Some(
            ids.iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
        );
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
    extension_ids: Vec<String>,
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
        Browser::Chrome | Browser::Brave | Browser::Chromium => "chromium",
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum InstallerAction {
    Install,
    Repair,
    Uninstall,
}

// Kept outside Tauri/engine startup: the installer must work with an already
// running app and must never acquire a download database or open a webview.
fn installer_setup(
    root: &Path,
    helper: &Path,
    action: InstallerAction,
    registry: &impl Registry,
) -> Result<(), String> {
    let mut failed = false;
    for browser in BROWSERS {
        let result = (|| {
            let current = registry.read(browser)?;
            let owned = current
                .as_ref()
                .is_some_and(|path| owned_registration(root, path, helper, browser));
            if action == InstallerAction::Uninstall {
                // An old installation's uninstaller cannot remove a newer
                // installation's helper, even when both registrations are ours.
                let targets_this_install = current
                    .as_ref()
                    .and_then(|path| read_bounded(path).ok())
                    .and_then(|bytes| serde_json::from_slice::<HostManifest>(&bytes).ok())
                    .is_some_and(|value| value.path == helper);
                if owned && targets_this_install {
                    unregister(root, helper, browser, true, registry)?;
                }
            } else if action == InstallerAction::Install || owned {
                let id = browser
                    .configured_id()
                    .ok_or("Missing packaged extension identity")?;
                register(root, helper, browser, &id, true, registry)?;
            }
            Ok::<(), String>(())
        })();
        if result.is_err() {
            failed = true;
        }
    }
    if failed {
        Err(
            "Some browser registrations need attention; unrelated registrations were preserved"
                .into(),
        )
    } else {
        Ok(())
    }
}

pub(crate) fn installer_cli() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let action = match args.first().map(String::as_str) {
        Some("--install-browser-integration") => InstallerAction::Install,
        Some("--repair-browser-integration") => InstallerAction::Repair,
        Some("--uninstall-browser-integration") => InstallerAction::Uninstall,
        _ => return None,
    };
    let result = (|| {
        if args.len() != 1 || !cfg!(windows) {
            return Err("Unsupported installer invocation".into());
        }
        allow_native_setup()?;
        let exe = std::env::current_exe().map_err(|_| "Cannot locate installed app")?;
        let resources = exe
            .parent()
            .ok_or("Cannot locate installed resources")?
            .join("browser-integration");
        let data =
            PathBuf::from(std::env::var_os("APPDATA").ok_or("Cannot locate per-user app data")?);
        if !data.is_absolute() {
            return Err("Per-user app data must be absolute".into());
        }
        let root = data.join("app.osdm.desktop/native-hosts/guided");
        installer_setup(&root, &helper_path(&resources), action, &SystemRegistry)
    })();
    Some(if result.is_ok() { 0 } else { 1 })
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
    for browser in BROWSERS {
        let current = if isolated {
            None
        } else {
            SystemRegistry.read(browser)?
        };
        let owned = current
            .as_ref()
            .is_some_and(|path| owned_registration(&root, path, &helper, browser));
        let value = current
            .as_ref()
            .and_then(|path| read_bounded(path).ok())
            .and_then(|bytes| serde_json::from_slice::<HostManifest>(&bytes).ok());
        let registered = owned && value.as_ref().is_some_and(|value| value.path == helper);
        let extension_ids = if owned {
            value
                .as_ref()
                .and_then(|value| manifest_ids(value, browser))
                .unwrap_or_default()
        } else {
            vec![]
        };
        browsers.push(BrowserStatus {
            browser,
            name: browser.name(),
            store_url: browser.store_url(),
            extension_id: browser.configured_id(),
            extension_ids,
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
pub(crate) fn browser_setup_set_identities(
    app: tauri::AppHandle,
    browser: Browser,
    extension_ids: Vec<String>,
    confirmed: bool,
) -> Result<(), String> {
    allow_native_setup()?;
    if !cfg!(windows) {
        return Err("Guided native-host registration currently requires Windows".into());
    }
    let _guard = SETUP.lock().map_err(|_| "Browser setup is unavailable")?;
    let (resources, root) = paths(&app)?;
    let helper = helper_path(&resources);
    if extension_ids.is_empty() {
        return unregister(&root, &helper, browser, confirmed, &SystemRegistry);
    }
    register_with_ids(
        &root,
        &helper,
        browser,
        &extension_ids[0],
        confirmed,
        &SystemRegistry,
        Some(&extension_ids),
    )
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
    #[cfg(windows)]
    {
        let _ = app;
        launch_browser_store(browser, &url)
    }
    #[cfg(not(windows))]
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Could not open the browser store".into())
}

#[cfg(windows)]
fn launch_browser_store(browser: Browser, url: &str) -> Result<(), String> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    };
    let (name, relative) = match browser {
        Browser::Chrome => ("chrome.exe", "Google/Chrome/Application/chrome.exe"),
        Browser::Edge => ("msedge.exe", "Microsoft/Edge/Application/msedge.exe"),
        Browser::Brave => (
            "brave.exe",
            "BraveSoftware/Brave-Browser/Application/brave.exe",
        ),
        Browser::Chromium => ("chrome.exe", "Chromium/Application/chrome.exe"),
        Browser::Firefox => ("firefox.exe", "Mozilla Firefox/firefox.exe"),
    };
    let mut candidates = Vec::new();
    // Chromium's chrome.exe App Paths entry can belong to Google Chrome. Do
    // not accidentally open Chrome when the user selected unbranded Chromium.
    if browser != Browser::Chromium {
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Ok(key) = RegKey::predef(hive).open_subkey(format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{name}"
            )) && let Ok(value) = key.get_value::<String, _>("")
            {
                candidates.push(PathBuf::from(value.trim_matches('"')));
            }
        }
    }
    for base in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(base) {
            candidates.push(PathBuf::from(base).join(relative));
        }
    }
    let exe = candidates.into_iter().find(|path| path.is_absolute() && path.file_name().and_then(|file| file.to_str()).is_some_and(|file| file.eq_ignore_ascii_case(name)) && path.is_file())
        .ok_or("Selected browser was not found. Install it, or open its extension store in that browser manually")?;
    std::process::Command::new(exe)
        .arg(url)
        .spawn()
        .map_err(|_| "Could not launch the selected browser's extension store")?;
    Ok(())
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
    #[test]
    fn upgrade_and_multiple_profile_identities_preserve_existing_connections() {
        let (dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        let next_helper = dir.path().join("new-install/osdm-native-host.exe");
        std::fs::create_dir(next_helper.parent().unwrap()).unwrap();
        std::fs::write(&next_helper, b"new synthetic helper").unwrap();
        let second = "pppppppppppppppppppppppppppppppp";
        register(
            &root,
            &next_helper,
            Browser::Chrome,
            second,
            true,
            &registry,
        )
        .unwrap();
        let current = registry.read(Browser::Chrome).unwrap().unwrap();
        let value: HostManifest = serde_json::from_slice(&read_bounded(&current).unwrap()).unwrap();
        assert_eq!(value.path, next_helper);
        assert_eq!(
            manifest_ids(&value, Browser::Chrome).unwrap(),
            vec![ID, second]
        );
        installer_setup(&root, &helper, InstallerAction::Uninstall, &registry).unwrap();
        assert_eq!(registry.read(Browser::Chrome).unwrap(), Some(current));
    }
    #[test]
    fn legacy_script_registration_migrates_without_replacing_its_file() {
        let (dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        let legacy = dir.path().join("chromium.json");
        let bytes =
            serde_json::to_vec_pretty(&manifest(&helper, Browser::Chrome, ID).unwrap()).unwrap();
        std::fs::write(&legacy, &bytes).unwrap();
        registry.write(Browser::Chrome, &legacy).unwrap();
        register(&root, &helper, Browser::Chrome, ID, true, &registry).unwrap();
        assert_ne!(
            registry.read(Browser::Chrome).unwrap(),
            Some(legacy.clone())
        );
        assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
    }
    #[test]
    fn silent_installer_only_repairs_owned_registrations_and_uninstall_preserves_foreign() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        installer_setup(&root, &helper, InstallerAction::Repair, &registry).unwrap();
        assert_eq!(registry.writes.get(), 0);
        installer_setup(&root, &helper, InstallerAction::Install, &registry).unwrap();
        for browser in BROWSERS {
            assert!(registry.read(browser).unwrap().is_some());
        }
        let foreign = helper.with_file_name("foreign.json");
        registry.write(Browser::Edge, &foreign).unwrap();
        assert!(installer_setup(&root, &helper, InstallerAction::Install, &registry).is_err());
        installer_setup(&root, &helper, InstallerAction::Uninstall, &registry).unwrap();
        assert_eq!(registry.read(Browser::Edge).unwrap(), Some(foreign));
        for browser in [
            Browser::Chrome,
            Browser::Brave,
            Browser::Chromium,
            Browser::Firefox,
        ] {
            assert!(registry.read(browser).unwrap().is_none());
        }
    }
    #[test]
    fn fallback_identity_does_not_invent_a_store_listing() {
        for browser in BROWSERS {
            assert!(valid_id(browser, &browser.configured_id().unwrap()));
        }
        assert_eq!(
            Browser::Brave.configured_id(),
            Browser::Chrome.configured_id()
        );
        if option_env!("OSDM_CHROME_EXTENSION_ID").is_none() {
            assert!(Browser::Chrome.store_url().is_none());
            assert!(Browser::Brave.store_url().is_none());
        }
    }
    #[test]
    fn identity_editor_adds_edits_and_removes_without_touching_other_browsers() {
        let (_dir, root, helper) = fixture();
        let registry = MemoryRegistry::default();
        register(&root, &helper, Browser::Edge, ID, true, &registry).unwrap();
        let edge = registry.read(Browser::Edge).unwrap();
        let ids = vec![ID.to_owned(), "pppppppppppppppppppppppppppppppp".to_owned()];
        register_with_ids(
            &root,
            &helper,
            Browser::Chrome,
            &ids[0],
            true,
            &registry,
            Some(&ids),
        )
        .unwrap();
        let edited = vec![ids[1].clone()];
        register_with_ids(
            &root,
            &helper,
            Browser::Chrome,
            &edited[0],
            true,
            &registry,
            Some(&edited),
        )
        .unwrap();
        let path = registry.read(Browser::Chrome).unwrap().unwrap();
        let value: HostManifest = serde_json::from_slice(&read_bounded(&path).unwrap()).unwrap();
        assert_eq!(manifest_ids(&value, Browser::Chrome).unwrap(), edited);
        assert_eq!(registry.read(Browser::Edge).unwrap(), edge);
        assert!(
            register_with_ids(
                &root,
                &helper,
                Browser::Chrome,
                ID,
                false,
                &registry,
                Some(&ids)
            )
            .is_err()
        );
        assert!(
            register_with_ids(
                &root,
                &helper,
                Browser::Chrome,
                ID,
                true,
                &registry,
                Some(&[ID.to_owned(), ID.to_owned()])
            )
            .is_err()
        );
        assert_eq!(registry.read(Browser::Chrome).unwrap(), Some(path));
    }
}
