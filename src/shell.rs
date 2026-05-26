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

const SH_HOOK_CONTENT: &str = r#"# Managed by  — do not edit by hand.
claude() {
  for arg in "$@"; do
    if [ "$arg" = "--resume" ]; then
      exec asm --filter claude
    fi
  done
  command claude "$@"
}
codex() {
  if [ "${1:-}" = "resume" ]; then
    exec asm --filter codex
  fi
  command codex "$@"
}
"#;

const RCFILE_MARKER: &str = "# asm shell hooks (installed by `asm install`)";
const ZSH_BASH_PATH_LINE: &str =
    "[[ \":$PATH:\" != *\":$HOME/.cargo/bin:\"* ]] && export PATH=\"$HOME/.cargo/bin:$PATH\"";
const SH_PATH_LINE: &str = "case \":$PATH:\" in *\":$HOME/.cargo/bin:\"*) ;; *) export PATH=\"$HOME/.cargo/bin:$PATH\";; esac";
const ZSH_SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.zsh ]] && source ~/.asm/shell-hooks.zsh";
const BASH_SOURCE_LINE: &str = "[[ -f ~/.asm/shell-hooks.bash ]] && source ~/.asm/shell-hooks.bash";
const SH_SOURCE_LINE: &str = "[ -f ~/.asm/shell-hooks.sh ] && . ~/.asm/shell-hooks.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
    Sh,
}

impl ShellKind {
    fn from_shell_path(shell: &str) -> Option<Self> {
        match Path::new(shell).file_name().and_then(|name| name.to_str()) {
            Some("zsh") => Some(Self::Zsh),
            Some("bash") => Some(Self::Bash),
            Some("sh") => Some(Self::Sh),
            _ => None,
        }
    }

