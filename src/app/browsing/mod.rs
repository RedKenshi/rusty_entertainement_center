use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::normalize_path;
use crate::debug;
use crate::structs::FolderNode;
use crate::ui::TreeItem;
use crate::utils::{flatten_along_path, format_playback_time, resume_position_to_store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivateResult {
    None,
    OpenedFolder,
    PlayFile { path: PathBuf, name: String },
}

pub struct BrowsingState {
    pub tree: FolderNode,
    pub stack: Vec<PathBuf>,
    pub selected: usize,
    visible: Vec<TreeItem>,
    resume_positions: HashMap<PathBuf, u64>,
}

impl BrowsingState {
    pub fn new(tree: FolderNode) -> Self {
        let mut state: BrowsingState = Self {
            tree,
            stack: Vec::new(),
            selected: 0,
            visible: Vec::new(),
            resume_positions: HashMap::new(),
        };
        state.rebuild_visible();
        state
    }

    pub fn visible_items(&self) -> &[TreeItem] {
        &self.visible
    }

    pub fn can_go_up(&self) -> bool {
        self.selected > 0
    }

    pub fn can_go_down(&self) -> bool {
        self.selected + 1 < self.visible.len()
    }

    pub fn go_up(&mut self) -> bool {
        if !self.can_go_up() {
            return false;
        }
        self.selected -= 1;
        self.apply_selection();
        true
    }

    pub fn go_down(&mut self) -> bool {
        if !self.can_go_down() {
            return false;
        }
        self.selected += 1;
        self.apply_selection();
        true
    }

    pub fn go_back(&mut self) -> bool {
        let Some(closed) = self.stack.pop() else {
            return false;
        };
        self.rebuild_visible();
        self.select_path(&closed);
        true
    }

    /// Replace the library tree after a rescan, keeping navigation when paths still exist.
    pub fn reload_tree(&mut self, new_tree: FolderNode) {
        let selected_path = self
            .visible
            .get(self.selected)
            .map(|item| item.path.clone());
        let stack_before = self.stack.len();
        self.tree = new_tree;
        prune_stack(&self.tree, &mut self.stack);
        self.rebuild_visible();
        if let Some(path) = selected_path {
            self.select_path(Path::new(path.as_str()));
        }
        debug::refresh(format!(
            "reload_tree: stack {stack_before} -> {}, selected={}, visible={}",
            self.stack.len(),
            self.selected,
            self.visible.len()
        ));
    }

    pub fn open_selected(&mut self) -> bool {
        let Some(target) = self.selected_folder_path() else {
            return false;
        };

        if self.stack.last().is_some_and(|path| *path == target) {
            return self.go_back();
        }

        let Some(stack) = stack_for_folder(&self.tree, &target) else {
            return false;
        };

        self.stack = stack;
        self.rebuild_visible();
        self.select_path(&target);
        true
    }

    /// Open a folder/volume, or return a file path when the selection is a video file.
    pub fn activate_selected(&mut self) -> ActivateResult {
        if let Some(item) = self.visible.get(self.selected) {
            if !item.is_folder && !item.is_volume {
                return ActivateResult::PlayFile {
                    path: PathBuf::from(item.path.as_str()),
                    name: item.name.to_string(),
                };
            }
        }

        if self.open_selected() {
            ActivateResult::OpenedFolder
        } else {
            ActivateResult::None
        }
    }

    pub fn file_duration_ms(&self, path: &Path) -> Option<u64> {
        find_file(&self.tree, path)
            .and_then(|file| file.metadata.as_ref())
            .and_then(|metadata| metadata.duration_ms)
    }

    pub fn set_resume_positions(&mut self, positions: HashMap<PathBuf, u64>) {
        self.resume_positions = positions;
        self.apply_resume_to_visible();
    }

    pub fn selected_file_path(&self) -> Option<PathBuf> {
        let item = self.visible.get(self.selected)?;
        if item.is_folder || item.is_volume {
            return None;
        }
        Some(PathBuf::from(item.path.as_str()))
    }

    pub fn selected_file_has_resume(&self) -> bool {
        self.selected_file_path()
            .is_some_and(|path| resume_position_for(&self.resume_positions, &path).is_some())
    }

    fn selected_folder_path(&self) -> Option<PathBuf> {
        let item = self.visible.get(self.selected)?;
        if !item.is_folder && !item.is_volume {
            return None;
        }
        Some(PathBuf::from(item.path.as_str()))
    }

    fn rebuild_visible(&mut self) {
        let mut items = Vec::new();
        flatten_along_path(&self.tree, &self.stack, &mut items);
        self.visible = items;
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        self.apply_selection();
        self.apply_resume_to_visible();
    }

    fn apply_resume_to_visible(&mut self) {
        let labels: Vec<(usize, slint::SharedString)> = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if item.is_folder || item.is_volume {
                    return Some((index, "".into()));
                }
                let path = PathBuf::from(item.path.as_str());
                let duration_ms = self.file_duration_ms(&path);
                let label = resume_position_for(&self.resume_positions, &path)
                    .and_then(|position_ms| resume_position_to_store(position_ms, duration_ms))
                    .map(format_playback_time)
                    .unwrap_or_default();
                Some((index, label.into()))
            })
            .collect();

        for (index, label) in labels {
            self.visible[index].resumePosition = label;
        }
    }

    fn apply_selection(&mut self) {
        for (index, item) in self.visible.iter_mut().enumerate() {
            item.selected = index == self.selected;
        }
    }

    fn select_path(&mut self, path: &Path) {
        let path: slint::SharedString = path.to_string_lossy().into_owned().into();
        self.selected = self
            .visible
            .iter()
            .position(|item| item.path == path)
            .unwrap_or(0);
        self.apply_selection();
    }
}

