use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use sqlx::{Pool, Sqlite};
use tokio::runtime::Runtime;

use super::connection;
use super::media_repository::SqliteMediaRepository;
use super::migrations;
use super::settings_repository::SqliteSettingsRepository;
use super::Result;
use crate::debug;

fn database_url(path: impl AsRef<Path>) -> String {
    format!(
        "sqlite:{}?mode=rwc",
        path.as_ref().to_string_lossy()
    )
}

/// Shared database handle: connection pool + Tokio runtime for async sqlx work.
pub struct Database {
    runtime: Arc<Runtime>,
    pool: Pool<Sqlite>,
    media: SqliteMediaRepository,
    settings: SqliteSettingsRepository,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        debug::db(format!("opening database at {}", path.display()));

        let runtime = Arc::new(
            Runtime::new().map_err(|err| sqlx::Error::Configuration(err.to_string().into()))?,
        );
        let url = database_url(&path);

        let pool = runtime.block_on(async {
            let pool = connection::connect(&url).await?;
            migrations::run(&pool).await?;
            Ok::<_, sqlx::Error>(pool)
        })?;

        let media = SqliteMediaRepository::new(pool.clone());
        let settings = SqliteSettingsRepository::new(pool.clone());

        debug::db("database ready");

        Ok(Self {
            runtime,
            pool,
            media,
            settings,
        })
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub fn media(&self) -> &SqliteMediaRepository {
        &self.media
    }

    pub fn settings(&self) -> &SqliteSettingsRepository {
        &self.settings
    }

    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        self.runtime.block_on(future)
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send,
    {
        self.runtime.spawn(future);
    }
}
