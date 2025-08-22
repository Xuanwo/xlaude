use anyhow::{Context, Result};

use crate::input::{get_command_arg, smart_select};
use crate::state::{WorktreeInfo, XlaudeState};

pub fn handle_dir(name: Option<String>) -> Result<()> {
    let state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        anyhow::bail!("No worktrees found. Create one first with 'xlaude create'");
    }

    // Get name from CLI args or pipe
    let target_name = get_command_arg(name)?;

    // Determine which worktree to get path for
    let (_key, worktree_info) = if let Some(n) = target_name {
        // Find worktree by name across all projects
        state
            .worktrees
            .iter()
            .find(|(_, w)| w.name == n)
            .map(|(k, w)| (k.clone(), w.clone()))
            .context(format!("Worktree '{n}' not found"))?
    } else {
        // Interactive selection - show repo/name format
        let worktree_list: Vec<(String, WorktreeInfo)> = state
            .worktrees
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let selection = smart_select("Select a worktree", &worktree_list, |(_, info)| {
            format!("{}/{}", info.repo_name, info.name)
        })?;

        match selection {
            Some(idx) => worktree_list[idx].clone(),
            None => anyhow::bail!(
                "Interactive selection not available in non-interactive mode. Please specify a worktree name."
            ),
        }
    };

    // Output only the path - no decorations, no colors
    // This makes it easy to use in shell commands: cd $(xlaude dir name)
    println!("{}", worktree_info.path.display());

    Ok(())
}
