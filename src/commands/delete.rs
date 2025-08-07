use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Confirm;

use crate::git::{execute_git, has_unpushed_commits, is_working_tree_clean};
use crate::state::XlaudeState;

pub fn handle_delete(name: Option<String>) -> Result<()> {
    let mut state = XlaudeState::load()?;

    // Determine which worktree to delete
    let (key, worktree_info) = if let Some(n) = name {
        // Find worktree by name across all projects
        state
            .worktrees
            .iter()
            .find(|(_, w)| w.name == n)
            .map(|(k, w)| (k.clone(), w.clone()))
            .context(format!("Worktree '{n}' not found"))?
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
            .iter()
            .find(|(_, w)| w.path.file_name().and_then(|n| n.to_str()) == Some(dir_name))
            .map(|(k, w)| (k.clone(), w.clone()))
            .context("Current directory is not a managed worktree")?
    };

    let worktree_name = worktree_info.name.clone();

    println!(
        "{} Checking worktree '{}'...",
        "🔍".yellow(),
        worktree_name.clone().cyan()
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

    // Remove worktree
    println!("{} Removing worktree...", "🗑️ ".yellow());
    execute_git(&["worktree", "remove", worktree_info.path.to_str().unwrap()])
        .context("Failed to remove worktree")?;

    // Try to delete branch
    println!(
        "{} Deleting branch '{}'...",
        "🗑️ ".yellow(),
        worktree_info.branch
    );

    // First try to delete with -d (safe delete)
    let result = execute_git(&["branch", "-d", &worktree_info.branch]);

    if result.is_err() {
        // Branch is not fully merged, ask for confirmation to force delete
        println!(
            "{} Branch '{}' is not fully merged",
            "⚠️ ".yellow(),
            worktree_info.branch.cyan()
        );

        let force_delete = if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
            // In non-interactive mode, don't force delete
            false
        } else {
            Confirm::new()
                .with_prompt("Do you want to force delete the branch?")
                .default(false)
                .interact()?
        };

        if force_delete {
            execute_git(&["branch", "-D", &worktree_info.branch])
                .context("Failed to force delete branch")?;
            println!("{} Branch deleted", "✅".green());
        } else {
            println!("{} Branch kept", "ℹ️ ".blue());
        }
    } else {
        println!("{} Branch deleted", "✅".green());
    }

    // Update state
    state.worktrees.remove(&key);
    state.save()?;

    println!(
        "{} Worktree '{}' deleted successfully",
        "✅".green(),
        worktree_name.cyan()
    );
    Ok(())
}
