use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Instant, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rayon::prelude::*;
use tracing::{info, info_span, warn};

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

#[derive(Debug)]
struct ParsedSession {
    source: Source,
    session: Session,
    path_mtime: i64,
}

pub fn reindex_all(
    index: &mut Index,
    claude_root: &Path,
    codex_root: &Path,
) -> Result<ReindexStats> {
    let discovered = {
        let span = info_span!("asm.reindex.discover");
        let _enter = span.enter();
        let started = Instant::now();
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
        info!(
            elapsed_ms = elapsed_ms(started),
            discovered = discovered.len(),
            "startup phase complete"
        );
        discovered
    };

    let mut stats = ReindexStats {
        discovered: discovered.len(),
        ..ReindexStats::default()
    };

    let mut candidates = Vec::new();
    {
        let span = info_span!("asm.reindex.filter");
        let _enter = span.enter();
        let started = Instant::now();
        for (source, path) in discovered {
            let path_mtime = match path_mtime(&path) {
                Ok(path_mtime) => path_mtime,
                Err(error) => {
                    stats.failed += 1;
                    warn!(path = %path.display(), error = %error, "failed to stat session file");
                    continue;
                }
            };

            if index.get_path_mtime(&path)? == Some(path_mtime) {
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
            elapsed_ms = elapsed_ms(started),
            changed = candidates.len(),
            skipped_unchanged = stats.skipped_unchanged,
            failed = stats.failed,
            "startup phase complete"
        );
    }

    info!(
        discovered = stats.discovered,
        changed = candidates.len(),
        skipped_unchanged = stats.skipped_unchanged,
        "session files discovered"
    );

    let parse_results: Vec<_> = {
        let span = info_span!("asm.reindex.parse");
        let _enter = span.enter();
        let started = Instant::now();
        let candidate_count = candidates.len();
        let parse_results: Vec<_> = candidates.into_par_iter().map(parse_candidate).collect();
        info!(
            elapsed_ms = elapsed_ms(started),
            candidates = candidate_count,
            "startup phase complete"
        );
        parse_results
    };
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
    {
        let span = info_span!("asm.reindex.write");
        let _enter = span.enter();
        let started = Instant::now();
        let sessions_only: Vec<_> = parsed_sessions
            .iter()
            .map(|parsed| (parsed.session.clone(), parsed.path_mtime))
            .collect();
        let result = index.upsert_sessions_only(&sessions_only);
        info!(
            elapsed_ms = elapsed_ms(started),
            parsed = stats.parsed,
            ok = result.is_ok(),
            "startup phase complete"
        );
        result?;
    }

    spawn_message_fts_upsert(index.path().to_path_buf(), parsed_sessions);

    index.mark_reindexed_now()?;

    info!(
        discovered = stats.discovered,
        parsed = stats.parsed,
        skipped_unchanged = stats.skipped_unchanged,
        failed = stats.failed,
        "reindex complete"
    );

    Ok(stats)
}

fn spawn_message_fts_upsert(index_path: PathBuf, parsed_sessions: Vec<ParsedSession>) {
    if parsed_sessions.is_empty() {
        return;
    }

    thread::spawn(move || {
        let span = info_span!("asm.reindex.fts_background");
        let _enter = span.enter();
        let started = Instant::now();
        let parsed = parsed_sessions.len();
        let result = (|| -> Result<(usize, usize)> {
            let mut background_index = Index::open_at(index_path)?;
            let mut failed = 0;
            let mut messages = Vec::with_capacity(parsed_sessions.len());
            for parsed_session in parsed_sessions {
                match extract_message_bodies(parsed_session.source, &parsed_session.session.path) {
                    Ok(message_bodies) => {
                        messages.push((parsed_session.session.id, message_bodies));
                    }
                    Err(error) => {
                        failed += 1;
                        warn!(
                            path = %parsed_session.session.path.display(),
                            error = %error,
                            "failed to extract message bodies for FTS"
                        );
                    }
                }
            }

            background_index.upsert_messages_fts(&messages)?;
            Ok((messages.len(), failed))
        })();

        match result {
            Ok((indexed, failed)) => info!(
                elapsed_ms = elapsed_ms(started),
                parsed, indexed, failed, "background FTS population complete"
            ),
            Err(error) => warn!(
                elapsed_ms = elapsed_ms(started),
                parsed,
                error = %error,
                "background FTS population failed"
            ),
        }
    });
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn parse_candidate(
    candidate: Candidate,
) -> std::result::Result<ParsedSession, (PathBuf, anyhow::Error)> {
    let session = match candidate.source {
        Source::Claude => ClaudeParser::parse(&candidate.path),
        Source::Codex => CodexParser::parse(&candidate.path),
    }
    .map_err(|error| (candidate.path.clone(), error))?;

    Ok(ParsedSession {
        source: candidate.source,
        session,
        path_mtime: candidate.path_mtime,
    })
}

fn extract_message_bodies(source: Source, path: &Path) -> Result<Vec<String>> {
    match source {
        Source::Claude => ClaudeParser::extract_message_bodies(path),
        Source::Codex => CodexParser::extract_message_bodies(path),
    }
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
