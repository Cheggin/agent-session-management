use std::{
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

pub fn scan_claude(root: &Path) -> Vec<PathBuf> {
    let projects = root.join("projects");
    let Ok(project_dirs) = fs::read_dir(projects) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for project_dir in project_dirs.flatten() {
        let Ok(file_type) = project_dir.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let Ok(files) = fs::read_dir(project_dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                paths.push(path);
            }
        }
    }

    paths.sort();
    paths
}

pub fn scan_codex(root: &Path) -> Vec<PathBuf> {
    let sessions = root.join("sessions");
    if !sessions.exists() {
        return Vec::new();
    }

    let mut paths: Vec<PathBuf> = WalkDir::new(sessions)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    paths.sort();
    paths
}
