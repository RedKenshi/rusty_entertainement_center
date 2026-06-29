use sqlx::{Pool, Sqlite};

use super::Result;
use crate::debug;

pub async fn run(pool: &Pool<Sqlite>) -> Result<()> {
    debug::db("running migrations");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|err| sqlx::Error::Migrate(Box::new(err)))?;
    debug::db("migrations complete");
    Ok(())
}
