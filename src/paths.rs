//! Filesystem + resource paths for Claude Desktop and the guise account store.
//!
//! guise's model: each saved account is a **permanent, independent userData
//! directory** that Claude Desktop is launched against via `--user-data-dir`.
//! Accounts never share a directory, so logging into one never logs out
//! another — no snapshotting, no token rotation races, no revocation.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Default Claude Desktop location (`.app` bundle on macOS, executable on Windows).
#[cfg(target_os = "macos")]
pub const DEFAULT_APP_PATH: &str = "/Applications/Claude.app";
#[cfg(target_os = "windows")]
pub const DEFAULT_APP_PATH: &str = "C:\\Program Files\\AnthropicClaude\\Claude.exe";

/// Resolved, machine-specific locations guise operates on.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~` — the user's home directory. Retained for path construction/tests.
    #[allow(dead_code)]
    pub home: PathBuf,
    /// `Claude.app` bundle.
    pub app: PathBuf,
    /// Root of the account store (`~/.config/guise` or legacy `~/.guise`).
    pub guise_root: PathBuf,
}

impl Paths {
    /// Resolve all paths from the environment.
    ///
    /// If data exists at the legacy `~/.guise` location, it is automatically
    /// migrated to `~/.config/guise` (XDG-compliant). The lookup order is:
    ///   1. `$XDG_CONFIG_HOME/guise` (if `$XDG_CONFIG_HOME` is set)
    ///   2. `~/.config/guise`
    ///   3. `~/.guise` (legacy, triggers migration)
    pub fn resolve() -> Result<Self> {
        let home = home_dir()?;
        let guise_root = resolve_guise_root(&home)?;
        Ok(Paths {
            home: home.clone(),
            app: PathBuf::from(DEFAULT_APP_PATH),
            guise_root,
        })
    }

    /// The Claude Desktop executable. On Windows `app` is already the `.exe`;
    /// on macOS it's inside the bundle.
    pub fn app_binary(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.app.join("Contents").join("MacOS").join("Claude")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.app.clone()
        }
    }

    /// Directory holding one subdirectory per saved account.
    pub fn accounts_dir(&self) -> PathBuf {
        self.guise_root.join("accounts")
    }

    /// guise's own settings file.
    pub fn tool_config_json(&self) -> PathBuf {
        self.guise_root.join("config.json")
    }

    /// Claude's standard `claude-code-sessions` location (also meld's default
    /// `sessions_root`). Used as the shared target so `meld` can merge chats.
    pub fn default_code_sessions_root(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.home
                .join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude-code-sessions")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.home
                .join("AppData")
                .join("Roaming")
                .join("Claude")
                .join("claude-code-sessions")
        }
    }

    /// meld's config file, if the user has meld installed.
    pub fn meld_config(&self) -> PathBuf {
        self.home.join(".meld").join("config.toml")
    }
}

/// Resolve the home directory (`$HOME` on Unix, `%USERPROFILE%` on Windows).
pub fn home_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| {
                anyhow!("%USERPROFILE% (or $HOME) is not set; cannot locate the home directory")
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| anyhow!("$HOME is not set; cannot locate the home directory"))
    }
}

/// Whether a path exists (file, dir, or symlink).
pub fn exists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// Validate that a directory exists, returning a helpful error otherwise.
pub fn require_dir(p: &Path, what: &str) -> Result<()> {
    let md =
        std::fs::metadata(p).with_context(|| format!("{what} not found at {}", p.display()))?;
    if !md.is_dir() {
        return Err(anyhow!("{what} at {} is not a directory", p.display()));
    }
    Ok(())
}

/// Determine the guise root directory, migrating from `~/.guise` to
/// `~/.config/guise` if the legacy location exists and the new one doesn't.
fn resolve_guise_root(home: &Path) -> Result<PathBuf> {
    let xdg_root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(val) if !val.is_empty() => PathBuf::from(val).join("guise"),
        _ => home.join(".config").join("guise"),
    };
    let legacy_root = home.join(".guise");

    if xdg_root.exists() {
        return Ok(xdg_root);
    }

    if legacy_root.exists() {
        migrate_legacy_to_xdg(&legacy_root, &xdg_root)?;
        return Ok(xdg_root);
    }

    // Neither exists yet — use the XDG path (created on first `guise add`).
    Ok(xdg_root)
}

/// Move the legacy `~/.guise` tree to the XDG location atomically:
/// rename the directory, then leave a symlink at the old path so any
/// external scripts that still reference `~/.guise` keep working.
fn migrate_legacy_to_xdg(legacy: &Path, xdg: &Path) -> Result<()> {
    if let Some(parent) = xdg.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::rename(legacy, xdg)
        .with_context(|| format!("migrating {} -> {}", legacy.display(), xdg.display()))?;

    // Best-effort compatibility symlink: ~/.guise -> ~/.config/guise
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(xdg, legacy);
    }

    eprintln!(
        "guise: migrated data from {} -> {}",
        legacy.display(),
        xdg.display()
    );
    Ok(())
}
