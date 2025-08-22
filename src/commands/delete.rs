use anyhow::{Context, Result};
use colored::Colorize;

use crate::git::{execute_git, has_unpushed_commits, is_working_tree_clean};
use crate::input::{get_command_arg, smart_confirm};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::execute_in_dir;

/// Represents the result of various checks performed before deletion
struct DeletionChecks {
    has_uncommitted_changes: bool,
    has_unpushed_commits: bool,
    branch_merged_via_git: bool,
    branch_merged_via_pr: bool,
}

impl DeletionChecks {
    fn branch_is_merged(&self) -> bool {
        self.branch_merged_via_git || self.branch_merged_via_pr
    }

    fn has_pending_work(&self) -> bool {
        self.has_uncommitted_changes || self.has_unpushed_commits
    }
}

/// Configuration for deletion behavior
struct DeletionConfig {
    is_interactive: bool,
    worktree_exists: bool,
    is_current_directory: bool,
}

impl DeletionConfig {
    fn from_env(worktree_info: &WorktreeInfo) -> Result<Self> {
        let current_dir = std::env::current_dir()?;

        Ok(Self {
            is_interactive: std::env::var("XLAUDE_NON_INTERACTIVE").is_err(),
            worktree_exists: worktree_info.path.exists(),
            is_current_directory: current_dir == worktree_info.path,
        })
    }
}

