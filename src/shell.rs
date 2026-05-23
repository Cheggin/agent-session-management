use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub const HOOK_CONTENT: &str = r#"# Managed by  — do not edit by hand.
claude() {
  for arg in "$@"; do
    if [[ "$arg" == "--resume" ]]; then
      exec asm --filter claude
    fi
  done
  command claude "$@"
}
codex() {
  if [[ "$1" == "resume" ]]; then
    exec asm --filter codex
  fi
  command codex "$@"
}
"#;

const ZSHRC_MARKER: &str = "# asm shell hooks (installed by `asm install`)";
const ZSHRC_SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.zsh ]] && source ~/.asm/shell-hooks.zsh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub hook_path: PathBuf,
    pub zshrc_path: PathBuf,
    pub wrote_hook: bool,
    pub appended_source_line: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallReport {
    pub hook_path: PathBuf,
    pub zshrc_path: PathBuf,
    pub removed_source_line: bool,
    pub deleted_hook: bool,
}

pub fn install_zsh() -> Result<InstallReport> {
    ensure_zsh_shell_or_exit();
    let home = dirs::home_dir().context("could not determine home directory")?;
    install_zsh_at(&home)
}

pub fn uninstall_zsh() -> Result<UninstallReport> {
    ensure_zsh_shell_or_exit();
    let home = dirs::home_dir().context("could not determine home directory")?;
    uninstall_zsh_at(&home)
}

pub fn install_zsh_at(home_dir: &Path) -> Result<InstallReport> {
    let asm_dir = home_dir.join(".asm");
    fs::create_dir_all(&asm_dir)
        .with_context(|| format!("failed to create {}", asm_dir.display()))?;

    let hook_path = asm_dir.join("shell-hooks.zsh");
    fs::write(&hook_path, HOOK_CONTENT)
        .with_context(|| format!("failed to write {}", hook_path.display()))?;

    let zshrc_path = home_dir.join(".zshrc");
    let mut zshrc = read_optional_to_string(&zshrc_path)?;
    let appended_source_line = !has_source_line(&zshrc);
    if appended_source_line {
        if !zshrc.is_empty() && !zshrc.ends_with('\n') {
            zshrc.push('\n');
        }
        zshrc.push_str(ZSHRC_MARKER);
        zshrc.push('\n');
        zshrc.push_str(ZSHRC_SOURCE_LINE);
        zshrc.push('\n');
        fs::write(&zshrc_path, zshrc)
            .with_context(|| format!("failed to write {}", zshrc_path.display()))?;
    }

    Ok(InstallReport {
        hook_path,
        zshrc_path,
        wrote_hook: true,
        appended_source_line,
    })
}

pub fn uninstall_zsh_at(home_dir: &Path) -> Result<UninstallReport> {
    let hook_path = home_dir.join(".asm").join("shell-hooks.zsh");
    let zshrc_path = home_dir.join(".zshrc");

    let mut removed_source_line = false;
    if zshrc_path.exists() {
        let zshrc = fs::read_to_string(&zshrc_path)
            .with_context(|| format!("failed to read {}", zshrc_path.display()))?;
        let filtered = remove_managed_zshrc_lines(&zshrc, &mut removed_source_line);
        if filtered != zshrc {
            fs::write(&zshrc_path, filtered)
                .with_context(|| format!("failed to write {}", zshrc_path.display()))?;
        }
    }

    let deleted_hook = match fs::remove_file(&hook_path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to delete {}", hook_path.display()));
        }
    };

    Ok(UninstallReport {
        hook_path,
        zshrc_path,
        removed_source_line,
        deleted_hook,
    })
}

pub fn print_install_report(report: &InstallReport) {
    if report.wrote_hook {
        println!("wrote ~/.asm/shell-hooks.zsh");
    }
    if report.appended_source_line {
        println!("appended source line to ~/.zshrc");
    } else {
        println!("source line already present in ~/.zshrc");
    }
    println!("restart your shell or run: source ~/.zshrc");
}

pub fn print_uninstall_report(report: &UninstallReport) {
    if report.removed_source_line {
        println!("removed source line from ~/.zshrc");
    } else {
        println!("source line not present in ~/.zshrc");
    }
    if report.deleted_hook {
        println!("deleted ~/.asm/shell-hooks.zsh");
    } else {
        println!("~/.asm/shell-hooks.zsh already absent");
    }
}

fn ensure_zsh_shell_or_exit() {
    let shell = env::var("SHELL").unwrap_or_else(|_| "<unset>".to_owned());
    if Path::new(&shell).file_name().and_then(|name| name.to_str()) != Some("zsh") {
        eprintln!("asm install currently supports zsh only; your shell is {shell}");
        std::process::exit(1);
    }
}

fn read_optional_to_string(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn has_source_line(zshrc: &str) -> bool {
    zshrc.lines().any(|line| line.trim() == ZSHRC_SOURCE_LINE)
}

fn remove_managed_zshrc_lines(zshrc: &str, removed_source_line: &mut bool) -> String {
    let mut kept = Vec::new();
    for line in zshrc.lines() {
        match line.trim() {
            ZSHRC_MARKER => {}
            ZSHRC_SOURCE_LINE => {
                *removed_source_line = true;
            }
            _ => kept.push(line),
        }
    }

    let mut out = kept.join("\n");
    if zshrc.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}
