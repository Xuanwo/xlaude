use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use dialoguer::{Confirm, Select};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::git::{get_current_branch, get_repo_name, is_base_branch, is_in_worktree};
use crate::options::OpenOptions;
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::sanitize_branch_name;

fn launch_claude_with_typing(type_text: Option<String>) -> Result<()> {
    let claude_cmd = std::env::var("XLAUDE_CLAUDE_CMD").unwrap_or_else(|_| "claude".to_string());

    if let Some(text) = type_text {
        // Test mode: just print the text to stdout
        if claude_cmd == "true" {
            println!("[TEST MODE] Simulating prompt execution in Claude Code:");
            println!("{}", text);
            return Ok(());
        }

        // Launch Claude with stdin pipe for typing
        let mut cmd = Command::new(&claude_cmd);

        if claude_cmd == "claude" {
            cmd.arg("--dangerously-skip-permissions");
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .envs(std::env::vars());

        let mut child = cmd.spawn().context("Failed to launch Claude")?;

        // Wait a bit for Claude to start up
        thread::sleep(Duration::from_millis(500));

        // Send the text to Claude's stdin
        if let Some(mut stdin) = child.stdin.take() {
            // Write to stdin and handle pipe errors properly
            writeln!(stdin, "{}", text).context("Failed to write to Claude's stdin")?;
            // Close stdin to signal end of input
            drop(stdin);
        }

        let status = child.wait().context("Failed to wait for Claude")?;

        if !status.success() {
            anyhow::bail!("Claude exited with error");
        }
    } else {
        // Launch Claude normally without stdin pipe
        let mut cmd = Command::new(&claude_cmd);

        if claude_cmd == "claude" {
            cmd.arg("--dangerously-skip-permissions");
        }

        cmd.envs(std::env::vars());

        let status = cmd.status().context("Failed to launch Claude")?;

        if !status.success() {
            anyhow::bail!("Claude exited with error");
        }
    }

    Ok(())
}

pub fn handle_open(name: Option<String>, options: OpenOptions) -> Result<()> {
    let mut state = XlaudeState::load()?;

    // Get the text to type, either from CLI arg or stdin
    let type_text = options.get_type_text()?;

    // Check if current path is a worktree when no name is provided
    // Note: base branches (main/master/develop) are not considered worktrees
    if name.is_none() && is_in_worktree()? && !is_base_branch()? {
        // Get current repository info
        let repo_name = get_repo_name().context("Not in a git repository")?;
        let current_branch = get_current_branch()?;
        let current_dir = std::env::current_dir()?;

        // Sanitize branch name for key lookup
        let worktree_name = sanitize_branch_name(&current_branch);

        // Check if this worktree is already managed
        let key = XlaudeState::make_key(&repo_name, &worktree_name);

        if state.worktrees.contains_key(&key) {
            // Already managed, open directly
            println!(
                "{} Opening current worktree '{}/{}'...",
                "🚀".green(),
                repo_name,
                worktree_name.cyan()
            );
        } else {
            // Not managed, ask if user wants to add it
            println!(
                "{} Current directory is a worktree but not managed by xlaude",
                "ℹ️".blue()
            );
            println!(
                "  {} {}/{}",
                "Worktree:".bright_black(),
                repo_name,
                current_branch
            );
            println!("  {} {}", "Path:".bright_black(), current_dir.display());

            // In non-interactive mode, skip the prompt
            let should_add = if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
                false
            } else {
                Confirm::new()
                    .with_prompt("Would you like to add this worktree to xlaude and open it?")
                    .default(true)
                    .interact()?
            };

            if !should_add {
                return Ok(());
            }

            // Add to state
            println!(
                "{} Adding worktree '{}' to xlaude management...",
                "➕".green(),
                worktree_name.cyan()
            );

            state.worktrees.insert(
                key.clone(),
                WorktreeInfo {
                    name: worktree_name.clone(),
                    branch: current_branch.clone(),
                    path: current_dir.clone(),
                    repo_name: repo_name.clone(),
                    created_at: Utc::now(),
                },
            );
            state.save()?;

            println!("{} Worktree added successfully", "✅".green());
            println!(
                "{} Opening worktree '{}/{}'...",
                "🚀".green(),
                repo_name,
                worktree_name.cyan()
            );
        }

        // Launch Claude in current directory
        return launch_claude_with_typing(type_text);
    }

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

        // Check for non-interactive mode
        if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
            anyhow::bail!(
                "Interactive selection not available in non-interactive mode. Please specify a worktree name."
            );
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

    launch_claude_with_typing(type_text)
}
