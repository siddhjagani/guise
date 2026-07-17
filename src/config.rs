//! Tool-level settings at `~/.guise/config.json`. Minimal: the instance model
//! removed almost all knobs (no relaunch/backup/sync toggles).

use crate::account::write_bytes_atomic;
use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Override for the `Claude.app` bundle path, if not at the default.
    #[serde(default)]
    pub app_path: Option<String>,
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
