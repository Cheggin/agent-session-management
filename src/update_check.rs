//! Async update notifier.
//!
//! Spawns a background thread on TUI launch that asks the npm registry for the
//! latest `@reaganhsu/asm` version and compares it against `CARGO_PKG_VERSION`.
//! Result is cached in `~/.config/asm/update-check.json` for 24 hours so the
//! network call only happens once per day. Opt out with `ASM_NO_UPDATE_CHECK=1`.
//!
//! On detection the TUI surfaces a toast: "update available: vX.Y.Z — run
//! `npm update -g @reaganhsu/asm`". The check is fail-open: any network
//! error / parse failure / unreadable cache is silently ignored.

use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, channel},
    thread,
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const NPM_LATEST_URL: &str = "https://registry.npmjs.org/@reaganhsu/asm/latest";
const CACHE_TTL_HOURS: i64 = 24;
const HTTP_TIMEOUT: Duration = Duration::from_secs(4);

pub fn update_command_hint() -> &'static str {
    "run `npm update -g @reaganhsu/asm`"
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub latest: String,
}

pub fn spawn_check() -> Option<Receiver<AvailableUpdate>> {
    if std::env::var("ASM_NO_UPDATE_CHECK").is_ok() {
        return None;
    }
    let (tx, rx) = channel();
    thread::Builder::new()
        .name("asm-update-check".into())
        .spawn(move || {
            let _ = run(tx);
        })
        .ok()?;
    Some(rx)
}

fn run(tx: Sender<AvailableUpdate>) -> Option<()> {
    let current = current_version();
    let cache_path = cache_path();
    let cached = cache_path.as_deref().and_then(read_cache);

    let latest = match &cached {
        Some(entry) if is_fresh(&entry.checked_at) => entry.latest_version.clone(),
        _ => {
            let fetched = fetch_latest_from_npm().ok()?;
            if let Some(path) = cache_path.as_deref() {
                let _ = write_cache(
                    path,
                    &CacheEntry {
                        checked_at: Utc::now(),
                        latest_version: fetched.clone(),
                    },
                );
            }
            fetched
        }
    };

    if is_newer(&latest, current) {
        let _ = tx.send(AvailableUpdate { latest });
    }
    Some(())
}

fn fetch_latest_from_npm() -> Result<String, &'static str> {
    let response = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .user_agent(&format!("asm/{}", current_version()))
        .build()
        .get(NPM_LATEST_URL)
        .call()
        .map_err(|_| "npm registry request failed")?;
    let body = response
        .into_string()
        .map_err(|_| "npm registry response not utf-8")?;
    let json: NpmManifest = serde_json::from_str(&body).map_err(|_| "invalid npm manifest json")?;
    Ok(json.version)
}

#[derive(Debug, Deserialize)]
struct NpmManifest {
    version: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    checked_at: DateTime<Utc>,
    latest_version: String,
}

fn cache_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".config").join("asm").join("update-check.json"))
}

fn read_cache(path: &std::path::Path) -> Option<CacheEntry> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache(path: &std::path::Path, entry: &CacheEntry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    fs::write(path, bytes)
}

fn is_fresh(checked_at: &DateTime<Utc>) -> bool {
    let age = Utc::now().signed_duration_since(*checked_at);
    age >= chrono::Duration::zero() && age < chrono::Duration::hours(CACHE_TTL_HOURS)
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let lhs = parse_semver(candidate);
    let rhs = parse_semver(current);
    match (lhs, rhs) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let trimmed = s.trim().trim_start_matches('v');
    let mut parts = trimmed.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_str = parts.next()?;
    let patch: u64 = patch_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_wins() {
        assert!(is_newer("0.1.2", "0.1.1"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.0", "0.1.1"));
    }

    #[test]
    fn double_digit_minor_is_newer_than_single() {
        assert!(is_newer("0.10.0", "0.9.9"));
    }

    #[test]
    fn v_prefix_tolerated() {
        assert!(is_newer("v1.0.0", "0.9.9"));
    }

    #[test]
    fn invalid_versions_never_trigger_notice() {
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(!is_newer("0.1.0", "garbage"));
    }

    #[test]
    fn fresh_within_ttl() {
        let now = Utc::now();
        assert!(is_fresh(&now));
        assert!(is_fresh(
            &(now - chrono::Duration::hours(CACHE_TTL_HOURS - 1))
        ));
        assert!(!is_fresh(
            &(now - chrono::Duration::hours(CACHE_TTL_HOURS + 1))
        ));
    }
}
