use std::process::Command;

use anyhow::{Context, Result};

use crate::{Agent, Session};

pub fn resume_command(session: &Session) -> Vec<String> {
    match session.agent {
        Agent::Claude => vec![
            "claude".to_owned(),
            "--resume".to_owned(),
            session.id.clone(),
        ],
        Agent::Codex => vec!["codex".to_owned(), "resume".to_owned(), session.id.clone()],
    }
}

pub fn dispatch_resume(session: &Session) -> Result<()> {
    if session.id.trim().is_empty() {
        eprintln!("asm: selected session has no session id");
        std::process::exit(1);
    }

    let argv = resume_command(session);
    exec_or_wait(&argv)
}

fn exec_or_wait(argv: &[String]) -> Result<()> {
    let (program, args) = argv
        .split_first()
        .context("resume command unexpectedly had no program")?;
    let mut command = Command::new(program);
    command.args(args);

    exec_or_wait_command(command, argv)
}

#[cfg(unix)]
fn exec_or_wait_command(mut command: Command, argv: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let error = command.exec();
    Err(error).with_context(|| format!("failed to exec resume command: {}", argv.join(" ")))
}

#[cfg(not(unix))]
fn exec_or_wait_command(mut command: Command, argv: &[String]) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to spawn resume command: {}", argv.join(" ")))?;
    std::process::exit(status.code().unwrap_or(1));
}
