use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::git;
use crate::state::XlaudeState;

pub fn handle_rename(old_name: String, new_name: String) -> Result<()> {
    let repo = git::get_repo_name()?;
    let mut state = XlaudeState::load()?;

    let old_key = XlaudeState::make_key(&repo, &old_name);
    let new_key = XlaudeState::make_key(&repo, &new_name);

    if !state.worktrees.contains_key(&old_key) {
        bail!("Worktree '{}' not found in repository '{}'", old_name, repo);
    }

    if state.worktrees.contains_key(&new_key) {
        bail!(
            "Worktree '{}' already exists in repository '{}'",
            new_name,
            repo
        );
    }

    let mut worktree_data = state
        .worktrees
        .remove(&old_key)
        .context("Failed to get worktree data")?;

    // Update the name field in the worktree info
    worktree_data.name = new_name.clone();

    state.worktrees.insert(new_key, worktree_data);
    state.save()?;

    println!(
        "{} {} {} {} {} {}",
        "✓".green(),
        "Renamed worktree".green(),
        old_name.cyan(),
        "to".green(),
        new_name.cyan(),
        format!("in repository '{}'", repo).dimmed()
    );

    Ok(())
}
