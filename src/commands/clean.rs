use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::git::list_worktrees;
use crate::state::XlaudeState;
use crate::utils::execute_in_dir;

pub fn handle_clean() -> Result<()> {
    let mut state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        println!("{} No worktrees in state", "✨".green());
        return Ok(());
    }

    println!("{} Checking for invalid worktrees...", "🔍".cyan());

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
                    "  {} Found invalid worktree: {} ({})",
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
            "{} Removed {} invalid worktree{}",
            "✅".green(),
            removed_count,
            if removed_count == 1 { "" } else { "s" }
        );
    } else {
        println!("{} All worktrees are valid", "✨".green());
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

    // Collect worktrees from each repository
    for repo_path in repo_paths {
        if repo_path.exists()
            && let Ok(worktrees) = execute_in_dir(&repo_path, list_worktrees)
        {
            all_worktrees.extend(worktrees);
        }
    }

    Ok(all_worktrees)
}
