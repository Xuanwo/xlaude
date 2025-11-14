use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CodexSession {
    pub id: String,
    pub cwd: PathBuf,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub last_user_message: Option<String>,
}

fn sessions_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XLAUDE_CODEX_SESSIONS_DIR") {
        return Some(PathBuf::from(dir));
    }

    let home = std::env::var("HOME").ok()?;
    let root = Path::new(&home).join(".codex").join("sessions");
    Some(root)
}

fn normalized_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn normalized_worktree_path(path: &Path) -> PathBuf {
    normalized_path(path)
}

fn read_sorted_directories(path: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();

    if !path.exists() {
        return Ok(dirs);
    }

    for entry in path
        .read_dir()
        .with_context(|| format!("Failed to read Codex session directory: {}", path.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }

    dirs.sort_by(|a, b| match b.file_name().cmp(&a.file_name()) {
        Ordering::Equal => b.cmp(a),
        other => other,
    });

    Ok(dirs)
}

fn read_sorted_files(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if !path.exists() {
        return Ok(files);
    }

    for entry in path
        .read_dir()
        .with_context(|| format!("Failed to read Codex session directory: {}", path.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push(entry.path());
        }
    }

    files.sort_by(|a, b| match b.file_name().cmp(&a.file_name()) {
        Ordering::Equal => b.cmp(a),
        other => other,
    });

    Ok(files)
}

fn parse_session_file(path: &Path) -> Result<Option<CodexSession>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open Codex session file: {}", path.display()))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines().map_while(Result::ok);

    let first_line = match lines.next() {
        Some(line) => line,
        None => return Ok(None),
    };

    let meta = serde_json::from_str::<Value>(&first_line)
        .with_context(|| format!("Failed to parse session meta in {}", path.display()))?;

    if meta.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return Ok(None);
    }

    let payload = meta
        .get("payload")
        .and_then(|p| p.as_object())
        .ok_or_else(|| anyhow::anyhow!("Session payload is missing in {}", path.display()))?;

    let id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Session id missing in {}", path.display()))?
        .to_string();

    let cwd_str = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let cwd = PathBuf::from(cwd_str);

    let start_timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let mut last_user_message = None;
    let mut last_timestamp = start_timestamp;

    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if value.get("type").and_then(|t| t.as_str()) != Some("response_item") {
            continue;
        }

        let Some(payload) = value.get("payload").and_then(|p| p.as_object()) else {
            continue;
        };

        let role = payload
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or_default();
        let kind = payload
            .get("type")
            .and_then(|k| k.as_str())
            .unwrap_or_default();
        if role != "user" || kind != "message" {
            continue;
        }

        let message_timestamp = value
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&Utc));

        if let Some(ts) = message_timestamp
            && last_timestamp.is_none_or(|current| ts > current)
        {
            last_timestamp = Some(ts);
        }

        if let Some(msg) = extract_user_message(payload)
            && !msg.trim().is_empty()
        {
            last_user_message = Some(msg);
        }
    }

    Ok(Some(CodexSession {
        id,
        cwd,
        last_timestamp,
        last_user_message,
    }))
}

fn extract_user_message(payload: &serde_json::Map<String, Value>) -> Option<String> {
    let content = payload.get("content")?;

    if let Some(text) = content.as_array() {
        let mut segments = Vec::new();
        for node in text {
            if let Some(item) = node.as_object() {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    segments.push(text);
                } else if let Some(inner) = item.get("content").and_then(|c| c.as_str()) {
                    segments.push(inner);
                }
            }
        }
        if segments.is_empty() {
            None
        } else {
            Some(segments.join("\n"))
        }
    } else {
        content.as_str().map(|s| s.to_string())
    }
}

fn iterate_session_files(descending: bool) -> Result<Vec<PathBuf>> {
    let Some(root) = sessions_root() else {
        return Ok(Vec::new());
    };

    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();

    let mut years = read_sorted_directories(&root)?;
    if !descending {
        years.reverse();
    }

    for year in years {
        let mut months = read_sorted_directories(&year)?;
        if !descending {
            months.reverse();
        }
        for month in months {
            let mut days = read_sorted_directories(&month)?;
            if !descending {
                days.reverse();
            }
            for day in days {
                let mut files = read_sorted_files(&day)?;
                if !descending {
                    files.reverse();
                }
                result.extend(files);
            }
        }
    }

    Ok(result)
}

fn matches_worktree(session_path: &Path, target_canonical: &Path, fallback: &Path) -> bool {
    session_path
        .canonicalize()
        .map(|canonical| canonical == target_canonical)
        .unwrap_or(false)
        || session_path == fallback
}

pub fn find_latest_session(worktree_path: &Path) -> Result<Option<CodexSession>> {
    let files = iterate_session_files(true)?;
    if files.is_empty() {
        return Ok(None);
    }

    let target_canonical = normalized_path(worktree_path);

    for file in files {
        let Some(session) = parse_session_file(&file)? else {
            continue;
        };

        if matches_worktree(&session.cwd, &target_canonical, worktree_path) {
            return Ok(Some(session));
        }
    }

    Ok(None)
}

pub fn recent_sessions(worktree_path: &Path, limit: usize) -> Result<(Vec<CodexSession>, usize)> {
    let files = iterate_session_files(true)?;
    if files.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let target_canonical = normalized_path(worktree_path);
    let mut sessions = Vec::new();
    let mut total = 0usize;

    for file in files {
        let Some(session) = parse_session_file(&file)? else {
            continue;
        };

        if !matches_worktree(&session.cwd, &target_canonical, worktree_path) {
            continue;
        }

        total += 1;
        if limit != 0 && sessions.len() < limit {
            sessions.push(session);
        }
    }

    Ok((sessions, total))
}

pub fn collect_recent_sessions_for_paths(
    worktree_paths: &[PathBuf],
    limit: usize,
) -> Result<HashMap<PathBuf, Vec<CodexSession>>> {
    if worktree_paths.is_empty() || limit == 0 {
        return Ok(HashMap::new());
    }

    let files = iterate_session_files(true)?;
    if files.is_empty() {
        return Ok(HashMap::new());
    }

    let mut targets: HashSet<PathBuf> = HashSet::new();
    for path in worktree_paths {
        targets.insert(normalized_path(path));
    }

    let mut satisfied: HashSet<PathBuf> = HashSet::new();
    let mut map: HashMap<PathBuf, Vec<CodexSession>> = HashMap::new();

    for file in files {
        if satisfied.len() == targets.len() {
            break;
        }

        let Some(session) = parse_session_file(&file)? else {
            continue;
        };

        let normalized = normalized_path(&session.cwd);
        if !targets.contains(&normalized) {
            continue;
        }

        let entry = map.entry(normalized.clone()).or_insert_with(Vec::new);
        if entry.len() >= limit {
            satisfied.insert(normalized);
            continue;
        }

        entry.push(session);
        if entry.len() == limit {
            satisfied.insert(normalized);
        }
    }

    Ok(map)
}
