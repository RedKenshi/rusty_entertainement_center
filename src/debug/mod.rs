/// Library refresh pipeline (debounce → rescan → UI).
pub fn refresh(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[refresh] {message}");
}

/// Volume scan / ffprobe progress — plain stdout, same as Slint renderer lines.
pub fn scan(message: impl std::fmt::Display) {
    use std::io::Write;
    println!("{message}");
    let _ = std::io::stdout().flush();
}

/// SQLite reads, writes, and reconciliation.
pub fn db(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[db] {message}");
}

/// Playback state and transport.
pub fn player(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[player] {message}");
}

/// Browsing UI interactions (selection, resume clear hold, …).
pub fn browse(message: impl std::fmt::Display) {
    #[cfg(debug_assertions)]
    eprintln!("[browse] {message}");
}
