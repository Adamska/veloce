use std::{sync::mpsc, time::Duration};

use veloce::player::RecentFiles;

fn reload(store: &std::path::Path) -> RecentFiles {
    let (checked, done) = mpsc::channel();
    let recent = RecentFiles::persistent(store.to_path_buf(), move || {
        let _ = checked.send(());
    });
    done.recv_timeout(Duration::from_secs(5))
        .expect("availability check finishes");
    recent
}

#[test]
fn recent_files_survive_a_restart_and_hide_deleted_media() {
    let directory = std::env::temp_dir().join(format!("veloce-recent-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temporary directory");
    let store = directory.join("state/recent");
    let (kept, deleted, again) = (
        directory.join("kept.mkv"),
        directory.join("deleted.mp4"),
        directory.join("again.webm"),
    );
    for path in [&kept, &deleted, &again] {
        std::fs::write(path, b"").expect("media placeholder");
    }

    let mut recent = reload(&store);
    assert!(recent.available().is_empty());
    for path in [&again, &kept, &deleted, &again] {
        recent.record(path);
    }
    let names = |recent: &RecentFiles| -> Vec<String> {
        recent
            .available()
            .iter()
            .filter_map(|entry| Some(entry.path.file_name()?.to_string_lossy().into_owned()))
            .collect()
    };
    assert_eq!(names(&recent), ["again.webm", "deleted.mp4", "kept.mkv"]);

    std::fs::remove_file(&deleted).expect("delete media");
    let mut recent = reload(&store);
    assert_eq!(names(&recent), ["again.webm", "kept.mkv"]);

    recent.forget(&std::fs::canonicalize(&again).expect("canonical path"));
    assert_eq!(names(&reload(&store)), ["kept.mkv"]);
    recent.clear();
    assert!(reload(&store).available().is_empty());
    let _ = std::fs::remove_dir_all(directory);
}
