use download_engine::Config;
use serde::{Deserialize, Serialize};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous},
};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub theme: Theme,
    pub speed_limit_bytes_per_second: u64,
    pub max_active_downloads: usize,
    pub connections_per_download: usize,
    pub max_connections: usize,
    pub max_connections_per_host: usize,
    pub split_threshold_mib: u64,
    pub max_retries: u32,
    pub default_directory: String,
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
    #[serde(default = "default_true")]
    pub include_prereleases: bool,
    #[serde(default = "default_categories")]
    pub categories: Vec<Category>,
    #[serde(default = "default_true")]
    pub remember_last_directory: bool,
    #[serde(default)]
    pub desktop: DesktopPreferences,
    #[serde(default = "default_columns")]
    pub visible_columns: Vec<String>,
    #[serde(default = "default_toolbar")]
    pub toolbar_actions: Vec<String>,
    #[serde(default)]
    pub temporary_directory: String,
    #[serde(default)]
    pub onboarding_complete: bool,
}

fn default_columns() -> Vec<String> {
    ["state", "rate", "eta", "size"]
        .into_iter()
        .map(String::from)
        .collect()
}
fn default_toolbar() -> Vec<String> {
    ["resume", "pause", "stop", "inspector"]
        .into_iter()
        .map(String::from)
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DesktopPreferences {
    pub silent: bool,
    pub completion_dialog: bool,
    pub progress_dialog: bool,
    pub minimized_progress: bool,
    pub confirm_downloads: bool,
    pub sound_file: String,
    pub scanner_enabled: bool,
    pub scanner_executable: String,
    pub scanner_arguments: Vec<String>,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            silent: false,
            completion_dialog: true,
            progress_dialog: false,
            minimized_progress: false,
            confirm_downloads: true,
            sound_file: String::new(),
            scanner_enabled: false,
            scanner_executable: String::new(),
            scanner_arguments: vec!["{file}".into()],
        }
    }
}

impl DesktopPreferences {
    fn validate(&self) -> Result<(), String> {
        #[cfg(windows)]
        if self.scanner_enabled
            && !Path::new(&self.scanner_executable)
                .extension()
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
                })
        {
            return Err("scanner must be a native .exe or .com executable".into());
        }
        if self.sound_file.len() > 32768
            || self.sound_file.contains('\0')
            || self.scanner_executable.contains('\0')
            || self.scanner_executable.len() > 32768
            || self.scanner_arguments.len() > 64
            || self
                .scanner_arguments
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
            || (!self.sound_file.is_empty() && !Path::new(&self.sound_file).is_absolute())
            || (!self.scanner_executable.is_empty()
                && !Path::new(&self.scanner_executable).is_absolute())
            || (self.scanner_enabled
                && (self.scanner_executable.is_empty()
                    || !self.scanner_arguments.iter().any(|arg| arg == "{file}")))
        {
            return Err("invalid desktop settings; scanner needs an absolute executable and a {file} argument".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub domains: Vec<String>,
    pub directory: String,
}

fn default_categories() -> Vec<Category> {
    [
        ("music", "Music", "mp3 flac wav ogg m4a"),
        ("video", "Video", "mp4 mkv webm avi mov"),
        ("programs", "Programs", "exe msi msix deb rpm"),
        ("documents", "Documents", "pdf txt doc docx epub"),
        ("compressed", "Compressed", "zip 7z rar tar gz bz2 xz zst"),
    ]
    .into_iter()
    .map(|(id, name, extensions)| Category {
        id: id.into(),
        name: name.into(),
        extensions: extensions.split_whitespace().map(String::from).collect(),
        domains: vec![],
        directory: String::new(),
    })
    .collect()
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            speed_limit_bytes_per_second: 0,
            max_active_downloads: 3,
            connections_per_download: 4,
            max_connections: 12,
            max_connections_per_host: 8,
            split_threshold_mib: 16,
            max_retries: 3,
            default_directory: String::new(),
            auto_update_check: true,
            include_prereleases: true,
            categories: default_categories(),
            remember_last_directory: true,
            desktop: DesktopPreferences::default(),
            visible_columns: default_columns(),
            toolbar_actions: default_toolbar(),
            temporary_directory: String::new(),
            onboarding_complete: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.desktop.validate()?;
        if !self.temporary_directory.is_empty()
            && (!Path::new(&self.temporary_directory).is_absolute()
                || self.temporary_directory.len() > 32768)
        {
            return Err("temporary directory must be absolute".into());
        }
        for (values, allowed) in [
            (&self.visible_columns, default_columns()),
            (&self.toolbar_actions, default_toolbar()),
        ] {
            let unique: std::collections::HashSet<_> = values.iter().collect();
            if values.len() != unique.len() || values.iter().any(|value| !allowed.contains(value)) {
                return Err("invalid toolbar or column choices".into());
            }
        }
        let mut ids = std::collections::HashSet::new();
        if self.categories.len() > 100
            || self.categories.iter().any(|category| {
                category.id.is_empty()
                    || category.id.len() > 128
                    || !ids.insert(&category.id)
                    || category.name.len() > 128
                    || category.directory.len() > 32768
                    || category.directory.contains('\0')
                    || category.extensions.len() > 200
                    || category.domains.len() > 200
                    || category
                        .extensions
                        .iter()
                        .chain(&category.domains)
                        .any(|rule| rule.len() > 253)
            })
        {
            return Err("invalid category settings".into());
        }
        if !(1..=32).contains(&self.max_active_downloads)
            || !(1..=8).contains(&self.connections_per_download)
            || !(1..=256).contains(&self.max_connections)
            || !(1..=64).contains(&self.max_connections_per_host)
            || self.max_retries > 10
            || self.split_threshold_mib > 1_048_576
            || self.speed_limit_bytes_per_second > 9_007_199_254_740_991
            || self.default_directory.len() > 32768
        {
            return Err("invalid settings values".into());
        }
        Ok(())
    }

    pub fn engine_config(&self) -> Config {
        Config {
            max_active_downloads: self.max_active_downloads,
            connections_per_download: self.connections_per_download,
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            split_threshold: self.split_threshold_mib * 1024 * 1024,
            bytes_per_second: self.speed_limit_bytes_per_second,
            max_retries: self.max_retries,
        }
    }
}

