use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::claude::get_claude_sessions;
use crate::codex;
use crate::state::XlaudeState;

#[derive(Debug, Serialize, Deserialize)]
struct JsonSessionInfo {
    last_user_message: String,
    last_timestamp: Option<DateTime<Utc>>,
    time_ago: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonWorktreeInfo {
    name: String,
    branch: String,
    path: String,
    repo_name: String,
    created_at: DateTime<Utc>,
    sessions: Vec<JsonSessionInfo>,
    codex_sessions: Vec<JsonCodexSessionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonOutput {
    worktrees: Vec<JsonWorktreeInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonCodexSessionInfo {
    id: String,
    last_user_message: Option<String>,
    last_timestamp: Option<DateTime<Utc>>,
    time_ago: String,
}

fn format_time_ago(timestamp: Option<DateTime<Utc>>) -> String {
    timestamp.map_or_else(
        || "unknown".to_string(),
        |ts| {
            let now = Utc::now();
            let diff = now.signed_duration_since(ts);

            if diff.num_minutes() < 60 {
                format!("{}m ago", diff.num_minutes())
            } else if diff.num_hours() < 24 {
                format!("{}h ago", diff.num_hours())
            } else {
                format!("{}d ago", diff.num_days())
            }
        },
    )
}

fn format_message_preview(message: &str, limit: usize) -> String {
    if message.len() <= limit {
        return message.to_string();
    }

    let mut truncated = String::new();
    let safe_limit = limit.saturating_sub(3);
    for ch in message.chars() {
        if truncated.len() + ch.len_utf8() > safe_limit {
            break;
        }
        truncated.push(ch);
    }
    truncated.push_str("...");
    truncated
}

pub fn handle_list(json: bool) -> Result<()> {
    let state = XlaudeState::load()?;

    if state.worktrees.is_empty() {
        if json {
            let output = JsonOutput { worktrees: vec![] };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("{} No active worktrees", "📭".yellow());
        }
        return Ok(());
    }

    if json {
        // JSON output
        let mut worktrees = Vec::new();

        for info in state.worktrees.values() {
            let claude_sessions = get_claude_sessions(&info.path);
            let json_sessions: Vec<JsonSessionInfo> = claude_sessions
                .into_iter()
                .map(|session| JsonSessionInfo {
                    last_user_message: session.last_user_message,
                    last_timestamp: session.last_timestamp,
                    time_ago: format_time_ago(session.last_timestamp),
                })
                .collect();

            let (codex_sessions, _) = codex::recent_sessions(&info.path, usize::MAX)?;
            let json_codex_sessions: Vec<JsonCodexSessionInfo> = codex_sessions
                .into_iter()
                .map(|session| JsonCodexSessionInfo {
                    id: session.id,
                    last_user_message: session.last_user_message,
                    last_timestamp: session.last_timestamp,
                    time_ago: format_time_ago(session.last_timestamp),
                })
                .collect();

            worktrees.push(JsonWorktreeInfo {
                name: info.name.clone(),
                branch: info.branch.clone(),
                path: info.path.display().to_string(),
                repo_name: info.repo_name.clone(),
                created_at: info.created_at,
                sessions: json_sessions,
                codex_sessions: json_codex_sessions,
            });
        }

        // Sort worktrees by repo name and then by name
        worktrees.sort_by(|a, b| {
            a.repo_name
                .cmp(&b.repo_name)
                .then_with(|| a.name.cmp(&b.name))
        });

        let output = JsonOutput { worktrees };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Original colored output
        println!("{} Active worktrees:", "📋".cyan());
        println!();

        // Group worktrees by repository
        let mut grouped: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for info in state.worktrees.values() {
            grouped
                .entry(info.repo_name.clone())
                .or_default()
                .push(info);
        }

        // Display grouped by repository
        for (repo_name, mut worktrees) in grouped {
            println!("  {} {}", "📦".blue(), repo_name.bold());

            // Sort worktrees within each repo by name
            worktrees.sort_by_key(|w| &w.name);

            for info in worktrees {
                println!("    {} {}", "•".green(), info.name.cyan());
                println!("      {} {}", "Path:".bright_black(), info.path.display());
                println!(
                    "      {} {}",
                    "Created:".bright_black(),
                    info.created_at.format("%Y-%m-%d %H:%M:%S")
                );

                // Get Claude sessions for this worktree
                let claude_sessions = get_claude_sessions(&info.path);
                if !claude_sessions.is_empty() {
                    println!(
                        "      {} {} session(s):",
                        "Claude:".bright_black(),
                        claude_sessions.len()
                    );
                    for session in claude_sessions.iter().take(3) {
                        let time_str = format_time_ago(session.last_timestamp);
                        let message = format_message_preview(&session.last_user_message, 60);

                        println!(
                            "        {} {} {}",
                            "-".bright_black(),
                            time_str.bright_black(),
                            message.bright_black()
                        );
                    }
                    if claude_sessions.len() > 3 {
                        println!(
                            "        {} ... and {} more",
                            "-".bright_black(),
                            claude_sessions.len() - 3
                        );
                    }
                }

                let (codex_sessions, codex_total) = codex::recent_sessions(&info.path, 3)?;
                if codex_total > 0 {
                    println!(
                        "      {} {} session(s):",
                        "Codex:".bright_black(),
                        codex_total
                    );
                    for session in &codex_sessions {
                        let time_str = format_time_ago(session.last_timestamp);
                        let message = session
                            .last_user_message
                            .as_deref()
                            .map(|msg| format_message_preview(msg, 60))
                            .unwrap_or_else(|| "(no user message)".to_string());

                        println!(
                            "        {} {} {}",
                            "-".bright_black(),
                            time_str.bright_black(),
                            message.bright_black()
                        );
                    }
                    if codex_total > codex_sessions.len() {
                        println!(
                            "        {} ... and {} more",
                            "-".bright_black(),
                            codex_total - codex_sessions.len()
                        );
                    }
                }
            }
            println!();
        }
    }

    Ok(())
}
