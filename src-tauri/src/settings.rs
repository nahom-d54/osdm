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
}
