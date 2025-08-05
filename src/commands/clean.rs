use anyhow::Result;
use colored::Colorize;

use crate::git::list_worktrees;
use crate::state::XlaudeState;

pub fn handle_clean() -> Result<()> {
    let mut state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        println!("{} No worktrees in state", "✨".green());
        return Ok(());
    }

    println!("{} Checking for invalid worktrees...", "🔍".cyan());

    // Get list of actual worktrees from git
    let actual_worktrees = list_worktrees()?;

    // Find worktrees in state that no longer exist
    let mut removed_count = 0;
    let mut worktrees_to_remove = Vec::new();

    for (name, info) in &state.worktrees {
        if !actual_worktrees.contains(&info.path) {
            println!(
                "  {} Found invalid worktree: {} ({})",
                "❌".red(),
                name.yellow(),
                info.path.display()
            );
            worktrees_to_remove.push(name.clone());
            removed_count += 1;
        }
    }

    // Remove invalid worktrees from state
    for name in worktrees_to_remove {
        state.worktrees.remove(&name);
    }

    if removed_count > 0 {
        state.save()?;
        println!(
            "{} Removed {} invalid worktree{}",
            "✅".green(),
            removed_count,
            if removed_count == 1 { "" } else { "s" }
        );
    } else {
        println!("{} All worktrees are valid", "✨".green());
    }

    Ok(())
}