fn resume_position_for(positions: &HashMap<PathBuf, u64>, path: &Path) -> Option<u64> {
    let normalized = PathBuf::from(normalize_path(path));
    positions
        .get(&normalized)
        .or_else(|| positions.get(path))
        .copied()
}

fn find_file<'a>(tree: &'a FolderNode, path: &Path) -> Option<&'a crate::structs::FileNode> {
    for volume in &tree.subfolders {
        if let Some(file) = find_file_in_folder(volume, path) {
            return Some(file);
        }
    }
    None
}

fn find_file_in_folder<'a>(
    folder: &'a FolderNode,
    path: &Path,
) -> Option<&'a crate::structs::FileNode> {
    for file in &folder.files {
        if file.path == path {
            return Some(file);
        }
    }
    for subfolder in &folder.subfolders {
        if let Some(file) = find_file_in_folder(subfolder, path) {
            return Some(file);
        }
    }
    None
}

fn prune_stack(tree: &FolderNode, stack: &mut Vec<PathBuf>) {
    while !stack.is_empty() {
        let Some(last) = stack.last() else {
            return;
        };
        if let Some(valid_stack) = stack_for_folder(tree, last) {
            *stack = valid_stack;
            return;
        }
        stack.pop();
    }
}

fn stack_for_folder(library: &FolderNode, target: &Path) -> Option<Vec<PathBuf>> {
    for volume in &library.subfolders {
        let mut stack = Vec::new();
        if build_stack_recursive(volume, target, &mut stack) {
            return Some(stack);
        }
    }
    None
}

