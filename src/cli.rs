use std::path::Path;

use anyhow::{Result, bail};

use crate::ui::filter::{Chip, parse_filter_text};

pub fn parse_list_filter_chips(
    expr: Option<&str>,
    current_cwd: &Path,
    global: bool,
) -> Result<Vec<Chip>> {
    let mut chips = match expr {
        Some(expr) => parse_filter_arg(expr, current_cwd)?,
        None => Vec::new(),
    };

    if !global && !chips.iter().any(|chip| matches!(chip, Chip::HereCwd(_))) {
        chips.push(Chip::HereCwd(canonical_path(current_cwd)));
    }

    Ok(chips)
}

pub fn parse_filter_arg(expr: &str, current_cwd: &Path) -> Result<Vec<Chip>> {
    let mut chips = Vec::new();
    let mut bad_tokens = Vec::new();

    for token in expr.split_whitespace() {
        let chip_expr = if token.starts_with('@') {
            token.to_owned()
        } else {
            format!("@{token}")
        };
        let parsed = parse_filter_text(&chip_expr, current_cwd);
        match parsed.chips.as_slice() {
            [chip] if parsed.search.is_empty() => chips.push(chip.clone()),
            _ => bad_tokens.push(token.to_owned()),
        }
    }

    if !bad_tokens.is_empty() {
        bail!("invalid --filter token(s): {}", quote_tokens(&bad_tokens));
    }

    Ok(chips)
}

fn canonical_path(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn quote_tokens(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
