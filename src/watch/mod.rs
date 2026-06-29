//! Filesystem watcher: signals when the library workspace changes on disk.

use std::path::Path;
use std::sync::mpsc;
use std::thread;

use notify::{Event, RecommendedWatcher, RecursiveMode, Result, Watcher};

/// Watch `root` recursively. Returns a channel that receives a unit value on each change batch.
pub fn watch_workspace(root: &str) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let root = root.to_string();

    thread::spawn(move || {
        let mut watcher: RecommendedWatcher =
            notify::recommended_watcher(move |result: Result<Event>| {
                match result {
                    Ok(_event) => {
                        let _ = tx.send(());
                    }
                    Err(_error) => {}
                }
            })
            .expect("failed to create filesystem watcher");

        watcher
            .watch(Path::new(&root), RecursiveMode::Recursive)
            .expect("failed to watch workspace");

        loop {
            thread::park();
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn copy_folder_emits_change_events() {
        let workspace = env!("CARGO_MANIFEST_DIR");
        let events = watch_workspace(workspace);
        let src = format!("{workspace}/volumeE/mp4");
        let dst = format!("{workspace}/volumeD/mp4-watch-test");

        let _ = fs::remove_dir_all(&dst);
        std::thread::sleep(Duration::from_millis(200));
        while events.try_recv().is_ok() {}

        let dst_for_copy = dst.clone();
        let copy = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            std::process::Command::new("cp")
                .args(["-R", &src, &dst_for_copy])
                .status()
                .expect("cp failed");
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut event_count = 0;
        while Instant::now() < deadline {
            while events.try_recv().is_ok() {
                event_count += 1;
            }
            if copy.is_finished() && event_count > 0 {
                std::thread::sleep(Duration::from_millis(500));
                while events.try_recv().is_ok() {
                    event_count += 1;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        copy.join().expect("copy thread panicked");
        while events.try_recv().is_ok() {
            event_count += 1;
        }
        let _ = fs::remove_dir_all(&dst);
        assert!(event_count > 0, "expected filesystem events when copying a folder");
    }
}
