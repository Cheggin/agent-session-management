use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result};
use rayon::prelude::*;
use tracing::{info, warn};

use crate::{
    ClaudeParser, CodexParser, Parser, Session,
    db::Index,
    discover::{scan_claude, scan_codex},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReindexStats {
    pub discovered: usize,
    pub parsed: usize,
    pub skipped_unchanged: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy)]
enum Source {
    Claude,
    Codex,
}

#[derive(Debug)]
struct Candidate {
    source: Source,
    path: PathBuf,
    path_mtime: i64,
}

pub fn reindex_all(
    index: &mut Index,
    claude_root: &Path,
    codex_root: &Path,
) -> Result<ReindexStats> {
    let mut discovered = Vec::new();
    discovered.extend(
        scan_claude(claude_root)
            .into_iter()
            .map(|path| (Source::Claude, path)),
    );
    discovered.extend(
        scan_codex(codex_root)
            .into_iter()
            .map(|path| (Source::Codex, path)),
    );

    let mut stats = ReindexStats {
        discovered: discovered.len(),
        ..ReindexStats::default()
    };

    let mut candidates = Vec::new();
    for (source, path) in discovered {
        let path_mtime = match path_mtime(&path) {
            Ok(path_mtime) => path_mtime,
            Err(error) => {
                stats.failed += 1;
                warn!(path = %path.display(), error = %error, "failed to stat session file");
                continue;
            }
        };

        if index.get_path_mtime(&path)? == Some(path_mtime)
            && index.path_has_message_bodies(&path)?
        {
            stats.skipped_unchanged += 1;
            continue;
        }

        candidates.push(Candidate {
            source,
            path,
            path_mtime,
        });
    }

    info!(
        discovered = stats.discovered,
        changed = candidates.len(),
        skipped_unchanged = stats.skipped_unchanged,
        "session files discovered"
    );

    let parse_results: Vec<_> = candidates.into_par_iter().map(parse_candidate).collect();
    let mut parsed_sessions = Vec::new();
    for result in parse_results {
        match result {
            Ok(parsed) => parsed_sessions.push(parsed),
            Err((path, error)) => {
                stats.failed += 1;
                warn!(path = %path.display(), error = %error, "failed to parse session file");
            }
        }
    }

    stats.parsed = parsed_sessions.len();
    index.upsert_sessions(&parsed_sessions)?;

    info!(
        discovered = stats.discovered,
        parsed = stats.parsed,
        skipped_unchanged = stats.skipped_unchanged,
        failed = stats.failed,
        "reindex complete"
    );

    Ok(stats)
}

fn parse_candidate(
    candidate: Candidate,
) -> std::result::Result<(Session, i64, Vec<String>), (PathBuf, anyhow::Error)> {
    let (session, message_bodies) = match candidate.source {
        Source::Claude => {
            let session = ClaudeParser::parse(&candidate.path);
            session.and_then(|session| {
                ClaudeParser::extract_message_bodies(&candidate.path)
                    .map(|message_bodies| (session, message_bodies))
            })
        }
        Source::Codex => {
            let session = CodexParser::parse(&candidate.path);
            session.and_then(|session| {
                CodexParser::extract_message_bodies(&candidate.path)
                    .map(|message_bodies| (session, message_bodies))
            })
        }
    }
    .map_err(|error| (candidate.path.clone(), error))?;

    Ok((session, candidate.path_mtime, message_bodies))
}

fn path_mtime(path: &Path) -> Result<i64> {
    let modified = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .modified()
        .with_context(|| format!("failed to read modified time for {}", path.display()))?;
    let nanos = modified
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("modified time is before Unix epoch for {}", path.display()))?
        .as_nanos();
    i64::try_from(nanos)
        .with_context(|| format!("modified time overflows i64 for {}", path.display()))
}
