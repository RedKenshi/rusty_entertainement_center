use std::path::PathBuf;

#[derive(Debug)]
pub struct FolderNode {
    pub path: PathBuf,
    pub name: String,
    // Tree structure
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
    // Lazy metadata (optional)
    pub metadata: Option<FileMetadata>,
}

#[derive(Debug)]
pub struct FileMetadata {
    pub size: Option<u64>,

    // video/audio only (optional later)
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub codec: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StorageDevice {
    pub id: String,

    pub mount_point: PathBuf,

    pub label: Option<String>,

    pub fs_type: Option<String>, // ext4, ntfs, vfat...

    pub device_type: DeviceType,
}
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    HDD,
    SSD,
    USB,
    SDCard,
    Unknown,
}
