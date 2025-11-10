use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use colored::Colorize;

use crate::commands::open::handle_open;
use crate::git::{execute_git, get_repo_name, update_submodules};
use crate::input::{get_command_arg, smart_confirm};
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::sanitize_branch_name;

pub fn handle_checkout(target: Option<String>) -> Result<()> {
    let raw_target = get_command_arg(target)?
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .context("Please provide a branch name or pull request number")?;

    let checkout_target = CheckoutTarget::parse(&raw_target)?;
    let repo_root_str = execute_git(&["rev-parse", "--show-toplevel"])?
        .trim()
        .to_string();
    let repo_root = PathBuf::from(&repo_root_str);
    let repo_name = get_repo_name().context("Not in a git repository")?;

    let branch_name = checkout_target.branch_name();
    let worktree_name = sanitize_branch_name(&branch_name);

    if let Some(existing) = find_existing_worktree(&repo_name, &branch_name)? {
        println!(
            "{} Worktree for {} already exists at {}",
            "⚠️".yellow(),
            checkout_target.describe().cyan(),
            existing.path.display()
        );
        println!(
            "  {} To open it manually, run: {} {}",
            "💡".cyan(),
            "xlaude open".cyan(),
            existing.name.cyan()
        );

        let should_open = smart_confirm(
            "Worktree already exists. Open it now with 'xlaude open'?",
            false,
        )?;

        if should_open {
            handle_open(Some(existing.name.clone()))?;
            return Ok(());
        }

        bail!(
            "Worktree '{}' already exists for {}",
            existing.name,
            checkout_target.describe()
        );
    }

    ensure_branch_ready(&checkout_target, &branch_name)?;

    println!(
        "{} Checking out {} into worktree '{}'...",
        "✨".green(),
        checkout_target.describe().cyan(),
        worktree_name.cyan()
    );

    let created_path = create_worktree(&repo_root, &repo_name, &branch_name, &worktree_name)?;

    println!(
        "{} Worktree created at: {}",
        "✅".green(),
        created_path.display()
    );
    println!(
        "  {} To open it later, run: {} {}",
        "💡".cyan(),
        "xlaude open".cyan(),
        worktree_name.cyan()
    );

    Ok(())
}

fn find_existing_worktree(repo_name: &str, branch_name: &str) -> Result<Option<ExistingWorktree>> {
    let state = XlaudeState::load()?;
    Ok(state
        .worktrees
        .values()
        .find(|w| w.repo_name == repo_name && w.branch == branch_name)
        .cloned()
        .map(ExistingWorktree))
}

fn ensure_branch_ready(target: &CheckoutTarget, branch_name: &str) -> Result<()> {
    match target {
        CheckoutTarget::Branch(_) => ensure_branch_available(branch_name),
        CheckoutTarget::PullRequest(pr_number) => fetch_pull_request(*pr_number, branch_name),
    }
}

fn ensure_branch_available(branch_name: &str) -> Result<()> {
    if branch_exists(branch_name) {
        return Ok(());
    }

    println!(
        "{} Branch '{}' not found locally. Attempting to fetch from origin...",
        "🌐".blue(),
        branch_name.cyan()
    );

    ensure_origin_remote()?;
    let fetch_spec = format!("{branch_name}:{branch_name}");
    execute_git(&["fetch", "origin", &fetch_spec])
        .with_context(|| format!("Failed to fetch branch '{branch_name}' from origin"))?;

    if branch_exists(branch_name) {
        Ok(())
    } else {
        bail!("Branch '{branch_name}' does not exist locally or on origin");
    }
}

fn fetch_pull_request(pr_number: u64, branch_name: &str) -> Result<()> {
    ensure_origin_remote()?;
    println!(
        "{} Fetching pull request #{} from origin...",
        "🌐".blue(),
        pr_number
    );

    let fetch_ref = format!("pull/{pr_number}/head:refs/heads/{branch_name}");
    execute_git(&["fetch", "origin", &fetch_ref])
        .with_context(|| format!("Failed to fetch pull request #{pr_number} from origin"))?;

    Ok(())
}

