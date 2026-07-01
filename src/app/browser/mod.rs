//! Filesystem scanner: turns directories on disk into an in-memory `FolderNode` tree.
//!
//! It does not handle navigation or UI state — that lives in `browsing`. Rescanning is
//! triggered by `watch` when the filesystem changes.

/// Project root (compile-time path). Scans and watches stay correct regardless of process cwd.
pub const WORKSPACE: &str = env!("CARGO_MANIFEST_DIR");

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use walkdir::WalkDir;

use super::probe::{is_video_path, probe_videos_parallel, should_ignore_entry_name};
use super::volumes;
use crate::debug;
use crate::structs::{FileMetadata, FileNode, FolderNode};

/// Videos shorter than this (when duration is known) are excluded from the library.
const MIN_DURATION_MS: u64 = 1000;

/// Media lives in this folder at the root of each mounted volume (e.g. `/mnt/volumeI/volume/`).
const VOLUME_CONTENT_DIR: &str = "volume";

/// Scan root for a mount: `<mount>/volume/` when present; fake dev volumes fall back to the mount.
fn volume_scan_root(mount: &Path, allow_mount_fallback: bool) -> Option<PathBuf> {
    let content = mount.join(VOLUME_CONTENT_DIR);
    if content.is_dir() {
        debug::scan(format!(
            "  content root: {} (inside {VOLUME_CONTENT_DIR}/)",
            content.display()
        ));
        return Some(content);
    }
    if allow_mount_fallback {
        debug::scan(format!(
            "  no {VOLUME_CONTENT_DIR}/ at {} — scanning mount root (fake volumes)",
            mount.display()
        ));
        return Some(mount.to_path_buf());
    }
    debug::scan(format!(
        "  no {VOLUME_CONTENT_DIR}/ at {} — volume skipped",
        mount.display()
    ));
    None
}

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
    compute_reduced_stats_inner(folder, 0);
}

fn compute_reduced_stats_inner(folder: &mut FolderNode, depth: usize) {
    let indent = "  ".repeat(depth);
    for subfolder in &mut folder.subfolders {
        compute_reduced_stats_inner(subfolder, depth + 1);
    }

    let direct_count = folder.files.len() as u32;
    let direct_size: u64 = folder.files.iter().map(file_size).sum();
    let direct_duration: u64 = folder.files.iter().map(file_duration_ms).sum();

    let child_count: u32 = folder.subfolders.iter().map(|s| s.reduced_number_of_file).sum();
    let child_size: u64 = folder.subfolders.iter().map(|s| s.reduced_size_of_files).sum();
    let child_duration: u64 = folder
        .subfolders
        .iter()
        .map(|s| s.reduced_duration_of_files)
        .sum();

    folder.reduced_number_of_file = direct_count + child_count;
    folder.reduced_size_of_files = direct_size + child_size;
    folder.reduced_duration_of_files = direct_duration + child_duration;

    debug::scan(format!(
        "{indent}stats: {} — {} direct + {} nested = {} video(s)",
        folder.path.display(),
        direct_count,
        child_count,
        folder.reduced_number_of_file
    ));
}

/// Removes subfolders whose subtree contains no valid video files.
fn prune_empty_subfolders(folder: &mut FolderNode) {
    for subfolder in &mut folder.subfolders {
        prune_empty_subfolders(subfolder);
    }
    for subfolder in &folder.subfolders {
        if subfolder.reduced_number_of_file == 0 {
            debug::scan(format!(
                "  prune: dropping empty folder {}",
                subfolder.path.display()
            ));
        }
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

fn apply_probes_and_filter(
    folder: &mut FolderNode,
    batch: &super::probe::ProbeBatchResult,
) {
    folder.files.retain_mut(|file| {
        if !is_video_path(&file.path) {
            debug::scan(format!(
                "  probe-filter: drop {} (not a video extension)",
                file.path.display()
            ));
            return false;
        }

        let Some(outcome) = batch.results.get(&file.path) else {
            if batch.ffprobe_available {
                debug::scan(format!(
                    "  probe-filter: drop {} (no ffprobe result)",
                    file.path.display()
                ));
                return false;
            }
            let keep = is_valid_video(file);
            debug::scan(format!(
                "  probe-filter: {} ffprobe unavailable — {}",
                file.path.display(),
                if keep { "keep" } else { "drop" }
            ));
            return keep;
        };

        let probe = match outcome {
            super::probe::VideoProbeOutcome::Failed => {
                debug::scan(format!(
                    "  probe-filter: drop {} (unreadable)",
                    file.path.display()
                ));
                return false;
            }
            super::probe::VideoProbeOutcome::Ok(probe) => probe,
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
        let duration_ms = metadata.duration_ms;
        let codec = metadata.codec.clone();
        let keep = is_valid_video(file);
        debug::scan(format!(
            "  probe-filter: {} duration={duration_ms:?} codec={codec:?} — {}",
            file.path.display(),
            if keep {
                "keep"
            } else {
                "drop (<1s)"
            }
        ));
        keep
    });

    for subfolder in &mut folder.subfolders {
        apply_probes_and_filter(subfolder, batch);
    }
}

fn finalize_tree(folder: &mut FolderNode) {
    debug::scan(format!("finalize: {}", folder.path.display()));
    debug::scan("  step: compute reduced stats");
    compute_reduced_stats(folder);
    debug::scan("  step: prune empty subfolders");
    prune_empty_subfolders(folder);
    debug::scan("  step: sort children");
    folder.sort_children();
    debug::scan(format!(
        "  finalize done: {} — {} video(s) in subtree",
        folder.path.display(),
        folder.reduced_number_of_file
    ));
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
    debug::scan(format!("walk: begin {root}"));
    let mut folders: HashMap<String, FolderNode> = HashMap::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !should_ignore_entry_name(&name)
        })
        .filter_map(|e: Result<walkdir::DirEntry, walkdir::Error>| e.ok())
    {
        let path = entry.path().to_path_buf();
        let path_str = path.to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_string();

        if entry.file_type().is_dir() {
            debug::scan(format!("  walk folder: {}", path.display()));
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

        let size = entry.metadata().ok().map(|m| m.len());
        debug::scan(format!(
            "  walk video: {} ({format}, {} bytes)",
            path.display(),
            size.map(|s| s.to_string()).unwrap_or_else(|| "?".into())
        ));

        let file = FileNode {
            path: path.clone(),
            name,
            format,
            metadata: Some(FileMetadata {
                size,
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
            } else {
                debug::scan(format!(
                    "  walk warn: parent folder missing for {}",
                    path.display()
                ));
            }
        }
    }

    debug::scan(format!("walk: nesting folders under {root}"));
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
                    debug::scan(format!(
                        "  nest: {} -> {}",
                        subfolder.path.display(),
                        parent_folder.path.display()
                    ));
                    parent_folder.subfolders.push(subfolder);
                } else {
                    debug::scan(format!(
                        "  nest warn: parent missing for {}",
                        path.display()
                    ));
                }
            }
        }
    }

    let mut root_node = folders.remove(root).expect("root folder not found");
    finalize_tree(&mut root_node);
    debug::scan(format!(
        "walk: done {root} — {} video(s)",
        root_node.reduced_number_of_file
    ));
    root_node
}

