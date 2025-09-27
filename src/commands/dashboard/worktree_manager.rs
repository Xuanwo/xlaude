use anyhow::Result;
use std::collections::HashSet;
use crate::claude_status::ClaudeStatus;
use crate::state::XlaudeState;
use crate::tmux::{SessionInfo, TmuxManager};
use super::state::{DashboardState, WorktreeDisplay};

pub struct WorktreeManager {
    tmux: TmuxManager,
}

impl WorktreeManager {
    pub fn new() -> Self {
        Self {
            tmux: TmuxManager::new(),
        }
    }

    pub fn refresh_worktrees(
        &self,
        state: &mut DashboardState,
        xlaude_state: &XlaudeState,
        sessions: &[SessionInfo],
    ) {
        state.worktrees.clear();

        self.cleanup_orphaned_sessions(xlaude_state, sessions);

        for (key, info) in &xlaude_state.worktrees {
            let safe_name = info.name.replace(['-', '.'], "_");
            let session = sessions
                .iter()
                .find(|s| s.project == safe_name || s.project == info.name);

            state.worktrees.push(WorktreeDisplay {
                name: info.name.clone(),
                repo: info.repo_name.clone(),
                key: key.clone(),
                has_session: session.is_some(),
                claude_status: state
                    .claude_statuses
                    .get(&info.name)
                    .cloned()
                    .unwrap_or(ClaudeStatus::NotRunning),
            });
        }

        state.worktrees
            .sort_by(|a, b| a.repo.cmp(&b.repo).then(a.name.cmp(&b.name)));
    }

    fn cleanup_orphaned_sessions(&self, xlaude_state: &XlaudeState, sessions: &[SessionInfo]) {
        let valid_worktree_names: HashSet<String> = xlaude_state
            .worktrees
            .values()
            .map(|info| info.name.clone())
            .collect();

        for session in sessions {
            let worktree_name = xlaude_state
                .worktrees
                .values()
                .find(|w| {
                    let safe_name = w.name.replace(['-', '.'], "_");
                    safe_name == session.project || w.name == session.project
                })
                .map(|w| w.name.clone());

            if worktree_name.is_none() {
                let session_matches_any = valid_worktree_names
                    .iter()
                    .any(|name| name.replace(['-', '.'], "_") == session.project);

                if !session_matches_any {
                    if let Err(e) = self.tmux.kill_session(&session.project) {
                        eprintln!(
                            "Failed to clean up orphaned tmux session {}: {}",
                            session.project, e
                        );
                    }
                }
            }
        }
    }

    pub fn update_claude_statuses(
        &self,
        state: &mut DashboardState,
        xlaude_state: &XlaudeState,
        sessions: &[SessionInfo],
    ) {
        state.claude_statuses.clear();
        for session in sessions {
            let worktree_name = xlaude_state
                .worktrees
                .values()
                .find(|w| {
                    let safe_name = w.name.replace(['-', '.'], "_");
                    safe_name == session.project || w.name == session.project
                })
                .map(|w| w.name.clone())
                .unwrap_or(session.project.clone());

            if let Ok(output) = self.tmux.capture_pane(&worktree_name, 100) {
                let status = state.status_detector.analyze_output(&output);
                state.claude_statuses.insert(worktree_name.clone(), status);

                if !session.is_attached {
                    state.preview_cache.insert(worktree_name, output);
                }
            }
        }
    }

    pub fn kill_session(&self, worktree_name: &str) -> Result<()> {
        self.tmux.kill_session(worktree_name)
    }

    pub fn session_exists(&self, project: &str) -> bool {
        self.tmux.session_exists(project)
    }

    pub fn create_session(&self, project: &str, path: &std::path::Path) -> Result<()> {
        self.tmux.create_session(project, path)
    }

    pub fn attach_session(&self, project: &str) -> Result<()> {
        self.tmux.attach_session(project)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        self.tmux.list_sessions()
    }
}