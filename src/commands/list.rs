use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::claude::get_claude_sessions;
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
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonOutput {
    worktrees: Vec<JsonWorktreeInfo>,
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
            let sessions = get_claude_sessions(&info.path);
            let json_sessions: Vec<JsonSessionInfo> = sessions
                .into_iter()
                .map(|session| {
                    let time_ago = session.last_timestamp.as_ref().map_or_else(
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

                    JsonSessionInfo {
                        last_user_message: session.last_user_message,
                        last_timestamp: session.last_timestamp,
                        time_ago,
                    }
                })
                .collect();

            worktrees.push(JsonWorktreeInfo {
                name: info.name.clone(),
                branch: info.branch.clone(),
                path: info.path.display().to_string(),
                repo_name: info.repo_name.clone(),
                created_at: info.created_at,
                sessions: json_sessions,
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
                let sessions = get_claude_sessions(&info.path);
                if !sessions.is_empty() {
                    println!(
                        "      {} {} session(s):",
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
                            "        {} {} {}",
                            "-".bright_black(),
                            time_str.bright_black(),
                            message.bright_black()
                        );
                    }
                    if sessions.len() > 3 {
                        println!(
                            "        {} ... and {} more",
                            "-".bright_black(),
                            sessions.len() - 3
                        );
                    }
                }
            }
            println!();
        }
    }

    Ok(())
}
