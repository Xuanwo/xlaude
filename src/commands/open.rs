use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Select;
use std::process::Command;

use crate::state::XlaudeState;

pub fn handle_open(name: Option<String>) -> Result<()> {
    let state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        anyhow::bail!("No worktrees found. Create one first with 'xlaude create'");
    }

    // Determine which worktree to open
    let worktree_name = if let Some(n) = name {
        // Verify the worktree exists
        if !state.worktrees.contains_key(&n) {
            anyhow::bail!("Worktree '{}' not found", n);
        }
        n
    } else {
        // Interactive selection
        let names: Vec<&String> = state.worktrees.keys().collect();
        let selection = Select::new()
            .with_prompt("Select a worktree to open")
            .items(&names)
            .interact()?;
        names[selection].clone()
    };

    let worktree_info = state
        .worktrees
        .get(&worktree_name)
        .context("Worktree not found")?;

    println!(
        "{} Opening worktree '{}'...",
        "🚀".green(),
        worktree_name.cyan()
    );

    // Change to worktree directory and launch Claude
    std::env::set_current_dir(&worktree_info.path).context("Failed to change directory")?;

    // Allow overriding claude command for testing
    let claude_cmd = std::env::var("XLAUDE_CLAUDE_CMD").unwrap_or_else(|_| "claude".to_string());
    let mut cmd = Command::new(&claude_cmd);

    // Only add the flag if we're using the real claude command
    if claude_cmd == "claude" {
        cmd.arg("--dangerously-skip-permissions");
    }

    // Inherit all environment variables
    cmd.envs(std::env::vars());

    let status = cmd.status().context("Failed to launch Claude")?;

    if !status.success() {
        anyhow::bail!("Claude exited with error");
    }

    Ok(())
}
