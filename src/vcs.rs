use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::git;
use crate::jj;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsType {
    Git,
    Jj,
}

pub fn detect_vcs() -> Result<VcsType> {
    // Try jj first - use jj root to check if we're in a jj repo
    // This works from any subdirectory within the jj repository
    if let Ok(output) = std::process::Command::new("jj").args(["root"]).output()
        && output.status.success()
    {
        return Ok(VcsType::Jj);
    }

    // Try git - check if we're in a git repo or worktree
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        && output.status.success()
    {
        return Ok(VcsType::Git);
    }

    anyhow::bail!("No version control system detected. Please run from a git or jj repository.")
}

pub fn get_repo_name(vcs: &VcsType) -> Result<String> {
    match vcs {
        VcsType::Git => git::get_repo_name(),
        VcsType::Jj => jj::get_repo_name(),
    }
}

pub fn get_current_branch_or_workspace(vcs: &VcsType) -> Result<String> {
    match vcs {
        VcsType::Git => git::get_current_branch(),
        VcsType::Jj => jj::get_current_workspace_name(),
    }
}

pub fn is_on_base_branch(vcs: &VcsType) -> Result<bool> {
    match vcs {
        VcsType::Git => git::is_base_branch(),
        VcsType::Jj => jj::is_on_trunk(),
    }
}

pub fn is_working_tree_clean(vcs: &VcsType) -> Result<bool> {
    match vcs {
        VcsType::Git => git::is_working_tree_clean(),
        VcsType::Jj => jj::is_working_copy_clean(),
    }
}

pub fn has_unpushed_changes(vcs: &VcsType) -> Result<bool> {
    match vcs {
        VcsType::Git => Ok(git::has_unpushed_commits()),
        VcsType::Jj => jj::has_unpushed_changes(),
    }
}

pub fn is_in_worktree_or_workspace(vcs: &VcsType) -> Result<bool> {
    match vcs {
        VcsType::Git => git::is_in_worktree(),
        VcsType::Jj => jj::is_in_workspace(),
    }
}

pub enum WorkspaceInfo {
    Git(PathBuf), // Path for git worktrees
    Jj(PathBuf),  // Path for jj workspaces
}

pub fn list_worktrees_or_workspaces(vcs: &VcsType) -> Result<Vec<WorkspaceInfo>> {
    match vcs {
        VcsType::Git => {
            let worktrees = git::list_worktrees()?;
            Ok(worktrees.into_iter().map(WorkspaceInfo::Git).collect())
        }
        VcsType::Jj => {
            let workspaces = jj::list_workspaces()?;
            Ok(workspaces
                .into_iter()
                .map(|(_, path)| WorkspaceInfo::Jj(path))
                .collect())
        }
    }
}

pub fn create_worktree_or_workspace(vcs: &VcsType, name: &str, destination: &Path) -> Result<()> {
    match vcs {
        VcsType::Git => {
            // For git, create branch and worktree
            git::execute_git(&["branch", name])?;
            git::execute_git(&["worktree", "add", destination.to_str().unwrap(), name])?;
            Ok(())
        }
        VcsType::Jj => jj::create_workspace(name, destination),
    }
}

pub fn remove_worktree_or_workspace(vcs: &VcsType, name: &str, path: &Path) -> Result<()> {
    match vcs {
        VcsType::Git => {
            git::execute_git(&["worktree", "remove", path.to_str().unwrap()])?;
            // Try to delete the branch (may fail if it has unpushed commits)
            let _ = git::execute_git(&["branch", "-d", name]);
            Ok(())
        }
        VcsType::Jj => {
            jj::forget_workspace(name)?;
            // Remove the directory
            if path.exists() {
                std::fs::remove_dir_all(path)?;
            }
            Ok(())
        }
    }
}
