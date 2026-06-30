use std::path::PathBuf;

#[derive(Debug)]
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

#[derive(Debug)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub format: String,
    pub metadata: Option<FileMetadata>,
}

#[derive(Debug)]
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
