use std::fs;

use asm::shell::{HOOK_CONTENT, ShellKind, install_shell_at, uninstall_shell_at};

const MARKER: &str = "# asm shell hooks (installed by `asm install`)";
const ZSH_SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.zsh ]] && source ~/.asm/shell-hooks.zsh";
const BASH_SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.bash ]] && source ~/.asm/shell-hooks.bash";
const PATH_LINE: &str =
    r#"[[ ":$PATH:" != *":$HOME/.cargo/bin:"* ]] && export PATH="$HOME/.cargo/bin:$PATH""#;

#[test]
fn install_zsh_writes_hook_and_appends_source_line_in_temp_home() {
    let temp = tempfile::tempdir().unwrap();

    let report = install_shell_at(temp.path(), ShellKind::Zsh).unwrap();

    assert!(report.wrote_hook);
    assert!(report.appended_source_line);
    assert_eq!(
        fs::read_to_string(temp.path().join(".asm/shell-hooks.zsh")).unwrap(),
        HOOK_CONTENT
    );
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert!(zshrc.contains(MARKER));
    assert_eq!(line_count(&zshrc, PATH_LINE), 1);
    assert_eq!(line_count(&zshrc, ZSH_SOURCE_LINE), 1);
    assert!(!temp.path().join(".bashrc").exists());
}

#[test]
fn install_zsh_is_idempotent_for_existing_source_line() {
    let temp = tempfile::tempdir().unwrap();

    install_shell_at(temp.path(), ShellKind::Zsh).unwrap();
    let report = install_shell_at(temp.path(), ShellKind::Zsh).unwrap();

    assert!(report.wrote_hook);
    assert!(!report.appended_source_line);
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert_eq!(line_count(&zshrc, ZSH_SOURCE_LINE), 1);
}

#[test]
fn uninstall_zsh_removes_hook_and_source_line_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    install_shell_at(temp.path(), ShellKind::Zsh).unwrap();

    let first = uninstall_shell_at(temp.path(), ShellKind::Zsh).unwrap();
    let second = uninstall_shell_at(temp.path(), ShellKind::Zsh).unwrap();

    assert!(first.removed_source_line);
    assert!(first.deleted_hook);
    assert!(!second.removed_source_line);
    assert!(!second.deleted_hook);
    assert!(!temp.path().join(".asm/shell-hooks.zsh").exists());
    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    assert_eq!(line_count(&zshrc, ZSH_SOURCE_LINE), 0);
    assert_eq!(line_count(&zshrc, PATH_LINE), 0);
    assert!(!zshrc.contains(MARKER));
}

#[test]
fn install_bash_writes_hook_and_appends_source_line_in_temp_home() {
    let temp = tempfile::tempdir().unwrap();

    let report = install_shell_at(temp.path(), ShellKind::Bash).unwrap();

    assert!(report.wrote_hook);
    assert!(report.appended_source_line);
    assert_eq!(
        fs::read_to_string(temp.path().join(".asm/shell-hooks.bash")).unwrap(),
        HOOK_CONTENT
    );
    let bashrc = fs::read_to_string(temp.path().join(".bashrc")).unwrap();
    assert!(bashrc.contains(MARKER));
    assert_eq!(line_count(&bashrc, PATH_LINE), 1);
    assert_eq!(line_count(&bashrc, BASH_SOURCE_LINE), 1);
    assert!(!temp.path().join(".zshrc").exists());
}

#[test]
fn install_bash_is_idempotent_for_existing_source_line() {
    let temp = tempfile::tempdir().unwrap();

    install_shell_at(temp.path(), ShellKind::Bash).unwrap();
    let report = install_shell_at(temp.path(), ShellKind::Bash).unwrap();

    assert!(report.wrote_hook);
    assert!(!report.appended_source_line);
    let bashrc = fs::read_to_string(temp.path().join(".bashrc")).unwrap();
    assert_eq!(line_count(&bashrc, BASH_SOURCE_LINE), 1);
}

#[test]
fn uninstall_bash_removes_hook_and_source_line_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    install_shell_at(temp.path(), ShellKind::Bash).unwrap();

    let first = uninstall_shell_at(temp.path(), ShellKind::Bash).unwrap();
    let second = uninstall_shell_at(temp.path(), ShellKind::Bash).unwrap();

    assert!(first.removed_source_line);
    assert!(first.deleted_hook);
    assert!(!second.removed_source_line);
    assert!(!second.deleted_hook);
    assert!(!temp.path().join(".asm/shell-hooks.bash").exists());
    let bashrc = fs::read_to_string(temp.path().join(".bashrc")).unwrap();
    assert_eq!(line_count(&bashrc, BASH_SOURCE_LINE), 0);
    assert_eq!(line_count(&bashrc, PATH_LINE), 0);
    assert!(!bashrc.contains(MARKER));
}

#[test]
fn zsh_and_bash_installs_are_idempotent_per_shell() {
    let temp = tempfile::tempdir().unwrap();

    install_shell_at(temp.path(), ShellKind::Zsh).unwrap();
    install_shell_at(temp.path(), ShellKind::Bash).unwrap();
    install_shell_at(temp.path(), ShellKind::Zsh).unwrap();
    install_shell_at(temp.path(), ShellKind::Bash).unwrap();

    let zshrc = fs::read_to_string(temp.path().join(".zshrc")).unwrap();
    let bashrc = fs::read_to_string(temp.path().join(".bashrc")).unwrap();
    assert_eq!(line_count(&zshrc, ZSH_SOURCE_LINE), 1);
    assert_eq!(line_count(&bashrc, BASH_SOURCE_LINE), 1);
}

fn line_count(rcfile: &str, expected_line: &str) -> usize {
    rcfile
        .lines()
        .filter(|line| line.trim() == expected_line)
        .count()
}
