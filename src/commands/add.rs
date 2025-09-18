use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::fs;

use crate::git::{get_current_branch, get_repo_name, is_in_worktree};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::sanitize_branch_name;

pub fn handle_add(name: Option<String>) -> Result<()> {
    // Check if we're in a git repository
    let repo_name = get_repo_name().context("Not in a git repository")?;

    // Check if we're in a worktree
    if !is_in_worktree()? {
        anyhow::bail!("Current directory is not a git worktree");
    }

    // Get current branch name
    let current_branch = get_current_branch()?;

    // Use provided name or default to sanitized branch name
    let worktree_name = match name {
        Some(n) => n,
        None => sanitize_branch_name(&current_branch),
    };

    // Get current directory
    let current_dir = std::env::current_dir()?;

    // Load state
    let mut state = XlaudeState::load()?;

    let normalize_path = |path: &std::path::Path| -> std::path::PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let current_dir_key = normalize_path(&current_dir);

    // Check if this path is already managed under another worktree
    if let Some(existing) = state
        .worktrees
        .values()
        .find(|info| normalize_path(&info.path) == current_dir_key)
    {
        anyhow::bail!(
            "Current directory '{}' is already managed by xlaude as '{}/{}'",
            current_dir.display(),
            existing.repo_name,
            existing.name
        );
    }

    // Check if already managed under the same name
    let key = XlaudeState::make_key(&repo_name, &worktree_name);
    if state.worktrees.contains_key(&key) {
        anyhow::bail!(
            "Worktree '{}/{}' is already managed by xlaude",
            repo_name,
            worktree_name
        );
    }

    println!(
        "{} Adding worktree '{}' to xlaude management...",
        "➕".green(),
        worktree_name.cyan()
    );

    // Add to state
    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: worktree_name.clone(),
            branch: current_branch,
            path: current_dir.clone(),
            repo_name,
            created_at: Utc::now(),
        },
    );
    state.save()?;

    println!(
        "{} Worktree '{}' added successfully",
        "✅".green(),
        worktree_name.cyan()
    );
    println!("  {} {}", "Path:".bright_black(), current_dir.display());

    Ok(())
}
