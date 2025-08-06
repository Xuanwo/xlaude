use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Confirm;

use crate::git::{execute_git, has_unpushed_commits, is_working_tree_clean};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::execute_in_dir;
use crate::vcs::{self, VcsType};

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
    vcs_type: VcsType,
}

impl DeletionConfig {
    fn from_env(worktree_info: &WorktreeInfo, vcs_type: VcsType) -> Result<Self> {
        let current_dir = std::env::current_dir()?;

        Ok(Self {
            is_interactive: std::env::var("XLAUDE_NON_INTERACTIVE").is_err(),
            worktree_exists: worktree_info.path.exists(),
            is_current_directory: current_dir == worktree_info.path,
            vcs_type,
        })
    }
}

pub fn handle_delete(name: Option<String>) -> Result<()> {
    let mut state = XlaudeState::load()?;

    // Detect VCS type
    let vcs_type = vcs::detect_vcs()?;

    let (key, worktree_info) = find_worktree_to_delete(&state, name)?;
    let config = DeletionConfig::from_env(&worktree_info, vcs_type)?;

    let workspace_type = match config.vcs_type {
        VcsType::Git => "worktree",
        VcsType::Jj => "workspace",
    };

    println!(
        "{} Checking {} '{}'...",
        "🔍".yellow(),
        workspace_type,
        worktree_info.name.cyan()
    );

    // Handle case where worktree directory doesn't exist
    if !config.worktree_exists {
        if !handle_missing_worktree(&worktree_info, &config, workspace_type)? {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    } else {
        // For Git, check branch status first (for output consistency)
        if config.vcs_type == VcsType::Git {
            println!(
                "{} Checking branch '{}'...",
                "🔍".yellow(),
                worktree_info.branch
            );
        }

        // Perform deletion checks
        let checks = perform_deletion_checks(&worktree_info, &config)?;

        if !confirm_deletion(&worktree_info, &checks, &config, workspace_type)? {
            println!("{} Cancelled", "❌".red());
            return Ok(());
        }
    }

    // Execute deletion
    perform_deletion(&worktree_info, &config, workspace_type)?;

    // Update state
    state.worktrees.remove(&key);
    state.save()?;

    println!(
        "{} {} '{}' deleted successfully",
        "✅".green(),
        workspace_type
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + &workspace_type[1..],
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
fn handle_missing_worktree(
    worktree_info: &WorktreeInfo,
    config: &DeletionConfig,
    workspace_type: &str,
) -> Result<bool> {
    println!(
        "{} {} directory not found at {}",
        "⚠️ ".yellow(),
        workspace_type
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + &workspace_type[1..],
        worktree_info.path.display()
    );
    println!(
        "  {} The {} may have been manually deleted",
        "ℹ️".blue(),
        workspace_type
    );

    if config.is_interactive {
        Confirm::new()
            .with_prompt(format!(
                "Remove this {} from xlaude management?",
                workspace_type
            ))
            .default(true)
            .interact()
            .context("Failed to get user confirmation")
    } else {
        Ok(true)
    }
}

/// Perform all checks needed before deletion
fn perform_deletion_checks(
    worktree_info: &WorktreeInfo,
    config: &DeletionConfig,
) -> Result<DeletionChecks> {
    execute_in_dir(&worktree_info.path, || {
        let has_uncommitted_changes = match config.vcs_type {
            VcsType::Git => !is_working_tree_clean()?,
            VcsType::Jj => !vcs::is_working_tree_clean(&config.vcs_type)?,
        };

        let has_unpushed_commits = match config.vcs_type {
            VcsType::Git => has_unpushed_commits(),
            VcsType::Jj => vcs::has_unpushed_changes(&config.vcs_type)?,
        };

        // Branch merge checks only apply to Git
        let (branch_merged_via_git, branch_merged_via_pr) = if config.vcs_type == VcsType::Git {
            let main_repo_path = get_main_repo_path(worktree_info)?;
            check_branch_merge_status(&main_repo_path, &worktree_info.branch)?
        } else {
            (false, false)
        };

        Ok(DeletionChecks {
            has_uncommitted_changes,
            has_unpushed_commits,
            branch_merged_via_git,
            branch_merged_via_pr,
        })
    })
}

/// Check if branch is merged via git or PR (Git only)
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
    config: &DeletionConfig,
    workspace_type: &str,
) -> Result<bool> {
    // Show warnings for pending work
    if checks.has_pending_work() {
        show_pending_work_warnings(checks, config.vcs_type);

        if !config.is_interactive {
            return Ok(false); // Don't delete in non-interactive mode with pending work
        }

        return Confirm::new()
            .with_prompt(format!(
                "Are you sure you want to delete this {workspace_type}?"
            ))
            .default(false)
            .interact()
            .context("Failed to get user confirmation");
    }

    // Show branch merge status (Git only)
    if config.vcs_type == VcsType::Git {
        if !checks.branch_is_merged() {
            show_unmerged_branch_warning(worktree_info);
        } else if checks.branch_merged_via_pr && !checks.branch_merged_via_git {
            println!("  {} Branch was merged via PR", "ℹ️".blue());
        }
    }

    // Ask for confirmation in interactive mode
    if config.is_interactive {
        Confirm::new()
            .with_prompt(format!(
                "Delete {} '{}'?",
                workspace_type, worktree_info.name
            ))
            .default(true)
            .interact()
            .context("Failed to get user confirmation")
    } else {
        Ok(!checks.has_pending_work())
    }
}

/// Show warnings for uncommitted changes or unpushed commits
fn show_pending_work_warnings(checks: &DeletionChecks, vcs_type: VcsType) {
    println!();
    if checks.has_uncommitted_changes {
        println!("{} You have uncommitted changes", "⚠️ ".red());
    }
    if checks.has_unpushed_commits {
        let unpushed_text = match vcs_type {
            VcsType::Git => "unpushed commits",
            VcsType::Jj => "unpushed changes",
        };
        println!("{} You have {}", "⚠️ ".red(), unpushed_text);
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
fn perform_deletion(
    worktree_info: &WorktreeInfo,
    config: &DeletionConfig,
    workspace_type: &str,
) -> Result<()> {
    match config.vcs_type {
        VcsType::Git => perform_git_deletion(worktree_info, config),
        VcsType::Jj => perform_jj_deletion(worktree_info, config, workspace_type),
    }
}

/// Perform Git-specific deletion
fn perform_git_deletion(worktree_info: &WorktreeInfo, config: &DeletionConfig) -> Result<()> {
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

/// Perform jj-specific deletion
fn perform_jj_deletion(
    worktree_info: &WorktreeInfo,
    config: &DeletionConfig,
    workspace_type: &str,
) -> Result<()> {
    // For jj, we need to change to a different directory if deleting current
    if config.is_current_directory {
        let main_repo_path = get_main_repo_path(worktree_info)?;
        std::env::set_current_dir(&main_repo_path)
            .context("Failed to change to main repository")?;
    }

    println!("{} Removing {}...", "🗑️ ".yellow(), workspace_type);
    vcs::remove_worktree_or_workspace(&config.vcs_type, &worktree_info.name, &worktree_info.path)?;
    Ok(())
}

/// Remove the worktree from git
fn remove_worktree(worktree_info: &WorktreeInfo, config: &DeletionConfig) -> Result<()> {
    if config.worktree_exists {
        println!("{} Removing worktree...", "🗑️ ".yellow());
        execute_git(&["worktree", "remove", worktree_info.path.to_str().unwrap()])
            .context("Failed to remove worktree")?;
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

    let force_delete = Confirm::new()
        .with_prompt("Branch is not fully merged. Force delete?")
        .default(false)
        .interact()?;

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
