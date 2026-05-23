use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use asm::{db::Index, reindex::reindex_all};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn incremental_reindex_skips_unchanged_files_and_reparses_touched_file() {
    let temp = tempfile::tempdir().unwrap();
    let index_dir = tempfile::tempdir().unwrap();
    let claude_path = copy_fixture(
        &fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl"),
        &temp
            .path()
            .join("claude/projects/-stub/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl"),
    );
    copy_fixture(
        &fixture(
            "tests/fixtures/codex/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl",
        ),
        &temp.path().join(
            "codex/sessions/2026/05/22/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl",
        ),
    );

    let mut index = Index::open_in_dir(index_dir.path()).unwrap();
    let first = reindex_all(
        &mut index,
        &temp.path().join("claude"),
        &temp.path().join("codex"),
    )
    .unwrap();
    assert!(first.discovered >= 2);
    assert!(first.parsed >= 2);
    assert_eq!(first.skipped_unchanged, 0);
    assert_eq!(first.failed, 0);

    let second = reindex_all(
        &mut index,
        &temp.path().join("claude"),
        &temp.path().join("codex"),
    )
    .unwrap();
    assert_eq!(second.skipped_unchanged, second.discovered);

    thread::sleep(Duration::from_millis(5));
    OpenOptions::new()
        .append(true)
        .open(&claude_path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let third = reindex_all(
        &mut index,
        &temp.path().join("claude"),
        &temp.path().join("codex"),
    )
    .unwrap();
    assert_eq!(third.parsed, 1);
}

fn copy_fixture(source: &Path, destination: &Path) -> PathBuf {
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(source, destination).unwrap();
    destination.to_path_buf()
}