pub struct SettingsStore(SqlitePool);

impl SettingsStore {
    pub async fn close(&self) {
        self.0.close().await;
    }
    pub async fn open(path: &Path) -> Result<Self, String> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|e| e.to_string())?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
        if !(0..=1).contains(&version) {
            return Err("settings database requires a newer application".into());
        }
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL)")
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("PRAGMA user_version=1")
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(Self(pool))
    }

    pub async fn load(&self) -> Result<Option<Settings>, String> {
        let value: Option<String> = sqlx::query_scalar("SELECT payload FROM settings WHERE id=1")
            .fetch_optional(&self.0)
            .await
            .map_err(|e| e.to_string())?;
        value
            .map(|value| {
                let settings: Settings = serde_json::from_str(&value).map_err(|e| e.to_string())?;
                settings.validate()?;
                Ok(settings)
            })
            .transpose()
    }

    pub async fn save(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        let value = serde_json::to_string(settings).map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO settings(id,payload) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload")
            .bind(value).execute(&self.0).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_actions_require_safe_explicit_configuration() {
        let mut settings = Settings::default();
        settings.desktop.scanner_enabled = true;
        assert!(settings.validate().is_err());
        settings.desktop.scanner_enabled = false;
        settings.toolbar_actions.push("pause".into());
        assert!(settings.validate().is_err());
        settings.toolbar_actions = default_toolbar();
        settings.visible_columns.push("unknown".into());
        assert!(settings.validate().is_err());
        settings.visible_columns = default_columns();
        settings.temporary_directory = "relative".into();
        assert!(settings.validate().is_err());
    }

    #[tokio::test]
    async fn settings_survive_reopen_and_reject_invalid_updates() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::open(&dir.path().join("settings.db"))
            .await
            .unwrap();
        assert!(store.load().await.unwrap().is_none());
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "theme":"dark", "speed_limit_bytes_per_second": 42,
            "max_active_downloads":3,"connections_per_download":4,"max_connections":12,
            "max_connections_per_host":8,"split_threshold_mib":16,"max_retries":3,
            "default_directory":"C:\\Downloads"
        }))
        .unwrap();
        assert!(settings.auto_update_check && settings.include_prereleases);
        assert_eq!(settings.categories, default_categories());
        assert!(settings.remember_last_directory);
        store.save(&settings).await.unwrap();
        let mut invalid = settings.clone();
        invalid.max_active_downloads = 0;
        assert!(store.save(&invalid).await.is_err());
        store.0.close().await;
        let reopened = SettingsStore::open(&dir.path().join("settings.db"))
            .await
            .unwrap();
        assert_eq!(reopened.load().await.unwrap(), Some(settings));
    }

    #[tokio::test]
    async fn category_edits_and_deletion_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.db");
        let store = SettingsStore::open(&path).await.unwrap();
        let mut settings = Settings::default();
        settings.categories[0].directory = "C:\\Music".into();
        settings.categories[0].domains.push("example.test".into());
        settings.categories.remove(1);
        settings.remember_last_directory = false;
        store.save(&settings).await.unwrap();
        let mut invalid = settings.clone();
        invalid.categories.push(invalid.categories[0].clone());
        assert!(store.save(&invalid).await.is_err());
        store.close().await;
        let reopened = SettingsStore::open(&path).await.unwrap();
        assert_eq!(reopened.load().await.unwrap(), Some(settings.clone()));
        settings.categories.clear();
        reopened.save(&settings).await.unwrap();
        assert!(
            reopened
                .load()
                .await
                .unwrap()
                .unwrap()
                .categories
                .is_empty()
        );
    }
}
