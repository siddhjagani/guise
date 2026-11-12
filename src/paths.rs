//! Filesystem + resource paths for Claude Desktop and the guise account store.
//!
//! guise's model: each saved account is a **permanent, independent userData
//! directory** that Claude Desktop is launched against via `--user-data-dir`.
//! Accounts never share a directory, so logging into one never logs out
//! another — no snapshotting, no token rotation races, no revocation.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Where Claude Desktop is installed, unless `app-path` overrides it: the
/// `.app` bundle on macOS; on Windows the first Claude executable found among the
/// known install locations.
fn default_app_path(home: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        let local = env_dir("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
        first_existing(windows_app_candidates(&local))
    } else {
        PathBuf::from("/Applications/Claude.app")
    }
}

/// A directory named by an environment variable, if it is set.
fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Claude Desktop's possible Windows locations, most likely first.
fn windows_app_candidates(local: &Path) -> Vec<PathBuf> {
    let aliases = local.join("Microsoft").join("WindowsApps");
    vec![
        // The packaged (MSIX) app from claude.ai/download: its app execution
        // alias, which stays put across updates and passes arguments through.
        // The per-package copy can't resolve to a different build.
        aliases
            .join("Claude_pzs8sxrjxfjjc")
            .join("claude-desktop.exe"),
        aliases.join("claude-desktop.exe"),
        // The legacy per-user .exe install. Its top-level claude.exe survives
        // updates and forwards arguments to the current version.
        local.join("AnthropicClaude").join("claude.exe"),
    ]
}

/// The first candidate that exists, else the first (most likely) one, so a
/// "not found" message names the usual location.
fn first_existing(candidates: Vec<PathBuf>) -> PathBuf {
    let found = candidates.iter().position(|p| exists(p)).unwrap_or(0);
    candidates.into_iter().nth(found).unwrap_or_default()
}

/// Resolved, machine-specific locations guise operates on.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~` — the user's home directory. Retained for path construction/tests.
    #[allow(dead_code)]
    pub home: PathBuf,
    /// Claude Desktop: the `Claude.app` bundle (macOS) or the Claude executable (Windows).
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
            app: default_app_path(&home),
            guise_root,
        })
    }

    /// Is Claude Desktop at `app`? An `.app` bundle on macOS; on Windows an
    /// `.exe`, possibly an app execution alias (a link only
    /// `symlink_metadata` can see, so not `Path::exists`).
    pub fn app_present(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.app.is_dir()
        } else {
            exists(&self.app) && !self.app.is_dir()
        }
    }

    /// Fail with a helpful message unless Claude Desktop is at `app`.
    pub fn require_app(&self) -> Result<()> {
        if self.app_present() {
            return Ok(());
        }
        Err(anyhow!(
            "Claude Desktop not found at {} — install it from https://claude.ai/download, \
             or point guise at it with `guise config set app-path <path>` \
             (`auto` restores detection)",
            self.app.display()
        ))
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

    /// Default shared `claude-code-sessions` folder, used when meld has no
    /// `sessions_root`. macOS: Claude's standard location (also meld's
    /// default). Windows: a folder in guise's own store — outside AppData, so
    /// the MSIX app's file virtualization never redirects it; point meld's
    /// `sessions_root` at it.
    pub fn default_code_sessions_root(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home
                .join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude-code-sessions")
        } else {
            self.guise_root.join("claude-code-sessions")
        }
    }

    /// meld's config file, if the user has meld installed. meld finds its
    /// home via `$HOME` first, then the platform home, so do the same.
    pub fn meld_config(&self) -> PathBuf {
        env_dir("HOME")
            .unwrap_or_else(|| self.home.clone())
            .join(".meld")
            .join("config.toml")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_app_detection_prefers_the_packaged_app() {
        let local = std::env::temp_dir().join(format!("guise-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        let candidates = windows_app_candidates(&local);
        let (alias, legacy) = (candidates[1].clone(), candidates[2].clone());
        let touch = |p: &Path| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        };

        // Nothing installed: name the usual location.
        assert_eq!(first_existing(candidates.clone()), candidates[0]);
        // Only the legacy install: use it.
        touch(&legacy);
        assert_eq!(first_existing(candidates.clone()), legacy);
        // The packaged app wins over the legacy install.
        touch(&alias);
        assert_eq!(first_existing(candidates), alias);
        let _ = std::fs::remove_dir_all(&local);
    }
}
