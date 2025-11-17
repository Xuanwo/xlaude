use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use axum::extract::{
    Path as AxumPath, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use once_cell::sync::Lazy;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::signal;
use tokio::sync::{Mutex, RwLock, broadcast};
use uuid::Uuid;

use shell_words::split as shell_split;

use crate::claude;
use crate::codex;
use crate::codex::CodexSession;
use crate::state::{WorktreeInfo, XlaudeState};
use crate::utils::prepare_agent_command;

const STATIC_INDEX: &str = include_str!("../dashboard/static/index.html");
const DEFAULT_ADDR: &str = "127.0.0.1:5710";
const DEFAULT_SESSION_LIMIT: usize = 5;
const SESSION_RETENTION_SECS: u64 = 300;
const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;
const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";

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
            "/api/worktrees/:repo/:name/live-session",
            post(api_resume_session),
        )
        .route("/api/sessions/:id/logs", get(api_get_session_logs))
        .route("/api/sessions/:id/send", post(api_send_session_message))
        .route("/api/sessions/:id/stream", get(api_stream_session))
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

async fn api_resume_session(
    AxumPath((repo, name)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    match start_live_session(&repo, &name).await {
        Ok(runtime) => {
            let events = runtime.snapshot().await;
            let response = StartSessionResponse {
                session_id: runtime.id().to_string(),
                events,
            };
            Json(response).into_response()
        }
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn api_get_session_logs(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    match get_session_runtime(&id).await {
        Some(runtime) => {
            let events = runtime.snapshot().await;
            Json(json!({ "sessionId": id, "events": events })).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

async fn api_send_session_message(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let Some(runtime) = get_session_runtime(&id).await else {
        return (StatusCode::NOT_FOUND, "Session not found").into_response();
    };

    let trimmed = req.message.trim();
    if trimmed.is_empty() {
        return (StatusCode::BAD_REQUEST, "Message cannot be empty").into_response();
    }

    runtime
        .push_message("user", "stdin", trimmed.to_string())
        .await;

    match runtime.write_stdin(trimmed).await {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(err) => {
            runtime
                .push_status("error", Some(format!("stdin write failed: {err}")))
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to write to session".to_string(),
            )
                .into_response()
        }
    }
}

async fn api_stream_session(
    AxumPath(id): AxumPath<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    match get_session_runtime(&id).await {
        Some(runtime) => ws.on_upgrade(move |socket| session_stream(socket, runtime)),
        None => (StatusCode::NOT_FOUND, "Session not found").into_response(),
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

async fn session_stream(socket: WebSocket, runtime: Arc<SessionRuntime>) {
    let (mut sender, mut receiver) = socket.split();
    for event in runtime.snapshot().await {
        if sender
            .send(Message::Text(
                serde_json::to_string(&event).unwrap_or_default(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut rx = runtime.subscribe();
    loop {
        tokio::select! {
            next = receiver.next() => {
                if matches!(next, None | Some(Err(_))) {
                    break;
                }
                if let Some(Ok(Message::Close(_))) = next {
                    break;
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(ev) => {
                        if sender.send(Message::Text(serde_json::to_string(&ev).unwrap_or_default())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

async fn start_live_session(
    repo: &str,
    name: &str,
) -> Result<Arc<SessionRuntime>, (StatusCode, String)> {
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

    if let Some(existing) = WORKTREE_SESSION_INDEX.read().await.get(&key).cloned()
        && let Some(runtime) = SESSION_REGISTRY.read().await.get(&existing).cloned()
    {
        return Ok(runtime);
    }

    let runtime = spawn_session(info).await.map_err(|err| {
        eprintln!("[dashboard] failed to spawn session: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to launch session".to_string(),
        )
    })?;

    WORKTREE_SESSION_INDEX
        .write()
        .await
        .insert(key.clone(), runtime.id().to_string());
    SESSION_REGISTRY
        .write()
        .await
        .insert(runtime.id().to_string(), runtime.clone());
    runtime.push_status("running", None).await;
    Ok(runtime)
}

async fn spawn_session(info: WorktreeInfo) -> Result<Arc<SessionRuntime>> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || spawn_session_blocking(info, handle))
        .await
        .context("spawn blocking session task failed")?
}

fn spawn_session_blocking(
    info: WorktreeInfo,
    handle: tokio::runtime::Handle,
) -> Result<Arc<SessionRuntime>> {
    let worktree_key = XlaudeState::make_key(&info.repo_name, &info.name);
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: PTY_ROWS,
        cols: PTY_COLS,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let (program, args) =
        prepare_agent_command(&info.path).context("Failed to resolve agent command")?;
    let mut builder = CommandBuilder::new(program);
    for arg in args {
        builder.arg(arg);
    }
    builder.cwd(info.path.clone());
    builder.env_clear();
    for (key, value) in std::env::vars() {
        builder.env(&key, value);
    }

    let mut child = pair
        .slave
        .spawn_command(builder)
        .context("Failed to spawn agent")?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("Failed to capture PTY writer")?;

    let runtime = Arc::new(SessionRuntime::new(worktree_key.clone(), writer));

    let reader_runtime = runtime.clone();
    let reader_handle = handle.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let (cleaned, responses) = scrub_terminal_queries(&buf[..n]);
                    for response in responses {
                        let runtime = reader_runtime.clone();
                        let handle = reader_handle.clone();
                        handle.spawn(async move {
                            if let Err(err) = runtime.write_bytes(response).await {
                                eprintln!("[dashboard] failed to send terminal response: {err:?}");
                            }
                        });
                    }
                    if cleaned.is_empty() {
                        continue;
                    }
                    let chunk = String::from_utf8_lossy(&cleaned).to_string();
                    let runtime = reader_runtime.clone();
                    reader_handle.spawn(async move {
                        runtime.push_message("assistant", "stdout", chunk).await;
                    });
                }
                Err(err) => {
                    let runtime = reader_runtime.clone();
                    reader_handle.spawn(async move {
                        runtime
                            .push_status("error", Some(format!("read error: {err}")))
                            .await;
                    });
                    break;
                }
            }
        }
    });

    let wait_runtime = runtime.clone();
    let wait_handle = handle.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) => {
            let mut detail = format!("exit code {}", status.exit_code());
            if !status.success() {
                detail.push_str(" (failed)");
            }
            let id = wait_runtime.id().to_string();
            let key = wait_runtime.worktree_key().to_string();
            wait_handle.spawn(async move {
                wait_runtime.push_status("stopped", Some(detail)).await;
                WORKTREE_SESSION_INDEX.write().await.remove(&key);
                schedule_session_cleanup(id).await;
            });
        }
        Err(err) => {
            let id = wait_runtime.id().to_string();
            let key = wait_runtime.worktree_key().to_string();
            wait_handle.spawn(async move {
                wait_runtime
                    .push_status("stopped", Some(format!("wait error: {err}")))
                    .await;
                WORKTREE_SESSION_INDEX.write().await.remove(&key);
                schedule_session_cleanup(id).await;
            });
        }
    });

    Ok(runtime)
}

async fn get_session_runtime(id: &str) -> Option<Arc<SessionRuntime>> {
    SESSION_REGISTRY.read().await.get(id).cloned()
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
    if let Some(ts) = git_status.last_commit_time
        && ts > last_activity
    {
        last_activity = ts;
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
struct StartSessionResponse {
    session_id: String,
    events: Vec<SessionEvent>,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    message: String,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionEvent {
    sequence: u64,
    timestamp: DateTime<Utc>,
    kind: String,
    role: Option<String>,
    channel: Option<String>,
    text: Option<String>,
    status: Option<String>,
    detail: Option<String>,
}

impl SessionEvent {
    fn message(sequence: u64, role: &str, channel: &str, text: String) -> Self {
        Self {
            sequence,
            timestamp: Utc::now(),
            kind: "message".to_string(),
            role: Some(role.to_string()),
            channel: Some(channel.to_string()),
            text: Some(text),
            status: None,
            detail: None,
        }
    }

    fn status(sequence: u64, status: &str, detail: Option<String>) -> Self {
        Self {
            sequence,
            timestamp: Utc::now(),
            kind: "status".to_string(),
            role: None,
            channel: None,
            text: None,
            status: Some(status.to_string()),
            detail,
        }
    }
}

struct SessionRuntime {
    id: String,
    worktree_key: String,
    log: Mutex<Vec<SessionEvent>>,
    counter: AtomicU64,
    tx: broadcast::Sender<SessionEvent>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl SessionRuntime {
    fn new(worktree_key: String, writer: Box<dyn Write + Send>) -> Self {
        let (tx, _rx) = broadcast::channel(512);
        Self {
            id: Uuid::new_v4().to_string(),
            worktree_key,
            log: Mutex::new(Vec::new()),
            counter: AtomicU64::new(0),
            tx,
            writer: Mutex::new(Some(writer)),
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn worktree_key(&self) -> &str {
        &self.worktree_key
    }

    fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.tx.subscribe()
    }

    async fn snapshot(&self) -> Vec<SessionEvent> {
        self.log.lock().await.clone()
    }

    async fn push_message(&self, role: &str, channel: &str, text: String) {
        let event = SessionEvent::message(
            self.counter.fetch_add(1, AtomicOrdering::SeqCst),
            role,
            channel,
            text,
        );
        self.push_event(event).await;
    }

    async fn push_status(&self, status: &str, detail: Option<String>) {
        let event = SessionEvent::status(
            self.counter.fetch_add(1, AtomicOrdering::SeqCst),
            status,
            detail,
        );
        self.push_event(event).await;
    }

    async fn push_event(&self, event: SessionEvent) {
        self.log.lock().await.push(event.clone());
        let _ = self.tx.send(event);
    }

    async fn write_stdin(&self, text: &str) -> Result<()> {
        let mut payload = text.as_bytes().to_vec();
        if !payload.ends_with(b"\n") {
            payload.push(b'\n');
        }
        self.write_bytes(payload).await
    }

    async fn write_bytes(&self, payload: Vec<u8>) -> Result<()> {
        let mut guard = self.writer.lock().await;
        let writer = guard
            .as_mut()
            .ok_or_else(|| anyhow!("session stdin is closed"))?;
        writer.write_all(&payload)?;
        writer.flush()?;
        Ok(())
    }
}

static SESSION_REGISTRY: Lazy<RwLock<HashMap<String, Arc<SessionRuntime>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static WORKTREE_SESSION_INDEX: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn summarize_git(path: &Path) -> GitStatusSummary {
    if !path.exists() {
        return GitStatusSummary {
            error: Some("Worktree path missing".to_string()),
            ..Default::default()
        };
    }

    let mut summary = GitStatusSummary::default();

    match StdCommand::new("git")
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
    let output = StdCommand::new("git")
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

    StdCommand::new(exe)
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
    let mut cmd = StdCommand::new(program);
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
    let mut cmd = StdCommand::new(program);
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
async fn schedule_session_cleanup(id: String) {
    let retention = Duration::from_secs(SESSION_RETENTION_SECS);
    tokio::spawn(async move {
        tokio::time::sleep(retention).await;
        SESSION_REGISTRY.write().await.remove(&id);
    });
}

fn scrub_terminal_queries(chunk: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut cleaned = Vec::with_capacity(chunk.len());
    let mut responses = Vec::new();
    let mut index = 0;
    while index < chunk.len() {
        if chunk[index..].starts_with(CURSOR_POSITION_QUERY) {
            responses.push(cursor_position_response());
            index += CURSOR_POSITION_QUERY.len();
            continue;
        }
        cleaned.push(chunk[index]);
        index += 1;
    }
    (cleaned, responses)
}

fn cursor_position_response() -> Vec<u8> {
    format!("\x1b[{};{}R", PTY_ROWS, PTY_COLS).into_bytes()
}
