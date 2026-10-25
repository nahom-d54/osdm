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
        if !(0..=4).contains(&version) {
            return Err(crate::Error::Invalid(
                "database was created by a newer engine version".into(),
            ));
        }
        let mut tx = pool.begin().await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY, position INTEGER NOT NULL, payload TEXT NOT NULL)")
            .execute(&mut *tx).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS orphaned_partials (id TEXT PRIMARY KEY, payload TEXT NOT NULL)")
            .execute(&mut *tx).await?;
        // Older engines must not discard organization metadata when saving.
        sqlx::query("PRAGMA user_version = 4")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
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

    pub async fn remove(&self, download: &Download, retain_partial: bool) -> Result<()> {
        let mut tx = self.0.begin().await?;
        if retain_partial {
            sqlx::query("INSERT INTO orphaned_partials(id, payload) VALUES (?, ?)")
                .bind(download.id.to_string())
                .bind(serde_json::to_string(&crate::cleanup::OwnedPartial::from(
                    download,
                ))?)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM downloads WHERE id=?")
            .bind(download.id.to_string())
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query("SELECT id FROM downloads ORDER BY position")
            .fetch_all(&mut *tx)
            .await?;
        for (position, row) in rows.iter().enumerate() {
            sqlx::query("UPDATE downloads SET position=? WHERE id=?")
                .bind(position as i64)
                .bind(row.try_get::<String, _>("id")?)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn orphans(&self) -> Result<Vec<crate::cleanup::OwnedPartial>> {
        let rows = sqlx::query("SELECT payload FROM orphaned_partials")
            .fetch_all(&self.0)
            .await?;
        rows.iter()
            .map(|row| Ok(serde_json::from_str(row.try_get("payload")?)?))
            .collect()
    }

    pub async fn forget_orphan(&self, id: crate::DownloadId) -> Result<()> {
        sqlx::query("DELETE FROM orphaned_partials WHERE id=?")
            .bind(id.to_string())
            .execute(&self.0)
            .await?;
        Ok(())
    }
}
