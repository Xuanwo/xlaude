use std::fs;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

pub fn handle_config() -> Result<()> {
    let editor = std::env::var("EDITOR")
        .context("EDITOR environment variable is not set; please export your preferred editor")?;

    let parts = shell_words::split(&editor)
        .map_err(|e| anyhow!("Failed to parse EDITOR command: {editor} ({e})"))?;

    if parts.is_empty() {
        bail!("EDITOR command is empty");
    }

    let state_path = crate::state::get_state_path()?;
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    let mut cmd = Command::new(&parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    cmd.arg(&state_path);

    let status = cmd
        .status()
        .with_context(|| format!("Failed to launch editor: {}", parts[0]))?;

    if !status.success() {
        bail!(
            "Editor exited with status: {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "terminated by signal".to_string())
        );
    }

    Ok(())
}
