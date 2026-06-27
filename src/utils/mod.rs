use std::path::PathBuf;

use crate::structs::{FileNode, FolderNode};
use crate::ui::TreeItem;

pub fn volume_to_tree_item(volume: &FolderNode, expanded: bool, selected: bool) -> TreeItem {
    TreeItem {
        name: volume.name.clone().into(),
        format: "volume".into(),
        codec: String::new().into(),
        bitrate: String::new().into(),
        size: String::new().into(),
        indent: 0,
        is_folder: false,
        is_volume: true,
        expanded,
        selected,
        path: volume.path.to_string_lossy().into_owned().into(),
        filesSize: format_file_size(Some(volume.reduced_size_of_files)).into(),
        filesDuration: format_duration(Some(volume.reduced_duration_of_files)).into(),
        nbFiles: volume.reduced_number_of_file as i32,
    }
}

pub fn folder_to_tree_item(
    folder: &FolderNode,
    indent: i32,
    expanded: bool,
    selected: bool,
) -> TreeItem {
    TreeItem {
        name: folder.name.clone().into(),
        format: "folder".into(),
        codec: String::new().into(),
        bitrate: String::new().into(),
        size: String::new().into(),
        indent,
        is_folder: true,
        is_volume: false,
        expanded,
        selected,
        path: folder.path.to_string_lossy().into_owned().into(),
        filesSize: format_file_size(Some(folder.reduced_size_of_files)).into(),
        filesDuration: format_duration(Some(folder.reduced_duration_of_files)).into(),
        nbFiles: folder.reduced_number_of_file as i32,
    }
}

pub fn file_to_tree_item(file: &FileNode, indent: i32, selected: bool) -> TreeItem {
    let metadata = file.metadata.as_ref();
    let codec = metadata
        .and_then(|metadata| metadata.codec.as_deref())
        .unwrap_or(file.format.as_str());

    TreeItem {
        name: file.name.clone().into(),
        format: file.format.clone().into(),
        codec: codec.into(),
        bitrate: format_bitrate(metadata.and_then(|metadata| metadata.bitrate)).into(),
        size: format_file_size(metadata.and_then(|m| m.size)).into(),
        indent,
        is_folder: false,
        is_volume: false,
        expanded: false,
        selected,
        path: file.path.to_string_lossy().into_owned().into(),
        filesSize: String::new().into(),
        filesDuration: format_duration(metadata.and_then(|metadata| metadata.duration_ms))
        .into(),
        nbFiles: 0,
    }
}

/// Flatten the volume library for display, expanding only along `stack`.
///
/// An empty stack lists detected volumes at indent 0. The hidden library root
/// is never shown. The deepest folder on the stack shows its contents inline.
pub fn flatten_along_path(library: &FolderNode, stack: &[PathBuf], out: &mut Vec<TreeItem>) {
    if stack.is_empty() {
        for volume in &library.subfolders {
            out.push(volume_to_tree_item(volume, false, false));
        }
        return;
    }

    for volume in &library.subfolders {
        let on_path = stack.first().is_some_and(|path| *path == volume.path);
        out.push(volume_to_tree_item(volume, on_path, false));

        if on_path {
            flatten_open_path(volume, &stack[1..], 0, out);
        }
    }
}

fn flatten_open_path(
    folder: &FolderNode,
    stack_tail: &[PathBuf],
    indent: i32,
    out: &mut Vec<TreeItem>,
) {
    if stack_tail.is_empty() {
        emit_folder_contents(folder, indent + 1, out);
        return;
    }

    let child_indent = indent + 1;

    for sub in &folder.subfolders {
        let on_path = stack_tail.first().is_some_and(|path| *path == sub.path);
        out.push(folder_to_tree_item(sub, child_indent, on_path, false));

        if on_path {
            flatten_open_path(sub, &stack_tail[1..], child_indent, out);
        }
    }
}

fn emit_folder_contents(folder: &FolderNode, indent: i32, out: &mut Vec<TreeItem>) {
    for sub in &folder.subfolders {
        out.push(folder_to_tree_item(sub, indent, false, false));
    }
    for file in &folder.files {
        out.push(file_to_tree_item(file, indent, false));
    }
}

pub fn format_file_size(octets: Option<u64>) -> String {
    let octets = octets.unwrap_or(0);

    let (value, unit) = if octets < 1024 {
        (octets.to_string(), "o")
    } else if octets < 1024 * 1024 {
        (format!("{:.2}", octets as f64 / 1024.0), "Ko")
    } else if octets < 1024 * 1024 * 1024 {
        (
            format!("{:.2}", octets as f64 / (1024.0 * 1024.0)),
            "Mo",
        )
    } else {
        (
            format!("{:.2}", octets as f64 / (1024.0 * 1024.0 * 1024.0)),
            "Go",
        )
    };

    format!("{:>8} {:<4}", value, unit)
}

pub fn format_duration(ms: Option<u64>) -> String {
    let ms = ms.unwrap_or(0);
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours >= 100 {
        format!("{:>4}h", hours)
    } else if hours > 0 {
        format!("{:>3}h {:>2}m", hours, minutes)
    } else if minutes > 0 {
        format!("{:>3}m {:>2}s", minutes, seconds)
    } else {
        format!("{:>5}s", seconds)
    }
}

