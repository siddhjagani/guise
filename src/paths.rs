//! Filesystem + resource paths for Claude Desktop and the guise account store.
//!
//! guise's model: each saved account is a **permanent, independent userData
//! directory** that Claude Desktop is launched against via `--user-data-dir`.
//! Accounts never share a directory, so logging into one never logs out
//! another — no snapshotting, no token rotation races, no revocation.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Default macOS application bundle location.
pub const DEFAULT_APP_PATH: &str = "/Applications/Claude.app";

/// Resolved, machine-specific locations guise operates on.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~` — the user's home directory. Retained for path construction/tests.
    #[allow(dead_code)]
    pub home: PathBuf,
    /// `Claude.app` bundle.
    pub app: PathBuf,
    /// `~/.guise` root of the account store.
    pub guise_root: PathBuf,
}

impl Paths {
    /// Resolve all paths from the environment.
    pub fn resolve() -> Result<Self> {
        let home = home_dir()?;
        Ok(Paths {
            home: home.clone(),
            app: PathBuf::from(DEFAULT_APP_PATH),
            guise_root: home.join(".guise"),
        })
    }

    /// The Claude Desktop executable inside the bundle.
    pub fn app_binary(&self) -> PathBuf {
        self.app.join("Contents").join("MacOS").join("Claude")
    }

    /// Directory holding one subdirectory per saved account.
    pub fn accounts_dir(&self) -> PathBuf {
        self.guise_root.join("accounts")
    }

    /// guise's own settings file.
    pub fn tool_config_json(&self) -> PathBuf {
        self.guise_root.join("config.json")
    }
}

/// Resolve the home directory from `$HOME`.
pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("$HOME is not set; cannot locate the home directory"))
}

/// Whether a path exists (file, dir, or symlink).
pub fn exists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// Validate that a directory exists, returning a helpful error otherwise.
pub fn require_dir(p: &Path, what: &str) -> Result<()> {
    let md = std::fs::metadata(p).with_context(|| format!("{what} not found at {}", p.display()))?;
    if !md.is_dir() {
        return Err(anyhow!("{what} at {} is not a directory", p.display()));
    }
    Ok(())
}
