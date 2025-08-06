use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::generate_random_name;
use crate::vcs::{self, VcsType};

pub fn handle_create(name: Option<String>) -> Result<()> {
    // Detect VCS type
    let vcs_type = vcs::detect_vcs()?;

    // Get repository name
    let repo_name = vcs::get_repo_name(&vcs_type)?;

    // Check if we're on a base branch (only for git)
    if vcs_type == VcsType::Git && !vcs::is_on_base_branch(&vcs_type)? {
        anyhow::bail!(
            "Must be on a base branch (main, master, or develop) to create a new worktree"
        );
    }

    // Generate name if not provided
    let workspace_name = match name {
        Some(n) => n,
        None => generate_random_name()?,
    };

    let workspace_type = match vcs_type {
        VcsType::Git => "worktree",
        VcsType::Jj => "workspace",
    };

    println!(
        "{} Creating {} '{}'...",
        "✨".green(),
        workspace_type,
        workspace_name.cyan()
    );

    // Create workspace directory path
    let workspace_dir = format!("../{repo_name}-{workspace_name}");
    let workspace_path = std::env::current_dir()?
        .parent()
        .unwrap()
        .join(format!("{repo_name}-{workspace_name}"));

    // Create worktree/workspace
    vcs::create_worktree_or_workspace(&vcs_type, &workspace_name, Path::new(&workspace_dir))?;

    // Copy CLAUDE.local.md if it exists
    let claude_local_md = Path::new("CLAUDE.local.md");
    if claude_local_md.exists() {
        let target_path = workspace_path.join("CLAUDE.local.md");
        fs::copy(claude_local_md, &target_path).context("Failed to copy CLAUDE.local.md")?;
        println!("{} Copied CLAUDE.local.md to workspace", "📄".green());
    }

    // Save state
    let mut state = XlaudeState::load()?;
    let key = XlaudeState::make_key(&repo_name, &workspace_name);
    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: workspace_name.clone(),
            branch: workspace_name.clone(), // For jj, this will be the workspace name
            path: workspace_path.clone(),
            repo_name,
            created_at: Utc::now(),
        },
    );
    state.save()?;

    println!(
        "{} {} created at: {}",
        "✅".green(),
        workspace_type
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + &workspace_type[1..],
        workspace_path.display()
    );
    println!(
        "  {} To open it, run: {} {}",
        "💡".cyan(),
        "xlaude open".cyan(),
        workspace_name.cyan()
    );

    Ok(())
}