pub fn format_bitrate(bps: Option<u32>) -> String {
    let bps = bps.unwrap_or(0);
    if bps == 0 {
        return String::new();
    }

    if bps >= 1_000_000 {
        format!("{:>5.1} Mb/s", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:>5.0} kb/s", bps as f64 / 1_000.0)
    } else {
        format!("{:>5} b/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{flatten_along_path, format_bitrate, format_duration, format_file_size};
    use crate::structs::{FileNode, FileMetadata, FolderNode};

    fn folder(path: &str, name: &str, subfolders: Vec<FolderNode>, files: Vec<FileNode>) -> FolderNode {
        FolderNode {
            path: PathBuf::from(path),
            name: name.to_string(),
            subfolders,
            files,
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    fn file(path: &str, name: &str) -> FileNode {
        FileNode {
            path: PathBuf::from(path),
            name: name.to_string(),
            format: "MKV".to_string(),
            metadata: Some(FileMetadata {
                size: Some(1024),
                duration_ms: None,
                bitrate: Some(8_500_000),
                codec: Some("HEVC".to_string()),
            }),
        }
    }

    fn library(volumes: Vec<FolderNode>) -> FolderNode {
        FolderNode {
            path: PathBuf::from("/library"),
            name: String::new(),
            subfolders: volumes,
            files: vec![],
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    fn names(items: &[crate::ui::TreeItem]) -> Vec<String> {
        items.iter().map(|item| item.name.to_string()).collect()
    }

    #[test]
    fn format_duration_renders_hours_minutes_and_seconds() {
        assert_eq!(format_duration(Some(0)), "    0s");
        assert_eq!(format_duration(Some(90_000)), "  1m 30s");
        assert_eq!(format_duration(Some(3_600_000)), "  1h  0m");
        assert_eq!(format_duration(Some(5_400_000)), "  1h 30m");
    }

    #[test]
    fn format_bitrate_renders_megabits_and_kilobits() {
        assert_eq!(format_bitrate(None), "");
        assert_eq!(format_bitrate(Some(850_000)), "  850 kb/s");
        assert_eq!(format_bitrate(Some(8_500_000)), "  8.5 Mb/s");
    }

    #[test]
    fn format_file_size_renders_units() {
        assert_eq!(format_file_size(Some(512)), "     512 o   ");
    }

    #[test]
    fn collapsed_library_shows_volumes() {
        let tree = library(vec![
            folder("/volumeD", "volumeD", vec![], vec![]),
            folder("/volumeE", "volumeE", vec![], vec![]),
        ]);
        let mut items = Vec::new();
        flatten_along_path(&tree, &[], &mut items);

        assert_eq!(names(&items), vec!["volumeD", "volumeE"]);
        assert!(items.iter().all(|item| item.is_volume));
    }

    #[test]
    fn opened_volume_shows_its_contents() {
        let tree = library(vec![
            folder(
                "/volumeD",
                "volumeD",
                vec![
                    folder("/volumeD/mkv", "mkv", vec![], vec![]),
                    folder("/volumeD/mp4", "mp4", vec![], vec![]),
                ],
                vec![],
            ),
            folder("/volumeE", "volumeE", vec![], vec![]),
        ]);
        let stack = vec![PathBuf::from("/volumeD")];
        let mut items = Vec::new();
        flatten_along_path(&tree, &stack, &mut items);

        assert_eq!(names(&items), vec!["volumeD", "mkv", "mp4", "volumeE"]);
        assert!(items[0].expanded);
    }

    #[test]
    fn expands_along_stack_and_shows_deepest_contents() {
        let tree = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![folder(
                "/volumeD/a2",
                "a2",
                vec![
                    folder("/volumeD/a2/b1", "b1", vec![], vec![]),
                    folder(
                        "/volumeD/a2/b5",
                        "b5",
                        vec![
                            folder("/volumeD/a2/b5/c1", "c1", vec![], vec![]),
                            folder(
                                "/volumeD/a2/b5/c3",
                                "c3",
                                vec![],
                                vec![file("/volumeD/a2/b5/c3/video.mkv", "video")],
                            ),
                            folder("/volumeD/a2/b5/c5", "c5", vec![], vec![]),
                        ],
                        vec![],
                    ),
                    folder("/volumeD/a2/b7", "b7", vec![], vec![]),
                ],
                vec![],
            )],
            vec![],
        )]);

        let stack = vec![
            PathBuf::from("/volumeD"),
            PathBuf::from("/volumeD/a2"),
            PathBuf::from("/volumeD/a2/b5"),
            PathBuf::from("/volumeD/a2/b5/c3"),
        ];
        let mut items = Vec::new();
        flatten_along_path(&tree, &stack, &mut items);

        assert_eq!(
            names(&items),
            vec!["volumeD", "a2", "b1", "b5", "c1", "c3", "video", "c5", "b7"]
        );
    }
}
