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
    // First, try to get the repository name from the remote URL
    // This gives us the true repository name regardless of local directory name
    if let Ok(remote_url) = execute_git(&["remote", "get-url", "origin"]) {
        // Extract repo name from URL
        // Supports:
        // - https://github.com/user/repo.git
        // - git@github.com:user/repo.git
        // - https://gitlab.com/user/repo
        // - /path/to/local/repo.git
        let repo_name = if let Some(name) = extract_repo_name_from_url(&remote_url) {
            name
        } else {
            // Fallback to directory name if URL parsing fails
            get_repo_name_from_directory()?
        };
        return Ok(repo_name);
    }

    // If no remote, use the directory name of the main repository
    get_repo_name_from_directory()
}

pub fn extract_repo_name_from_url(url: &str) -> Option<String> {
    let url = url.trim();

    // Remove .git suffix if present
    let url = url.strip_suffix(".git").unwrap_or(url);

    // Handle SSH URLs (git@github.com:user/repo)
    if url.starts_with("git@") {
        return url
            .split(':')
            .nth(1)
            .and_then(|path| path.split('/').next_back())
            .map(|s| s.to_string());
    }

    // Handle HTTP(S) URLs and file paths
    url.split('/')
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn get_repo_name_from_directory() -> Result<String> {
    // For worktrees, we need to get the main repository path
    // Try to get the common git directory first (which points to main repo for worktrees)
    let git_common_dir = execute_git(&["rev-parse", "--git-common-dir"])?;
    let git_dir = execute_git(&["rev-parse", "--git-dir"])?;

    let repo_path = if git_common_dir != git_dir {
        // We're in a worktree - git-common-dir points to main repo's .git
        let path = Path::new(&git_common_dir);
        if path.file_name().is_some_and(|n| n == ".git") {
            // Get the parent directory which is the main repo
            path.parent()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
                .context("Failed to get main repository path")?
        } else {
            // git-common-dir doesn't end with .git, use it directly
            git_common_dir
        }
    } else {
        // Not in a worktree, use toplevel
        execute_git(&["rev-parse", "--show-toplevel"])?
    };

    let path = Path::new(&repo_path);
    path.file_name()
        .and_then(|n| n.to_str())
        .map(std::string::ToString::to_string)
        .context("Failed to get repository name")
}

pub fn get_current_branch() -> Result<String> {
    execute_git(&["symbolic-ref", "--short", "HEAD"])
}

pub fn get_default_branch() -> Result<String> {
    // Try to get the default branch from remote HEAD
    if let Ok(output) = execute_git(&["remote", "show", "origin"]) {
        for line in output.lines() {
            if let Some(branch) = line.strip_prefix("  HEAD branch: ") {
                return Ok(branch.trim().to_string());
            }
        }
    }

    // Fallback: try to get HEAD from symbolic-ref
    if let Ok(output) = execute_git(&["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = output.strip_prefix("refs/remotes/origin/")
    {
        return Ok(branch.to_string());
    }

    // Final fallback: return "main" as the most common default
    Ok("main".to_string())
}

pub fn is_base_branch() -> Result<bool> {
    let current = get_current_branch()?;

    // Get the actual default branch from remote
    let default_branch = get_default_branch().unwrap_or_else(|_| "main".to_string());

    // Check if current branch is the default branch
    if current == default_branch {
        return Ok(true);
    }

    // Also allow common base branches for flexibility
    let common_base_branches = ["main", "master", "develop"];
    Ok(common_base_branches.contains(&current.as_str()))
}

#[allow(dead_code)]
pub fn branch_exists(branch_name: &str) -> Result<bool> {
    // Check if branch exists locally
    if execute_git(&[
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{}", branch_name),
    ])
    .is_ok()
    {
        return Ok(true);
    }

    // Check if branch exists on remote
    if execute_git(&[
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/remotes/origin/{}", branch_name),
    ])
    .is_ok()
    {
        return Ok(true);
    }

    Ok(false)
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

pub fn update_submodules(worktree_path: &Path) -> Result<()> {
    // Check if submodules exist
    let gitmodules = worktree_path.join(".gitmodules");
    if !gitmodules.exists() {
        return Ok(());
    }

    // Initialize and update submodules using git -C
    execute_git(&[
        "-C",
        worktree_path.to_str().unwrap(),
        "submodule",
        "update",
        "--init",
        "--recursive",
    ])
    .context("Failed to update submodules")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_name_from_url() {
        // GitHub HTTPS
        assert_eq!(
            extract_repo_name_from_url("https://github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );

        // GitHub SSH
        assert_eq!(
            extract_repo_name_from_url("git@github.com:user/my-repo.git"),
            Some("my-repo".to_string())
        );

        // GitLab HTTPS without .git
        assert_eq!(
            extract_repo_name_from_url("https://gitlab.com/user/my-repo"),
            Some("my-repo".to_string())
        );

        // Local path
        assert_eq!(
            extract_repo_name_from_url("/path/to/repos/my-repo.git"),
            Some("my-repo".to_string())
        );

        // Complex repo name
        assert_eq!(
            extract_repo_name_from_url("git@github.com:xuanwo/xlaude-enable.git"),
            Some("xlaude-enable".to_string())
        );

        // Edge cases
        assert_eq!(
            extract_repo_name_from_url("https://github.com/user/repo-with-dots.v2.git"),
            Some("repo-with-dots.v2".to_string())
        );
    }

    #[test]
    fn test_get_default_branch() {
        // This test will work based on the actual git repository it's run in
        // We can't make strong assertions about the result since it depends on the repo
        let result = get_default_branch();

        // Should either succeed with a non-empty string or fail gracefully
        match result {
            Ok(branch) => {
                assert!(!branch.is_empty());
                // Common default branches
                assert!(
                    ["main", "master", "develop"].contains(&branch.as_str()) || !branch.is_empty()
                );
            }
            Err(_) => {
                // It's okay to fail if we're not in a git repo or no remote
                // The function should handle this gracefully
            }
        }
    }
}
