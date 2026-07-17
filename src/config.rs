//! Tool-level settings at `~/.guise/config.json`. Minimal: the instance model
//! removed almost all knobs (no relaunch/backup/sync toggles).

use crate::account::write_bytes_atomic;
use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Override for the `Claude.app` bundle path, if not at the default.
    #[serde(default)]
    pub app_path: Option<String>,

    /// Point every account's `claude-code-sessions` at one shared folder so
    /// `meld` can merge Claude Code chats across accounts. On by default; it's
    /// inert if you don't use meld (each account still only shows its own
    /// chats until meld copies the others in). See README "Works with meld".
    #[serde(default = "default_true")]
    pub share_code_sessions: bool,

    /// Where the shared `claude-code-sessions` lives. Defaults to meld's
    /// configured `sessions_root`, else Claude's standard location.
    #[serde(default)]
    pub code_sessions_root: Option<String>,
}

impl Default for ToolConfig {
    fn default() -> Self {
        ToolConfig {
            app_path: None,
            share_code_sessions: true,
            code_sessions_root: None,
        }
    }
}

impl ToolConfig {
    pub fn load(paths: &Paths) -> Result<Self> {
        let p = paths.tool_config_json();
        if !p.exists() {
            return Ok(ToolConfig::default());
        }
        let raw =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", p.display()))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        std::fs::create_dir_all(&paths.guise_root)
            .with_context(|| format!("creating {}", paths.guise_root.display()))?;
        let body = serde_json::to_vec_pretty(self)?;
        write_bytes_atomic(&paths.tool_config_json(), &body)
    }
}