fn build_stack_recursive(node: &FolderNode, target: &Path, stack: &mut Vec<PathBuf>) -> bool {
    stack.push(node.path.clone());
    if node.path == target {
        return true;
    }
    for sub in &node.subfolders {
        if build_stack_recursive(sub, target, stack) {
            return true;
        }
    }
    stack.pop();
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::{FileNode, FolderNode};

    fn folder(path: &str, name: &str, subfolders: Vec<FolderNode>) -> FolderNode {
        FolderNode {
            path: PathBuf::from(path),
            name: name.to_string(),
            subfolders,
            files: vec![],
            reduced_number_of_file: 0,
            reduced_size_of_files: 0,
            reduced_duration_of_files: 0,
        }
    }

    #[test]
    fn volumes_start_collapsed_and_open() {
        let tree = library(vec![
            folder("/volumeD", "volumeD", vec![folder("/volumeD/mkv", "mkv", vec![])]),
            folder("/volumeE", "volumeE", vec![]),
        ]);
        let mut state = BrowsingState::new(tree);

        assert!(state.stack.is_empty());
        assert_eq!(state.visible_items().len(), 2);
        assert_eq!(state.visible_items()[0].name, "volumeD");

        assert!(state.open_selected());
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.visible_items().len(), 3);
    }

    #[test]
    fn go_back_selects_closed_volume() {
        let tree = library(vec![
            folder(
                "/volumeD",
                "volumeD",
                vec![
                    folder("/volumeD/mkv", "mkv", vec![]),
                    folder("/volumeD/mp4", "mp4", vec![]),
                ],
            ),
            folder("/volumeE", "volumeE", vec![]),
        ]);
        let mut state = BrowsingState::new(tree);

        state.selected = 0;
        state.open_selected();
        state.selected = 1;
        state.open_selected();
        assert_eq!(state.stack.len(), 2);

        assert!(state.go_back());
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.visible_items()[state.selected].name, "mkv");
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

    #[test]
    fn root_starts_collapsed_and_opens() {
        let tree = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![folder("/volumeD/mkv", "mkv", vec![])],
        )]);
        let mut state = BrowsingState::new(tree);

        assert!(state.stack.is_empty());
        assert_eq!(state.visible_items().len(), 1);

        assert!(state.open_selected());
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.visible_items().len(), 2);
    }

    #[test]
    fn reload_tree_preserves_open_path_and_selection() {
        let tree = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![
                folder("/volumeD/mkv", "mkv", vec![]),
                folder("/volumeD/mp4", "mp4", vec![]),
            ],
        )]);
        let mut state = BrowsingState::new(tree);

        state.selected = 0;
        state.open_selected();
        state.selected = 2;
        assert_eq!(state.visible_items()[state.selected].name, "mp4");

        let updated = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![
                folder("/volumeD/mkv", "mkv", vec![]),
                folder("/volumeD/mp4", "mp4", vec![]),
                folder("/volumeD/avi", "avi", vec![]),
            ],
        )]);
        state.reload_tree(updated);

        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.visible_items().len(), 4);
        assert_eq!(state.visible_items()[state.selected].name, "mp4");
    }

    #[test]
    fn reload_tree_prunes_stack_when_open_folder_removed() {
        let tree = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![folder("/volumeD/mkv", "mkv", vec![])],
        )]);
        let mut state = BrowsingState::new(tree);

        state.open_selected();
        state.selected = 1;
        state.open_selected();
        assert_eq!(state.stack.len(), 2);

        let updated = library(vec![folder("/volumeD", "volumeD", vec![])]);
        state.reload_tree(updated);

        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.stack[0], PathBuf::from("/volumeD"));
        assert_eq!(state.visible_items().len(), 1);
    }

    #[test]
    fn activate_selected_returns_file_without_changing_stack() {
        use crate::structs::FileMetadata;

        let mkv_folder = FolderNode {
            path: PathBuf::from("/volumeD/mkv"),
            name: "mkv".into(),
            subfolders: vec![],
            files: vec![FileNode {
                path: PathBuf::from("/volumeD/mkv/video.mkv"),
                name: "video".into(),
                format: "MKV".into(),
                metadata: Some(FileMetadata {
                    size: Some(100),
                    duration_ms: Some(120_000),
                    bitrate: None,
                    codec: None,
                    width: None,
                    height: None,
                    audio_track_count: None,
                    subtitle_track_count: None,
                }),
            }],
            reduced_number_of_file: 1,
            reduced_size_of_files: 100,
            reduced_duration_of_files: 120_000,
        };
        let tree = library(vec![folder("/volumeD", "volumeD", vec![mkv_folder])]);
        let mut state = BrowsingState::new(tree);

        state.selected = 0;
        state.open_selected();
        state.selected = 1;
        state.open_selected();
        state.selected = 2;

        match state.activate_selected() {
            ActivateResult::PlayFile { path, name } => {
                assert_eq!(path, PathBuf::from("/volumeD/mkv/video.mkv"));
                assert_eq!(name, "video");
            }
            other => panic!("expected PlayFile, got {other:?}"),
        }
        assert_eq!(state.stack.len(), 2);
        assert_eq!(
            state.file_duration_ms(Path::new("/volumeD/mkv/video.mkv")),
            Some(120_000)
        );
    }

    #[test]
    fn go_back_selects_closed_folder() {
        let tree = library(vec![folder(
            "/volumeD",
            "volumeD",
            vec![
                folder("/volumeD/mkv", "mkv", vec![]),
                folder("/volumeD/mp4", "mp4", vec![]),
            ],
        )]);
        let mut state = BrowsingState::new(tree);

        state.selected = 0;
        state.open_selected();

        state.selected = 1;
        state.open_selected();
        assert_eq!(state.stack.len(), 2);

        assert!(state.go_back());
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.visible_items()[state.selected].name, "mkv");
    }
}
