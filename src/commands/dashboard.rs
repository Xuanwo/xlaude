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
}

struct WorktreeDisplay {
    name: String,
    repo: String,
    key: String,
    has_session: bool,
    is_attached: bool,
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
                is_attached: session.map(|s| s.is_attached).unwrap_or(false),
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

        // Update worktree list
        self.refresh_worktrees();

        // Update preview cache for inactive sessions
        self.preview_cache.clear();
        for session in &self.sessions {
            if !session.is_attached {
                // Find the original worktree name for this session
                let worktree_name = self
                    .worktrees
                    .iter()
                    .find(|w| {
                        let safe_name = w.name.replace(['-', '.'], "_");
                        safe_name == session.project || w.name == session.project
                    })
                    .map(|w| w.name.clone())
                    .unwrap_or(session.project.clone());

                if let Ok(preview) = self.tmux.capture_pane(&worktree_name, 15) {
                    self.preview_cache.insert(worktree_name, preview);
                }
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

            // Handle events with timeout for auto-refresh
            if event::poll(Duration::from_secs(5))? {
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
                        InputResult::Continue => {}
                    }
                }
            } else {
                // Auto-refresh every 5 seconds
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

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                return Ok(InputResult::Exit);
            }
            KeyCode::Char('?') | KeyCode::Char('h') => {
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
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // Create new worktree (exit dashboard to run create command)
                disable_raw_mode()?;
                execute!(io::stdout(), LeaveAlternateScreen)?;

                println!("Creating new worktree...");
                if let Err(e) = crate::commands::handle_create(None) {
                    eprintln!("Failed to create worktree: {}", e);
                    std::thread::sleep(Duration::from_secs(2));
                }

                enable_raw_mode()?;
                execute!(io::stdout(), EnterAlternateScreen)?;
                self.refresh()?;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
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
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.refresh()?;
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
                Constraint::Length(2), // Help bar
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

        // Help bar
        let help = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" Open  "),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw(" New  "),
            Span::styled("d", Style::default().fg(Color::Yellow)),
            Span::raw(" Stop  "),
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

            // Status icon
            let status = if worktree.is_attached {
                "●"
            } else if worktree.has_session {
                "○"
            } else {
                "◌"
            };

            let status_color = if worktree.is_attached {
                Color::Green
            } else if worktree.has_session {
                Color::Yellow
            } else {
                Color::DarkGray
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
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        if session.is_attached {
                            "Active"
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
                    Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(session.format_time(session.created_at)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Last activity: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(session.format_time(session.last_activity)),
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
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
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
}

enum InputResult {
    Exit,
    Attach(String),
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
