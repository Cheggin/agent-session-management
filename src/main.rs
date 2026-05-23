use std::path::PathBuf;

use anyhow::{Context, Result};
use asm::{
    cli::parse_filter_arg,
    db::Index,
    export::{export_session, export_session_to_path},
    fork::{end_cut_index, fork_session},
    reindex::reindex_all,
    resume::dispatch_resume,
    shell::{install_zsh, print_install_report, print_uninstall_report, uninstall_zsh},
    ui::{self, filter::Chip},
};
use clap::{Parser as ClapParser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Debug, ClapParser)]
#[command(name = "asm", version, about = "Agent Session Manager")]
struct Cli {
    /// Seed the list with filter chips, e.g. --filter "claude branch:main".
    #[arg(long, value_name = "expr")]
    filter: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build or refresh the SQLite session index.
    Reindex,
    /// Install zsh shell hooks for claude --resume and codex resume.
    Install,
    /// Uninstall zsh shell hooks.
    Uninstall,
    /// Interactive list view.
    Ls {
        /// Seed the list with filter chips, e.g. --filter "claude branch:main".
        #[arg(long, value_name = "expr")]
        filter: Option<String>,
    },
    /// Resume a session by id (ships in Step 6).
    Resume { id: String },
    /// Fork a session by id.
    Fork {
        id: String,
        /// Zero-based source JSONL line index to cut at. Defaults to the end.
        #[arg(long, value_name = "n")]
        at: Option<usize>,
        /// Write the fork but do not exec into the resume command.
        #[arg(long)]
        no_resume: bool,
    },

    /// Export a session to markdown.
    Export {
        id: String,
        /// Output markdown file path. Defaults to ./asm-export-<agent>-<id>.md.
        #[arg(long, value_name = "path")]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        None => ui::run_with_filter(parse_filter_or_exit(cli.filter.as_deref())?)?,
        Some(Command::Reindex) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let (claude_root, codex_root) = default_agent_roots()?;
            let mut index = Index::open()?;
            let stats = reindex_all(&mut index, &claude_root, &codex_root)?;
            println!(
                "ReindexStats: discovered={} parsed={} skipped_unchanged={} failed={}",
                stats.discovered, stats.parsed, stats.skipped_unchanged, stats.failed
            );
        }
        Some(Command::Install) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let report = install_zsh()?;
            print_install_report(&report);
        }
        Some(Command::Uninstall) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let report = uninstall_zsh()?;
            print_uninstall_report(&report);
        }
        Some(Command::Ls { filter }) => {
            let filter = filter.as_deref().or(cli.filter.as_deref());
            ui::run_with_filter(parse_filter_or_exit(filter)?)?
        }
        Some(Command::Resume { id }) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let index = Index::open()?;
            let sessions = index.list_all()?;
            let session = sessions
                .into_iter()
                .find(|session| session.id == id)
                .with_context(|| format!("no indexed session found with id {id}"))?;
            dispatch_resume(&session)?;
        }
        Some(Command::Fork { id, at, no_resume }) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let index = Index::open()?;
            let sessions = index.list_all()?;
            let session = sessions
                .into_iter()
                .find(|session| session.id == id)
                .with_context(|| format!("no indexed session found with id {id}"))?;
            let cut_index = match at {
                Some(cut_index) => cut_index,
                None => end_cut_index(&session.path)?,
            };
            let current_cwd =
                std::env::current_dir().context("failed to read current directory")?;
            let fork_path =
                fork_session(&session, &session.path, cut_index, &current_cwd, no_resume)?;
            if no_resume {
                println!("{}", fork_path.display());
            }
        }
        Some(Command::Export { id, out }) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            let index = Index::open()?;
            let sessions = index.list_all()?;
            let session = sessions
                .into_iter()
                .find(|session| session.id == id)
                .with_context(|| format!("no indexed session found with id {id}"))?;
            let export_path = match out {
                Some(path) if path.is_dir() => export_session(&session, &path)?,
                Some(path) => export_session_to_path(&session, &path)?,
                None => {
                    let current_cwd =
                        std::env::current_dir().context("failed to read current directory")?;
                    export_session(&session, &current_cwd)?
                }
            };
            eprintln!("exported to {}", export_path.display());
        }
    }

    Ok(())
}

fn reject_filter_for_non_list(filter: Option<&str>) {
    if filter.is_some() {
        eprintln!("--filter is only supported by `asm` and `asm ls`");
        std::process::exit(2);
    }
}

fn parse_filter_or_exit(filter: Option<&str>) -> Result<Vec<Chip>> {
    let Some(filter) = filter else {
        return Ok(Vec::new());
    };
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    match parse_filter_arg(filter, &current_cwd) {
        Ok(chips) => Ok(chips),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}

fn default_agent_roots() -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok((home.join(".claude"), home.join(".codex")))
}
