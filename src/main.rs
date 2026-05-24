#[cfg(not(test))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use asm::{
    cli::parse_list_filter_chips,
    db::Index,
    export::{export_session, export_session_to_path},
    fork::{ForkRoots, end_cut_index, fork_session, fork_session_timed},
    reindex::reindex_all,
    resume::{dispatch_resume, resume_command},
    shell::{install_zsh, print_install_report, print_uninstall_report, uninstall_zsh},
    ui::{self, filter::Chip},
};
use clap::{Parser as ClapParser, Subcommand};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, ClapParser)]
#[command(name = "asm", version, about = "Agent Session Manager")]
struct Cli {
    /// Seed the list with filter chips, e.g. --filter "claude branch:main".
    #[arg(long, value_name = "expr")]
    filter: Option<String>,
    /// Show sessions across all directories instead of starting with @here.
    #[arg(long)]
    global: bool,
    /// Draw the first TUI frame, print startup wall-clock timing, then exit.
    #[arg(long)]
    benchmark: bool,
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
        /// Show sessions across all directories instead of starting with @here.
        #[arg(long)]
        global: bool,
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

    /// Benchmark fork phases (read/validate/write) without resuming. Created
    /// fork files are deleted after each iteration.
    #[command(hide = true)]
    BenchFork {
        id: String,
        /// Zero-based source JSONL line index to cut at. Defaults to the end.
        #[arg(long, value_name = "n")]
        at: Option<usize>,
        /// Number of iterations.
        #[arg(long, default_value_t = 5)]
        repeat: usize,
    },

    /// Benchmark the resume code path up to (but not including) exec.
    #[command(hide = true)]
    BenchResume {
        id: String,
        /// Number of iterations.
        #[arg(long, default_value_t = 5)]
        repeat: usize,
    },
}

fn main() -> Result<()> {
    let startup_started = Instant::now();
    let cli = Cli::parse();
    let tracing_init_started = Instant::now();
    let _file_tracing_guard = if is_tui_command(&cli.command) {
        Some(init_file_tracing(&default_tui_log_path()?)?)
    } else {
        init_stderr_tracing()?;
        None
    };
    {
        let span = tracing::info_span!("asm.tracing_init");
        let _enter = span.enter();
        tracing::info!(
            elapsed_ms = elapsed_ms(tracing_init_started),
            "startup phase complete"
        );
    }
    reject_global_for_non_list(cli.global, &cli.command);
    reject_benchmark_for_non_list(cli.benchmark, &cli.command);

    match cli.command {
        None => {
            let chips = parse_list_filter_or_exit(cli.filter.as_deref(), cli.global)?;
            if cli.benchmark {
                ui::run_benchmark_with_filter_started(chips, startup_started)?
            } else {
                ui::run_with_filter_started(chips, startup_started)?
            }
        }
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
        Some(Command::Ls { filter, global }) => {
            let filter = filter.as_deref().or(cli.filter.as_deref());
            let chips = parse_list_filter_or_exit(filter, cli.global || global)?;
            if cli.benchmark {
                ui::run_benchmark_with_filter_started(chips, startup_started)?
            } else {
                ui::run_with_filter_started(chips, startup_started)?
            }
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
        Some(Command::BenchFork { id, at, repeat }) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            run_bench_fork(&id, at, repeat)?;
        }
        Some(Command::BenchResume { id, repeat }) => {
            reject_filter_for_non_list(cli.filter.as_deref());
            run_bench_resume(&id, repeat)?;
        }
    }

    Ok(())
}

