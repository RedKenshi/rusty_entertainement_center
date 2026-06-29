use std::path::{Path, PathBuf};

use async_trait::async_trait;
use sqlx::{Pool, Sqlite};

use crate::debug;

use super::{Result, SettingsRepository};

const LAST_OPENED_FOLDER_KEY: &str = "last_opened_folder";

pub struct SqliteSettingsRepository {
    pool: Pool<Sqlite>,
}

impl SqliteSettingsRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get_last_opened_folder(&self) -> Result<Option<PathBuf>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                .bind(LAST_OPENED_FOLDER_KEY)
                .fetch_optional(&self.pool)
                .await?;

        let Some(json) = value else {
            debug::db("settings get last_opened_folder -> none");
            return Ok(None);
        };

        let path = serde_json::from_str::<String>(&json)
            .map(PathBuf::from)
            .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;

        debug::db(format!(
            "settings get last_opened_folder -> {}",
            path.display()
        ));
        Ok(Some(path))
    }

    async fn set_last_opened_folder(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string(&path.to_string_lossy())
            .map_err(|err| sqlx::Error::Encode(err.into()))?;

        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(LAST_OPENED_FOLDER_KEY)
        .bind(json)
        .execute(&self.pool)
        .await?;

        debug::db(format!(
            "settings set last_opened_folder = {}",
            path.display()
        ));
        Ok(())
    }
}
