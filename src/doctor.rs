//! `guise doctor` — environment + account diagnostics. Read-only.

use crate::account;
use crate::app::{self, AppControl};
use crate::config::ToolConfig;
use crate::paths::Paths;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct AccountHealth {
    pub slot: u32,
    pub name: String,
    pub email: Option<String>,
    pub running: bool,
    pub initialized: bool,
    /// Chats symlinked into the shared meld folder.
    pub chats_shared: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub accounts: Vec<AccountHealth>,
    pub healthy: bool,
}

pub fn run(paths: &Paths) -> Result<Report> {
    let mut checks = Vec::new();

    let app_ok = paths.app.exists();
    checks.push(Check {
        name: "Claude.app".into(),
        ok: app_ok,
        detail: paths.app.display().to_string(),
    });

    let bin_ok = paths.app_binary().exists();
    checks.push(Check {
        name: "Claude executable".into(),
        ok: bin_ok,
        detail: paths.app_binary().display().to_string(),
    });

    let store_ok = paths.guise_root.exists();
    checks.push(Check {
        name: "guise store (~/.guise)".into(),
        ok: true,
        detail: if store_ok {
            paths.guise_root.display().to_string()
        } else {
            "will be created on first `guise add`".into()
        },
    });

    let cfg = ToolConfig::load(paths).unwrap_or_default();
    let sessions_target = if let Some(r) = &cfg.code_sessions_root {
        std::path::PathBuf::from(r)
    } else {
        account::meld_sessions_root(&paths.meld_config())
            .unwrap_or_else(|| paths.default_code_sessions_root())
    };
    checks.push(Check {
        name: "meld chat sharing".into(),
        ok: true,
        detail: if cfg.share_code_sessions {
            format!("on → {}", sessions_target.display())
        } else {
            "off (guise config set share-code-sessions on)".into()
        },
    });

    let ctrl = app::control();
    let mut accounts = Vec::new();
    for a in account::list_accounts(paths)? {
        let running = ctrl.is_instance_running(&a.data_dir()).unwrap_or(false);
        // "Initialized" = Claude has written into the data dir at least once
        // (i.e. the account has been opened and presumably logged in).
        let initialized = a.data_dir().join("config.json").exists();
        let chats_shared = account::is_code_sessions_linked(&a, &sessions_target);
        accounts.push(AccountHealth {
            slot: a.meta.slot,
            name: a.meta.name.clone(),
            email: a.meta.email.clone(),
            running,
            initialized,
            chats_shared,
        });
    }

    let healthy = app_ok && bin_ok;
    Ok(Report {
        checks,
        accounts,
        healthy,
    })
}

pub fn print_human(report: &Report) {
    println!("guise checkup\n");
    for c in &report.checks {
        println!(
            "  {} {:<24} {}",
            if c.ok { "✓" } else { "✗" },
            c.name,
            c.detail
        );
    }
    println!();
    if report.accounts.is_empty() {
        println!("  No saved accounts yet — run `guise add <name>`.");
    } else {
        println!("  Saved accounts:");
        for a in &report.accounts {
            let dot = if a.running { " ● running" } else { "" };
            let state = if a.initialized {
                ""
            } else {
                "  (not signed in yet — run `guise open` and log in)"
            };
            let shared = if a.chats_shared {
                "  ↔ chats shared"
            } else {
                ""
            };
            match &a.email {
                Some(e) => println!(
                    "    {}. {}  ·  {}{}{}{}",
                    a.slot, a.name, e, dot, state, shared
                ),
                None => println!("    {}. {}{}{}{}", a.slot, a.name, dot, state, shared),
            }
        }
    }
    println!(
        "\n  overall: {}",
        if report.healthy {
            "healthy ✓"
        } else {
            "needs attention ✗"
        }
    );
}