fn ensure_origin_remote() -> Result<()> {
    execute_git(&["remote", "get-url", "origin"])
        .context("Remote 'origin' is not configured. Please add a remote before using checkout.")?;
    Ok(())
}

fn branch_exists(branch_name: &str) -> bool {
    execute_git(&["show-ref", "--verify", &format!("refs/heads/{branch_name}")]).is_ok()
}

fn create_worktree(
    repo_root: &Path,
    repo_name: &str,
    branch_name: &str,
    worktree_name: &str,
) -> Result<PathBuf> {
    let repo_root_str = repo_root
        .to_str()
        .context("Repository path contains invalid UTF-8")?;

    let worktree_parent = repo_root
        .parent()
        .context("Repository root has no parent directory for worktrees")?;
    let worktree_path = worktree_parent.join(format!("{repo_name}-{worktree_name}"));

    if worktree_path.exists() {
        bail!(
            "Directory '{}' already exists. Please remove it or choose another branch.",
            worktree_path.display()
        );
    }

    let existing_worktrees = list_worktrees_for_repo(repo_root)?;
    if existing_worktrees.iter().any(|w| w == &worktree_path) {
        bail!(
            "A git worktree already exists at '{}'. Remove it or pick a different branch.",
            worktree_path.display()
        );
    }

    let mut state = XlaudeState::load()?;
    let key = XlaudeState::make_key(repo_name, worktree_name);
    if state.worktrees.contains_key(&key) {
        bail!(
            "A worktree named '{}' is already tracked for '{}'.",
            worktree_name,
            repo_name
        );
    }

    let worktree_arg = worktree_path
        .to_str()
        .context("Worktree path contains invalid UTF-8")?;

    execute_git(&[
        "-C",
        repo_root_str,
        "worktree",
        "add",
        worktree_arg,
        branch_name,
    ])
    .context("Failed to create worktree")?;

    if let Err(e) = update_submodules(&worktree_path) {
        println!(
            "{} Warning: Failed to update submodules: {}",
            "⚠️".yellow(),
            e
        );
    } else {
        let gitmodules = worktree_path.join(".gitmodules");
        if gitmodules.exists() {
            println!("{} Updated submodules", "📦".green());
        }
    }

    let claude_local = repo_root.join("CLAUDE.local.md");
    if claude_local.exists() {
        let target = worktree_path.join("CLAUDE.local.md");
        fs::copy(&claude_local, &target).context("Failed to copy CLAUDE.local.md")?;
        println!("{} Copied CLAUDE.local.md to worktree", "📄".green());
    }

    state.worktrees.insert(
        key,
        WorktreeInfo {
            name: worktree_name.to_string(),
            branch: branch_name.to_string(),
            path: worktree_path.clone(),
            repo_name: repo_name.to_string(),
            created_at: Utc::now(),
        },
    );
    state.save()?;

    Ok(worktree_path)
}

fn list_worktrees_for_repo(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let repo_root_str = repo_root
        .to_str()
        .context("Repository path contains invalid UTF-8")?;
    let output = execute_git(&["-C", repo_root_str, "worktree", "list", "--porcelain"])?;

    let mut worktrees = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktrees.push(PathBuf::from(path));
        }
    }

    Ok(worktrees)
}

#[derive(Clone)]
struct ExistingWorktree(WorktreeInfo);

impl std::ops::Deref for ExistingWorktree {
    type Target = WorktreeInfo;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

enum CheckoutTarget {
    Branch(String),
    PullRequest(u64),
}

impl CheckoutTarget {
    fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("Target cannot be empty");
        }

        let digits_only = trimmed.trim_start_matches('#');
        if !digits_only.is_empty() && digits_only.chars().all(|c| c.is_ascii_digit()) {
            let value = digits_only
                .parse::<u64>()
                .context("Invalid pull request number")?;
            return Ok(Self::PullRequest(value));
        }

        Ok(Self::Branch(trimmed.to_string()))
    }

    fn branch_name(&self) -> String {
        match self {
            Self::Branch(name) => name.clone(),
            Self::PullRequest(number) => format!("pr/{number}"),
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Branch(name) => format!("branch '{name}'"),
            Self::PullRequest(number) => format!("pull request #{number}"),
        }
    }
}
