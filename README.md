# xlaude - Xuanwo's Claude Code

A CLI tool for managing Claude instances with git worktrees or jj workspaces for parallel development workflows.

## Features

- **VCS Support**: Works with both Git worktrees and Jujutsu (jj) workspaces
- **Create isolated workspaces**: Each Claude instance runs in its own worktree/workspace
- **Seamless switching**: Open and switch between multiple development contexts
- **Smart cleanup**: Safely delete worktrees/workspaces with uncommitted change detection
- **Session tracking**: View Claude conversation history across instances
- **Random naming**: Generate memorable names using BIP39 word list

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

## Usage

### Create a new workspace

```bash
# Create with custom name
xlaude create feature-auth

# Create with random name (e.g., "dolphin", "rabbit")
xlaude create
```

This creates:
- **Git**: A new worktree at `../<repo>-<name>` and a corresponding branch
- **Jujutsu**: A new workspace at `../<repo>-<name>` with its own working copy

### Open an existing workspace

```bash
# Open specific workspace
xlaude open feature-auth

# Open current directory if it's a worktree
xlaude open

# Interactive selection (when not in a worktree)
xlaude open
```

This switches to the worktree directory and launches Claude with `--dangerously-skip-permissions`. When run without arguments in a worktree directory, it opens the current worktree directly.

### Add existing worktree/workspace

```bash
# Add current worktree/workspace with its name
cd ../myproject-bugfix
xlaude add

# Add with custom name
xlaude add hotfix
```

### List all workspaces

```bash
xlaude list
```

Shows all managed worktrees/workspaces with:
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

### Clean up invalid worktrees/workspaces

```bash
xlaude clean
```

Removes worktrees/workspaces from xlaude's state that no longer exist. This is useful when:
- You've used `git worktree remove` or `jj workspace forget` directly
- Worktree/workspace directories were manually deleted
- Maintaining consistency between VCS and xlaude state

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

## Configuration

State is persisted to `~/.config/xlaude/state.json`.

### State Format

- Worktree keys use format: `<repo-name>/<worktree-name>` (v0.3+)
- Automatic migration from older formats
- Tracks creation time and Claude session history

## Requirements

- Git with worktree support OR Jujutsu (jj)
- Claude CLI installed
- Rust (for building from source)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.