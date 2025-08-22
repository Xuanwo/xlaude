use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::Shell;

mod claude;
mod commands;
mod completions;
mod git;
mod input;
mod state;
mod utils;

use commands::{
    handle_add, handle_clean, handle_create, handle_delete, handle_dir, handle_list, handle_open,
    handle_rename,
};

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
    /// Rename a worktree
    Rename {
        /// Current name of the worktree
        old_name: String,
        /// New name for the worktree
        new_name: String,
    },
    /// List all active Claude instances
    List,
    /// Clean up invalid worktrees from state
    Clean,
    /// Get the directory path of a worktree
    Dir {
        /// Name of the worktree (interactive selection if not provided)
        name: Option<String>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Output worktree info for shell completions (hidden)
    #[command(hide = true)]
    CompleteWorktrees {
        /// Output format: simple or detailed
        #[arg(long, default_value = "simple")]
        format: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Create { name } => handle_create(name),
        Commands::Open { name } => handle_open(name),
        Commands::Delete { name } => handle_delete(name),
        Commands::Add { name } => handle_add(name),
        Commands::Rename { old_name, new_name } => handle_rename(old_name, new_name),
        Commands::List => handle_list(),
        Commands::Clean => handle_clean(),
        Commands::Dir { name } => handle_dir(name),
        Commands::Completions { shell } => completions::handle_completions(shell),
        Commands::CompleteWorktrees { format } => commands::handle_complete_worktrees(&format),
    }
}