/// Runs ffprobe (in parallel) on every video in `tree`, filters short clips, and refreshes stats.
pub fn probe_library(tree: &mut FolderNode) {
    let mut paths = Vec::new();
    collect_video_paths(tree, &mut paths);
    if paths.is_empty() {
        debug::scan("probing skipped: no video files");
        return;
    }

    let workers = super::probe::probe_worker_count();
    debug::scan(format!(
        "probing {} file(s) with {workers} worker(s)...",
        paths.len()
    ));
    let started = Instant::now();
    let batch = probe_videos_parallel(&paths);
    debug::scan(format!(
        "probe: applying metadata to tree ({} result(s))...",
        batch.results.len()
    ));
    let before = tree.reduced_number_of_file;
    apply_probes_and_filter(tree, &batch);
    debug::scan("probe: re-finalizing tree after filter");
    finalize_tree(tree);
    debug::scan(format!(
        "probing done in {:?}: {} file(s) kept ({} short/invalid removed)",
        started.elapsed(),
        tree.reduced_number_of_file,
        before.saturating_sub(tree.reduced_number_of_file)
    ));
}

/// Fast scan of all volumes — no ffprobe.
pub fn scan_volume_library(workspace: &str) -> FolderNode {
    let workspace_path = Path::new(workspace);
    let volume_roots = volumes::list_volume_roots(workspace);
    let fake = volumes::use_fake_volumes();

    debug::scan(format!(
        "discovering volumes (fake_volumes={fake}): {} mount(s) found",
        volume_roots.len()
    ));
    for volume in &volume_roots {
        debug::scan(format!(
            "  mount candidate: [{}] {}",
            volume.name,
            volume.path.display()
        ));
    }

    if volume_roots.is_empty() {
        debug::scan("no volumes to scan — check mounts under /mnt, /media, or /run/media");
    }

    let mut volume_nodes = Vec::new();
    let scan_started = Instant::now();

    for volume in volume_roots {
        let Some(scan_root) = volume_scan_root(&volume.path, fake) else {
            continue;
        };
        let scan_root_str = scan_root
            .to_str()
            .expect("volume scan path must be valid UTF-8");
        debug::scan(format!(
            "scanning volume [{}] at {}...",
            volume.name,
            scan_root.display()
        ));
        let started = Instant::now();
        let mut node = scan_tree(scan_root_str);
        node.name = volume.name.clone();
        node.path = volume.path.clone();
        debug::scan(format!(
            "  volume [{}]: {} video file(s) in {:?}",
            volume.name,
            node.reduced_number_of_file,
            started.elapsed()
        ));
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
    let before_retain = root.subfolders.len();
    for volume in &root.subfolders {
        if volume.reduced_number_of_file == 0 {
            debug::scan(format!(
                "  hide empty volume: [{}] {}",
                volume.name,
                volume.path.display()
            ));
        }
    }
    root.subfolders
        .retain(|volume| volume.reduced_number_of_file > 0);
    let skipped = before_retain.saturating_sub(root.subfolders.len());
    debug::scan(format!(
        "fast scan finished in {:?}: {} volume(s) with videos, {skipped} empty volume(s) hidden, {} file(s) total",
        scan_started.elapsed(),
        root.subfolders.len(),
        root.reduced_number_of_file
    ));
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
