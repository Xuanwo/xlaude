use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct TmuxManager {
    session_prefix: String,
}

impl TmuxManager {
    pub fn new() -> Self {
        TmuxManager {
            session_prefix: "xlaude".to_string(),
        }
    }

    /// Check if tmux is available
    pub fn is_available() -> bool {
        Command::new("which")
            .arg("tmux")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Create a new tmux session for a project
    pub fn create_session(&self, project: &str, work_dir: &Path) -> Result<()> {
        let session_name = self.make_session_name(project);

        // Check if session already exists
        if self.session_exists(project) {
            return Ok(());
        }

        // Create custom tmux config
        let config_path = self.create_custom_config()?;

        // Create detached tmux session with custom config and start claude directly
        let output = Command::new("tmux")
            .args([
                "-f",
                &config_path,
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-c",
                work_dir.to_str().unwrap(),
                "claude",
                "--dangerously-skip-permissions",
            ])
            .output()
            .context("Failed to create tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create tmux session: {}", stderr);
        }

        // Set key bindings for the session after creation
        // This ensures Ctrl+Q works even if config wasn't fully loaded
        Command::new("tmux")
            .args([
                "bind-key",
                "-t",
                &session_name,
                "-n",
                "C-q",
                "detach-client",
            ])
            .output()?;

        // Set status bar for this specific session
        self.configure_session_status(&session_name)?;

        Ok(())
    }

    /// Check if a session exists
    pub fn session_exists(&self, project: &str) -> bool {
        let session_name = self.make_session_name(project);

        Command::new("tmux")
            .args(["has-session", "-t", &session_name])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Attach to a session
    pub fn attach_session(&self, project: &str) -> Result<()> {
        let session_name = self.make_session_name(project);

        if !self.session_exists(project) {
            anyhow::bail!("Session {} does not exist", session_name);
        }

        // Ensure status bar is configured before attaching
        self.configure_session_status(&session_name)?;

        // Directly attach to the session without transition screen
        let status = Command::new("tmux")
            .args(["attach-session", "-t", &session_name])
            .status()
            .context("Failed to attach to tmux session")?;

        // After detach, clear screen and flush output
        print!("\x1b[2J\x1b[H");
        std::io::Write::flush(&mut std::io::stdout())?;

        if !status.success() {
            eprintln!("Warning: tmux attach exited with non-zero status");
        }

        Ok(())
    }

    /// Kill a session
    pub fn kill_session(&self, project: &str) -> Result<()> {
        let session_name = self.make_session_name(project);

        if !self.session_exists(project) {
            return Ok(()); // Already gone
        }

        Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output()
            .context("Failed to kill tmux session")?;

        Ok(())
    }

    /// List all xlaude sessions
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let output = Command::new("tmux")
            .args([
                "list-sessions",
                "-F",
                "#{session_name}:#{session_created}:#{session_attached}:#{session_activity}",
            ])
            .output()
            .context("Failed to list tmux sessions")?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut sessions = Vec::new();

        for line in output_str.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 {
                let full_name = parts[0];

                // Only process xlaude sessions (format: xlaude_project)
                let prefix = format!("{}_", self.session_prefix);
                if !full_name.starts_with(&prefix) {
                    continue;
                }

                // Extract project name from session name (xlaude_project)
                let project = if let Some(proj) = full_name.strip_prefix(&prefix) {
                    // Keep the safe name as-is for matching
                    proj.to_string()
                } else {
                    continue;
                };

                sessions.push(SessionInfo {
                    project,
                    created_at: parts[1].parse().unwrap_or(0),
                    is_attached: parts[2] != "0",
                    last_activity: parts[3].parse().unwrap_or(0),
                });
            }
        }

        Ok(sessions)
    }

    /// Capture recent output from a session
    pub fn capture_pane(&self, project: &str, lines: usize) -> Result<String> {
        let session_name = self.make_session_name(project);

        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &session_name,
                "-p", // print to stdout
                "-S",
                &format!("-{}", lines), // last N lines
            ])
            .output()
            .context("Failed to capture pane")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Configure status bar for a specific session
    fn configure_session_status(&self, session_name: &str) -> Result<()> {
        // Set status bar on
        Command::new("tmux")
            .args(["set-option", "-t", session_name, "status", "on"])
            .output()?;

        // Set status position to top
        Command::new("tmux")
            .args(["set-option", "-t", session_name, "status-position", "top"])
            .output()?;

        // Set status style (menu bar style)
        Command::new("tmux")
            .args([
                "set-option",
                "-t",
                session_name,
                "status-style",
                "bg=colour238,fg=colour250",
            ])
            .output()?;

        // Set left section with project name
        let project = session_name.strip_prefix("xlaude_").unwrap_or(session_name);
        let left_text = format!(" 📂 xlaude: {} ", project.replace('_', "-"));
        Command::new("tmux")
            .args(["set-option", "-t", session_name, "status-left", &left_text])
            .output()?;

        // Set right section with shortcut hint
        Command::new("tmux")
            .args([
                "set-option",
                "-t",
                session_name,
                "status-right",
                " Ctrl+Q: Dashboard ",
            ])
            .output()?;

        // Set lengths
        Command::new("tmux")
            .args(["set-option", "-t", session_name, "status-left-length", "50"])
            .output()?;

        Command::new("tmux")
            .args([
                "set-option",
                "-t",
                session_name,
                "status-right-length",
                "20",
            ])
            .output()?;

        // Clear center content
        Command::new("tmux")
            .args([
                "set-window-option",
                "-t",
                session_name,
                "window-status-current-format",
                "",
            ])
            .output()?;

        Command::new("tmux")
            .args([
                "set-window-option",
                "-t",
                session_name,
                "window-status-format",
                "",
            ])
            .output()?;

        Ok(())
    }

    /// Create custom tmux configuration
    fn create_custom_config(&self) -> Result<String> {
        let config = r#"# Menu bar style status at top
set -g status on
set -g status-position top
set -g status-style "bg=colour238,fg=colour250"
set -g status-left " 📂 xlaude "
set -g status-right " Ctrl+Q: Dashboard "
set -g status-left-length 50
set -g status-right-length 20
set -g window-status-current-format ""
set -g window-status-format ""

# Hide other UI elements
set -g pane-border-status off
set -g display-panes-time 1
set -g display-time 1
set -g visual-activity off
set -g visual-bell off
set -g visual-silence off
set -g bell-action none

# Single key to return to dashboard
bind-key -n C-q detach-client

# No prefix key - this is important for Ctrl+Q to work
set -g prefix None
unbind C-b

# Better colors
set -g default-terminal "screen-256color"
set -ga terminal-overrides ",*256col*:Tc"

# Large history
set -g history-limit 50000

# No delays
set -s escape-time 0

# Mouse support
set -g mouse on"#;

        let config_path = "/tmp/xlaude-tmux.conf";
        fs::write(config_path, config)?;
        Ok(config_path.to_string())
    }

    fn make_session_name(&self, project: &str) -> String {
        // Replace special characters that tmux doesn't like in session names
        let safe_project = project.replace(['-', '.'], "_");
        format!("{}_{}", self.session_prefix, safe_project)
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub project: String,
    pub created_at: i64,
    pub is_attached: bool,
    pub last_activity: i64,
}

impl SessionInfo {
    pub fn format_time(&self, timestamp: i64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let diff = now - timestamp;

        if diff < 60 {
            format!("{}s ago", diff)
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }
}
