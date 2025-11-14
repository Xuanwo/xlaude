use anyhow::{Context, Result};
use rand::seq::IndexedRandom;
use rand::{RngCore, SeedableRng};
use std::path::Path;

pub fn generate_random_name() -> Result<String> {
    // Allow setting seed for testing
    let mut rng = if let Ok(seed_str) = std::env::var("XLAUDE_TEST_SEED") {
        let seed: u64 = seed_str.parse().unwrap_or(42);
        Box::new(rand::rngs::StdRng::seed_from_u64(seed)) as Box<dyn RngCore>
    } else {
        Box::new(rand::rng()) as Box<dyn RngCore>
    };

    // Generate 128 bits of entropy for a 12-word mnemonic
    let mut entropy = [0u8; 16];
    rng.fill_bytes(&mut entropy);

    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)?;
    let words: Vec<&str> = mnemonic.words().collect();

    // Use the same RNG for choosing the word
    let mut chooser_rng = if let Ok(seed_str) = std::env::var("XLAUDE_TEST_SEED") {
        let seed: u64 = seed_str.parse().unwrap_or(42);
        rand::rngs::StdRng::seed_from_u64(seed)
    } else {
        let mut entropy_rng = rand::rng();
        rand::rngs::StdRng::from_rng(&mut entropy_rng)
    };

    words
        .choose(&mut chooser_rng)
        .map(|&word| word.to_string())
        .context("Failed to generate random name")
}

/// Sanitize a branch name for use in directory names
/// Replaces forward slashes with hyphens to avoid creating subdirectories
pub fn sanitize_branch_name(branch: &str) -> String {
    branch.replace('/', "-")
}

pub fn execute_in_dir<P, F, R>(path: P, f: F) -> Result<R>
where
    P: AsRef<Path>,
    F: FnOnce() -> Result<R>,
{
    let original_dir = std::env::current_dir().context("Failed to get current directory")?;
    std::env::set_current_dir(&path)
        .with_context(|| format!("Failed to change to directory: {}", path.as_ref().display()))?;

    let result = f();

    std::env::set_current_dir(&original_dir).context("Failed to restore original directory")?;

    result
}

/// Resolve agent command from state or default, and split into program + args.
pub fn resolve_agent_command() -> Result<(String, Vec<String>)> {
    let state = crate::state::XlaudeState::load()?;
    let cmdline = state
        .agent
        .clone()
        .unwrap_or_else(crate::state::get_default_agent);

    // Use shell-style splitting to handle quotes and spaces.
    let parts = shell_words::split(&cmdline)
        .map_err(|e| anyhow::anyhow!("Invalid agent command: {} ({e})", cmdline))?;

    if parts.is_empty() {
        anyhow::bail!("Agent command is empty");
    }

    let program = parts[0].clone();
    let args = parts[1..].to_vec();
    Ok((program, args))
}

const CODEX_OPTIONS_WITH_VALUES: &[&str] = &[
    "-c",
    "--config",
    "--enable",
    "--disable",
    "-i",
    "--image",
    "-m",
    "--model",
    "-p",
    "--profile",
    "-s",
    "--sandbox",
    "-a",
    "--ask-for-approval",
    "--add-dir",
    "-C",
    "--cd",
];

fn codex_has_positional_arguments(args: &[String]) -> bool {
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--" {
            return index + 1 < args.len();
        }

        let (option_name, has_inline_value) = match arg.split_once('=') {
            Some((name, value)) => (name, !value.is_empty()),
            None => (arg.as_str(), false),
        };

        if CODEX_OPTIONS_WITH_VALUES.contains(&option_name) {
            if !has_inline_value {
                index += 1;
            }
            index += 1;
            continue;
        }

        if arg.starts_with('-') {
            index += 1;
            continue;
        }

        return true;
    }

    false
}

pub fn prepare_agent_command(worktree_path: &Path) -> Result<(String, Vec<String>)> {
    let (program, args) = resolve_agent_command()?;

    if !program.eq_ignore_ascii_case("codex") {
        return Ok((program, args));
    }

    if codex_has_positional_arguments(&args) {
        return Ok((program, args));
    }

    let Some(session) = crate::codex::find_latest_session(worktree_path)? else {
        return Ok((program, args));
    };

    let mut new_args = args;
    new_args.push("resume".to_string());
    new_args.push(session.id);

    Ok((program, new_args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn prepare_agent_command_resumes_latest_codex_session() {
        let _guard = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();

        let config_dir = TempDir::new().unwrap();
        let sessions_dir = TempDir::new().unwrap();
        let worktree_dir = TempDir::new().unwrap();

        fs::create_dir_all(config_dir.path()).unwrap();
        fs::create_dir_all(sessions_dir.path()).unwrap();

        let state = json!({
            "worktrees": {},
            "agent": "codex"
        });
        fs::write(
            config_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();

        let worktree_path = worktree_dir.path().canonicalize().unwrap();
        let worktree_str = worktree_path.to_string_lossy().to_string();

        let session_dir = sessions_dir.path().join("2025").join("10").join("27");
        fs::create_dir_all(&session_dir).unwrap();

        let session_meta = json!({
            "timestamp": "2025-10-27T05:29:08.620Z",
            "type": "session_meta",
            "payload": {
                "id": "session-123",
                "timestamp": "2025-10-27T05:29:08.601Z",
                "cwd": worktree_str,
                "originator": "codex_cli_rs",
                "cli_version": "0.50.0"
            }
        });

        let user_message = json!({
            "timestamp": "2025-10-27T05:30:00.000Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "resume me"
                    }
                ]
            }
        });

        fs::write(
            session_dir.join("rollout-test.jsonl"),
            format!("{session_meta}\n{user_message}\n"),
        )
        .unwrap();

        let config_dir_str = config_dir.path().to_string_lossy().to_string();
        let sessions_dir_str = sessions_dir.path().to_string_lossy().to_string();

        temp_env::with_vars(
            [
                ("XLAUDE_CONFIG_DIR", Some(config_dir_str.as_str())),
                ("XLAUDE_CODEX_SESSIONS_DIR", Some(sessions_dir_str.as_str())),
            ],
            || {
                let (program, args) = prepare_agent_command(&worktree_path).unwrap();
                assert_eq!(program, "codex");
                assert_eq!(args, vec!["resume".to_string(), "session-123".to_string()]);
            },
        );
    }
}
