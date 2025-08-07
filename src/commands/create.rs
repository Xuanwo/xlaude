use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::git::{execute_git, get_repo_name, is_base_branch};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::generate_random_name;

pub fn handle_create(name: Option<String>) -> Result<()> {
    // Check if we're in a git repository
    let repo_name = get_repo_name().context("Not in a git repository")?;

    // Check if we're on a base branch
    if !is_base_branch()? {
        anyhow::bail!(
            "Must be on a base branch (main, master, or develop) to create a new worktree"
        );
    }

    // Generate name if not provided
    let worktree_name = match name {
        Some(n) => n,
        None => generate_random_name()?,
    };

    println!(
        "{} Creating worktree '{}'...",
        "✨".green(),
        worktree_name.cyan()
    );

    // Create branch
    execute_git(&["branch", &worktree_name]).context("Failed to create branch")?;

    // Create worktree
    let worktree_dir = format!("../{repo_name}-{worktree_name}");
    execute_git(&["worktree", "add", &worktree_dir, &worktree_name])
        .context("Failed to create worktree")?;

    // Get absolute path
    let worktree_path = std::env::current_dir()?
        .parent()
        .unwrap()
        .join(format!("{repo_name}-{worktree_name}"));

    // Copy CLAUDE.local.md if it exists
    let claude_local_md = Path::new("CLAUDE.local.md");
    if claude_local_md.exists() {
        let target_path = worktree_path.join("CLAUDE.local.md");
        fs::copy(claude_local_md, &target_path).context("Failed to copy CLAUDE.local.md")?;
        println!("{} Copied CLAUDE.local.md to worktree", "📄".green());
    }

    // Save state
    let mut state = XlaudeState::load()?;
    let key = XlaudeState::make_key(&repo_name, &worktree_name);
    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: worktree_name.clone(),
            branch: worktree_name.clone(),
            path: worktree_path.clone(),
            repo_name,
            created_at: Utc::now(),
        },
    );
    state.save()?;

    println!(
        "{} Worktree created at: {}",
        "✅".green(),
        worktree_path.display()
    );
    println!(
        "  {} To open it, run: {} {}",
        "💡".cyan(),
        "xlaude open".cyan(),
        worktree_name.cyan()
    );

    Ok(())
}
