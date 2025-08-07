use assert_cmd::Command;
use insta::{assert_json_snapshot, assert_snapshot};
use regex::Regex;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct TestContext {
    temp_dir: TempDir,
    temp_dir_str: String, // Store temp dir path as string for easy replacement
    repo_dir: PathBuf,
    config_dir: PathBuf,
}

impl TestContext {
    fn new(repo_name: &str) -> Self {
        let temp_dir = TempDir::new().unwrap();
        // Get the canonical path to handle symlinks and /private prefix on macOS
        let temp_dir_str = temp_dir
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp_dir.path().to_path_buf())
            .to_string_lossy()
            .to_string();
        let repo_dir = temp_dir.path().join(repo_name);
        let config_dir = temp_dir.path().join(".config/xlaude");

        // Initialize test git repo
        Self::init_test_repo(&repo_dir);

        // Create config directory
        fs::create_dir_all(&config_dir).unwrap();

        Self {
            temp_dir,
            temp_dir_str,
            repo_dir,
            config_dir,
        }
    }

    fn init_test_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();

        // Initialize git repository
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();

        // Configure git user for tests
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .unwrap();

        // Create initial commit
        fs::write(path.join("README.md"), "# Test Repo").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn xlaude(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("xlaude").unwrap();
        cmd.current_dir(&self.repo_dir)
            .env("HOME", self.temp_dir.path())
            .env("XLAUDE_CONFIG_DIR", &self.config_dir)
            // Mock claude command as echo
            .env("XLAUDE_CLAUDE_CMD", "true")
            // Disable color output for consistent snapshots
            .env("NO_COLOR", "1")
            // Enable non-interactive mode for testing
            .env("XLAUDE_NON_INTERACTIVE", "1");

        cmd.args(args);
        cmd
    }

    fn xlaude_in_dir(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("xlaude").unwrap();
        cmd.current_dir(dir)
            .env("HOME", self.temp_dir.path())
            .env("XLAUDE_CONFIG_DIR", &self.config_dir)
            .env("XLAUDE_CLAUDE_CMD", "true")
            .env("NO_COLOR", "1")
            .env("XLAUDE_NON_INTERACTIVE", "1");

        cmd.args(args);
        cmd
    }

    fn read_state(&self) -> serde_json::Value {
        let state_path = self.config_dir.join("state.json");
        if state_path.exists() {
            let content = fs::read_to_string(state_path).unwrap();
            serde_json::from_str(&content).unwrap()
        } else {
            json!({ "worktrees": [] })
        }
    }

    fn write_state(&self, state: &serde_json::Value) {
        let state_path = self.config_dir.join("state.json");
        fs::write(state_path, serde_json::to_string_pretty(state).unwrap()).unwrap();
    }

    fn worktree_exists(&self, name: &str) -> bool {
        self.temp_dir
            .path()
            .join(format!("test-repo-{name}"))
            .exists()
    }

    fn redact_paths(&self, text: &str) -> String {
        // Replace the actual temp directory path with a placeholder
        text.replace(&self.temp_dir_str, "/tmp/TEST_DIR")
    }

    fn redact_output(&self, text: &str) -> String {
        // Redact both paths and timestamps
        let mut result = self.redact_paths(text);

        // Replace timestamps like "2024-01-01 12:34:56" with "[TIMESTAMP]"
        let re = Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").unwrap();
        result = re.replace_all(&result, "[TIMESTAMP]").to_string();

        result
    }
}

// Create command tests
#[test]
fn test_create_with_name() {
    let ctx = TestContext::new("test-repo");

    // Execute command
    let output = ctx.xlaude(&["create", "feature-x"]).assert().success();

    // Snapshot test output with path redaction
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted = ctx.redact_paths(&stdout);
    assert_snapshot!(redacted);

    // Snapshot test state with redactions for dynamic values
    let mut state = ctx.read_state();
    // Manually redact dynamic values
    if let Some(worktrees) = state["worktrees"].as_object_mut() {
        for (_, worktree) in worktrees {
            worktree["created_at"] = json!("[TIMESTAMP]");
            if let Some(path) = worktree["path"].as_str() {
                worktree["path"] = json!(ctx.redact_paths(path));
            }
        }
    }
    assert_json_snapshot!(state);

    // Verify worktree was created
    assert!(ctx.worktree_exists("feature-x"));
}