    fn hook_file(self) -> &'static str {
        match self {
            Self::Zsh => "shell-hooks.zsh",
            Self::Bash => "shell-hooks.bash",
            Self::Sh => "shell-hooks.sh",
        }
    }

    fn hook_content(self) -> &'static str {
        match self {
            Self::Zsh | Self::Bash => HOOK_CONTENT,
            Self::Sh => SH_HOOK_CONTENT,
        }
    }

    fn rcfile(self) -> &'static str {
        match self {
            Self::Zsh => ".zshrc",
            Self::Bash => ".bashrc",
            Self::Sh => ".profile",
        }
    }

    fn path_line(self) -> &'static str {
        match self {
            Self::Zsh | Self::Bash => ZSH_BASH_PATH_LINE,
            Self::Sh => SH_PATH_LINE,
        }
    }

    fn source_line(self) -> &'static str {
        match self {
            Self::Zsh => ZSH_SOURCE_LINE,
            Self::Bash => BASH_SOURCE_LINE,
            Self::Sh => SH_SOURCE_LINE,
        }
    }

    fn reload_command(self) -> &'static str {
        match self {
            Self::Zsh => "source ~/.zshrc",
            Self::Bash => "source ~/.bashrc",
            Self::Sh => ". ~/.profile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub shell: ShellKind,
    pub hook_path: PathBuf,
    pub rcfile_path: PathBuf,
    pub wrote_hook: bool,
    pub appended_source_line: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallReport {
    pub shell: ShellKind,
    pub hook_path: PathBuf,
    pub rcfile_path: PathBuf,
    pub removed_source_line: bool,
    pub deleted_hook: bool,
}

pub fn install_shell() -> Result<InstallReport> {
    let shell = detect_shell_or_exit();
    let home = dirs::home_dir().context("could not determine home directory")?;
    install_shell_at(&home, shell)
}

pub fn uninstall_shell() -> Result<UninstallReport> {
    let shell = detect_shell_or_exit();
    let home = dirs::home_dir().context("could not determine home directory")?;
    uninstall_shell_at(&home, shell)
}

pub fn install_shell_at(home_dir: &Path, shell: ShellKind) -> Result<InstallReport> {
    let asm_dir = home_dir.join(".asm");
    fs::create_dir_all(&asm_dir)
        .with_context(|| format!("failed to create {}", asm_dir.display()))?;

    let hook_path = asm_dir.join(shell.hook_file());
    fs::write(&hook_path, shell.hook_content())
        .with_context(|| format!("failed to write {}", hook_path.display()))?;

    let rcfile_path = home_dir.join(shell.rcfile());
    let mut rcfile = read_optional_to_string(&rcfile_path)?;
    let appended_block = !has_marker(&rcfile);
    if appended_block {
        if !rcfile.is_empty() && !rcfile.ends_with('\n') {
            rcfile.push('\n');
        }
        rcfile.push_str(RCFILE_MARKER);
        rcfile.push('\n');
        rcfile.push_str(shell.path_line());
        rcfile.push('\n');
        rcfile.push_str(shell.source_line());
        rcfile.push('\n');
        fs::write(&rcfile_path, rcfile)
            .with_context(|| format!("failed to write {}", rcfile_path.display()))?;
    }

    Ok(InstallReport {
        shell,
        hook_path,
        rcfile_path,
        wrote_hook: true,
        appended_source_line: appended_block,
    })
}

pub fn uninstall_shell_at(home_dir: &Path, shell: ShellKind) -> Result<UninstallReport> {
    let hook_path = home_dir.join(".asm").join(shell.hook_file());
    let rcfile_path = home_dir.join(shell.rcfile());

    let mut removed_source_line = false;
    if rcfile_path.exists() {
        let rcfile = fs::read_to_string(&rcfile_path)
            .with_context(|| format!("failed to read {}", rcfile_path.display()))?;
        let filtered = remove_managed_rcfile_lines(&rcfile, shell, &mut removed_source_line);
        if filtered != rcfile {
            fs::write(&rcfile_path, filtered)
                .with_context(|| format!("failed to write {}", rcfile_path.display()))?;
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
        shell,
        hook_path,
        rcfile_path,
        removed_source_line,
        deleted_hook,
    })
}

pub fn print_install_report(report: &InstallReport) {
    if report.wrote_hook {
        println!("wrote ~/.asm/{}", report.shell.hook_file());
    }
    if report.appended_source_line {
        println!(
            "appended managed block to ~/{} (PATH + source line)",
            report.shell.rcfile()
        );
    } else {
        println!(
            "managed block already present in ~/{}",
            report.shell.rcfile()
        );
    }
    println!(
        "restart your shell or run: {}",
        report.shell.reload_command()
    );
}

pub fn print_uninstall_report(report: &UninstallReport) {
    if report.removed_source_line {
        println!("removed source line from ~/{}", report.shell.rcfile());
    } else {
        println!("source line not present in ~/{}", report.shell.rcfile());
    }
    if report.deleted_hook {
        println!("deleted ~/.asm/{}", report.shell.hook_file());
    } else {
        println!("~/.asm/{} already absent", report.shell.hook_file());
    }
}

fn detect_shell_or_exit() -> ShellKind {
    let shell = env::var("SHELL").unwrap_or_else(|_| "<unset>".to_owned());
    match ShellKind::from_shell_path(&shell) {
        Some(kind) => kind,
        None => {
            eprintln!("asm install currently supports zsh, bash, and sh; your shell is {shell}");
            std::process::exit(1);
        }
    }
}

fn read_optional_to_string(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn has_marker(rcfile: &str) -> bool {
    rcfile.lines().any(|line| line.trim() == RCFILE_MARKER)
}

fn remove_managed_rcfile_lines(
    rcfile: &str,
    shell: ShellKind,
    removed_source_line: &mut bool,
) -> String {
    let mut kept = Vec::new();
    for line in rcfile.lines() {
        let trimmed = line.trim();
        if trimmed == RCFILE_MARKER || trimmed == shell.path_line() {
            continue;
        }
        if trimmed == shell.source_line() {
            *removed_source_line = true;
            continue;
        }
        kept.push(line);
    }

    let mut out = kept.join("\n");
    if rcfile.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}
