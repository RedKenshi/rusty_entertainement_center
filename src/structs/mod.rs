use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FolderNode {
    pub path: PathBuf,
    pub name: String,
    pub subfolders: Vec<FolderNode>,
    pub files: Vec<FileNode>,
    /// Total video file count in this folder and all descendants.
    pub reduced_number_of_file: u32,
    /// Total byte size of those files (from scan-time metadata).
    pub reduced_size_of_files: u64,
    /// Total playback duration in milliseconds (from ffprobe when available).
    pub reduced_duration_of_files: u64,
}

impl FolderNode {
    /// Sort subfolders and files alphabetically by display name (case-insensitive).
    pub fn sort_children(&mut self) {
        self.subfolders
            .sort_by(|left, right| cmp_name(&left.name, &right.name));
        self.files
            .sort_by(|left, right| cmp_name(&left.name, &right.name));
        for subfolder in &mut self.subfolders {
            subfolder.sort_children();
        }
    }
}

fn cmp_name(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_ascii_lowercase()
        .cmp(&right.to_ascii_lowercase())
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub format: String,
    pub metadata: Option<FileMetadata>,
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_track_count: Option<u32>,
    pub subtitle_track_count: Option<u32>,
}
