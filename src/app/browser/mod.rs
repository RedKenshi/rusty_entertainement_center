//! Filesystem scanner: turns directories on disk into an in-memory `FolderNode` tree.
//!
//! It does not handle navigation or UI state — that lives in `browsing`. Rescanning is
//! triggered by `watch` when the filesystem changes.

/// Project root (compile-time path). Scans and watches stay correct regardless of process cwd.
pub const WORKSPACE: &str = env!("CARGO_MANIFEST_DIR");

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::probe::{is_video_path, probe_videos_parallel};
use super::volumes;
use crate::structs::{FileMetadata, FileNode, FolderNode};

/// Videos shorter than this (when duration is known) are excluded from the library.
const MIN_DURATION_MS: u64 = 1000;

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

/// Removes subfolders whose subtree contains no valid video files.
fn prune_empty_subfolders(folder: &mut FolderNode) {
    for subfolder in &mut folder.subfolders {
        prune_empty_subfolders(subfolder);
    }
    folder
        .subfolders
        .retain(|subfolder| subfolder.reduced_number_of_file > 0);
}

fn is_valid_video(file: &FileNode) -> bool {
    match file
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.duration_ms)
    {
        Some(duration_ms) => duration_ms >= MIN_DURATION_MS,
        None => true,
    }
}

fn collect_video_paths(folder: &FolderNode, out: &mut Vec<PathBuf>) {
    for file in &folder.files {
        if is_video_path(&file.path) {
            out.push(file.path.clone());
        }
    }
    for subfolder in &folder.subfolders {
        collect_video_paths(subfolder, out);
    }
}

fn apply_probes_and_filter(folder: &mut FolderNode, probes: &HashMap<PathBuf, super::probe::VideoProbe>) {
    folder.files.retain_mut(|file| {
        if !is_video_path(&file.path) {
            return false;
        }
        let Some(probe) = probes.get(&file.path) else {
            return is_valid_video(file);
        };
        let metadata = file.metadata.get_or_insert_with(|| FileMetadata {
            size: None,
            duration_ms: None,
            bitrate: None,
            codec: None,
            width: None,
            height: None,
            audio_track_count: None,
            subtitle_track_count: None,
        });
        metadata.duration_ms = probe.duration_ms;
        metadata.bitrate = probe.bitrate;
        metadata.codec = probe.codec.clone();
        metadata.width = probe.width;
        metadata.height = probe.height;
        metadata.audio_track_count = probe.audio_track_count;
        metadata.subtitle_track_count = probe.subtitle_track_count;
        is_valid_video(file)
    });

    for subfolder in &mut folder.subfolders {
        apply_probes_and_filter(subfolder, probes);
    }
}

fn finalize_tree(folder: &mut FolderNode) {
    compute_reduced_stats(folder);
    prune_empty_subfolders(folder);
    folder.sort_children();
}

/// Empty library root shown before the first scan completes.
pub fn empty_library_root(workspace: &str) -> FolderNode {
    FolderNode {
        path: PathBuf::from(workspace),
        name: String::from("root"),
        subfolders: vec![],
        files: vec![],
        reduced_number_of_file: 0,
        reduced_size_of_files: 0,
        reduced_duration_of_files: 0,
    }
}

/// Recursively scans `root` and builds a nested `FolderNode` tree without ffprobe.
///
/// Only paths with a known video extension are included as files. ffprobe runs later in
/// [`probe_library`].
pub fn scan_tree(root: &str) -> FolderNode {
    let mut folders: HashMap<String, FolderNode> = HashMap::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e: Result<walkdir::DirEntry, walkdir::Error>| e.ok())
    {
        let path = entry.path().to_path_buf();
        let path_str = path.to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_string();

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
            continue;
        }

        if !is_video_path(&path) {
            continue;
        }

        let fullname = entry.file_name().to_string_lossy().to_string();
        let (name, format) = fullname
            .rsplit_once('.')
            .map(|(name, ext)| (name.to_string(), ext.to_string().to_uppercase()))
            .unwrap_or_else(|| (fullname.clone(), String::new()));

        let file = FileNode {
            path: path.clone(),
            name,
            format,
            metadata: Some(FileMetadata {
                size: entry.metadata().ok().map(|m| m.len()),
                duration_ms: None,
                bitrate: None,
                codec: None,
                width: None,
                height: None,
                audio_track_count: None,
                subtitle_track_count: None,
            }),
        };

        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if let Some(parent_folder) = folders.get_mut(&parent_str) {
                parent_folder.files.push(file);
            }
        }
    }

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

    let mut root = folders.remove(root).expect("root folder not found");
    finalize_tree(&mut root);
    root
}

