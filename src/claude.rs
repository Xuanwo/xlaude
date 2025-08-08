use chrono::{DateTime, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug)]
pub struct SessionInfo {
    pub last_user_message: String,
    pub last_timestamp: Option<DateTime<Utc>>,
}

pub fn get_claude_sessions(project_path: &Path) -> Vec<SessionInfo> {
    // Get home directory
    let Ok(home) = std::env::var("HOME") else {
        return vec![];
    };

    // Construct path to Claude projects directory
    let claude_projects_dir = Path::new(&home).join(".claude").join("projects");

    // Get canonical path of the project
    let Ok(canonical_path) = project_path.canonicalize() else {
        return vec![];
    };

    // Convert path to Claude's format (replace / with -)
    let encoded_path = canonical_path.to_string_lossy().replace('/', "-");

    let project_dir = claude_projects_dir.join(&encoded_path);

    // List session files (.jsonl files)
    let mut sessions = vec![];
    if let Ok(entries) = fs::read_dir(&project_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
            {
                // Read session data from the file
                let mut last_user_message = String::new();
                let mut last_timestamp = None;

                if let Ok(file) = fs::File::open(entry.path()) {
                    let reader = BufReader::new(file);
                    let mut user_messages = Vec::new();

                    for line in reader.lines().map_while(Result::ok) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line)
                            && json.get("type").and_then(|t| t.as_str()) == Some("user")
                        {
                            // Extract timestamp
                            if let Some(ts_str) = json.get("timestamp").and_then(|t| t.as_str())
                                && let Ok(ts) = DateTime::parse_from_rfc3339(ts_str)
                            {
                                last_timestamp = Some(ts.with_timezone(&Utc));
                            }

                            // Extract message content
                            if let Some(message) = json.get("message") {
                                let content =
                                    message.get("content").and_then(|c| c.as_str()).map_or_else(
                                        || {
                                            message
                                                .get("content")
                                                .and_then(|c| c.as_array())
                                                .map_or_else(String::new, |content_arr| {
                                                    content_arr
                                                        .iter()
                                                        .filter_map(|item| {
                                                            item.get("text")
                                                                .and_then(|t| t.as_str())
                                                        })
                                                        .collect::<Vec<_>>()
                                                        .join(" ")
                                                })
                                        },
                                        std::string::ToString::to_string,
                                    );

                                // Filter out system messages and empty content
                                if !content.is_empty()
                                    && !content.starts_with("<local-command")
                                    && !content.starts_with("<command-")
                                    && !content.starts_with("Caveat:")
                                    && !content.contains("[Request interrupted")
                                {
                                    user_messages.push(content);
                                }
                            }
                        }
                    }

                    // Get the last meaningful user message
                    if let Some(msg) = user_messages.last() {
                        last_user_message.clone_from(msg);
                    }
                }

                // Only add sessions with user messages
                if !last_user_message.is_empty() {
                    sessions.push(SessionInfo {
                        last_user_message,
                        last_timestamp,
                    });
                }
            }
        }
    }

    // Sort by timestamp (most recent first)
    sessions.sort_by(|a, b| match (&b.last_timestamp, &a.last_timestamp) {
        (Some(b_ts), Some(a_ts)) => b_ts.cmp(a_ts),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    sessions
}
