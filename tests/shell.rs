use std::fs;

use asm::shell::{HOOK_CONTENT, install_zsh_at, uninstall_zsh_at};

const SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.zsh ]] && source ~/.asm/shell-hooks.zsh";

#[test]
fn install_zsh_writes_hook_and_appends_source_line_in_temp_home() {
    let temp = tempfile::tempdir().unwrap();

    let report = install_zsh_at(temp.path()).unwrap();

    assert!(report.wrote_hook);
    assert!(report.appended_source_line);
    assert_eq!(
        fs::read_to_string(temp.path().join(".asm/shell-hooks.zsh")).unwrap(),
        HOOK_CONTENT
    );
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert!(zshrc.contains("# asm shell hooks (installed by `asm install`)"));
    assert_eq!(source_line_count(&zshrc), 1);
}

#[test]
fn install_zsh_is_idempotent_for_existing_source_line() {
    let temp = tempfile::tempdir().unwrap();

    install_zsh_at(temp.path()).unwrap();
    let report = install_zsh_at(temp.path()).unwrap();

    assert!(report.wrote_hook);
    assert!(!report.appended_source_line);
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert_eq!(source_line_count(&zshrc), 1);
}

#[test]
fn uninstall_zsh_removes_hook_and_source_line_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    install_zsh_at(temp.path()).unwrap();

    let first = uninstall_zsh_at(temp.path()).unwrap();
    let second = uninstall_zsh_at(temp.path()).unwrap();

    assert!(first.removed_source_line);
    assert!(first.deleted_hook);
    assert!(!second.removed_source_line);
    assert!(!second.deleted_hook);
    assert!(!temp.path().join(".asm/shell-hooks.zsh").exists());
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert_eq!(source_line_count(&zshrc), 0);
    assert!(!zshrc.contains("# asm shell hooks (installed by `asm install`)"));
}

fn source_line_count(zshrc: &str) -> usize {
    zshrc
        .lines()
        .filter(|line| line.trim() == SOURCE_LINE)
        .count()
}
