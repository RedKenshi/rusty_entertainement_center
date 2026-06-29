use std::path::Path;

use crate::structs::FolderNode;

/// Canonical absolute path string used as the `media_state.path` key.
pub fn normalize_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Walk the library tree and collect every video file path (normalized).
pub fn collect_file_paths(tree: &FolderNode) -> Vec<String> {
    let mut paths = Vec::new();
    collect_from_folder(tree, &mut paths);
    paths
}

fn collect_from_folder(folder: &FolderNode, paths: &mut Vec<String>) {
    for file in &folder.files {
        paths.push(normalize_path(&file.path));
    }
    for subfolder in &folder.subfolders {
        collect_from_folder(subfolder, paths);
    }
}
