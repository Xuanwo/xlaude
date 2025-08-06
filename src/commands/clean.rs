use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::state::XlaudeState;
use crate::utils::execute_in_dir;
use crate::vcs::{self, WorkspaceInfo};

pub fn handle_clean() -> Result<()> {
    let mut state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        println!("{} No worktrees/workspaces in state", "✨".green());
        return Ok(());
    }

    println!(
        "{} Checking for invalid worktrees/workspaces...",
        "🔍".cyan()
    );

    // Collect all actual worktrees from all repositories
    let actual_worktrees = collect_all_worktrees(&state)?;

    // Find and remove invalid worktrees
    let mut removed_count = 0;
    let worktrees_to_remove: Vec<_> = state
        .worktrees
        .iter()
        .filter_map(|(name, info)| {
            if !actual_worktrees.contains(&info.path) {
                println!(
                    "  {} Found invalid worktree/workspace: {} ({})",
                    "❌".red(),
                    name.yellow(),
                    info.path.display()
                );
                removed_count += 1;
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    // Remove invalid worktrees from state
    for name in worktrees_to_remove {
        state.worktrees.remove(&name);
    }

    if removed_count > 0 {
        state.save()?;
        println!(
            "{} Removed {} invalid worktree{}/workspace{}",
            "✅".green(),
            removed_count,
            if removed_count == 1 { "" } else { "s" },
            if removed_count == 1 { "" } else { "s" }
        );
    } else {
        println!("{} All worktrees/workspaces are valid", "✨".green());
    }

    Ok(())
}

fn collect_all_worktrees(state: &XlaudeState) -> Result<HashSet<PathBuf>> {
    let mut all_worktrees = HashSet::new();

    // Get unique repository paths
    let repo_paths: HashSet<_> = state
        .worktrees
        .values()
        .filter_map(|info| info.path.parent().map(|p| p.join(&info.repo_name)))
        .collect();

    // Collect worktrees/workspaces from each repository
    for repo_path in repo_paths {
        if repo_path.exists() {
            // Use execute_in_dir to safely change directories
            let _ = execute_in_dir(&repo_path, || {
                // Detect VCS type and get workspaces
                if let Ok(vcs_type) = vcs::detect_vcs()
                    && let Ok(workspaces) = vcs::list_worktrees_or_workspaces(&vcs_type)
                {
                    // Extract paths from WorkspaceInfo
                    for workspace in workspaces {
                        match workspace {
                            WorkspaceInfo::Git(path) => all_worktrees.insert(path),
                            WorkspaceInfo::Jj(path) => all_worktrees.insert(path),
                        };
                    }
                }
                Ok(())
            });
        }
    }

    Ok(all_worktrees)
}
