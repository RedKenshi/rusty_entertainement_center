//! Filesystem scanner: turns directories on disk into an in-memory `FolderNode` tree.
//!
//! It does not handle navigation or UI state — that lives in `browsing`. Rescanning is
//! triggered by `watch` when the filesystem changes.

/// Project root (compile-time path). Scans and watches stay correct regardless of process cwd.
pub const WORKSPACE: &str = env!("CARGO_MANIFEST_DIR");

use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

use super::probe::probe_video;
use crate::structs::{FileMetadata, FileNode, FolderNode};

/// File extensions treated as playable video files. Everything else is ignored.
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "webm", "m4v", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "vob",
    "ogv", "3gp",
];

fn file_size(file: &FileNode) -> u64 {
    file.metadata
        .as_ref()
        .and_then(|metadata| metadata.size)
        .unwrap_or(0)
}

fn file_duration_ms(file: &FileNode) -> u64 {
    file.metadata
        .as_ref()
        .and_then(|metadata| metadata.duration_ms)
        .unwrap_or(0)
}

/// Post-order pass: fills reduced file count, size, and duration on every node.
fn compute_reduced_stats(folder: &mut FolderNode) {
    let direct_count = folder.files.len() as u32;
    let direct_size: u64 = folder.files.iter().map(file_size).sum();
    let direct_duration: u64 = folder.files.iter().map(file_duration_ms).sum();

    let mut child_count = 0;
    let mut child_size = 0;
    let mut child_duration = 0;
    for subfolder in &mut folder.subfolders {
        compute_reduced_stats(subfolder);
        child_count += subfolder.reduced_number_of_file;
        child_size += subfolder.reduced_size_of_files;
        child_duration += subfolder.reduced_duration_of_files;
    }

    folder.reduced_number_of_file = direct_count + child_count;
    folder.reduced_size_of_files = direct_size + child_size;
    folder.reduced_duration_of_files = direct_duration + child_duration;
}

/// Returns true when `path` has a known video extension (case-insensitive).
fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|video_ext| ext.eq_ignore_ascii_case(video_ext))
        })
        .unwrap_or(false)
}

/// Recursively scans `root` and builds a nested `FolderNode` tree.
///
/// Only video files are included as `FileNode` entries. All directories under `root` become
/// folders, even when empty.
///
/// Implementation uses four passes over the filesystem:
/// 1. Collect every directory into a flat `HashMap` keyed by full path string.
/// 2. Walk files, filter to videos, attach each `FileNode` to its parent folder in the map.
/// 3. Link folders into a tree by moving each node from the map into its parent's `subfolders`.
/// 4. Aggregate subtree file counts, sizes, and durations into each folder's `reduced_*` fields.
pub fn build_tree(root: &str) -> FolderNode {
    // Flat index of every directory found under `root`, keyed by absolute path.
    // Built first so file pass can attach children without worrying about tree shape yet.
    let mut folders: HashMap<String, FolderNode> = HashMap::new();

    // Pass 1 — register all directories as empty `FolderNode`s.
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e: Result<walkdir::DirEntry, walkdir::Error>| e.ok())
    {
        let path: std::path::PathBuf = entry.path().to_path_buf();
        let path_str: String = path.to_string_lossy().to_string();
        let name: String = entry.file_name().to_string_lossy().to_string();

        if entry.file_type().is_dir() {
            folders.insert(
                path_str,
                FolderNode {
                    path,
                    name,
                    subfolders: vec![],
                    files: vec![],
                    reduced_number_of_file: 0,
                    reduced_size_of_files: 0,
                    reduced_duration_of_files: 0,
                },
            );
        }
    }

    // Pass 2 — collect video files, probe duration, attach to parent folder.
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path().to_path_buf();
        if !is_video_file(&path) {
            continue;
        }

        let fullname = entry.file_name().to_string_lossy().to_string();

        // Split "movie.mkv" → name "movie", format "MKV".
        let (name, format) = fullname
            .rsplit_once('.')
            .map(|(name, ext)| (name.to_string(), ext.to_string().to_uppercase()))
            .unwrap_or_else(|| (fullname.clone(), String::new()));

        let probe = probe_video(&path);
        let file = FileNode {
            path: path.clone(),
            name,
            format,
            metadata: Some(FileMetadata {
                size: entry.metadata().ok().map(|m| m.len()),
                duration_ms: probe.duration_ms,
                bitrate: probe.bitrate,
                codec: probe.codec,
                width: probe.width,
                height: probe.height,
            }),
        };

        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if let Some(parent_folder) = folders.get_mut(&parent_str) {
                parent_folder.files.push(file);
            }
        }
    }

    // Pass 3 — nest folders: deepest paths first so parents still exist in the map when linking.
    let mut keys: Vec<String> = folders.keys().cloned().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(Path::new(k).components().count()));

    for path_str in keys {
        if path_str == root {
            continue;
        }

        let path = Path::new(&path_str);
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if let Some(subfolder) = folders.remove(&path_str) {
                if let Some(parent_folder) = folders.get_mut(&parent_str) {
                    parent_folder.subfolders.push(subfolder);
                }
            }
        }
    }

    // The scan root is the top of this tree; all other nodes now live under it.
    let mut root = folders.remove(root).expect("root folder not found");
    compute_reduced_stats(&mut root);
    root
}

