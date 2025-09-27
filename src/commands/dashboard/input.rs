use super::state::{DashboardMode, DashboardState};
use crate::state::XlaudeState;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

pub enum InputResult {
    Exit,
    Attach(String),
    CreateWorktree(Option<String>, Option<String>),
    Continue,
}

pub fn handle_input(
    key: KeyEvent,
    state: &mut DashboardState,
    xlaude_state: &mut XlaudeState,
) -> Result<InputResult> {
    match state.mode.clone() {
        DashboardMode::Help => {
            state.mode = DashboardMode::Normal;
            Ok(InputResult::Continue)
        }
        DashboardMode::Config { mut editor_input } => {
            let result = handle_config_input(key, &mut editor_input, xlaude_state)?;
            if matches!(result, InputResult::Exit) {
                if matches!(key.code, KeyCode::Enter) {
                    state.set_status_message("✅ Editor configured".to_string());
                }
                state.mode = DashboardMode::Normal;
                Ok(InputResult::Continue)
            } else {
                state.mode = DashboardMode::Config { editor_input };
                Ok(InputResult::Continue)
            }
        }
        DashboardMode::Create { mut input, repo } => {
            let result = handle_create_input(key, &mut input)?;
            match result {
                InputResult::CreateWorktree(name, _) => {
                    state.mode = DashboardMode::Normal;
                    Ok(InputResult::CreateWorktree(name, repo))
                }
                InputResult::Continue => {
                    state.mode = DashboardMode::Create { input, repo };
                    Ok(InputResult::Continue)
                }
                _ => {
                    state.mode = DashboardMode::Normal;
                    Ok(result)
                }
            }
        }
        DashboardMode::Normal => handle_normal_input(key, state),
    }
}

fn handle_config_input(
    key: KeyEvent,
    editor_input: &mut String,
    xlaude_state: &mut XlaudeState,
) -> Result<InputResult> {
    match key.code {
        KeyCode::Esc => {
            return Ok(InputResult::Exit);
        }
        KeyCode::Enter => {
            let editor = editor_input.trim();
            if !editor.is_empty() {
                xlaude_state.editor = Some(editor.to_string());
                xlaude_state.save()?;
            }
            return Ok(InputResult::Exit);
        }
        KeyCode::Backspace => {
            editor_input.pop();
        }
        KeyCode::Char(c) => {
            editor_input.push(c);
        }
        _ => {}
    }
    Ok(InputResult::Continue)
}

fn handle_create_input(key: KeyEvent, input: &mut String) -> Result<InputResult> {
    match key.code {
        KeyCode::Esc => {
            return Ok(InputResult::Exit);
        }
        KeyCode::Enter => {
            let name = if input.trim().is_empty() {
                None
            } else {
                Some(input.trim().to_string())
            };
            return Ok(InputResult::CreateWorktree(name, None));
        }
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Char(c) => {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                input.push(c);
            }
        }
        _ => {}
    }
    Ok(InputResult::Continue)
}

fn handle_normal_input(key: KeyEvent, state: &mut DashboardState) -> Result<InputResult> {
    match key.code {
        KeyCode::Char('q' | 'Q') => {
            return Ok(InputResult::Exit);
        }
        KeyCode::Char('?' | 'h') => {
            state.mode = DashboardMode::Help;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_selection_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_selection_down();
        }
        KeyCode::Enter => {
            if let Some(worktree) = state.get_selected_worktree() {
                return Ok(InputResult::Attach(worktree.name.clone()));
            }
        }
        KeyCode::Char('n' | 'N') => {
            let repo = if let Some(worktree) = state.get_selected_worktree() {
                Some(worktree.repo.clone())
            } else {
                state.worktrees.first().map(|w| w.repo.clone())
            };

            state.mode = DashboardMode::Create {
                input: String::new(),
                repo,
            };
        }
        KeyCode::Char('d' | 'D') => {
            if let Some(worktree) = state.get_selected_worktree()
                && worktree.has_session
            {
                // This will be handled by the caller
                // We just mark that we want to delete this session
            }
        }
        KeyCode::Char('r' | 'R') => {
            // Refresh will be handled by the caller
        }
        KeyCode::Char('c' | 'C') => {
            let editor_input = String::new();
            state.mode = DashboardMode::Config { editor_input };
        }
        _ => {}
    }

    Ok(InputResult::Continue)
}
