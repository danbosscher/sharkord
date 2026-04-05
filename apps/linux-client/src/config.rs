use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

const APP_CONFIG_DIR: &str = "sharkord-linux-client";
const APP_CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredConfig {
    pub server: String,
    pub username: String,
    pub auth_token: Option<String>,
    pub last_channel_id: Option<u64>,
}

fn config_dir() -> Result<PathBuf> {
    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg_config_home).join(APP_CONFIG_DIR));
    }

    let home = env::var_os("HOME").context("HOME is not set")?;

    Ok(PathBuf::from(home).join(".config").join(APP_CONFIG_DIR))
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(APP_CONFIG_FILE))
}

pub fn load_config() -> Result<Option<StoredConfig>> {
    let path = config_path()?;

    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    let parsed = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;

    Ok(Some(parsed))
}

pub fn save_config(config: &StoredConfig) -> Result<()> {
    let path = config_path()?;
    let dir = path
        .parent()
        .context("config path missing parent directory")?;

    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create config dir {}", dir.display()))?;
    fs::write(&path, serde_json::to_vec_pretty(config)?)
        .with_context(|| format!("failed to write config file {}", path.display()))?;

    Ok(())
}
