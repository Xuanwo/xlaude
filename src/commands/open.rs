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
    let (_key, worktree_info) = if let Some(n) = name {
        // Find worktree by name across all projects
        state
            .worktrees
            .iter()
            .find(|(_, w)| w.name == n)
            .map(|(k, w)| (k.clone(), w.clone()))
            .context(format!("Worktree '{n}' not found"))?
    } else {
        // Interactive selection - show repo/name format
        let mut display_names: Vec<String> = Vec::new();
        let mut keys: Vec<String> = Vec::new();

        for (key, info) in &state.worktrees {
            display_names.push(format!("{}/{}", info.repo_name, info.name));
            keys.push(key.clone());
        }

        let selection = Select::new()
            .with_prompt("Select a worktree to open")
            .items(&display_names)
            .interact()?;

        let selected_key = keys[selection].clone();
        let selected_info = state.worktrees.get(&selected_key).unwrap().clone();
        (selected_key, selected_info)
    };

    let worktree_name = &worktree_info.name;

    println!(
        "{} Opening worktree '{}/{}'...",
        "🚀".green(),
        worktree_info.repo_name,
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
