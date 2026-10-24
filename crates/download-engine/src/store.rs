use crate::{Download, Result};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, str::FromStr};

pub(crate) struct Store(SqlitePool);

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite:")?
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;
        if version > 1 {
            return Err(crate::Error::Invalid(
                "database was created by a newer engine version".into(),
            ));
        }
        sqlx::query("CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY, position INTEGER NOT NULL, payload TEXT NOT NULL)")
            .execute(&pool).await?;
        sqlx::query("PRAGMA user_version = 1")
            .execute(&pool)
            .await?;
        Ok(Self(pool))
    }

    pub async fn load(&self) -> Result<Vec<Download>> {
        let rows = sqlx::query("SELECT payload FROM downloads ORDER BY position")
            .fetch_all(&self.0)
            .await?;
        rows.iter()
            .map(|row| Ok(serde_json::from_str(row.try_get("payload")?)?))
            .collect()
    }

    pub async fn save(&self, download: &Download, position: usize) -> Result<()> {
        sqlx::query("INSERT INTO downloads(id, position, payload) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET position=excluded.position, payload=excluded.payload")
            .bind(download.id.to_string()).bind(position as i64).bind(serde_json::to_string(download)?)
            .execute(&self.0).await?;
        Ok(())
    }

    pub async fn reorder(&self, downloads: &[Download]) -> Result<()> {
        let mut tx = self.0.begin().await?;
        for (position, download) in downloads.iter().enumerate() {
            sqlx::query("UPDATE downloads SET position=? WHERE id=?")
                .bind(position as i64)
                .bind(download.id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