/// Runs ffprobe (in parallel) on every video in `tree`, filters short clips, and refreshes stats.
pub fn probe_library(tree: &mut FolderNode) {
    let mut paths = Vec::new();
    collect_video_paths(tree, &mut paths);
    let probes = probe_videos_parallel(&paths);
    apply_probes_and_filter(tree, &probes);
    finalize_tree(tree);
}

/// Fast scan of all volumes — no ffprobe.
pub fn scan_volume_library(workspace: &str) -> FolderNode {
    let workspace_path = Path::new(workspace);
    let mut volume_nodes = Vec::new();

    for volume in volumes::list_volume_roots(workspace) {
        let volume_root = volume.path.to_str().expect("volume path must be valid UTF-8");
        let mut node = scan_tree(volume_root);
        node.name = volume.name;
        volume_nodes.push(node);
    }

    let mut root = FolderNode {
        path: workspace_path.to_path_buf(),
        name: String::from("root"),
        subfolders: volume_nodes,
        files: vec![],
        reduced_number_of_file: 0,
        reduced_size_of_files: 0,
        reduced_duration_of_files: 0,
    };
    finalize_tree(&mut root);
    root.subfolders
        .retain(|volume| volume.reduced_number_of_file > 0);
    root
}

/// Full scan: directory walk plus parallel ffprobe (used by tests and sync rebuilds).
pub fn build_volume_library(workspace: &str) -> FolderNode {
    let mut root = scan_volume_library(workspace);
    probe_library(&mut root);
    root
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{compute_reduced_stats, is_valid_video, prune_empty_subfolders, scan_tree};
    use crate::app::probe::is_video_path;
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
                audio_track_count: None,
                subtitle_track_count: None,
            }),
        }
    }

    fn folder(
        path: &str,
        subfolders: Vec<FolderNode>,
        files: Vec<FileNode>,
    ) -> FolderNode {
        let name = Path::new(path)
            .file_name()
            .and_then(|segment| segment.to_str())
            .unwrap_or(path)
            .to_string();
        FolderNode {
            path: PathBuf::from(path),
            name,
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

    #[test]
    fn sort_children_orders_folders_and_files_alphabetically() {
        let mut tree = FolderNode {
            path: PathBuf::from("/vol"),
            name: "vol".into(),
            subfolders: vec![
                folder_node("/vol/zebra", "zebra"),
                folder_node("/vol/alpha", "alpha"),
            ],
            files: vec![
                file_node("/vol/wed.mkv", "wed"),
                file_node("/vol/Mon.mkv", "Mon"),
                file_node("/vol/abc.mkv", "abc"),
            ],
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        };

        tree.sort_children();

        assert_eq!(
            tree.subfolders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "zebra"]
        );
        assert_eq!(
            tree.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["abc", "Mon", "wed"]
        );
    }

    fn folder_node(path: &str, name: &str) -> FolderNode {
        FolderNode {
            path: PathBuf::from(path),
            name: name.into(),
            subfolders: vec![],
            files: vec![],
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    fn file_node(path: &str, name: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: name.into(),
            format: "MKV".into(),
            metadata: None,
        }
    }

    #[test]
    fn is_valid_video_rejects_short_clips_and_keeps_unknown_duration() {
        assert!(!is_valid_video(&file_with_stats("/a.mkv", 1, 999)));
        assert!(is_valid_video(&file_with_stats("/b.mkv", 1, 1000)));
        assert!(is_valid_video(&file_node("/c.mkv", "c")));
    }

    #[test]
    fn prune_empty_subfolders_removes_folders_without_valid_videos() {
        let mut tree = folder(
            "/volumeD",
            vec![
                folder("/volumeD/empty", vec![], vec![]),
                folder(
                    "/volumeD/movies",
                    vec![],
                    vec![file_with_stats("/volumeD/movies/full.mkv", 100, 60_000)],
                ),
            ],
            vec![],
        );
        compute_reduced_stats(&mut tree);
        prune_empty_subfolders(&mut tree);

        assert_eq!(tree.subfolders.len(), 1);
        assert_eq!(tree.subfolders[0].name, "movies");
        assert_eq!(tree.reduced_number_of_file, 1);
    }

    #[test]
    fn scan_tree_does_not_include_non_video_files() {
        fn collect_files<'a>(folder: &'a FolderNode, out: &mut Vec<&'a FileNode>) {
            out.extend(folder.files.iter());
            for subfolder in &folder.subfolders {
                collect_files(subfolder, out);
            }
        }

        let workspace = env!("CARGO_MANIFEST_DIR");
        let root = format!("{workspace}/volumeD/mkv");
        if !Path::new(&root).is_dir() {
            return;
        }

        let tree = scan_tree(&root);
        let mut files = Vec::new();
        collect_files(&tree, &mut files);
        assert!(!files.is_empty());
        assert!(files.iter().all(|file| is_video_path(&file.path)));
    }
}
