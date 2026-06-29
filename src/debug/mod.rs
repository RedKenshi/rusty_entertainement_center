/// Library refresh pipeline (debounce → rescan → UI).
pub fn refresh(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[refresh] {message}");
}

/// SQLite reads, writes, and reconciliation.
pub fn db(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[db] {message}");
}
