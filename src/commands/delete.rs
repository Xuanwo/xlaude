use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Confirm;

use crate::git::{execute_git, has_unpushed_commits, is_working_tree_clean};
use crate::state::XlaudeState;

pub fn handle_delete(name: Option<String>) -> Result<()> {
    let mut state = XlaudeState::load()?;

    // Determine which worktree to delete
    let worktree_name = if let Some(n) = name {
        n
    } else {
        // Get current directory name to find current worktree
        let current_dir = std::env::current_dir()?;
        let dir_name = current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .context("Failed to get current directory name")?;

        // Find matching worktree
        state
            .worktrees
            .values()
            .find(|w| w.path.file_name().and_then(|n| n.to_str()) == Some(dir_name))
            .map(|w| w.name.clone())
            .context("Current directory is not a managed worktree")?
    };

    let worktree_info = state
        .worktrees
        .get(&worktree_name)
        .context("Worktree not found")?;

    println!(
        "{} Checking worktree '{}'...",
        "🔍".yellow(),
        worktree_name.cyan()
    );

    // Change to worktree directory to check status
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&worktree_info.path)
        .context("Failed to change to worktree directory")?;

    // Check for uncommitted changes
    let has_changes = !is_working_tree_clean()?;
    let has_unpushed = has_unpushed_commits();

    if has_changes || has_unpushed {
        println!();
        if has_changes {
            println!("{} You have uncommitted changes", "⚠️ ".red());
        }
        if has_unpushed {
            println!("{} You have unpushed commits", "⚠️ ".red());
        }

        // Allow non-interactive mode for testing
        let confirmed = if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
            // In non-interactive mode, don't proceed with deletion if there are changes
            false
        } else {
            Confirm::new()
                .with_prompt("Are you sure you want to delete this worktree?")
                .default(false)
                .interact()?
        };

        if !confirmed {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    } else if std::env::var("XLAUDE_NON_INTERACTIVE").is_err() {
        // Only ask for confirmation if not in non-interactive mode
        let confirmed = Confirm::new()
            .with_prompt(format!("Delete worktree '{worktree_name}'?"))
            .default(true)
            .interact()?;

        if !confirmed {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    }

    // Change back to original directory
    std::env::set_current_dir(&original_dir)?;

    // Check if branch is fully merged before asking about force delete
    println!(
        "{} Checking branch '{}'...",
        "🔍".yellow(),
        worktree_info.branch
    );

    // Check if branch is fully merged by checking if it would need -D to delete
    let output = std::process::Command::new("git")
        .args(["branch", "--merged"])
        .output()
        .context("Failed to check merged branches")?;

    let merged_branches = String::from_utf8_lossy(&output.stdout);
    let branch_is_merged = merged_branches
        .lines()
        .any(|line| line.trim().trim_start_matches('*').trim() == worktree_info.branch);

    let should_force_delete = if !branch_is_merged {
        // Branch is not fully merged, ask for confirmation to force delete
        println!(
            "{} Branch '{}' is not fully merged",
            "⚠️ ".yellow(),
            worktree_info.branch.cyan()
        );

        if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
            // In non-interactive mode, don't force delete
            false
        } else {
            Confirm::new()
                .with_prompt("Do you want to force delete the branch?")
                .default(false)
                .interact()?
        }
    } else {
        false
    };

    // Now remove worktree
    println!("{} Removing worktree...", "🗑️ ".yellow());
    execute_git(&["worktree", "remove", worktree_info.path.to_str().unwrap()])
        .context("Failed to remove worktree")?;

    // Delete branch based on earlier decision
    println!(
        "{} Deleting branch '{}'...",
        "🗑️ ".yellow(),
        worktree_info.branch
    );

    if should_force_delete {
        execute_git(&["branch", "-D", &worktree_info.branch])
            .context("Failed to force delete branch")?;
        println!("{} Branch deleted", "✅".green());
    } else {
        let result = execute_git(&["branch", "-d", &worktree_info.branch]);
        if result.is_ok() {
            println!("{} Branch deleted", "✅".green());
        } else {
            println!("{} Branch kept (not fully merged)", "ℹ️ ".blue());
        }
    }

    // Update state
    state.worktrees.remove(&worktree_name);
    state.save()?;

    println!(
        "{} Worktree '{}' deleted successfully",
        "✅".green(),
        worktree_name.cyan()
    );
    Ok(())
}
