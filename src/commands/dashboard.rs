use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::io;
use std::time::Duration;

use crate::claude_status::{ClaudeStatus, ClaudeStatusDetector};
use crate::state::XlaudeState;
use crate::tmux::{SessionInfo, TmuxManager};

pub struct Dashboard {
    tmux: TmuxManager,
    state: XlaudeState,
    sessions: Vec<SessionInfo>,
    worktrees: Vec<WorktreeDisplay>,
    selected: usize,
    list_index_map: Vec<Option<usize>>, // Maps list index to worktree index
    preview_cache: std::collections::HashMap<String, String>,
    list_state: ListState,
    show_help: bool,
    create_mode: bool,
    create_input: String,
    create_repo: Option<String>, // Repository context for creating worktree
    status_message: Option<String>, // Status message to display
    status_message_timer: u8,    // Timer to clear status message
    status_detector: ClaudeStatusDetector,
    claude_statuses: std::collections::HashMap<String, ClaudeStatus>,
    config_mode: bool,
    config_editor_input: String,
}

struct WorktreeDisplay {
    name: String,
    repo: String,
    key: String,
    has_session: bool,
    claude_status: ClaudeStatus,
}

impl Dashboard {
    pub fn new() -> Result<Self> {
        let tmux = TmuxManager::new();
        let state = XlaudeState::load()?;
        let sessions = tmux.list_sessions().unwrap_or_default();

        let mut dashboard = Dashboard {
            tmux,
            state,
            sessions,
            worktrees: Vec::new(),
            selected: 0,
            list_index_map: Vec::new(),
            preview_cache: std::collections::HashMap::new(),
            list_state: ListState::default(),
            show_help: false,
            create_mode: false,
            create_input: String::new(),
            create_repo: None,
            status_message: None,
            status_message_timer: 0,
            status_detector: ClaudeStatusDetector::new(),
            claude_statuses: std::collections::HashMap::new(),
            config_mode: false,
            config_editor_input: String::new(),
        };

        dashboard.refresh_worktrees();
        dashboard.list_state.select(Some(0));

        Ok(dashboard)
    }

