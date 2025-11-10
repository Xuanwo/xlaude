# xlaude

> Manage Claude or Codex coding sessions by turning every git worktree into its own agent playground.

xlaude keeps large projects organized by pairing each feature branch with a dedicated AI session. It automates worktree creation, keeps track of conversation history, and provides an opinionated dashboard so you can pause, resume, and clean up work in seconds.

## Why xlaude?

- **Worktree-native workflow** – every feature branch lives in `../<repo>-<worktree>` with automatic branch creation, sanitized names, and submodule updates.
- **Session awareness** – `list` and the dashboard read Claude (`~/.claude/projects`) and Codex (`~/.codex/sessions`) logs to surface the last user prompt and activity timestamps per worktree.
- **Agent agnostic** – configure a single `agent` command (default `claude --dangerously-skip-permissions`). When that command is `codex`, xlaude auto-appends `resume <session-id>` matching the worktree.
- **Automation ready** – every subcommand accepts piped input, honors `XLAUDE_YES`/`XLAUDE_NON_INTERACTIVE`, and exposes a hidden completion helper for shell integration.
- **tmux dashboard** – start long-lived sessions, preview output, attach/detach with `Ctrl+Q`, and spawn new worktrees without leaving the keyboard.

## Installation

### Prerequisites

- Git with worktree support (git ≥ 2.36 recommended).
- Rust toolchain (for `cargo install` or local builds).
- Claude CLI or any other agent command you plan to run.
- `tmux` (only required for `xlaude dashboard`).
- Optional but recommended: GitHub CLI (`gh`) so `delete` can detect merged PRs even after squash merges.

### From crates.io

```bash
cargo install xlaude
```

### From source

```bash
git clone https://github.com/xuanwo/xlaude
cd xlaude
cargo build --release
```

Use `target/release/xlaude` or add it to your `PATH`.

### Upgrading

Re-run `cargo install xlaude` to pull the latest published release, or `git pull && cargo build --release` if you track `main`.

## Shell completions

Generate completion scripts for bash, zsh, or fish:

```bash
xlaude completions bash > ~/.bash_completion.d/xlaude
xlaude completions zsh  > ~/.zfunc/_xlaude
xlaude completions fish > ~/.config/fish/completions/xlaude.fish
```

The completions use the hidden `xlaude complete-worktrees --format=detailed` helper to surface worktree names, repositories, and recent session counts.

## Configuration & state

### State file

State lives in a JSON file that xlaude migrates automatically:

- macOS: `~/Library/Application Support/com.xuanwo.xlaude/state.json`
- Linux: `~/.config/xlaude/state.json`
- Windows: `%APPDATA%\xuanwo\xlaude\config\state.json`

Each entry is keyed by `<repo-name>/<worktree-name>` (introduced in v0.3). Use `XLAUDE_CONFIG_DIR` to override the directory for testing or portable setups.

### Agent command

Set the global `agent` field to the exact command line xlaude should launch for every worktree. Example:

```json
{
  "agent": "codex --dangerously-bypass-approvals-and-sandbox",
  "editor": "zed",
  "worktrees": {
    "repo/feature": { /* ... */ }
  }
}
```

- Default value: `claude --dangerously-skip-permissions`.
- The command is split with shell-style rules, so quotes are supported. Pipelines or redirects should live in a wrapper script.
- When the program name is `codex` and no positional arguments were supplied, xlaude will locate the latest session under `~/.codex/sessions` (or `XLAUDE_CODEX_SESSIONS_DIR`) whose `cwd` matches the worktree and automatically append `resume <session-id>`.

### Editor binding

Set the optional `editor` field to enable `Ctrl+O` inside tmux sessions (e.g., `"editor": "code -n"`). Use `xlaude dashboard`, hit `c`, type the command, and the state file will be updated. Alternatively, run `xlaude config` to open `state.json` in `$EDITOR`.

### Worktree creation defaults

- `xlaude create` and `checkout` copy `CLAUDE.local.md` into the new worktree if it exists at the repo root.
- Submodules are initialized with `git submodule update --init --recursive` in every new worktree.
- Branch names are sanitized (`feature/foo` → `feature-foo`) before creating the directory.

## Command reference

### `xlaude create [name]`

- Must be run from a base branch (`main`, `master`, `develop`, or the remote default). The branch check is skipped when the dashboard calls this command for you.
- Without a name, xlaude selects a random BIP39 word; set `XLAUDE_TEST_SEED` for deterministic names in CI.
- Rejects duplicate worktree directories or existing state entries.
- Offers to open the new worktree unless `XLAUDE_NO_AUTO_OPEN` or `XLAUDE_TEST_MODE` is set.

```bash
xlaude create auth-gateway
xlaude create # -> ../repo-harbor
```

### `xlaude checkout <branch | pr-number>`

- Accepts either a branch name or a GitHub pull request number (with or without `#`).
- Ensures the branch exists locally by fetching `origin/<branch>` when missing.
- For PR numbers, fetches `pull/<n>/head` into `pr/<n>` before creating the worktree.
- If the branch already has a managed worktree, xlaude offers to open it instead of duplicating the environment.

### `xlaude open [name]`

