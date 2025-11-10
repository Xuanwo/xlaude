# xlaude - Xuanwo's Claude Code

A CLI tool for managing Claude instances with git worktree for parallel development workflows.

## A Personal Tool, Made for You to Customize

This project is designed as a personal workflow tool, tailored to my specific development needs. While you're welcome to use it as-is, I won't be merging features that I don't personally use. Instead, I encourage you to **fork this project** and make it your own! Feel free to customize it to perfectly fit your workflow, that's the beauty of open source. Your fork might even become the perfect solution for others with similar needs.

## Features

- **Create isolated workspaces**: Each Claude instance runs in its own git worktree
- **Seamless switching**: Open and switch between multiple development contexts
- **Smart cleanup**: Safely delete worktrees with uncommitted change detection
- **Session tracking**: View Claude conversation history across instances
- **Random naming**: Generate memorable names using BIP39 word list
- **Dashboard mode**: Run multiple Claude instances in background and switch between them (requires tmux)
- **Pipe input support**: Integrate with Unix tools for automation
- **Configurable agent**: Use a global command to launch your preferred agent (default: Claude)

## Installation

```bash
cargo install xlaude
```

Or build from source:

```bash
git clone https://github.com/xuanwo/xlaude
cd xlaude
cargo build --release
```

### Shell Completions

xlaude supports tab completion for bash, zsh, and fish shells. After installation, generate and install the completion script:

```bash
# Bash
xlaude completions bash > ~/.bash_completion.d/xlaude
# Or add to .bashrc:
# eval "$(xlaude completions bash)"

# Zsh
xlaude completions zsh > ~/.zfunc/_xlaude
# Then add to .zshrc: fpath=(~/.zfunc $fpath)

# Fish
xlaude completions fish > ~/.config/fish/completions/xlaude.fish
```

The completions provide:

- Command and subcommand completion
- Dynamic worktree name completion for `open`, `dir`, `delete`, and `rename` commands
- Rich descriptions showing repository name and session count (zsh/fish)

## Usage

### Create a new workspace

```bash
# Create with custom name
xlaude create feature-auth

# Create with random name (e.g., "dolphin", "rabbit")
xlaude create

# Create from existing branch (local or remote)
xlaude create existing-branch
```

This creates a new git worktree at `../<repo>-<name>`. If the branch doesn't exist, it creates a new one. If the branch already exists (locally or on remote), it creates the worktree from that existing branch.

### Checkout a branch or pull request

```bash
# Checkout an existing local or remote branch
xlaude checkout feature-auth

# Checkout GitHub pull request #123 (requires `origin` remote)
xlaude checkout 123
```

`checkout` reuses the target branch without creating a new one. If xlaude is already managing a worktree for that branch/PR, it warns and offers to open it instead. When a numeric argument is provided, xlaude fetches `pull/<number>/head` from the `origin` remote into a local `pr/<number>` branch before creating the worktree.

### Open an existing workspace

```bash
# Open specific workspace
xlaude open feature-auth

# Open current directory if it's a worktree
xlaude open

# Interactive selection (when not in a worktree)
xlaude open
```

This switches to the worktree directory and launches the configured agent command. By default it is `claude --dangerously-skip-permissions`. When run without arguments in a worktree directory, it opens the current worktree directly.

### Agent configuration

You can configure a single, global agent command used by both `open` and the Dashboard (tmux sessions).

Edit your xlaude state file and set `agent` to a full command line string:

```json
{
  "worktrees": { /* ... */ },
  "editor": "zed",
  "agent": "codex"
}
```

Notes:

- Default when omitted: `claude --dangerously-skip-permissions`.
- The value is parsed with shell‑style splitting; quoted arguments are supported. Avoid shell pipelines/redirections here — if needed, wrap them in a small script and set `agent` to that script path.
- The same `agent` is used everywhere (no per‑repo/worktree overrides, no environment variable overrides).
- State file location (by platform): see the “Configuration” section below.

#### Using Codex

To launch Codex instead of Claude, update the state file (on macOS: `~/Library/Application Support/com.xuanwo.xlaude/state.json`) and set:

```json
{
  "agent": "codex --dangerously-bypass-approvals-and-sandbox"
}
```

When the program name is `codex` and no positional arguments are provided, xlaude automatically appends `resume <session-id>` with the most recent Codex session whose working directory matches the selected worktree. This lets you reopen the previous conversation without manual steps.

Codex session logs are discovered under `~/.codex/sessions` by default. If you store sessions elsewhere, export `XLAUDE_CODEX_SESSIONS_DIR=/custom/path` before running `xlaude` so the resume logic can locate them.

### Add existing worktree

```bash
# Add current worktree with branch name
cd ../myproject-bugfix
xlaude add

# Add with custom name
xlaude add hotfix
```

