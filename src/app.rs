//! Claude Desktop process control for the per-account instance model.
//!
//! Each account runs as its own Claude process launched with
//! `--user-data-dir=<account data dir>`. We detect a specific account's
//! instance by matching that flag in the process command line, launch it
//! detached, bring the app to the foreground, and quit a specific instance.
//!
//! Behind an `AppControl` trait so the logic is testable with a fake.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Control over Claude Desktop instances. Testable via a fake.
pub trait AppControl {
    /// Launch a Claude instance bound to `data_dir`. `app_bundle` is the
    /// `.app` path. Fully detached from the caller's terminal.
    fn launch_instance(&self, app_bundle: &Path, data_dir: &Path) -> Result<()>;
    /// Is a Claude instance for exactly this `data_dir` currently running?
    fn is_instance_running(&self, data_dir: &Path) -> Result<bool>;
    /// Bring Claude to the foreground.
    fn activate(&self, app_bundle: &Path) -> Result<()>;
    /// Quit the Claude instance bound to `data_dir` (gracefully, then firmly).
    fn quit_instance(&self, data_dir: &Path, timeout: Duration) -> Result<()>;
}

/// Production implementation talking to the real OS.
pub struct RealApp;

impl AppControl for RealApp {
    fn launch_instance(&self, app_bundle: &Path, data_dir: &Path) -> Result<()> {
        // `open -n` starts a fresh instance and hands it entirely to
        // LaunchServices — so it does NOT stay tied to the caller's terminal
        // session (an earlier direct-binary launch did, which stole focus and
        // could take the terminal down with it). `--args` passes the data dir
        // through to Electron.
        let status = Command::new("open")
            .arg("-n")
            .arg("-a")
            .arg(app_bundle)
            .arg("--args")
            .arg(format!("--user-data-dir={}", data_dir.display()))
            .status()
            .context("launching Claude instance via `open`")?;
        if !status.success() {
            return Err(anyhow!("failed to launch Claude for {}", data_dir.display()));
        }
        Ok(())
    }

    fn is_instance_running(&self, data_dir: &Path) -> Result<bool> {
        Ok(!pids_for_data_dir(data_dir)?.is_empty())
    }

    fn activate(&self, app_bundle: &Path) -> Result<()> {
        // Focus the app without opening a new instance. Best-effort.
        let _ = Command::new("open").arg("-a").arg(app_bundle).status();
        Ok(())
    }

    fn quit_instance(&self, data_dir: &Path, timeout: Duration) -> Result<()> {
        let pids = pids_for_data_dir(data_dir)?;
        if pids.is_empty() {
            return Ok(());
        }
        for pid in &pids {
            let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).output();
        }
        let deadline = Instant::now() + timeout;
        loop {
            if pids_for_data_dir(data_dir)?.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                for pid in pids_for_data_dir(data_dir)? {
                    let _ = Command::new("kill").arg("-KILL").arg(pid.to_string()).output();
                }
                return Ok(());
            }
            sleep(Duration::from_millis(200));
        }
    }
}

/// PIDs whose command line binds them to exactly this userData dir. Matches the
/// full `--user-data-dir=<dir>` token so one account's helpers never match
/// another's, and excludes guise itself.
fn pids_for_data_dir(data_dir: &Path) -> Result<Vec<i32>> {
    let needle = format!("--user-data-dir={}", data_dir.display());
    let out = Command::new("pgrep")
        .arg("-f")
        .arg(&needle)
        .output()
        .context("running pgrep")?;
    let me = std::process::id() as i32;
    let pids = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|s| s.parse::<i32>().ok())
        .filter(|&p| p != me)
        .collect();
    Ok(pids)
}

/// Construct the production app controller.
pub fn control() -> RealApp {
    RealApp
}
