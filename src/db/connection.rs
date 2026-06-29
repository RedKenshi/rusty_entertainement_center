use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};

use super::Result;
use crate::debug;

pub async fn connect(url: &str) -> Result<Pool<Sqlite>> {
    debug::db(format!("connecting to {url}"));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await?;
    debug::db("connected");
    Ok(pool)
}
