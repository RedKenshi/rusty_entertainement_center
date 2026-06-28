use std::path::{Path, PathBuf};

use crate::debug;
use crate::structs::FolderNode;
use crate::ui::TreeItem;
use crate::utils::flatten_along_path;

pub struct BrowsingState {
    pub tree: FolderNode,
    pub stack: Vec<PathBuf>,
    pub selected: usize,
    visible: Vec<TreeItem>,
}

impl BrowsingState {
    pub fn new(tree: FolderNode) -> Self {
        let mut state: BrowsingState = Self {
            tree,
            stack: Vec::new(),
            selected: 0,
            visible: Vec::new(),
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

    fn rebuild_visible(&mut self) {
        let mut items = Vec::new();
        flatten_along_path(&self.tree, &self.stack, &mut items);
        self.visible = items;
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        self.apply_selection();
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

    fn selected_folder_path(&self) -> Option<PathBuf> {
        let item = self.visible.get(self.selected)?;
        if !item.is_folder && !item.is_volume {
            return None;
        }
        Some(PathBuf::from(item.path.as_str()))
    }
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
    use crate::structs::FolderNode;

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
