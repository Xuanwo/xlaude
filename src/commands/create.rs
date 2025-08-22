use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use dialoguer::Confirm;
use std::fs;
use std::path::Path;

use crate::commands::open::handle_open;
use crate::git::{execute_git, get_repo_name, is_base_branch, update_submodules};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::{generate_random_name, sanitize_branch_name};

pub fn handle_create(name: Option<String>) -> Result<()> {
    // Check if we're in a git repository
    let repo_name = get_repo_name().context("Not in a git repository")?;

    // Check if we're on a base branch
    if !is_base_branch()? {
        anyhow::bail!(
            "Must be on a base branch (main, master, or develop) to create a new worktree"
        );
    }

    // Generate name if not provided
    let branch_name = match name {
        Some(n) => n,
        None => generate_random_name()?,
    };

    // Sanitize the branch name for use in directory names
    let worktree_name = sanitize_branch_name(&branch_name);

    println!(
        "{} Creating worktree '{}' with branch '{}'...",
        "✨".green(),
        worktree_name.cyan(),
        branch_name.cyan()
    );

    // Create branch
    execute_git(&["branch", &branch_name]).context("Failed to create branch")?;

    // Create worktree with sanitized directory name
    let worktree_dir = format!("../{repo_name}-{worktree_name}");
    execute_git(&["worktree", "add", &worktree_dir, &branch_name])
        .context("Failed to create worktree")?;

    // Get absolute path
    let worktree_path = std::env::current_dir()?
        .parent()
        .unwrap()
        .join(format!("{repo_name}-{worktree_name}"));

    // Update submodules if they exist
    if let Err(e) = update_submodules(&worktree_path) {
        println!(
            "{} Warning: Failed to update submodules: {}",
            "⚠️".yellow(),
            e
        );
    } else {
        // Check if submodules were actually updated
        let gitmodules = worktree_path.join(".gitmodules");
        if gitmodules.exists() {
            println!("{} Updated submodules", "📦".green());
        }
    }

    // Copy CLAUDE.local.md if it exists
    let claude_local_md = Path::new("CLAUDE.local.md");
    if claude_local_md.exists() {
        let target_path = worktree_path.join("CLAUDE.local.md");
        fs::copy(claude_local_md, &target_path).context("Failed to copy CLAUDE.local.md")?;
        println!("{} Copied CLAUDE.local.md to worktree", "📄".green());
    }

    // Save state
    let mut state = XlaudeState::load()?;
    let key = XlaudeState::make_key(&repo_name, &worktree_name);
    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: worktree_name.clone(),
            branch: branch_name.clone(),
            path: worktree_path.clone(),
            repo_name,
            created_at: Utc::now(),
        },
    );
    state.save()?;

    println!(
        "{} Worktree created at: {}",
        "✅".green(),
        worktree_path.display()
    );

    // Ask if user wants to open the worktree
    // Check for non-interactive mode
    let should_open = if std::env::var("XLAUDE_NON_INTERACTIVE").is_ok() {
        println!(
            "  {} To open it, run: {} {}",
            "💡".cyan(),
            "xlaude open".cyan(),
            worktree_name.cyan()
        );
        false
    } else {
        Confirm::new()
            .with_prompt("Would you like to open the worktree now?")
            .default(true)
            .interact()?
    };

    if should_open {
        handle_open(Some(worktree_name.clone()))?;
    } else if std::env::var("XLAUDE_NON_INTERACTIVE").is_err() {
        println!(
            "  {} To open it later, run: {} {}",
            "💡".cyan(),
            "xlaude open".cyan(),
            worktree_name.cyan()
        );
    }

    Ok(())
}
