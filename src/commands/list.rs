use anyhow::Result;
use chrono::Utc;
use colored::Colorize;

use crate::claude::get_claude_sessions;
use crate::state::XlaudeState;

pub fn handle_list() -> Result<()> {
    let state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        println!("{} No active worktrees", "📭".yellow());
        return Ok(());
    }

    println!("{} Active worktrees:", "📋".cyan());
    println!();

    // Sort worktrees by path for consistent output
    let mut worktrees: Vec<_> = state.worktrees.values().collect();
    worktrees.sort_by_key(|w| &w.path);

    for info in worktrees {
        println!("  {} {}", "•".green(), info.name.cyan());
        println!("    {} {}", "Repository:".bright_black(), info.repo_name);
        println!("    {} {}", "Path:".bright_black(), info.path.display());
        println!(
            "    {} {}",
            "Created:".bright_black(),
            info.created_at.format("%Y-%m-%d %H:%M:%S")
        );

        // Get Claude sessions for this worktree
        let sessions = get_claude_sessions(&info.path);
        if !sessions.is_empty() {
            println!(
                "    {} {} session(s):",
                "Claude:".bright_black(),
                sessions.len()
            );
            for session in sessions.iter().take(3) {
                // Format time
                let time_str = session.last_timestamp.as_ref().map_or_else(
                    || "unknown".to_string(),
                    |ts| {
                        let now = Utc::now();
                        let diff = now.signed_duration_since(*ts);

                        if diff.num_minutes() < 60 {
                            format!("{}m ago", diff.num_minutes())
                        } else if diff.num_hours() < 24 {
                            format!("{}h ago", diff.num_hours())
                        } else {
                            format!("{}d ago", diff.num_days())
                        }
                    },
                );

                // Truncate message if too long
                let message = if session.last_user_message.len() > 60 {
                    let mut truncated = String::new();
                    for ch in session.last_user_message.chars() {
                        if truncated.len() + ch.len_utf8() > 57 {
                            break;
                        }
                        truncated.push(ch);
                    }
                    format!("{truncated}...")
                } else {
                    session.last_user_message.clone()
                };

                println!(
                    "      {} {} {}",
                    "-".bright_black(),
                    time_str.bright_black(),
                    message.bright_black()
                );
            }
            if sessions.len() > 3 {
                println!(
                    "      {} ... and {} more",
                    "-".bright_black(),
                    sessions.len() - 3
                );
            }
        }

        println!();
    }

    Ok(())
}
