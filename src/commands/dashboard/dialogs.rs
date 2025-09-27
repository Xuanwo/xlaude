use super::state::{DashboardMode, DashboardState};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render_dialogs(f: &mut Frame, state: &DashboardState) {
    match &state.mode {
        DashboardMode::Help => render_help_dialog(f),
        DashboardMode::Create { input, repo } => render_create_dialog(f, input, repo.as_deref()),
        DashboardMode::Config { editor_input } => render_config_dialog(f, editor_input),
        DashboardMode::Normal => {}
    }
}

fn render_help_dialog(f: &mut Frame) {
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

fn render_create_dialog(f: &mut Frame, input: &str, repo: Option<&str>) {
    let area = centered_rect(50, 30, f.area());
    let clear = Clear;
    f.render_widget(clear, area);

    let repo_text = repo.unwrap_or("current repository");
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
                format!("{}_", input),
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

    if input.is_empty() {
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

fn render_config_dialog(f: &mut Frame, editor_input: &str) {
    let area = centered_rect(60, 40, f.area());
    let clear = Clear;
    f.render_widget(clear, area);

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
                format!("{}_", editor_input),
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
