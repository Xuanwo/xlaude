use crate::claude_status::{ClaudeStatus, ClaudeStatusDetector};
use ratatui::widgets::ListState;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WorktreeDisplay {
    pub name: String,
    pub repo: String,
    pub key: String,
    pub has_session: bool,
    pub claude_status: ClaudeStatus,
}

#[derive(Debug, Clone)]
pub enum DashboardMode {
    Normal,
    Help,
    Create { input: String, repo: Option<String> },
    Config { editor_input: String },
}

impl Default for DashboardMode {
    fn default() -> Self {
        Self::Normal
    }
}

pub struct DashboardState {
    pub mode: DashboardMode,
    pub worktrees: Vec<WorktreeDisplay>,
    pub selected: usize,
    pub list_index_map: Vec<Option<usize>>,
    pub list_state: ListState,
    pub status_message: Option<String>,
    pub status_message_timer: u8,
    pub preview_cache: HashMap<String, String>,
    pub claude_statuses: HashMap<String, ClaudeStatus>,
    pub status_detector: ClaudeStatusDetector,
}

impl DashboardState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            mode: DashboardMode::default(),
            worktrees: Vec::new(),
            selected: 0,
            list_index_map: Vec::new(),
            list_state,
            status_message: None,
            status_message_timer: 0,
            preview_cache: HashMap::new(),
            claude_statuses: HashMap::new(),
            status_detector: ClaudeStatusDetector::new(),
        }
    }

    pub fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
        self.status_message_timer = 5;
    }

    pub fn clear_status_message(&mut self) {
        self.status_message = None;
        self.status_message_timer = 0;
    }

    pub fn update_status_timer(&mut self) {
        if self.status_message.is_some() {
            if self.status_message_timer > 0 {
                self.status_message_timer -= 1;
            } else {
                self.clear_status_message();
            }
        }
    }

    pub fn get_selected_worktree(&self) -> Option<&WorktreeDisplay> {
        self.list_index_map
            .get(self.selected)
            .and_then(|idx| *idx)
            .and_then(|idx| self.worktrees.get(idx))
    }

    pub fn move_selection_up(&mut self) {
        if self.selected > 0 {
            let mut prev = self.selected - 1;
            loop {
                if self.list_index_map[prev].is_some() {
                    self.selected = prev;
                    self.list_state.select(Some(self.selected));
                    break;
                }
                if prev == 0 {
                    break;
                }
                prev -= 1;
            }
        }
    }

    pub fn move_selection_down(&mut self) {
        let mut next = self.selected + 1;
        while next < self.list_index_map.len() {
            if self.list_index_map[next].is_some() {
                self.selected = next;
                self.list_state.select(Some(self.selected));
                break;
            }
            next += 1;
        }
    }

    pub fn find_worktree_by_name(&self, name: &str) -> Option<usize> {
        for (idx, mapped_idx) in self.list_index_map.iter().enumerate() {
            if let Some(worktree_idx) = mapped_idx
                && let Some(worktree) = self.worktrees.get(*worktree_idx)
                && worktree.name == name
            {
                return Some(idx);
            }
        }
        None
    }

    pub fn focus_on_worktree(&mut self, name: &str) {
        if let Some(idx) = self.find_worktree_by_name(name) {
            self.selected = idx;
            self.list_state.select(Some(idx));
        }
    }
}
