use std::collections::HashSet;

use sqlx::{Pool, Sqlite};

use crate::debug;
use crate::structs::FolderNode;

use super::paths::collect_file_paths;
use super::Result;

/// Align `media_state` with the current library tree: remove orphans, seed new files.
pub async fn sync_tree(pool: &Pool<Sqlite>, tree: &FolderNode) -> Result<()> {
    let paths = collect_file_paths(tree);
    let pruned = prune_stale(pool, &paths).await?;
    let seeded = seed_missing(pool, &paths).await?;
    debug::db(format!(
        "sync_tree: {} live paths, pruned {pruned}, seeded {seeded}",
        paths.len()
    ));
    Ok(())
}

async fn prune_stale(pool: &Pool<Sqlite>, live_paths: &[String]) -> Result<usize> {
    if live_paths.is_empty() {
        let result = sqlx::query("DELETE FROM media_state")
            .execute(pool)
            .await?;
        return Ok(result.rows_affected() as usize);
    }

    let live: HashSet<&str> = live_paths.iter().map(String::as_str).collect();
    let stored: Vec<(String,)> = sqlx::query_as("SELECT path FROM media_state")
        .fetch_all(pool)
        .await?;

    let mut pruned = 0usize;
    for (path,) in stored {
        if !live.contains(path.as_str()) {
            sqlx::query("DELETE FROM media_state WHERE path = ?")
                .bind(&path)
                .execute(pool)
                .await?;
            pruned += 1;
        }
    }

    Ok(pruned)
}

async fn seed_missing(pool: &Pool<Sqlite>, paths: &[String]) -> Result<usize> {
    let mut tx = pool.begin().await?;
    let mut seeded = 0usize;

    for path in paths {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO media_state (path, favorite, resume_position_ms, last_watched_at)
             VALUES (?, 0, NULL, NULL)",
        )
        .bind(path)
        .execute(&mut *tx)
        .await?;
        seeded += result.rows_affected() as usize;
    }

    tx.commit().await?;
    Ok(seeded)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::structs::{FileNode, FolderNode};

    async fn memory_pool() -> Pool<Sqlite> {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::migrations::run(&pool).await.unwrap();
        pool
    }

    fn file(path: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: "video".into(),
            format: "MKV".into(),
            metadata: None,
        }
    }

    fn folder(path: &str, files: Vec<FileNode>) -> FolderNode {
        FolderNode {
            path: PathBuf::from(path),
            name: path.into(),
            subfolders: vec![],
            files,
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    #[tokio::test]
    async fn sync_tree_seeds_new_paths_and_prunes_removed() {
        let pool = memory_pool().await;
        let tree = folder("/vol", vec![file("/vol/a.mkv"), file("/vol/b.mkv")]);
        sync_tree(&pool, &tree).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);

        let tree = folder("/vol", vec![file("/vol/b.mkv"), file("/vol/c.mkv")]);
        sync_tree(&pool, &tree).await.unwrap();

        let paths: Vec<String> = sqlx::query_scalar("SELECT path FROM media_state ORDER BY path")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.ends_with("b.mkv")));
        assert!(paths.iter().any(|p| p.ends_with("c.mkv")));
        assert!(!paths.iter().any(|p| p.ends_with("a.mkv")));
    }
}