    fn refresh_worktrees(&mut self) {
        self.worktrees.clear();

        // Get all worktrees from state
        for (key, info) in &self.state.worktrees {
            // Match session by converting name to safe format (same as tmux session name)
            let safe_name = info.name.replace(['-', '.'], "_");
            let session = self
                .sessions
                .iter()
                .find(|s| s.project == safe_name || s.project == info.name);

            self.worktrees.push(WorktreeDisplay {
                name: info.name.clone(),
                repo: info.repo_name.clone(),
                key: key.clone(),
                has_session: session.is_some(),
                claude_status: self
                    .claude_statuses
                    .get(&info.name)
                    .cloned()
                    .unwrap_or(ClaudeStatus::NotRunning),
            });
        }

        // Sort by repo and name
        self.worktrees
            .sort_by(|a, b| a.repo.cmp(&b.repo).then(a.name.cmp(&b.name)));
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Reload state
        self.state = XlaudeState::load()?;

        // Refresh tmux sessions
        self.sessions = self.tmux.list_sessions().unwrap_or_default();

        // Update Claude statuses for all running sessions
        self.claude_statuses.clear();
        for session in &self.sessions {
            // Find the original worktree name
            let worktree_name = self
                .state
                .worktrees
                .values()
                .find(|w| {
                    let safe_name = w.name.replace(['-', '.'], "_");
                    safe_name == session.project || w.name == session.project
                })
                .map(|w| w.name.clone())
                .unwrap_or(session.project.clone());

            // Capture pane output and analyze status
            if let Ok(output) = self.tmux.capture_pane(&worktree_name, 50) {
                let status = self.status_detector.analyze_output(&output);
                self.claude_statuses.insert(worktree_name.clone(), status);

                // Cache preview for inactive sessions
                if !session.is_attached {
                    self.preview_cache.insert(worktree_name, output);
                }
            }
        }

        // Update worktree list
        self.refresh_worktrees();

        // Update preview cache for inactive sessions (already done above)
        for session in &self.sessions {
            if !session.is_attached {
                // Already handled above
            }
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Initial refresh
        self.refresh()?;

        let result = self.run_loop(&mut terminal);

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;

            // Clear status message after a few renders
            if self.status_message.is_some() {
                if self.status_message_timer > 0 {
                    self.status_message_timer -= 1;
                } else {
                    self.status_message = None;
                }
            }

            // Handle events with shorter timeout for more responsive updates
            if event::poll(Duration::from_millis(500))? {
                if let Event::Key(key) = event::read()? {
                    match self.handle_input(key)? {
                        InputResult::Exit => break,
                        InputResult::Attach(project) => {
                            // Clean up terminal before attaching
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;

                            // Attach to tmux session
                            if let Err(e) = self.attach_to_project(&project) {
                                eprintln!("Failed to attach: {}", e);
                            }

                            // Restore terminal after detach
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.hide_cursor()?;

                            // Force clear and redraw
                            terminal.clear()?;

                            // Refresh state after returning
                            self.refresh()?;
                        }
                        InputResult::CreateWorktree(name, repo) => {
                            // Find the repo path if specified
                            let repo_path = if let Some(repo_name) = &repo {
                                // Find the repo path from existing worktrees
                                // If worktree is at /path/parent/repo-worktree
                                // Then main repo should be at /path/parent/repo
                                if let Some(worktree) =
                                    self.worktrees.iter().find(|w| w.repo == *repo_name)
                                    && let Some(info) = self.state.worktrees.get(&worktree.key)
                                    && let Some(parent) = info.path.parent()
                                {
                                    let path = parent.join(repo_name);
                                    if path.exists() {
                                        Some(path)
                                    } else {
                                        // If not found, maybe it's the current directory
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            // Create the worktree quietly in background
                            let created_name =
                                match crate::commands::create::handle_create_in_dir_quiet(
                                    name, repo_path, true,
                                ) {
                                    Ok(name) => name,
                                    Err(e) => {
                                        // Show error message in status area (we'll add this)
                                        self.status_message =
                                            Some(format!("Failed to create worktree: {}", e));
                                        continue;
                                    }
                                };

                            // Refresh to show new worktree
                            self.refresh()?;

                            // Auto-focus on the newly created worktree
                            if let Some(repo_name) = repo {
                                let key = format!("{}/{}", repo_name, created_name);
                                // Find the new worktree in the list and set focus
                                for (idx, mapped_idx) in self.list_index_map.iter().enumerate() {
                                    if let Some(worktree_idx) = mapped_idx
                                        && let Some(worktree) = self.worktrees.get(*worktree_idx)
                                        && (worktree.key == key || worktree.name == created_name)
                                    {
                                        self.selected = idx;
                                        self.list_state.select(Some(idx));
                                        break;
                                    }
                                }
                            }

                            // Show success message
                            self.status_message =
                                Some(format!("✅ Created worktree: {}", created_name));
                            self.status_message_timer = 5; // Show for 5 seconds
                        }
                        InputResult::Continue => {}
                    }
                }
            } else {
                // Auto-refresh every 500ms for better responsiveness
                self.refresh()?;
            }
        }

        Ok(())
    }

    fn handle_input(&mut self, key: KeyEvent) -> Result<InputResult> {
        if self.show_help {
            self.show_help = false;
            return Ok(InputResult::Continue);
        }

        // Handle config mode input
        if self.config_mode {
            match key.code {
                KeyCode::Esc => {
                    // Cancel config mode without saving
                    self.config_mode = false;
                    // Restore original editor value
                    self.config_editor_input = self.state.editor.clone().unwrap_or_default();
                }
                KeyCode::Enter => {
                    // Save configuration
                    let editor = self.config_editor_input.trim();
                    if !editor.is_empty() {
                        self.state.editor = Some(editor.to_string());
                        self.state.save()?;
                        self.status_message = Some(format!("✅ Editor set to: {}", editor));
                        self.status_message_timer = 5;
                    }
                    self.config_mode = false;
                }
                KeyCode::Backspace => {
                    self.config_editor_input.pop();
                }
                KeyCode::Char(c) => {
                    self.config_editor_input.push(c);
                }
                _ => {}
            }
            return Ok(InputResult::Continue);
        }

        // Handle create mode input
        if self.create_mode {
            match key.code {
                KeyCode::Esc => {
                    // Cancel create mode
                    self.create_mode = false;
                    self.create_input.clear();
                }
                KeyCode::Enter => {
                    // Create worktree with entered name or use None for random name
                    let name = if self.create_input.trim().is_empty() {
                        None
                    } else {
                        Some(self.create_input.trim().to_string())
                    };
                    let repo = self.create_repo.clone();

                    // Exit create mode
                    self.create_mode = false;
                    self.create_input.clear();

                    // Create the worktree
                    return Ok(InputResult::CreateWorktree(name, repo));
                }
                KeyCode::Backspace => {
                    self.create_input.pop();
                }
                KeyCode::Char(c) => {
                    // Only allow alphanumeric, dash, and underscore
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.create_input.push(c);
                    }
                }
                _ => {}
            }
            return Ok(InputResult::Continue);
        }

        match key.code {
            KeyCode::Char('q' | 'Q') => {
                return Ok(InputResult::Exit);
            }
            KeyCode::Char('?' | 'h') => {
                self.show_help = true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // Move up, skipping repository headers
                if self.selected > 0 {
                    let mut prev = self.selected - 1;
                    loop {
                        if self.list_index_map[prev].is_some() {
                            // Found a selectable item
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
            KeyCode::Down | KeyCode::Char('j') => {
                // Move down, skipping repository headers
                let mut next = self.selected + 1;
                while next < self.list_index_map.len() {
                    if self.list_index_map[next].is_some() {
                        // Found a selectable item
                        self.selected = next;
                        self.list_state.select(Some(self.selected));
                        break;
                    }
                    next += 1;
                }
            }
            KeyCode::Enter => {
                // Get the actual worktree index from the mapping
                if let Some(Some(worktree_idx)) = self.list_index_map.get(self.selected)
                    && let Some(worktree) = self.worktrees.get(*worktree_idx)
                {
                    return Ok(InputResult::Attach(worktree.name.clone()));
                }
            }
            KeyCode::Char('n' | 'N') => {
                // Enter create mode with dialog
                self.create_mode = true;
                self.create_input.clear();

                // Determine repository context from current selection
                if let Some(Some(worktree_idx)) = self.list_index_map.get(self.selected)
                    && let Some(worktree) = self.worktrees.get(*worktree_idx)
                {
                    self.create_repo = Some(worktree.repo.clone());
                } else {
                    // Find the first repository in the list if no specific selection
                    self.create_repo = self.worktrees.first().map(|w| w.repo.clone());
                }
            }
            KeyCode::Char('d' | 'D') => {
                // Get the actual worktree index from the mapping
                if let Some(Some(worktree_idx)) = self.list_index_map.get(self.selected)
                    && let Some(worktree) = self.worktrees.get(*worktree_idx)
                {
                    // Kill tmux session if exists
                    if worktree.has_session {
                        self.tmux.kill_session(&worktree.name)?;
                    }
                    self.refresh()?;
                }
            }
            KeyCode::Char('r' | 'R') => {
                self.refresh()?;
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                // Enter config mode
                self.config_mode = true;
                self.config_editor_input = self.state.editor.clone().unwrap_or_default();
            }
            _ => {}
        }

        Ok(InputResult::Continue)
    }

    fn attach_to_project(&mut self, project: &str) -> Result<()> {
        // Get worktree info
        let worktree = self
            .worktrees
            .iter()
            .find(|w| w.name == project)
            .context("Worktree not found")?;

        let info = self
            .state
            .worktrees
            .get(&worktree.key)
            .context("Worktree info not found")?;

        // Check if path exists
        if !info.path.exists() {
            anyhow::bail!("Worktree path does not exist: {}", info.path.display());
        }

        // Create session if it doesn't exist
        if !self.tmux.session_exists(project) {
            println!("Creating new tmux session for {}...", project);
            self.tmux.create_session(project, &info.path)?;
            // Give tmux time to initialize
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        // Attach to the session
        self.tmux.attach_session(project)?;

        Ok(())
    }

    fn render(&mut self, f: &mut Frame) {
        if self.show_help {
            self.render_help(f);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Menu bar
                Constraint::Min(10),   // Main content
                Constraint::Length(if self.status_message.is_some() { 3 } else { 2 }), // Help/status bar
            ])
            .split(f.area());

        // Menu bar (matching tmux style)
        let menu_bar = Paragraph::new(" 📂 xlaude: Dashboard").style(
            Style::default()
                .bg(Color::Rgb(68, 68, 68))
                .fg(Color::Rgb(250, 250, 250)),
        );
        f.render_widget(menu_bar, chunks[0]);

        // Main content area - split horizontally
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Project list
                Constraint::Percentage(60), // Preview
            ])
            .split(chunks[1]);

        // Project list
        self.render_project_list(f, main_chunks[0]);

        // Preview pane
        self.render_preview(f, main_chunks[1]);

        // Help/Status bar
        if let Some(ref status) = self.status_message {
            // Split the help area into status and help
            let help_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Status message
                    Constraint::Length(1), // Help line
                ])
                .split(chunks[2]);

            // Show status message
            let status_widget =
                Paragraph::new(format!(" {}", status)).style(Style::default().fg(Color::Green));
            f.render_widget(status_widget, help_chunks[0]);

            // Show help bar
            let help = Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" Open  "),
                Span::styled("n", Style::default().fg(Color::Yellow)),
                Span::raw(" New  "),
                Span::styled("d", Style::default().fg(Color::Yellow)),
                Span::raw(" Stop  "),
                Span::styled("c", Style::default().fg(Color::Yellow)),
                Span::raw(" Config  "),
                Span::styled("r", Style::default().fg(Color::Yellow)),
                Span::raw(" Refresh  "),
                Span::styled("?", Style::default().fg(Color::Yellow)),
                Span::raw(" Help  "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(" Quit "),
            ]))
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(help, help_chunks[1]);
        } else {
            // Just show help bar
            let help = Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" Open  "),
                Span::styled("n", Style::default().fg(Color::Yellow)),
                Span::raw(" New  "),
                Span::styled("d", Style::default().fg(Color::Yellow)),
                Span::raw(" Stop  "),
                Span::styled("c", Style::default().fg(Color::Yellow)),
                Span::raw(" Config  "),
                Span::styled("r", Style::default().fg(Color::Yellow)),
                Span::raw(" Refresh  "),
                Span::styled("?", Style::default().fg(Color::Yellow)),
                Span::raw(" Help  "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(" Quit "),
            ]))
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(help, chunks[2]);
        }

        // Render create dialog if in create mode
        if self.create_mode {
            self.render_create_dialog(f);
        }

        // Render config dialog if in config mode
        if self.config_mode {
            self.render_config_dialog(f);
        }
    }

    fn render_project_list(&mut self, f: &mut Frame, area: Rect) {
        let mut items = Vec::new();
        let mut current_repo = String::new();
        self.list_index_map.clear();

        for (worktree_idx, worktree) in self.worktrees.iter().enumerate() {
            // Add repo header if changed
            if worktree.repo != current_repo {
                current_repo = worktree.repo.clone();
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    format!("📁 {}", current_repo),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )])));
                self.list_index_map.push(None); // Header has no worktree
            }

            // Status icon based on Claude status
            let (status, status_color) = if !worktree.has_session {
                ("◌", Color::DarkGray)
            } else {
                (
                    worktree.claude_status.display_icon(),
                    worktree.claude_status.color(),
                )
            };

            // Build item
            let item = Line::from(vec![
                Span::raw("  "),
                Span::styled(status, Style::default().fg(status_color)),
                Span::raw(" "),
                Span::raw(&worktree.name),
            ]);

            items.push(ListItem::new(item));
            self.list_index_map.push(Some(worktree_idx)); // Map to actual worktree
        }

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Projects (↑↓ to navigate) "),
            )
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.list_state.clone());
    }

    fn render_preview(&self, f: &mut Frame, area: Rect) {
        // Get the actual worktree from mapping
        let worktree_idx = self.list_index_map.get(self.selected).and_then(|idx| *idx);

        if let Some(idx) = worktree_idx
            && let Some(worktree) = self.worktrees.get(idx)
        {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Project: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&worktree.name),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Repository: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(&worktree.repo),
                ]),
                Line::from(""),
            ];

            // Add session info (match by safe name)
            let safe_name = worktree.name.replace(['-', '.'], "_");
            if let Some(session) = self
                .sessions
                .iter()
                .find(|s| s.project == safe_name || s.project == worktree.name)
            {
                // Show both session status and Claude status
                lines.push(Line::from(vec![
                    Span::styled("Session: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        if session.is_attached {
                            "Attached"
                        } else {
                            "Background"
                        },
                        Style::default().fg(if session.is_attached {
                            Color::Green
                        } else {
                            Color::Yellow
                        }),
                    ),
                ]));

                lines.push(Line::from(vec![
                    Span::styled("Claude: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!(
                            "{} {}",
                            worktree.claude_status.display_icon(),
                            worktree.claude_status.display_text()
                        ),
                        Style::default().fg(worktree.claude_status.color()),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(SessionInfo::format_time(session.created_at)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Last activity: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(SessionInfo::format_time(session.last_activity)),
                ]));
                lines.push(Line::from(""));

                // Show preview if available
                if !session.is_attached
                    && let Some(preview) = self.preview_cache.get(&worktree.name)
                {
                    lines.push(Line::from(Span::styled(
                        "Recent output:",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    lines.push(Line::from("─".repeat(area.width as usize - 2)));

                    for line in preview.lines().take(10) {
                        lines.push(Line::from(line.to_string()));
                    }
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled("Session: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled("Not running", Style::default().fg(Color::DarkGray)),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press Enter to start Claude session",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }

            let preview = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Details "))
                .wrap(Wrap { trim: true });

            f.render_widget(preview, area);
        }
    }

    fn render_help(&self, f: &mut Frame) {
        let help_text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "xlaude dashboard - Help",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Navigation:",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("↑/k", Style::default().fg(Color::Yellow)),
                Span::raw("    Move up"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("↓/j", Style::default().fg(Color::Yellow)),
                Span::raw("    Move down"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw("  Open selected project"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Actions:",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("n", Style::default().fg(Color::Yellow)),
                Span::raw("      Create new worktree"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("d", Style::default().fg(Color::Yellow)),
                Span::raw("      Stop Claude session"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("r", Style::default().fg(Color::Yellow)),
                Span::raw("      Refresh list"),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw("      Quit dashboard"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "In Claude session:",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Ctrl+Q", Style::default().fg(Color::Yellow)),
                Span::raw(" Return to dashboard"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )),
        ];

        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title(" Help "))
            .alignment(Alignment::Left);

        let area = centered_rect(60, 80, f.area());
        f.render_widget(help, area);
    }

    fn render_create_dialog(&self, f: &mut Frame) {
        // Calculate dialog area (centered, 50% width, 30% height)
        let area = centered_rect(50, 30, f.area());

        // Clear the dialog area
        let clear = ratatui::widgets::Clear;
        f.render_widget(clear, area);

        // Create the dialog content
        let repo_text = self.create_repo.as_deref().unwrap_or("current repository");
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("Creating new worktree in {}", repo_text),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Enter worktree name:"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{}_", self.create_input),
                    Style::default().bg(Color::DarkGray).fg(Color::White),
                ),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" to create  "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(" to cancel"),
            ]),
        ];

        // If input is empty, show a hint
        if self.create_input.is_empty() {
            lines.insert(
                7,
                Line::from(Span::styled(
                    "  (leave empty for random name)",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )),
            );
        }

        let dialog = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Create New Worktree ")
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .alignment(Alignment::Center);

        f.render_widget(dialog, area);
    }

    fn render_config_dialog(&self, f: &mut Frame) {
        // Calculate dialog area (centered, 60% width, 40% height)
        let area = centered_rect(60, 40, f.area());

        // Clear the dialog area
        let clear = ratatui::widgets::Clear;
        f.render_widget(clear, area);

        // Create the dialog content
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Configuration",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Editor command for opening projects:"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{}_", self.config_editor_input),
                    Style::default().bg(Color::DarkGray).fg(Color::White),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Examples: zed, code, vim, nvim, subl, 'code -n'",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )),
            Line::from(""),
            Line::from("This editor will be used when pressing Ctrl+O in tmux sessions."),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" to save  "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(" to cancel"),
            ]),
        ];

        let dialog = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Configuration ")
                    .border_style(Style::default().fg(Color::Blue)),
            )
            .alignment(Alignment::Center);

        f.render_widget(dialog, area);
    }
}

enum InputResult {
    Exit,
    Attach(String),
    CreateWorktree(Option<String>, Option<String>), // optional name and optional repo context
    Continue,
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn handle_dashboard() -> Result<()> {
    // Check if tmux is available
    if !TmuxManager::is_available() {
        anyhow::bail!(
            "tmux is not installed. Please install tmux to use the dashboard feature.\n\
             On macOS: brew install tmux\n\
             On Ubuntu/Debian: apt-get install tmux"
        );
    }

    let mut dashboard = Dashboard::new()?;
    dashboard.run()?;

    Ok(())
}
