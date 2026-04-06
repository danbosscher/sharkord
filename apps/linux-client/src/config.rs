use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

const APP_CONFIG_DIR: &str = "sharkord-linux-client";
const APP_CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedSession {
    pub server: String,
    pub username: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub last_channel_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredConfig {
    pub server: String,
    pub username: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub last_channel_id: Option<u64>,
    #[serde(default)]
    pub sessions: Vec<SavedSession>,
}

impl StoredConfig {
    pub fn saved_session(&self, server: &str, username: &str) -> Option<&SavedSession> {
        self.sessions
            .iter()
            .find(|session| session.server == server && session.username == username)
    }

    pub fn upsert_saved_session(&mut self, saved_session: SavedSession) {
        if let Some(existing_session) = self.sessions.iter_mut().find(|session| {
            session.server == saved_session.server && session.username == saved_session.username
        }) {
            *existing_session = saved_session;
        } else {
            self.sessions.push(saved_session);
        }
    }

    pub fn clear_saved_session(&mut self, server: &str, username: &str) {
        if let Some(existing_session) = self
            .sessions
            .iter_mut()
            .find(|session| session.server == server && session.username == username)
        {
            existing_session.auth_token = None;
            existing_session.last_channel_id = None;
        }
    }

    pub fn selected_saved_session(&self) -> Option<SavedSession> {
        self.saved_session(&self.server, &self.username)
            .cloned()
            .or_else(|| {
                (!self.server.is_empty() || !self.username.is_empty()).then_some(SavedSession {
                    server: self.server.clone(),
                    username: self.username.clone(),
                    auth_token: self.auth_token.clone(),
                    last_channel_id: self.last_channel_id,
                })
            })
    }

    fn normalize_legacy_fields(&mut self) {
        if !self.server.is_empty() || !self.username.is_empty() {
            let legacy_session = SavedSession {
                server: self.server.clone(),
                username: self.username.clone(),
                auth_token: self.auth_token.clone(),
                last_channel_id: self.last_channel_id,
            };

            self.upsert_saved_session(legacy_session);
        }
    }
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
    let mut parsed: StoredConfig = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;
    parsed.normalize_legacy_fields();

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
