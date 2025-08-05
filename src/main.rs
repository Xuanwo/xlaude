use anyhow::Result;
use clap::{Parser, Subcommand};

mod claude;
mod commands;
mod git;
mod state;
mod utils;

use commands::{handle_add, handle_clean, handle_create, handle_delete, handle_list, handle_open};

#[derive(Parser)]
#[command(name = "xlaude")]
#[command(about = "Manage Claude instances with git worktrees", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new git worktree
    Create {
        /// Name for the worktree (random BIP39 word if not provided)
        name: Option<String>,
    },
    /// Open an existing worktree and launch Claude
    Open {
        /// Name of the worktree to open (interactive selection if not provided)
        name: Option<String>,
    },
    /// Delete a worktree and clean up
    Delete {
        /// Name of the worktree to delete (current if not provided)
        name: Option<String>,
    },
    /// Add current worktree to xlaude management
    Add {
        /// Name for the worktree (defaults to current branch name)
        name: Option<String>,
    },
    /// List all active Claude instances
    List,
    /// Clean up invalid worktrees from state
    Clean,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Create { name } => handle_create(name),
        Commands::Open { name } => handle_open(name),
        Commands::Delete { name } => handle_delete(name),
        Commands::Add { name } => handle_add(name),
        Commands::List => handle_list(),
        Commands::Clean => handle_clean(),
    }
}
