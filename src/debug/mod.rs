use crate::structs::FolderNode;

pub fn print_folder(folder: &FolderNode) {
    let mut tree_text = String::new();
    tree_to_string(folder, 0, &mut tree_text);
    println!("{tree_text}");
}

fn tree_to_string(folder: &FolderNode, indent: usize, out: &mut String) {
    out.push_str(&format!("{}📁 {}\n", " ".repeat(indent), folder.name));

    for subfolder in &folder.subfolders {
        tree_to_string(subfolder, indent + 2, out);
    }

    for file in &folder.files {
        out.push_str(&format!(
            "{}📄 {}\n",
            " ".repeat(indent + 2),
            file.name
        ));
    }
}

/// Library refresh pipeline (`watch` → debounce → rescan → UI).
pub fn refresh(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[refresh] {message}");
}

/// Filesystem watcher callbacks.
pub fn watch(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[watch] {message}");
}
