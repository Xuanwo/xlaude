use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::signal;

use shell_words::split as shell_split;

use crate::claude;
use crate::codex;
use crate::codex::CodexSession;
use crate::state::{WorktreeInfo, XlaudeState};

const STATIC_INDEX: &str = include_str!("../dashboard/static/index.html");
const DEFAULT_ADDR: &str = "127.0.0.1:5710";
const DEFAULT_SESSION_LIMIT: usize = 5;

#[derive(Clone)]
pub struct DashboardConfig {
    session_limit: usize,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            session_limit: DEFAULT_SESSION_LIMIT,
        }
    }
}

pub fn run_dashboard(address: Option<String>, auto_open: bool) -> Result<()> {
    let addr: SocketAddr = address
        .unwrap_or_else(|| DEFAULT_ADDR.to_string())
        .parse()
        .context("Invalid bind address for dashboard")?;

    let config = DashboardConfig::default();
    let runtime = tokio::runtime::Runtime::new().context("Failed to start async runtime")?;
    runtime.block_on(async move { start_server(addr, config, auto_open).await })
}

async fn start_server(addr: SocketAddr, config: DashboardConfig, auto_open: bool) -> Result<()> {
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/worktrees", get(api_worktrees))
        .route(
            "/api/worktrees/:repo/:name/actions",
            post(api_worktree_action),
        )
        .route(
            "/api/settings",
            get(api_get_settings).post(api_update_settings),
        )
        .with_state(config);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind dashboard listener")?;
    let actual_addr = listener
        .local_addr()
        .context("Failed to read listener address")?;

    println!("🚀 xlaude dashboard available at http://{actual_addr} (press Ctrl+C to stop)");

    if auto_open {
        let url = format!("http://{actual_addr}");
        if let Err(err) = webbrowser::open(&url) {
            eprintln!("⚠️  Unable to open browser automatically: {err}");
        }
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Dashboard server exited unexpectedly")?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    println!("👋 Stopping dashboard");
}

async fn serve_index() -> Html<&'static str> {
    Html(STATIC_INDEX)
}