/// Builds the top-level library tree for the app.
///
/// Scans `workspace` for directories whose names start with `volume` (e.g. `volumeD`, `volumeE`,
/// `volumeF`), runs `build_tree` on each one, and returns a hidden root node whose `subfolders`
/// are those volumes. The root itself is never shown in the UI.
pub fn build_volume_library(workspace: &str) -> FolderNode {
    let workspace_path = Path::new(workspace);
    let mut volumes = Vec::new();

    let entries = std::fs::read_dir(workspace).unwrap_or_else(|_| {
        panic!("failed to read workspace directory: {workspace}")
    });

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("volume") {
            continue;
        }

        let volume_root = path.to_str().expect("volume path must be valid UTF-8");
        volumes.push((name, build_tree(volume_root)));
    }

    volumes.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut root = FolderNode {
        path: workspace_path.to_path_buf(),
        name: String::from("root"),
        subfolders: volumes.into_iter().map(|(_, node)| node).collect(),
        files: vec![],
        reduced_number_of_file: 0,
        reduced_size_of_files: 0,
        reduced_duration_of_files: 0,
    };
    compute_reduced_stats(&mut root);
    root
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::compute_reduced_stats;
    use crate::structs::{FileMetadata, FileNode, FolderNode};

    fn file_with_stats(path: &str, size: u64, duration_ms: u64) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: "video".to_string(),
            format: "MKV".to_string(),
            metadata: Some(FileMetadata {
                size: Some(size),
                duration_ms: Some(duration_ms),
                bitrate: None,
                codec: None,
                width: None,
                height: None,
            }),
        }
    }

    fn folder(
        path: &str,
        subfolders: Vec<FolderNode>,
        files: Vec<FileNode>,
    ) -> FolderNode {
        FolderNode {
            path: PathBuf::from(path),
            name: path.rsplit('/').next_back().unwrap_or(path).to_string(),
            subfolders,
            files,
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    #[test]
    fn reduced_stats_sums_files_and_subfolders() {
        let mut tree = folder(
            "/volumeD",
            vec![folder(
                "/volumeD/movies",
                vec![],
                vec![
                    file_with_stats("/volumeD/movies/a.mkv", 100, 60_000),
                    file_with_stats("/volumeD/movies/b.mkv", 250, 120_000),
                ],
            )],
            vec![file_with_stats("/volumeD/root.mkv", 50, 30_000)],
        );

        compute_reduced_stats(&mut tree);

        assert_eq!(tree.reduced_number_of_file, 3);
        assert_eq!(tree.reduced_size_of_files, 400);
        assert_eq!(tree.reduced_duration_of_files, 210_000);
        assert_eq!(tree.subfolders[0].reduced_number_of_file, 2);
        assert_eq!(tree.subfolders[0].reduced_size_of_files, 350);
        assert_eq!(tree.subfolders[0].reduced_duration_of_files, 180_000);
    }

    #[test]
    fn empty_folder_has_zero_reduced_stats() {
        let mut tree = folder("/empty", vec![], vec![]);
        compute_reduced_stats(&mut tree);
        assert_eq!(tree.reduced_number_of_file, 0);
        assert_eq!(tree.reduced_size_of_files, 0);
        assert_eq!(tree.reduced_duration_of_files, 0);
    }
}