### List all workspaces

```bash
xlaude list
```

Shows all managed worktrees with:

- Name, repository, and path
- Creation time
- Recent Claude sessions (up to 3)
- Last user message from each session

### Delete a workspace

```bash
# Delete current workspace
xlaude delete

# Delete specific workspace
xlaude delete feature-auth
```

Performs safety checks for:

- Uncommitted changes
- Unpushed commits
- Branch merge status
- Confirms before deletion when needed

### Clean up invalid worktrees

```bash
xlaude clean
```

Removes worktrees from state management that have been manually deleted using `git worktree remove`.

### Rename a worktree

```bash
xlaude rename <old_name> <new_name>
```

Renames a worktree in xlaude management. This only updates the xlaude state and doesn't affect the actual git worktree or directory.

### Get worktree directory path

```bash
# Get path for specific worktree
xlaude dir feature-auth

# Interactive selection
xlaude dir
```

Returns the absolute path of a worktree. Useful for integration with other tools:

```bash
# Quick directory switching
cd $(xlaude dir feature-auth)

# Add shell function for convenience (in .bashrc/.zshrc)
xcd() { cd $(xlaude dir "$@"); }
xcd feature-auth

# Open in editor
code $(xlaude dir feature-auth)
vim $(xlaude dir feature-auth)/src/main.rs
```

### Interactive Dashboard (requires tmux)

```bash
xlaude dashboard
```

Launches an interactive TUI dashboard for managing multiple Claude sessions:

- **View all worktrees**: See status of all projects and Claude sessions
- **Background sessions**: Run multiple Claude instances simultaneously
- **Quick switching**: Press Enter to attach to a session, Ctrl+Q to return to dashboard
- **Session preview**: View recent output from background sessions
- **Keyboard shortcuts**:
  - `↑/↓` or `j/k`: Navigate project list
  - `Enter`: Attach to selected project
  - `Ctrl+Q`: Detach from Claude back to dashboard
  - `n`: Create new worktree
  - `d`: Stop selected Claude session
  - `r`: Refresh list
  - `?`: Show help
  - `q`: Quit dashboard

**Note**: Dashboard requires tmux. Install with:
- macOS: `brew install tmux`
- Ubuntu/Debian: `apt-get install tmux`
- Fedora: `dnf install tmux`

## Typical Workflow

1. **Start a new feature**:

   ```bash
   xlaude create auth-system
   xlaude open auth-system
   ```

2. **Work on the feature** with Claude assistance

3. **Switch contexts**:

   ```bash
   xlaude open  # Select another workspace
   # Or if you're already in a worktree directory:
   cd ../project-feature
   xlaude open  # Opens current worktree directly
   ```

4. **Clean up** when done:

   ```bash
   xlaude delete auth-system
   # Or clean up all invalid worktrees:
   xlaude clean
   ```

## Pipe Input and Automation

xlaude supports pipe input for automation and integration with other Unix tools:

### Basic pipe input

```bash
# Provide branch name via pipe
echo "feature-x" | xlaude create

# Select worktree via pipe
echo "feature-x" | xlaude open
echo "feature-x" | xlaude dir
```

### Auto-confirmation

```bash
# Auto-confirm deletion with yes
yes | xlaude delete feature-x

# Use environment variable for force-yes
XLAUDE_YES=1 xlaude delete feature-x
```

### Integration with other tools

```bash
# Use with fzf for fuzzy selection
xlaude list | fzf | xlaude open

# Batch operations
for branch in feature-1 feature-2; do
    echo $branch | xlaude create
done

# Chain with other commands
echo "hotfix" | xlaude create && xlaude open hotfix
```

### Priority order

When multiple input sources are available:

1. Command-line arguments (highest priority)
2. Piped input
3. Interactive prompts (lowest priority)

```bash
# CLI argument takes precedence over pipe
echo "wrong-name" | xlaude open correct-name  # Opens "correct-name"
```

## Configuration

State is persisted to platform-specific locations:

- macOS: `~/Library/Application Support/com.xuanwo.xlaude/state.json`
- Linux: `~/.config/xlaude/state.json`
- Windows: `%APPDATA%\xuanwo\xlaude\config\state.json`

### State Format

- Worktree keys use format: `<repo-name>/<worktree-name>` (v0.3+)
- Automatic migration from older formats
- Tracks creation time and Claude session history

### Environment Variables

- `XLAUDE_YES`: Set to "1" to auto-confirm all prompts
- `XLAUDE_NON_INTERACTIVE`: Set to "1" to disable interactive prompts
- `XLAUDE_CLAUDE_CMD`: Override the Claude command (default: "claude")

## Requirements

- Git with worktree support
- Claude CLI installed
- Rust (for building from source)
- tmux (optional, for dashboard mode)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
