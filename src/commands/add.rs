use anyhow::Result;
use chrono::Utc;
use colored::Colorize;

use crate::state::{WorktreeInfo, XlaudeState};
use crate::vcs::{self, VcsType};

pub fn handle_add(name: Option<String>) -> Result<()> {
    // Detect VCS type
    let vcs_type = vcs::detect_vcs()?;

    // Get repository name
    let repo_name = vcs::get_repo_name(&vcs_type)?;

    // Check if we're in a worktree/workspace
    if !vcs::is_in_worktree_or_workspace(&vcs_type)? {
        let workspace_type = match vcs_type {
            VcsType::Git => "git worktree",
            VcsType::Jj => "jj workspace",
        };
        anyhow::bail!("Current directory is not a {}", workspace_type);
    }

    // Get current branch/workspace name
    let current_branch_or_workspace = vcs::get_current_branch_or_workspace(&vcs_type)?;

    // Use provided name or default to branch/workspace name
    let workspace_name = name.unwrap_or_else(|| current_branch_or_workspace.clone());

    // Get current directory
    let current_dir = std::env::current_dir()?;

    // Check if already managed
    let mut state = XlaudeState::load()?;
    let key = XlaudeState::make_key(&repo_name, &workspace_name);
    if state.worktrees.contains_key(&key) {
        anyhow::bail!(
            "Workspace '{}/{}' is already managed by xlaude",
            repo_name,
            workspace_name
        );
    }

    let workspace_type = match vcs_type {
        VcsType::Git => "worktree",
        VcsType::Jj => "workspace",
    };

    println!(
        "{} Adding {} '{}' to xlaude management...",
        "➕".green(),
        workspace_type,
        workspace_name.cyan()
    );

    // Add to state
    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: workspace_name.clone(),
            branch: current_branch_or_workspace,
            path: current_dir.clone(),
            repo_name,
            created_at: Utc::now(),
        },
    );
    state.save()?;

    println!(
        "{} {} '{}' added successfully",
        "✅".green(),
        workspace_type
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + &workspace_type[1..],
        workspace_name.cyan()
    );
    println!("  {} {}", "Path:".bright_black(), current_dir.display());

    Ok(())
}
