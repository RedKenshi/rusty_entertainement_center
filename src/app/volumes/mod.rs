//! Volume discovery: fake `volume*` folders for dev, or real mounted storage on the host.

mod fake;
mod real;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeRoot {
    pub name: String,
    pub path: PathBuf,
}

/// `FAKE_VOLUMES=1` (or `true`/`yes`) uses `volume*` directories under the workspace.
pub fn use_fake_volumes() -> bool {
    match std::env::var("FAKE_VOLUMES") {
        Ok(value) => matches!(
            value.trim(),
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
        ),
        Err(_) => false,
    }
}

pub fn list_volume_roots(workspace: &str) -> Vec<VolumeRoot> {
    let mut roots = if use_fake_volumes() {
        fake::list(workspace)
    } else {
        real::list()
    };
    roots.sort_by(|left, right| left.name.cmp(&right.name));
    roots
}

/// Directories to watch for library changes (workspace in fake mode, mount parents in real mode).
pub fn watch_roots(workspace: &str) -> Vec<PathBuf> {
    if use_fake_volumes() {
        vec![PathBuf::from(workspace)]
    } else {
        real::watch_roots()
    }
}
