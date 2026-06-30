use super::VolumeRoot;

pub fn list(workspace: &str) -> Vec<VolumeRoot> {
    let entries = std::fs::read_dir(workspace).unwrap_or_else(|_| {
        panic!("failed to read workspace directory: {workspace}")
    });

    let mut volumes = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("volume") {
            continue;
        }

        volumes.push(VolumeRoot { name, path });
    }

    volumes
}
