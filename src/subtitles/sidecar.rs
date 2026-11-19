use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 4] = ["srt", "ass", "ssa", "vtt"];

pub fn stem_of(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// Subtitle files next to `media`: `Movie.srt` first, then tagged ones like `Movie.fr.srt`.
pub fn sidecars(media: &Path) -> Vec<PathBuf> {
    let Some(stem) = stem_of(media) else {
        return Vec::new();
    };
    let directory = match media.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let tagged = format!("{stem}.");
    let mut found: Vec<(bool, PathBuf)> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension.to_lowercase().as_str()))
        })
        .filter_map(|path| {
            let name = stem_of(&path)?;
            let exact = name == stem;
            (exact || name.starts_with(&tagged)).then_some((!exact, path))
        })
        .filter(|(_, path)| path.is_file())
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}
