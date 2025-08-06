use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn execute_jj(args: &[&str]) -> Result<String> {
    let output = Command::new("jj")
        .args(args)
        .output()
        .context("Failed to execute jj command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj command failed: {}", stderr);
    }
}

pub fn get_repo_name() -> Result<String> {
    let root = execute_jj(&["workspace", "root"])?;
    let workspace_root = Path::new(&root);
    let current_workspace = get_current_workspace_name()?;

    if current_workspace == "default" {
        // In default workspace, the workspace root is the repo root
        workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(std::string::ToString::to_string)
            .context("Failed to get repository name")
    } else {
        // In a named workspace, extract repo name from directory name
        let workspace_dir_name = workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .context("Failed to get workspace directory name")?;

        // Assuming format: repo-workspace
        if let Some(dash_pos) = workspace_dir_name.rfind('-') {
            Ok(workspace_dir_name[..dash_pos].to_string())
        } else {
            // Fallback: use the whole name
            Ok(workspace_dir_name.to_string())
        }
    }
}

pub fn get_current_workspace_name() -> Result<String> {
    let workspaces = execute_jj(&["workspace", "list"])?;

    // Parse workspace list to find the current one (marked with @)
    for line in workspaces.lines() {
        if line.contains('@')
            && let Some(name) = line.split(':').next()
        {
            return Ok(name.trim().to_string());
        }
    }

    // If no @ found, we're likely in default workspace
    Ok("default".to_string())
}

pub fn is_working_copy_clean() -> Result<bool> {
    let status = execute_jj(&["st"])?;
    Ok(status.contains("The working copy has no changes"))
}

pub fn has_unpushed_changes() -> Result<bool> {
    // Check if current revision is ahead of trunk
    let log = execute_jj(&["log", "--no-graph", "-r", "trunk()..@"])?;
    Ok(!log.trim().is_empty())
}

pub fn is_in_workspace() -> Result<bool> {
    // Check if we're in a jj workspace by looking for .jj directory
    let jj_path = Path::new(".jj");
    Ok(jj_path.exists() && jj_path.is_dir())
}

pub fn list_workspaces() -> Result<Vec<(String, PathBuf)>> {
    let output = execute_jj(&["workspace", "list"])?;
    let mut workspaces = Vec::new();

    // Get the workspace root to figure out repo structure
    let workspace_root = execute_jj(&["workspace", "root"])?;
    let current_root = Path::new(&workspace_root);

    // Find the actual repository root
    // If we're in default workspace, current_root is the repo root
    // If we're in a named workspace, parent is where all workspaces live
    let current_workspace_name = get_current_workspace_name()?;
    let (repo_root, repo_name) = if current_workspace_name == "default" {
        let repo_name = current_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .context("Failed to get repo name")?;
        (current_root.to_path_buf(), repo_name)
    } else {
        // We're in a named workspace, parent contains all workspaces
        let parent = current_root
            .parent()
            .context("Failed to get parent directory")?;
        // Extract repo name from workspace directory name (repo-workspace format)
        let workspace_dir_name = current_root
            .file_name()
            .and_then(|n| n.to_str())
            .context("Failed to get workspace directory name")?;

        // Find repo name by removing the workspace suffix
        let repo_name = if let Some(dash_pos) = workspace_dir_name.rfind('-') {
            workspace_dir_name[..dash_pos].to_string()
        } else {
            workspace_dir_name.to_string()
        };

        (parent.join(&repo_name), repo_name)
    };

    for line in output.lines() {
        if let Some(name) = line.split(':').next() {
            let name = name.trim().to_string();
            let workspace_path = if name == "default" {
                repo_root.clone()
            } else {
                repo_root
                    .parent()
                    .unwrap_or(&repo_root)
                    .join(format!("{repo_name}-{name}"))
            };
            workspaces.push((name, workspace_path));
        }
    }

    Ok(workspaces)
}

pub fn create_workspace(name: &str, destination: &Path) -> Result<()> {
    execute_jj(&[
        "workspace",
        "add",
        destination.to_str().unwrap(),
        "--name",
        name,
    ])?;
    Ok(())
}

pub fn forget_workspace(name: &str) -> Result<()> {
    execute_jj(&["workspace", "forget", name])?;
    Ok(())
}

pub fn is_on_trunk() -> Result<bool> {
    // Check if we're on trunk (main/master/develop equivalent in jj)
    let current = execute_jj(&["log", "--no-graph", "-r", "@", "--template", "commit_id"])?;
    let trunk = execute_jj(&[
        "log",
        "--no-graph",
        "-r",
        "trunk()",
        "--template",
        "commit_id",
    ])?;
    Ok(current == trunk)
}