#[test]
fn test_create_random_name() {
    let ctx = TestContext::new("test-repo");

    // Set fixed random seed for reproducibility
    let output = ctx
        .xlaude(&["create"])
        .env("XLAUDE_TEST_SEED", "42")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Verify a worktree was created (name will vary based on random selection)
    assert!(stdout.contains("Creating worktree"));
    assert!(stdout.contains("Worktree created at"));
}

#[test]
fn test_create_on_wrong_branch() {
    let ctx = TestContext::new("test-repo");

    // Switch to non-main branch
    std::process::Command::new("git")
        .args(["checkout", "-b", "feature-branch"])
        .current_dir(&ctx.repo_dir)
        .output()
        .unwrap();

    let output = ctx.xlaude(&["create", "test"]).assert().failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    let redacted = ctx.redact_paths(&stderr);
    assert_snapshot!(redacted);
}

// List command tests
#[test]
fn test_list_empty() {
    let ctx = TestContext::new("test-repo");

    let output = ctx.xlaude(&["list"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted = ctx.redact_paths(&stdout);
    assert_snapshot!(redacted);
}

#[test]
fn test_list_with_worktrees() {
    let ctx = TestContext::new("test-repo");

    // Create a few worktrees
    ctx.xlaude(&["create", "feature-a"]).assert().success();
    ctx.xlaude(&["create", "feature-b"]).assert().success();

    let output = ctx.xlaude(&["list"]).assert().success();

    // Manually redact timestamps and paths in output for consistent snapshots
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted_stdout = ctx.redact_output(&stdout);
    assert_snapshot!(redacted_stdout);
}

// Delete command tests
#[test]
fn test_delete_clean_worktree() {
    let ctx = TestContext::new("test-repo");

    // Create worktree
    ctx.xlaude(&["create", "to-delete"]).assert().success();

    // Delete worktree (in non-interactive mode, clean worktree will be deleted automatically)
    let output = ctx.xlaude(&["delete", "to-delete"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted = ctx.redact_paths(&stdout);
    assert_snapshot!(redacted);

    // Verify worktree was deleted
    assert!(!ctx.worktree_exists("to-delete"));

    // Verify state was updated
    let state = ctx.read_state();
    assert_eq!(state["worktrees"].as_object().unwrap().len(), 0);
}

#[test]
fn test_delete_with_changes() {
    let ctx = TestContext::new("test-repo");

    // Create worktree
    ctx.xlaude(&["create", "with-changes"]).assert().success();

    // Create uncommitted changes in worktree
    let worktree_path = ctx.temp_dir.path().join("test-repo-with-changes");
    fs::write(worktree_path.join("new-file.txt"), "content").unwrap();

    // Try to delete, in non-interactive mode it will be cancelled automatically
    let output = ctx.xlaude(&["delete", "with-changes"]).assert().success();

    // Check that output mentions uncommitted changes and cancellation
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("uncommitted changes"));
    assert!(stdout.contains("Cancelled"));

    // Verify worktree was not deleted
    assert!(worktree_path.exists());
}

#[test]
fn test_delete_current_worktree() {
    let ctx = TestContext::new("test-repo");

    // Create worktree
    ctx.xlaude(&["create", "current"]).assert().success();

    // Switch to the worktree directory
    let worktree_path = ctx.temp_dir.path().join("test-repo-current");

    // Delete from within the worktree (no name specified)
    let output = ctx
        .xlaude_in_dir(&worktree_path, &["delete"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    // Just check that the command succeeded and output something reasonable
    assert!(stdout.contains("Checking worktree") || stdout.contains("deleted"));
}

// Add command tests
#[test]
fn test_add_existing_worktree() {
    let ctx = TestContext::new("test-repo");

    // Manually create worktree
    std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "../test-repo-manual",
            "-b",
            "manual-branch",
        ])
        .current_dir(&ctx.repo_dir)
        .output()
        .unwrap();

    // Switch to manually created worktree
    let manual_worktree = ctx.temp_dir.path().join("test-repo-manual");

    let output = ctx
        .xlaude_in_dir(&manual_worktree, &["add", "manual"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted = ctx.redact_paths(&stdout);
    assert_snapshot!(redacted);

    // Verify state with manual redactions
    let mut state = ctx.read_state();
    if let Some(worktrees) = state["worktrees"].as_object_mut() {
        for (_, worktree) in worktrees {
            worktree["created_at"] = json!("[TIMESTAMP]");
            if let Some(path) = worktree["path"].as_str() {
                worktree["path"] = json!(ctx.redact_paths(path));
            }
        }
    }
    assert_json_snapshot!(state);
}

#[test]
fn test_add_without_name() {
    let ctx = TestContext::new("test-repo");

    // Manually create worktree
    std::process::Command::new("git")
        .args(["worktree", "add", "../test-repo-auto", "-b", "auto-branch"])
        .current_dir(&ctx.repo_dir)
        .output()
        .unwrap();

    let auto_worktree = ctx.temp_dir.path().join("test-repo-auto");

    // Add without specifying name (should use branch name)
    let output = ctx
        .xlaude_in_dir(&auto_worktree, &["add"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Adding worktree") || stdout.contains("added successfully"));

    // Verify it was added with branch name
    let state = ctx.read_state();
    let worktrees = state["worktrees"].as_object().unwrap();
    assert!(worktrees.contains_key("auto-branch"));
}

// Clean command tests
#[test]
fn test_clean_invalid_worktrees() {
    let ctx = TestContext::new("test-repo");

    // Create a valid worktree
    ctx.xlaude(&["create", "valid"]).assert().success();

    // Manually corrupt state file by adding invalid worktree
    let state = ctx.read_state();
    // Convert to proper structure for xlaude state format
    let worktrees_obj = state["worktrees"].as_object().cloned().unwrap_or_default();
    let mut new_worktrees = serde_json::Map::new();

    // Copy existing worktrees
    for (k, v) in worktrees_obj {
        new_worktrees.insert(k, v);
    }

    // Add invalid worktree
    new_worktrees.insert(
        "invalid".to_string(),
        json!({
            "name": "invalid",
            "branch": "invalid",
            "repo_name": "test-repo",
            "path": "/non/existent/path",
            "created_at": "2024-01-01T00:00:00Z"
        }),
    );

    let mut new_state = serde_json::Map::new();
    new_state.insert("worktrees".to_string(), json!(new_worktrees));
    ctx.write_state(&json!(new_state));

    // Run clean
    let output = ctx.xlaude(&["clean"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let redacted = ctx.redact_paths(&stdout);
    assert_snapshot!(redacted);

    // Verify state was cleaned
    let cleaned_state = ctx.read_state();
    let worktrees = cleaned_state["worktrees"].as_object().unwrap();
    assert_eq!(worktrees.len(), 1);
    assert!(worktrees.contains_key("valid"));
}

#[test]
fn test_clean_with_no_invalid() {
    let ctx = TestContext::new("test-repo");

    // Create valid worktrees
    ctx.xlaude(&["create", "valid1"]).assert().success();
    ctx.xlaude(&["create", "valid2"]).assert().success();

    // Run clean (should find no invalid worktrees)
    let output = ctx.xlaude(&["clean"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains("No invalid worktrees found") || stdout.contains("All worktrees are valid")
    );
}

// Open command tests (basic, since we can't actually launch Claude)
#[test]
fn test_open_specific_worktree() {
    let ctx = TestContext::new("test-repo");

    // Create worktree
    ctx.xlaude(&["create", "to-open"]).assert().success();

    // Mock claude command to verify it would be called
    let output = ctx
        .xlaude(&["open", "to-open"])
        .env("XLAUDE_CLAUDE_CMD", "true") // Use 'true' command which always succeeds
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Opening worktree"));
}

#[test]
fn test_open_nonexistent_worktree() {
    let ctx = TestContext::new("test-repo");

    let output = ctx.xlaude(&["open", "nonexistent"]).assert().failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("not found") || stderr.contains("No worktree"));
}
