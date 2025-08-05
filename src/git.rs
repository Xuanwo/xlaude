use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn execute_git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("Failed to execute git command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Git command failed: {}", stderr);
    }
}

pub fn get_repo_name() -> Result<String> {
    let toplevel = execute_git(&["rev-parse", "--show-toplevel"])?;
    let path = Path::new(&toplevel);
    path.file_name()
        .and_then(|n| n.to_str())
        .map(std::string::ToString::to_string)
        .context("Failed to get repository name")
}

pub fn get_current_branch() -> Result<String> {
    execute_git(&["symbolic-ref", "--short", "HEAD"])
}

pub fn is_base_branch() -> Result<bool> {
    let current = get_current_branch()?;
    let base_branches = ["main", "master", "develop"];
    Ok(base_branches.contains(&current.as_str()))
}

pub fn is_working_tree_clean() -> Result<bool> {
    let status = execute_git(&["status", "--porcelain"])?;
    Ok(status.is_empty())
}

pub fn has_unpushed_commits() -> bool {
    execute_git(&["log", "@{u}.."]).is_ok_and(|output| !output.is_empty())
}

pub fn is_in_worktree() -> Result<bool> {
    // Check if we're in a worktree by looking for .git file (not directory)
    let git_path = Path::new(".git");
    if git_path.exists() && git_path.is_file() {
        return Ok(true);
    }

    // Alternative: check git worktree list
    match execute_git(&["rev-parse", "--git-common-dir"]) {
        Ok(common_dir) => {
            let current_git_dir = execute_git(&["rev-parse", "--git-dir"])?;
            Ok(common_dir != current_git_dir)
        }
        Err(_) => Ok(false),
    }
}

pub fn list_worktrees() -> Result<Vec<PathBuf>> {
    let output = execute_git(&["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktrees.push(PathBuf::from(path));
        }
    }

    Ok(worktrees)
}
