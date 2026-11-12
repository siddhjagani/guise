//! Tool-level settings at `~/.config/guise/config.json`. Minimal: the instance model
//! removed almost all knobs (no relaunch/backup/sync toggles).

use crate::account::write_bytes_atomic;
use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Chat sharing is on by default on macOS only. On Windows it relies on a
/// junction inside the profile, which is untested against Claude Desktop's
/// check for redirected private folders (a failing check silently drops new
/// chats), so there it is opt-in.
fn default_share() -> bool {
    !cfg!(windows)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Override for where Claude Desktop is (`Claude.app` on macOS,
    /// the Claude executable on Windows), if not at the default.
    #[serde(default)]
    pub app_path: Option<String>,

    /// Point every account's `claude-code-sessions` at one shared folder so
    /// `meld` can merge Claude Code chats across accounts. On by default on
    /// macOS (off on Windows, see `default_share`); it's inert if you don't
    /// use meld (each account still only shows its own chats until meld
    /// copies the others in).
    /// Only an explicit `config set` is saved, so the platform default applies
    /// otherwise; read it through `sharing()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_code_sessions: Option<bool>,

    /// Where the shared `claude-code-sessions` lives. Defaults to meld's
    /// configured `sessions_root`, else Claude's standard location.
    #[serde(default)]
    pub code_sessions_root: Option<String>,
}

impl ToolConfig {
    /// Is chat sharing on (the explicit setting, else the platform default)?
    pub fn sharing(&self) -> bool {
        self.share_code_sessions.unwrap_or_else(default_share)
    }

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
