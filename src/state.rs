use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub repo_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct XlaudeState {
    pub worktrees: HashMap<String, WorktreeInfo>,
}

impl XlaudeState {
    pub fn load() -> Result<Self> {
        let config_path = get_config_path()?;
        if config_path.exists() {
            let content = fs::read_to_string(&config_path).context("Failed to read config file")?;
            serde_json::from_str(&content).context("Failed to parse config file")
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        let content = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
        fs::write(&config_path, content).context("Failed to write config file")?;
        Ok(())
    }
}

fn get_config_path() -> Result<PathBuf> {
    // Allow overriding config directory for testing
    if let Ok(config_dir) = std::env::var("XLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(config_dir).join("state.json"));
    }
    
    let proj_dirs = ProjectDirs::from("com", "xuanwo", "xlaude")
        .context("Failed to determine config directory")?;
    Ok(proj_dirs.config_dir().join("state.json"))
}
