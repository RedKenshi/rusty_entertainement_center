use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::path::Path;

use super::VolumeRoot;

#[cfg(target_os = "linux")]
pub fn list() -> Vec<VolumeRoot> {
    linux_mounts()
}

#[cfg(target_os = "macos")]
pub fn list() -> Vec<VolumeRoot> {
    macos_volumes()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn list() -> Vec<VolumeRoot> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn watch_roots() -> Vec<PathBuf> {
    ["/media", "/mnt", "/run/media"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .collect()
}

#[cfg(target_os = "macos")]
pub fn watch_roots() -> Vec<PathBuf> {
    let volumes = PathBuf::from("/Volumes");
    if volumes.is_dir() {
        vec![volumes]
    } else {
        Vec::new()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn watch_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn linux_mounts() -> Vec<VolumeRoot> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let root_disk_id = root_disk_id_from_mounts(&mounts);
    let mut volumes = Vec::new();

    for line in mounts.lines() {
        let Some((source, mount_point, fs_type)) = parse_mount_line(line) else {
            continue;
        };
        if !is_volume_mount(&source, &mount_point, &fs_type, root_disk_id.as_deref()) {
            continue;
        }
        if !Path::new(&mount_point).is_dir() {
            continue;
        }

        let name = mount_display_name(&mount_point);
        volumes.push(VolumeRoot {
            name,
            path: PathBuf::from(mount_point),
        });
    }

    dedupe_by_path(volumes)
}

#[cfg(target_os = "macos")]
fn macos_volumes() -> Vec<VolumeRoot> {
    let mut volumes = Vec::new();
    let entries = std::fs::read_dir("/Volumes").unwrap_or_else(|_| {
        panic!("failed to read /Volumes")
    });

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name == "Macintosh HD" {
            continue;
        }

        volumes.push(VolumeRoot { name, path });
    }

    volumes
}

#[cfg(target_os = "linux")]
fn mount_display_name(mount_point: &str) -> String {
    Path::new(mount_point)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(mount_point)
        .to_string()
}

#[cfg(target_os = "linux")]
fn dedupe_by_path(mut volumes: Vec<VolumeRoot>) -> Vec<VolumeRoot> {
    volumes.sort_by(|left, right| left.path.cmp(&right.path));
    volumes.dedup_by(|left, right| left.path == right.path);
    volumes
}

#[cfg(target_os = "linux")]
fn parse_mount_line(line: &str) -> Option<(String, String, String)> {
    let mut fields = line.split_whitespace();
    let source = fields.next()?.to_string();
    let mount_point = unescape_mount_path(fields.next()?);
    let fs_type = fields.next()?.to_string();
    Some((source, mount_point, fs_type))
}

#[cfg(target_os = "linux")]
fn unescape_mount_path(path: &str) -> String {
    path.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(target_os = "linux")]
fn root_disk_id_from_mounts(mounts: &str) -> Option<String> {
    for line in mounts.lines() {
        let Some((source, mount_point, _)) = parse_mount_line(line) else {
            continue;
        };
        if mount_point == "/" {
            return block_device_disk_id(&source);
        }
    }
    None
}

/// Maps `/dev/mmcblk0p2` → `mmcblk0`, `/dev/nvme0n1p2` → `nvme0n1`, `/dev/sda1` → `sda`.
#[cfg(target_os = "linux")]
fn block_device_disk_id(source: &str) -> Option<String> {
    let device = source.strip_prefix("/dev/")?;
    if device.is_empty() {
        return None;
    }

    if let Some(idx) = device.rfind('p') {
        let partition = &device[idx + 1..];
        let disk = &device[..idx];
        if !partition.is_empty()
            && partition.chars().all(|c| c.is_ascii_digit())
            && (disk.starts_with("mmcblk") || disk.contains("nvme"))
        {
            return Some(disk.to_string());
        }
    }

    let disk = device.trim_end_matches(|c: char| c.is_ascii_digit());
    if disk.is_empty() {
        None
    } else {
        Some(disk.to_string())
    }
}

#[cfg(target_os = "linux")]
fn is_on_root_disk(source: &str, root_disk_id: Option<&str>) -> bool {
    let Some(root_disk) = root_disk_id else {
        return false;
    };
    block_device_disk_id(source).as_deref() == Some(root_disk)
}

#[cfg(target_os = "linux")]
fn is_volume_mount(
    source: &str,
    mount_point: &str,
    fs_type: &str,
    root_disk_id: Option<&str>,
) -> bool {
    if SKIP_FS_TYPES.contains(&fs_type) {
        return false;
    }

    if is_on_root_disk(source, root_disk_id) {
        return false;
    }

    if SKIP_MOUNT_POINTS
        .iter()
        .any(|skip| mount_point == *skip)
    {
        return false;
    }

    let under_media = mount_point.starts_with("/media/")
        || mount_point.starts_with("/mnt/")
        || mount_point.starts_with("/run/media/");

    if under_media {
        return source.starts_with("/dev/") || STORAGE_FS_TYPES.contains(&fs_type);
    }

    source.starts_with("/dev/") && STORAGE_FS_TYPES.contains(&fs_type)
}

#[cfg(target_os = "linux")]
const SKIP_MOUNT_POINTS: &[&str] = &[
    "/",
    "/boot",
    "/boot/firmware",
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/tmp",
    "/var",
    "/snap",
];

#[cfg(target_os = "linux")]
const SKIP_FS_TYPES: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "tmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "pstore",
    "bpf",
    "tracefs",
    "debugfs",
    "securityfs",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "binfmt_misc",
    "autofs",
    "overlay",
    "squashfs",
    "rpc_pipefs",
];

#[cfg(target_os = "linux")]
const STORAGE_FS_TYPES: &[&str] = &[
    "ext4",
    "ext3",
    "ext2",
    "vfat",
    "exfat",
    "ntfs",
    "ntfs3",
    "btrfs",
    "xfs",
    "f2fs",
    "hfsplus",
    "ufs",
    "msdos",
    "fuseblk",
];

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn parse_mount_line_handles_spaces() {
        let line = "/dev/sda1 /media/pi/My\\040Disk ext4 rw,relatime 0 0";
        assert_eq!(
            parse_mount_line(line),
            Some((
                "/dev/sda1".to_string(),
                "/media/pi/My Disk".to_string(),
                "ext4".to_string()
            ))
        );
    }

    #[test]
    fn linux_usb_mount_is_volume() {
        assert!(is_volume_mount(
            "/dev/sdb1",
            "/media/pi/USB_DRIVE",
            "vfat",
            Some("mmcblk0"),
        ));
        assert!(!is_volume_mount(
            "/dev/mmcblk0p2",
            "/",
            "ext4",
            Some("mmcblk0"),
        ));
        assert!(!is_volume_mount("tmpfs", "/run", "tmpfs", Some("mmcblk0")));
    }

    #[test]
    fn root_disk_partitions_are_never_volumes() {
        assert!(!is_volume_mount(
            "/dev/mmcblk0p1",
            "/boot/firmware",
            "vfat",
            Some("mmcblk0"),
        ));
        assert!(!is_volume_mount(
            "/dev/mmcblk0p2",
            "/mnt/bind-root",
            "ext4",
            Some("mmcblk0"),
        ));
    }

    #[test]
    fn block_device_disk_id_parses_common_paths() {
        assert_eq!(
            block_device_disk_id("/dev/mmcblk0p2").as_deref(),
            Some("mmcblk0")
        );
        assert_eq!(
            block_device_disk_id("/dev/nvme0n1p2").as_deref(),
            Some("nvme0n1")
        );
        assert_eq!(block_device_disk_id("/dev/sda1").as_deref(), Some("sda"));
    }
}