- With a name, finds the corresponding worktree across all repositories and launches the configured agent.
- Without a name and while standing inside a non-base worktree, it reuses the current directory. If the worktree is not tracked yet, xlaude offers to add it to `state.json`.
- Otherwise, presents an interactive selector (`fzf`-like list) or honors piped input.
- Every environment variable from the parent shell is forwarded to the agent process. When stdin is piped into `xlaude`, it is drained and not passed to the agent to avoid stuck sessions.

### `xlaude add [name]`

Attach the current git worktree (where `.git` is a file) to xlaude state. Name defaults to the sanitized branch. The command refuses to add the same path twice, even under a different alias.

### `xlaude rename <old> <new>`

Renames the entry in `state.json` within the current repository, keeping the underlying directory and git branch unchanged.

### `xlaude list [--json]`

- Default output groups worktrees by repository, showing path, creation timestamp, and recent sessions.
- Claude sessions are read from `~/.claude/projects/<encoded-path>`; up to three per worktree are previewed with "time ago" labels.
- Codex sessions are read from the sessions archive, showing the last user utterance when available.
- `--json` emits a machine-readable structure:

```json
{
  "worktrees": [
    {
      "name": "auth-gateway",
      "branch": "feature/auth-gateway",
      "path": "/repos/repo-auth-gateway",
      "repo_name": "repo",
      "created_at": "2025-10-30T02:41:18Z",
      "sessions": [ { "last_user_message": "Deploy staging", "time_ago": "5m ago" } ],
      "codex_sessions": [ ... ]
    }
  ]
}
```

### `xlaude dir [name]`

Prints the absolute path of a worktree with no ANSI formatting, making it ideal for subshells:

```bash
cd $(xlaude dir auth-gateway)
```

When no argument is provided, an interactive selector (or piped input) chooses the worktree.

### `xlaude delete [name]`

- If run without arguments, targets the worktree that matches the current directory.
- Refuses to proceed when there are uncommitted changes or unpushed commits unless you confirm.
- Checks whether the branch is merged either via `git branch --merged` or GitHub PR history (`gh pr list --state merged --head <branch>`). Squash mergers are therefore detected.
- Removes the git worktree (force-removing if needed), prunes it if the directory already disappeared, and deletes the local branch after confirmation.

### `xlaude clean`

Cross-checks `state.json` against actual `git worktree list` output for every known repository. Any missing directories are removed from state with a concise report.

### `xlaude dashboard`

- Requires `tmux` in `PATH`.
- Lists all managed worktrees along with Claude/Codex status (Waiting, Processing, Idle, Error) inferred from recent pane output.
- Creates dedicated tmux sessions named `xlaude_<worktree>`; inside a session:
  - `Ctrl+Q` detaches back to the dashboard.
  - `Ctrl+T` toggles a right-hand terminal pane.
  - `Ctrl+O` launches the configured `editor` with the current path.
- Dashboard key map:
  - `Enter`: attach to selected worktree (creating the session if needed).
  - `n`: prompt for a name and create a new worktree in the same repository.
  - `d`: stop the active Claude session (kills the tmux session).
  - `c`: edit the `editor` command.
  - `r`: refresh; `?`: help overlay; `q`: quit.

### `xlaude config`

Opens the state file in `$EDITOR`, creating parent directories as needed. Use this to hand-edit `agent`, `editor`, or worktree metadata.

### `xlaude completions <shell>`

Prints shell completion scripts. Combine with `complete-worktrees` for dynamic worktree hints.

### `xlaude complete-worktrees [--format=simple|detailed]` (hidden)

Emits sorted worktree names. The `detailed` format prints `name<TAB>repo<TAB>path<TAB>session-summary` and is consumed by the provided zsh/fish completion functions. You can also call it in custom tooling.

## Automation & non-interactive usage

Input priority is always **CLI argument > piped input > interactive prompt**. Example: `echo feature-x | xlaude open correct-name` opens `correct-name`.

Environment switches:

| Variable | Effect |
| --- | --- |
| `XLAUDE_YES=1` | Auto-confirm every prompt (used by `delete`, `create`, etc.). |
| `XLAUDE_NON_INTERACTIVE=1` | Disable interactive prompts/selectors; commands fall back to defaults or fail fast. |
| `XLAUDE_NO_AUTO_OPEN=1` | Skip the “open now?” question after `create`. |
| `XLAUDE_CONFIG_DIR=/tmp/xlaude-config` | Redirect both reads and writes of `state.json`. |
| `XLAUDE_CODEX_SESSIONS_DIR=/path/to/sessions` | Point Codex session discovery to a non-default location. |
| `XLAUDE_TEST_SEED=42` | Deterministically pick random names (handy for tests). |
| `XLAUDE_TEST_MODE=1` | Test harness flag; suppresses some interactivity (also skips auto-open). |

Piped input works with selectors and confirmations. For example, `yes | xlaude delete feature-x` or `printf "1\n" | xlaude open` to pick the first entry.

## Typical workflow

```bash
# 1. Create an isolated workspace from main
xlaude create payments-strategy

# 2. Start working with your agent
xlaude open payments-strategy

# 3. Pause and switch via dashboard
xlaude dashboard

# 4. Inspect outstanding worktrees across repositories
xlaude list --json | jq '.worktrees | length'

# 5. Clean up after merge
xlaude delete payments-strategy
```

## License

Apache License 2.0. See `LICENSE` for details.
