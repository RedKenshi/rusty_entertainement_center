use sqlx::{Pool, Sqlite};

use super::Result;

#[derive(sqlx::FromRow)]
struct MediaStateRow {
    path: String,
    favorite: i32,
    resume_position_ms: Option<i64>,
    last_watched_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct SettingRow {
    key: String,
    value: String,
}

/// Print every row in `media_state` and `settings` to stdout.
pub async fn dump(pool: &Pool<Sqlite>) -> Result<()> {
    let media = sqlx::query_as::<_, MediaStateRow>(
        "SELECT path, favorite, resume_position_ms, last_watched_at
         FROM media_state
         ORDER BY path",
    )
    .fetch_all(pool)
    .await?;

    let settings = sqlx::query_as::<_, SettingRow>(
        "SELECT key, value FROM settings ORDER BY key",
    )
    .fetch_all(pool)
    .await?;

    println!("=== media_state ({} rows) ===", media.len());
    if media.is_empty() {
        println!("  (empty)");
    } else {
        for row in &media {
            let favorite = if row.favorite != 0 { "yes" } else { "no" };
            let resume = row
                .resume_position_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "-".into());
            let last_watched = row
                .last_watched_at
                .map(|ts| ts.to_string())
                .unwrap_or_else(|| "-".into());
            println!(
                "  {}\n    favorite={favorite}  resume_ms={resume}  last_watched_at={last_watched}",
                row.path
            );
        }
    }

    println!("=== settings ({} rows) ===", settings.len());
    if settings.is_empty() {
        println!("  (empty)");
    } else {
        for row in &settings {
            println!("  {} = {}", row.key, row.value);
        }
    }

    Ok(())
}
