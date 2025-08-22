use anyhow::{Context, Result};
use dialoguer::Select;

use crate::state::XlaudeState;

pub fn handle_dir(name: Option<String>) -> Result<()> {
    let state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        anyhow::bail!("No worktrees found. Create one first with 'xlaude create'");
    }

    // Determine which worktree to get path for
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
            .with_prompt("Select a worktree")
            .items(&display_names)
            .interact()?;

        let selected_key = keys[selection].clone();
        let selected_info = state.worktrees.get(&selected_key).unwrap().clone();
        (selected_key, selected_info)
    };

    // Output only the path - no decorations, no colors
    // This makes it easy to use in shell commands: cd $(xlaude dir name)
    println!("{}", worktree_info.path.display());

    Ok(())
}