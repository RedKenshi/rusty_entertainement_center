#![allow(dead_code)] // favorite toggle lands in a later UI pass

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sqlx::{Pool, Sqlite};

use crate::debug;

use super::Result;

#[derive(Debug, Clone)]
pub struct MediaState {
    pub path: PathBuf,
    pub favorite: bool,
    pub resume_position_ms: Option<u64>,
    pub last_watched_at: Option<SystemTime>,
}

#[async_trait]
pub trait MediaStateRepository {
    async fn get(&self, path: &Path) -> Result<Option<MediaState>>;
    async fn save(&self, state: &MediaState) -> Result<()>;
}

pub struct SqliteMediaRepository {
    pool: Pool<Sqlite>,
}

impl SqliteMediaRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }
}

fn system_time_to_i64(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

fn i64_to_system_time(secs: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)
}

#[async_trait]
impl MediaStateRepository for SqliteMediaRepository {
    async fn get(&self, path: &Path) -> Result<Option<MediaState>> {
        let path = super::paths::normalize_path(path);
        let row = sqlx::query_as::<_, MediaStateRow>(
            "SELECT path, favorite, resume_position_ms, last_watched_at FROM media_state WHERE path = ?",
        )
        .bind(&path)
        .fetch_optional(&self.pool)
        .await?;

        let found = row.is_some();
        debug::db(format!("media_state get {path} -> {}", if found { "found" } else { "none" }));
        Ok(row.map(Into::into))
    }

    async fn save(&self, state: &MediaState) -> Result<()> {
        let path = super::paths::normalize_path(&state.path);
        let favorite = i32::from(state.favorite);
        let resume_position_ms = state.resume_position_ms.map(|ms| ms as i64);
        let last_watched_at = state.last_watched_at.map(system_time_to_i64);

        debug::db(format!(
            "media_state save {path} favorite={favorite} resume={resume_position_ms:?}"
        ));

        sqlx::query(
            "INSERT INTO media_state (path, favorite, resume_position_ms, last_watched_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET
                favorite = excluded.favorite,
                resume_position_ms = excluded.resume_position_ms,
                last_watched_at = excluded.last_watched_at",
        )
        .bind(path)
        .bind(favorite)
        .bind(resume_position_ms)
        .bind(last_watched_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct MediaStateRow {
    path: String,
    favorite: i32,
    resume_position_ms: Option<i64>,
    last_watched_at: Option<i64>,
}

impl From<MediaStateRow> for MediaState {
    fn from(row: MediaStateRow) -> Self {
        Self {
            path: PathBuf::from(row.path),
            favorite: row.favorite != 0,
            resume_position_ms: row.resume_position_ms.map(|ms| ms.max(0) as u64),
            last_watched_at: row.last_watched_at.map(i64_to_system_time),
        }
    }
}
