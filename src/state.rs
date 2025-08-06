use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub repo_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct XlaudeState {
    // Key format: "{repo_name}/{worktree_name}"
    pub worktrees: HashMap<String, WorktreeInfo>,
}

impl XlaudeState {
    pub fn make_key(repo_name: &str, worktree_name: &str) -> String {
        format!("{repo_name}/{worktree_name}")
    }

    pub fn load() -> Result<Self> {
        let config_path = get_config_path()?;
        if config_path.exists() {
            // Open file with shared lock for reading
            let mut file = OpenOptions::new()
                .read(true)
                .open(&config_path)
                .context("Failed to open config file")?;

            // Acquire shared lock (blocks until available)
            file.lock_shared()
                .context("Failed to acquire shared lock on config file")?;

            let mut content = String::new();
            file.read_to_string(&mut content)
                .context("Failed to read config file")?;

            // Lock is automatically released when file is dropped
            drop(file);

            let mut state: Self =
                serde_json::from_str(&content).context("Failed to parse config file")?;

            // ============================================================================
            // MIGRATION LOGIC: Upgrade from v0.2 to v0.3 format
            // TODO: Remove this migration code after v0.3 is stable and most users have upgraded
            //
            // In v0.2, keys were just the worktree name: "feature-x"
            // In v0.3, keys include the repo name: "repo-name/feature-x"
            // ============================================================================
            let needs_migration = state.worktrees.keys().any(|k| !k.contains('/'));

            if needs_migration {
                eprintln!("🔄 Migrating xlaude state from v0.2 to v0.3 format...");

                let mut migrated_worktrees = HashMap::new();
                for (old_key, info) in state.worktrees {
                    // Check if this entry needs migration (doesn't contain '/')
                    let new_key = if old_key.contains('/') {
                        // Already in new format, keep as-is
                        old_key
                    } else {
                        // Old format, create new key
                        Self::make_key(&info.repo_name, &info.name)
                    };
                    migrated_worktrees.insert(new_key, info);
                }

                state.worktrees = migrated_worktrees;

                // Save the migrated state immediately
                state.save().context("Failed to save migrated state")?;
                eprintln!("✅ Migration completed successfully");
            }
            // ============================================================================
            // END OF MIGRATION LOGIC
            // ============================================================================

            Ok(state)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        // Serialize state first (before acquiring lock)
        let content = serde_json::to_string_pretty(self).context("Failed to serialize state")?;

        // Open or create file with exclusive lock for writing
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&config_path)
            .context("Failed to open config file for writing")?;

        // Acquire exclusive lock (blocks until available)
        file.lock()
            .context("Failed to acquire exclusive lock on config file")?;

        // Write content
        file.write_all(content.as_bytes())
            .context("Failed to write config file")?;
        file.flush().context("Failed to flush config file")?;

        // Lock is automatically released when file is dropped
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
