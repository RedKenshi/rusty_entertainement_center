use std::path::{Path, PathBuf};

use async_trait::async_trait;

mod connection;
mod database;
pub mod inspect;
mod media_repository;
mod migrations;
mod paths;
pub mod reconcile;
mod settings_repository;

pub use database::Database;
pub use media_repository::{MediaState, MediaStateRepository, SqliteMediaRepository};
pub use paths::normalize_path;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

#[async_trait]
pub trait SettingsRepository {
    async fn get_last_opened_folder(&self) -> Result<Option<PathBuf>>;
    #[allow(dead_code)] // used when browsing persists the open folder
    async fn set_last_opened_folder(&self, path: &Path) -> Result<()>;
}