fn run_bench_fork(id: &str, at: Option<usize>, repeat: usize) -> Result<()> {
    let repeat = repeat.max(1);
    let lookup_started = Instant::now();
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
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    let home = dirs::home_dir().context("could not determine home directory")?;
    let roots = ForkRoots::from_home(&home);
    let lookup_us = lookup_started.elapsed().as_micros();

    eprintln!(
        "bench-fork: id={} agent={:?} cut_index={} repeat={}",
        session.id, session.agent, cut_index, repeat
    );
    eprintln!("bench-fork: index+lookup={}us", lookup_us);

    let mut read = Vec::with_capacity(repeat);
    let mut validate = Vec::with_capacity(repeat);
    let mut write = Vec::with_capacity(repeat);
    let mut total = Vec::with_capacity(repeat);
    let mut last_line_count = 0usize;
    let mut created: Vec<PathBuf> = Vec::with_capacity(repeat);

    for i in 0..repeat {
        let (result, timings) =
            fork_session_timed(&session, &session.path, cut_index, &current_cwd, &roots)?;
        last_line_count = timings.line_count;
        read.push(timings.read_jsonl_us);
        validate.push(timings.validate_us);
        write.push(timings.transform_write_us);
        total.push(timings.total_us);
        eprintln!(
            "  iter {}: total={}us read={}us validate={}us write={}us lines={}",
            i + 1,
            timings.total_us,
            timings.read_jsonl_us,
            timings.validate_us,
            timings.transform_write_us,
            timings.line_count,
        );
        created.push(result.path);
    }

    print_stats("read_jsonl   ", &read);
    print_stats("validate     ", &validate);
    print_stats("transform+wr ", &write);
    print_stats("TOTAL        ", &total);
    eprintln!("bench-fork: line_count={}", last_line_count);

    for path in created {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

fn run_bench_resume(id: &str, repeat: usize) -> Result<()> {
    let repeat = repeat.max(1);
    let lookup_started = Instant::now();
    let index = Index::open()?;
    let sessions = index.list_all()?;
    let session = sessions
        .into_iter()
        .find(|session| session.id == id)
        .with_context(|| format!("no indexed session found with id {id}"))?;
    let lookup_us = lookup_started.elapsed().as_micros();

    eprintln!(
        "bench-resume: id={} agent={:?} repeat={}",
        session.id, session.agent, repeat
    );
    eprintln!("bench-resume: index+lookup={}us", lookup_us);

    let mut argv_times = Vec::with_capacity(repeat);
    for i in 0..repeat {
        let t = Instant::now();
        let argv = resume_command(&session);
        let elapsed = t.elapsed().as_micros();
        argv_times.push(elapsed);
        eprintln!(
            "  iter {}: argv_build={}us argv={:?}",
            i + 1,
            elapsed,
            argv
        );
    }

    print_stats("argv_build   ", &argv_times);
    eprintln!(
        "bench-resume: NOTE the actual exec(2) call is a single syscall (~10us);"
    );
    eprintln!("              perceived latency is dominated by claude/codex startup.");
    Ok(())
}

fn print_stats(label: &str, samples: &[u128]) {
    if samples.is_empty() {
        return;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let min = sorted[0];
    let max = *sorted.last().unwrap();
    let median = sorted[sorted.len() / 2];
    let mean = samples.iter().sum::<u128>() / samples.len() as u128;
    eprintln!(
        "{label} min={}us median={}us mean={}us max={}us",
        min, median, mean, max
    );
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn is_tui_command(command: &Option<Command>) -> bool {
    matches!(command, None | Some(Command::Ls { .. }))
}

fn reject_filter_for_non_list(filter: Option<&str>) {
    if filter.is_some() {
        eprintln!("--filter is only supported by `asm` and `asm ls`");
        std::process::exit(2);
    }
}

fn reject_global_for_non_list(global: bool, command: &Option<Command>) {
    if global && !is_tui_command(command) {
        eprintln!("--global is only supported by `asm` and `asm ls`");
        std::process::exit(2);
    }
}

fn reject_benchmark_for_non_list(benchmark: bool, command: &Option<Command>) {
    if benchmark && !is_tui_command(command) {
        eprintln!("--benchmark is only supported by `asm` and `asm ls`");
        std::process::exit(2);
    }
}

fn parse_list_filter_or_exit(filter: Option<&str>, global: bool) -> Result<Vec<Chip>> {
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    match parse_list_filter_chips(filter, &current_cwd, global) {
        Ok(chips) => Ok(chips),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

fn init_stderr_tracing() -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(layer)
        .try_init()
        .context("failed to initialize stderr tracing")
}

fn init_file_tracing(log_path: &Path) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let parent = log_path
        .parent()
        .context("asm log path must include a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create asm log directory {}", parent.display()))?;
    let file_name = log_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("asm log path must include a valid UTF-8 file name")?;
    let file_appender = tracing_appender::rolling::never(parent, file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .compact();
    tracing_subscriber::registry()
        .with(env_filter)
        .with(layer)
        .try_init()
        .context("failed to initialize file tracing")?;
    Ok(guard)
}

fn default_tui_log_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".config").join("asm").join("asm.log"))
}

fn default_agent_roots() -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok((home.join(".claude"), home.join(".codex")))
}