pub fn handle_delete(name: Option<String>) -> Result<()> {
    let mut state = XlaudeState::load()?;

    // Get name from CLI args or pipe
    let target_name = get_command_arg(name)?;
    let (key, worktree_info) = find_worktree_to_delete(&state, target_name)?;
    let config = DeletionConfig::from_env(&worktree_info)?;

    println!(
        "{} Checking worktree '{}'...",
        "🔍".yellow(),
        worktree_info.name.cyan()
    );

    // Handle case where worktree directory doesn't exist
    if !config.worktree_exists {
        if !handle_missing_worktree(&worktree_info, &config)? {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    } else {
        // Check branch status first (for output consistency)
        println!(
            "{} Checking branch '{}'...",
            "🔍".yellow(),
            worktree_info.branch
        );

        // Perform deletion checks
        let checks = perform_deletion_checks(&worktree_info)?;

        if !confirm_deletion(&worktree_info, &checks, &config)? {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    }

    // Execute deletion
    perform_deletion(&worktree_info, &config)?;

    // Update state
    state.worktrees.remove(&key);
    state.save()?;

    println!(
        "{} Worktree '{}' deleted successfully",
        "✅".green(),
        worktree_info.name.cyan()
    );
    Ok(())
}

/// Find the worktree to delete based on the provided name or current directory
fn find_worktree_to_delete(
    state: &XlaudeState,
    name: Option<String>,
) -> Result<(String, WorktreeInfo)> {
    if let Some(n) = name {
        // Find worktree by name across all projects
        state
            .worktrees
            .iter()
            .find(|(_, w)| w.name == n)
            .map(|(k, w)| (k.clone(), w.clone()))
            .context(format!("Worktree '{n}' not found"))
    } else {
        // Find worktree by current directory
        find_current_worktree(state)
    }
}

/// Find the worktree that matches the current directory
fn find_current_worktree(state: &XlaudeState) -> Result<(String, WorktreeInfo)> {
    let current_dir = std::env::current_dir()?;
    let dir_name = current_dir
        .file_name()
        .and_then(|n| n.to_str())
        .context("Failed to get current directory name")?;

    state
        .worktrees
        .iter()
        .find(|(_, w)| w.path.file_name().and_then(|n| n.to_str()) == Some(dir_name))
        .map(|(k, w)| (k.clone(), w.clone()))
        .context("Current directory is not a managed worktree")
}

/// Handle the case where worktree directory doesn't exist
fn handle_missing_worktree(worktree_info: &WorktreeInfo, _config: &DeletionConfig) -> Result<bool> {
    println!(
        "{} Worktree directory not found at {}",
        "⚠️ ".yellow(),
        worktree_info.path.display()
    );
    println!(
        "  {} The worktree may have been manually deleted",
        "ℹ️".blue()
    );

    smart_confirm("Remove this worktree from xlaude management?", true)
}

/// Perform all checks needed before deletion
fn perform_deletion_checks(worktree_info: &WorktreeInfo) -> Result<DeletionChecks> {
    execute_in_dir(&worktree_info.path, || {
        let has_uncommitted_changes = !is_working_tree_clean()?;
        let has_unpushed_commits = has_unpushed_commits();

        // Check branch merge status in main repo
        let main_repo_path = get_main_repo_path(worktree_info)?;
        let (branch_merged_via_git, branch_merged_via_pr) =
            check_branch_merge_status(&main_repo_path, &worktree_info.branch)?;

        Ok(DeletionChecks {
            has_uncommitted_changes,
            has_unpushed_commits,
            branch_merged_via_git,
            branch_merged_via_pr,
        })
    })
}

/// Check if branch is merged via git or PR
fn check_branch_merge_status(
    main_repo_path: &std::path::Path,
    branch: &str,
) -> Result<(bool, bool)> {
    execute_in_dir(main_repo_path, || {
        // Check traditional git merge
        let output = std::process::Command::new("git")
            .args(["branch", "--merged"])
            .output()
            .context("Failed to check merged branches")?;

        let merged_branches = String::from_utf8_lossy(&output.stdout);
        let is_merged_git = merged_branches
            .lines()
            .any(|line| line.trim().trim_start_matches('*').trim() == branch);

        // Check if merged via PR (works for squash merge)
        let is_merged_pr = check_branch_merged_via_pr(branch);

        Ok((is_merged_git, is_merged_pr))
    })
}

/// Check if branch was merged via GitHub PR
fn check_branch_merged_via_pr(branch: &str) -> bool {
    std::process::Command::new("gh")
        .args([
            "pr", "list", "--state", "merged", "--head", branch, "--json", "number",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|json| serde_json::from_str::<Vec<serde_json::Value>>(&json).ok())
        .map(|prs| !prs.is_empty())
        .unwrap_or(false)
}

/// Confirm deletion with the user based on checks
fn confirm_deletion(
    worktree_info: &WorktreeInfo,
    checks: &DeletionChecks,
    _config: &DeletionConfig,
) -> Result<bool> {
    // Show warnings for pending work
    if checks.has_pending_work() {
        show_pending_work_warnings(checks);

        return smart_confirm("Are you sure you want to delete this worktree?", false);
    }

    // Show branch merge status
    if !checks.branch_is_merged() {
        show_unmerged_branch_warning(worktree_info);
    } else if checks.branch_merged_via_pr && !checks.branch_merged_via_git {
        println!("  {} Branch was merged via PR", "ℹ️".blue());
    }

    // Ask for confirmation
    smart_confirm(&format!("Delete worktree '{}'?", worktree_info.name), true)
}

/// Show warnings for uncommitted changes or unpushed commits
fn show_pending_work_warnings(checks: &DeletionChecks) {
    println!();
    if checks.has_uncommitted_changes {
        println!("{} You have uncommitted changes", "⚠️ ".red());
    }
    if checks.has_unpushed_commits {
        println!("{} You have unpushed commits", "⚠️ ".red());
    }
}

/// Show warning for unmerged branch
fn show_unmerged_branch_warning(worktree_info: &WorktreeInfo) {
    println!(
        "{} Branch '{}' is not fully merged",
        "⚠️ ".yellow(),
        worktree_info.branch.cyan()
    );
    println!("  {} No merged PR found for this branch", "ℹ️".blue());
}

/// Perform the actual deletion of worktree and branch
fn perform_deletion(worktree_info: &WorktreeInfo, config: &DeletionConfig) -> Result<()> {
    let main_repo_path = get_main_repo_path(worktree_info)?;

    // Change to main repo if we're deleting current directory
    if config.is_current_directory {
        std::env::set_current_dir(&main_repo_path)
            .context("Failed to change to main repository")?;
    }

    execute_in_dir(&main_repo_path, || {
        // Remove or prune worktree
        remove_worktree(worktree_info, config)?;

        // Delete branch
        delete_branch(worktree_info, config)?;

        Ok(())
    })
}

/// Remove the worktree from git
fn remove_worktree(worktree_info: &WorktreeInfo, config: &DeletionConfig) -> Result<()> {
    if config.worktree_exists {
        println!("{} Removing worktree...", "🗑️ ".yellow());

        // First attempt: try normal removal
        let result = execute_git(&["worktree", "remove", worktree_info.path.to_str().unwrap()]);

        // If failed, might be due to submodules - try with force flag
        if result.is_err() {
            println!(
                "{} Standard removal failed, trying force removal...",
                "⚠️ ".yellow()
            );
            execute_git(&[
                "worktree",
                "remove",
                "--force",
                worktree_info.path.to_str().unwrap(),
            ])
            .context("Failed to force remove worktree")?;
        }
    } else {
        println!("{} Pruning non-existent worktree...", "🗑️ ".yellow());
        execute_git(&["worktree", "prune"]).context("Failed to prune worktree")?;
    }
    Ok(())
}

/// Delete the branch from git
fn delete_branch(worktree_info: &WorktreeInfo, config: &DeletionConfig) -> Result<()> {
    println!(
        "{} Deleting branch '{}'...",
        "🗑️ ".yellow(),
        worktree_info.branch
    );

    // First try safe delete
    if execute_git(&["branch", "-d", &worktree_info.branch]).is_ok() {
        println!("{} Branch deleted", "✅".green());
        return Ok(());
    }

    // Branch is not fully merged, ask for force delete
    if !config.is_interactive {
        println!("{} Branch kept (not fully merged)", "ℹ️ ".blue());
        return Ok(());
    }

    let force_delete = smart_confirm("Branch is not fully merged. Force delete?", false)?;

    if force_delete {
        execute_git(&["branch", "-D", &worktree_info.branch])
            .context("Failed to force delete branch")?;
        println!("{} Branch force deleted", "✅".green());
    } else {
        println!("{} Branch kept", "ℹ️ ".blue());
    }

    Ok(())
}

/// Get the path to the main repository from worktree info
fn get_main_repo_path(worktree_info: &WorktreeInfo) -> Result<std::path::PathBuf> {
    let parent = worktree_info
        .path
        .parent()
        .context("Failed to get parent directory")?;

    Ok(parent.join(&worktree_info.repo_name))
}