async fn api_worktrees(State(config): State<DashboardConfig>) -> impl IntoResponse {
    let limit = config.session_limit;
    match tokio::task::spawn_blocking(move || build_dashboard_payload(limit)).await {
        Ok(Ok(payload)) => Json(payload).into_response(),
        Ok(Err(err)) => {
            eprintln!("[dashboard] failed to gather worktree info: {err:?}");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
        Err(err) => {
            eprintln!("[dashboard] worker thread panicked: {err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "dashboard worker panicked".to_string(),
            )
                .into_response()
        }
    }
}

async fn api_worktree_action(
    AxumPath((repo, name)): AxumPath<(String, String)>,
    Json(req): Json<ActionRequest>,
) -> impl IntoResponse {
    match handle_worktree_action(&repo, &name, req.action.as_str()) {
        Ok(response) => Json(response).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn api_get_settings() -> impl IntoResponse {
    match load_settings_payload() {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => {
            eprintln!("[dashboard] failed to load settings: {err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load settings".to_string(),
            )
                .into_response()
        }
    }
}

async fn api_update_settings(Json(req): Json<SettingsPayload>) -> impl IntoResponse {
    match update_settings_state(req) {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => {
            eprintln!("[dashboard] failed to update settings: {err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update settings".to_string(),
            )
                .into_response()
        }
    }
}

fn build_dashboard_payload(limit: usize) -> Result<DashboardPayload> {
    let state = XlaudeState::load()?;
    let worktree_paths: Vec<PathBuf> = state
        .worktrees
        .values()
        .map(|info| info.path.clone())
        .collect();

    let (codex_sessions, codex_error) =
        match codex::collect_recent_sessions_for_paths(&worktree_paths, limit) {
            Ok(map) => (map, None),
            Err(err) => {
                eprintln!("[dashboard] failed to collect Codex sessions: {err:?}");
                (HashMap::new(), Some(err.to_string()))
            }
        };

    let codex_context = CodexContext {
        sessions: codex_sessions,
        error: codex_error,
    };

    let mut worktrees: Vec<_> = state
        .worktrees
        .values()
        .map(|info| summarize_worktree(info, limit, &codex_context))
        .collect();

    worktrees.sort_by(|a, b| {
        a.repo_name
            .cmp(&b.repo_name)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(DashboardPayload {
        generated_at: Utc::now(),
        worktrees,
    })
}

fn summarize_worktree(
    info: &WorktreeInfo,
    limit: usize,
    codex_ctx: &CodexContext,
) -> WorktreeSummary {
    let git_status = summarize_git(&info.path);
    let claude_sessions = claude::get_claude_sessions(&info.path);
    let mut sessions = Vec::new();

    for session in claude_sessions.into_iter().take(limit) {
        sessions.push(SessionPreview {
            provider: "Claude".to_string(),
            message: Some(session.last_user_message),
            timestamp: session.last_timestamp,
        });
    }

    let session_error = codex_ctx.error.clone();
    if codex_ctx.error.is_none() {
        let normalized = codex::normalized_worktree_path(&info.path);
        if let Some(entries) = codex_ctx.sessions.get(&normalized) {
            for session in entries.iter().take(limit) {
                let fallback = format!("Session {}", short_session_id(session));
                let message = session.last_user_message.clone().unwrap_or(fallback);
                sessions.push(SessionPreview {
                    provider: "Codex".to_string(),
                    message: Some(message),
                    timestamp: session.last_timestamp,
                });
            }
        }
    }

    sessions.sort_by(|a, b| compare_option_desc(a.timestamp, b.timestamp));
    sessions.truncate(limit);

    let mut last_activity = info.created_at;
    if let Some(ts) = git_status.last_commit_time {
        if ts > last_activity {
            last_activity = ts;
        }
    }
    for entry in &sessions {
        if let Some(ts) = entry.timestamp
            && ts > last_activity
        {
            last_activity = ts;
        }
    }

    WorktreeSummary {
        key: format!("{}/{}", info.repo_name, info.name),
        repo_name: info.repo_name.clone(),
        name: info.name.clone(),
        branch: info.branch.clone(),
        path: info.path.display().to_string(),
        created_at: info.created_at,
        last_activity,
        git_status,
        sessions,
        session_error,
    }
}

fn load_settings_payload() -> Result<SettingsPayload> {
    let state = XlaudeState::load()?;
    Ok(SettingsPayload {
        editor: state.editor.clone(),
        terminal: state.shell.clone(),
    })
}

fn update_settings_state(req: SettingsPayload) -> Result<SettingsPayload> {
    let mut state = XlaudeState::load()?;
    state.editor = normalize_setting(req.editor);
    state.shell = normalize_setting(req.terminal);
    state.save()?;
    Ok(SettingsPayload {
        editor: state.editor.clone(),
        terminal: state.shell.clone(),
    })
}

fn normalize_setting(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn compare_option_desc(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Ordering {
    match (a, b) {
        (Some(a_ts), Some(b_ts)) => b_ts.cmp(&a_ts),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn short_session_id(session: &CodexSession) -> String {
    let id = &session.id;
    if id.len() <= 6 {
        id.clone()
    } else {
        id.chars()
            .rev()
            .take(6)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    }
}

struct CodexContext {
    sessions: HashMap<PathBuf, Vec<CodexSession>>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardPayload {
    generated_at: DateTime<Utc>,
    worktrees: Vec<WorktreeSummary>,
}

#[derive(Deserialize)]
struct ActionRequest {
    action: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionResponse {
    message: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SettingsPayload {
    editor: Option<String>,
    terminal: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeSummary {
    key: String,
    repo_name: String,
    name: String,
    branch: String,
    path: String,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    git_status: GitStatusSummary,
    sessions: Vec<SessionPreview>,
    session_error: Option<String>,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct GitStatusSummary {
    clean: bool,
    staged_files: usize,
    unstaged_files: usize,
    untracked_files: usize,
    conflict_files: usize,
    last_commit_message: Option<String>,
    last_commit_time: Option<DateTime<Utc>>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionPreview {
    provider: String,
    message: Option<String>,
    timestamp: Option<DateTime<Utc>>,
}

fn summarize_git(path: &Path) -> GitStatusSummary {
    if !path.exists() {
        return GitStatusSummary {
            error: Some("Worktree path missing".to_string()),
            ..Default::default()
        };
    }

    let mut summary = GitStatusSummary::default();

    match Command::new("git")
        .current_dir(path)
        .args(["status", "--short"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                apply_status_line(line, &mut summary);
            }
            summary.clean = summary.staged_files == 0
                && summary.unstaged_files == 0
                && summary.untracked_files == 0
                && summary.conflict_files == 0;
        }
        Ok(output) => {
            summary.error = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
            return summary;
        }
        Err(err) => {
            summary.error = Some(err.to_string());
            return summary;
        }
    }

    if let Some(commit) = read_last_commit(path) {
        summary.last_commit_message = Some(commit.message);
        summary.last_commit_time = Some(commit.timestamp);
    }

    summary
}

fn apply_status_line(line: &str, summary: &mut GitStatusSummary) {
    if line.starts_with("??") {
        summary.untracked_files += 1;
        return;
    }
    if line.starts_with("!!") {
        return;
    }

    let mut chars = line.chars();
    if let Some(first) = chars.next() {
        match first {
            ' ' => {}
            'U' => summary.conflict_files += 1,
            _ => summary.staged_files += 1,
        }
    }
    if let Some(second) = chars.next() {
        match second {
            ' ' => {}
            'U' => summary.conflict_files += 1,
            _ => summary.unstaged_files += 1,
        }
    }
}

struct CommitSummary {
    message: String,
    timestamp: DateTime<Utc>,
}

fn read_last_commit(path: &Path) -> Option<CommitSummary> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["log", "-1", "--pretty=format:%s%x1f%cI"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return None;
    }

    let mut parts = stdout.split('\u{1f}');
    let message = parts.next()?.trim().to_string();
    let timestamp_str = parts.next()?.trim();
    let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()?;

    Some(CommitSummary { message, timestamp })
}

fn handle_worktree_action(
    repo: &str,
    name: &str,
    action: &str,
) -> Result<ActionResponse, (StatusCode, String)> {
    let state = XlaudeState::load().map_err(|err| {
        eprintln!("[dashboard] failed to load state: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load state".to_string(),
        )
    })?;

    let key = XlaudeState::make_key(repo, name);
    let info = state.worktrees.get(&key).cloned().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Worktree '{repo}/{name}' not found"),
        )
    })?;

    let editor_override = state.editor.clone();
    let shell_override = state.shell.clone();

    match action {
        "open_agent" => launch_agent(&info).map(|_| ActionResponse {
            message: format!("Launching agent for {}/{}", info.repo_name, info.name),
        }),
        "open_shell" => launch_shell(&info, shell_override).map(|_| ActionResponse {
            message: format!("Opening shell in {}", info.path.display()),
        }),
        "open_editor" => launch_editor(&info.path, editor_override).map(|_| ActionResponse {
            message: format!("Opening editor for {}", info.path.display()),
        }),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("Unsupported action '{other}'"),
        )),
    }
}

fn editor_command(override_cmd: Option<String>) -> String {
    override_cmd
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("XLAUDE_DASHBOARD_EDITOR").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "code".to_string())
}

fn shell_command(override_cmd: Option<String>) -> String {
    override_cmd
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("XLAUDE_DASHBOARD_SHELL").ok())
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/zsh".to_string())
}

fn launch_agent(info: &WorktreeInfo) -> Result<(), (StatusCode, String)> {
    let exe = std::env::current_exe().map_err(|err| {
        eprintln!("[dashboard] failed to locate binary: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to locate xlaude binary".to_string(),
        )
    })?;

    Command::new(exe)
        .arg("open")
        .arg(&info.name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            eprintln!("[dashboard] failed to launch agent: {err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to launch agent".to_string(),
            )
        })
}

fn launch_shell(
    info: &WorktreeInfo,
    shell_override: Option<String>,
) -> Result<(), (StatusCode, String)> {
    let command = shell_command(shell_override);
    let mut parts = shell_split(&command).map_err(|err| {
        eprintln!("[dashboard] failed to parse shell command: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to parse shell command".to_string(),
        )
    })?;
    if parts.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Shell command is empty".to_string(),
        ));
    }

    let program = parts.remove(0);
    let mut cmd = Command::new(program);
    cmd.args(parts);
    cmd.current_dir(&info.path);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.spawn().map(|_| ()).map_err(|err| {
        eprintln!("[dashboard] failed to open shell: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to open shell".to_string(),
        )
    })
}

fn launch_editor(path: &Path, editor_override: Option<String>) -> Result<(), (StatusCode, String)> {
    let command = editor_command(editor_override);
    let mut parts = shell_split(&command).map_err(|err| {
        eprintln!("[dashboard] failed to parse editor command: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to parse editor command".to_string(),
        )
    })?;
    if parts.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Editor command is empty".to_string(),
        ));
    }

    let program = parts.remove(0);
    let mut cmd = Command::new(program);
    cmd.args(parts);
    cmd.arg(path);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.spawn().map_err(|err| {
        eprintln!("[dashboard] failed to spawn editor: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to open editor".to_string(),
        )
    })?;
    Ok(())
}
