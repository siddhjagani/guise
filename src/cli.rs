//! CLI surface (clap) + dispatch for the per-account instance model.
//!
//! Bare `guise` opens a picker; `guise <name>` opens that account's window.
//! Each account is a permanent Claude userData dir launched with
//! `--user-data-dir`, so accounts stay logged in side by side.

use crate::account::{self, Account};
use crate::app::{self, AppControl};
use crate::config::ToolConfig;
use crate::doctor;
use crate::paths::Paths;
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use std::time::Duration;

pub const RESERVED: [&str; 7] = ["open", "add", "ls", "rm", "doctor", "config", "all"];
const QUIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Parser, Debug)]
#[command(
    name = "guise",
    version,
    about = "Switch between Claude Desktop accounts — each stays logged in, in its own window.",
    long_about = "guise keeps each Claude Desktop account in its own permanent profile so they \
stay logged in side by side. Run `guise` to pick one, or `guise <name>` to open it."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Open an account's window (bare `guise <name>` does the same).
    Open { name: String },
    /// Open every saved account's window at once.
    All,
    /// Save a new account: opens a fresh Claude window for you to log into.
    Add {
        name: String,
        /// Email to show in the listing (optional label).
        #[arg(long)]
        email: Option<String>,
    },
    /// List saved accounts (● marks the ones open right now).
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Forget a saved account — closes its window and deletes its profile.
    Rm { name: String },
    /// Environment + account diagnostics.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Show or set preferences.
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    Set { key: String, value: String },
    Get { key: String },
}

/// Entry point: apply the ambiguity rule, then run.
pub fn dispatch(argv: Vec<String>) -> Result<()> {
    let mut paths = Paths::resolve()?;
    if let Ok(cfg) = ToolConfig::load(&paths) {
        if let Some(ap) = cfg.app_path {
            paths.app = std::path::PathBuf::from(ap);
        }
    }

    let rest = &argv[1..];
    if rest.is_empty() {
        return picker(&paths);
    }

    let first = rest[0].as_str();
    let cli = if first.starts_with('-') || RESERVED.contains(&first) {
        Cli::parse_from(&argv)
    } else {
        let mut synth = vec![argv[0].clone(), "open".to_string()];
        synth.extend(rest.iter().cloned());
        Cli::parse_from(&synth)
    };
    run(&paths, cli.command)
}

fn run(paths: &Paths, command: Commands) -> Result<()> {
    match command {
        Commands::Open { name } => {
            let account = account::resolve_account(paths, &name)?;
            open_account(paths, &account)
        }
        Commands::All => open_all(paths),
        Commands::Add { name, email } => add_account(paths, &name, email),
        Commands::Ls { json } => list(paths, json),
        Commands::Rm { name } => {
            let account = account::resolve_account(paths, &name)?;
            remove_account(paths, &account)
        }
        Commands::Doctor { json } => {
            let report = doctor::run(paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                doctor::print_human(&report);
            }
            if !report.healthy {
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::Config { action } => config_cmd(paths, action),
    }
}

/// Ensure an account's window is open (launch if needed) and bring it forward.
fn open_account(paths: &Paths, account: &Account) -> Result<()> {
    crate::paths::require_dir(&paths.app, "Claude.app")?;
    let ctrl = app::control();
    let data = account.data_dir();

    if ctrl.is_instance_running(&data)? {
        ctrl.activate(&paths.app)?;
        println!("✓ {} is already open — bringing it to the front.", account.meta.name);
        return Ok(());
    }

    ctrl.launch_instance(&paths.app, &data)?;

    // Record last-opened (best effort).
    let mut updated = account.clone();
    updated.meta.last_opened = Some(now());
    let _ = updated.save_meta();

    println!("✓ Opened {}.", account.meta.name);
    Ok(())
}

/// Open every saved account's window (launch the ones not already running).
fn open_all(paths: &Paths) -> Result<()> {
    crate::paths::require_dir(&paths.app, "Claude.app")?;
    let accounts = account::list_accounts(paths)?;
    if accounts.is_empty() {
        println!("No saved accounts yet. Run `guise add <name>`.");
        return Ok(());
    }
    let ctrl = app::control();
    let mut opened = 0;
    for a in &accounts {
        if ctrl.is_instance_running(&a.data_dir())? {
            continue;
        }
        ctrl.launch_instance(&paths.app, &a.data_dir())?;
        opened += 1;
        // Small stagger so simultaneous launches don't race LaunchServices.
        std::thread::sleep(Duration::from_millis(700));
    }
    println!(
        "✓ Opened {} of {} account window{} ({} already open).",
        opened,
        accounts.len(),
        if accounts.len() == 1 { "" } else { "s" },
        accounts.len() - opened
    );
    Ok(())
}

/// Create a new account and open a fresh window for the user to log into.
fn add_account(paths: &Paths, name: &str, email: Option<String>) -> Result<()> {
    crate::paths::require_dir(&paths.app, "Claude.app")?;
    let account = account::create_account(paths, name, email, now())?;
    let ctrl = app::control();
    ctrl.launch_instance(&paths.app, &account.data_dir())?;
    println!(
        "✓ Created {}. A fresh Claude window is opening — log into this account there.\n  From now on: `guise {}` reopens it, already logged in.",
        account.meta.name, account.meta.name
    );
    Ok(())
}

/// Close an account's window and delete its saved profile.
fn remove_account(_paths: &Paths, account: &Account) -> Result<()> {
    let ctrl = app::control();
    if ctrl.is_instance_running(&account.data_dir())? {
        ctrl.quit_instance(&account.data_dir(), QUIT_TIMEOUT)?;
    }
    account::delete_account(account)?;
    println!("✓ Removed {}.", account.meta.name);
    Ok(())
}

fn picker(paths: &Paths) -> Result<()> {
    let accounts = account::list_accounts(paths)?;
    if accounts.is_empty() {
        println!("No saved accounts yet. Run `guise add <name>` to save one.");
        return Ok(());
    }
    let ctrl = app::control();
    let running: Vec<bool> = accounts
        .iter()
        .map(|a| ctrl.is_instance_running(&a.data_dir()).unwrap_or(false))
        .collect();

    // First item opens everything; the rest are individual accounts.
    let mut items: Vec<String> = vec!["★ Open ALL accounts".to_string()];
    items.extend(accounts.iter().zip(&running).map(|(a, &r)| {
        let dot = if r { " ●" } else { "" };
        match &a.meta.email {
            Some(e) => format!("{}. {}{}  ·  {}", a.meta.slot, a.meta.name, dot, e),
            None => format!("{}. {}{}", a.meta.slot, a.meta.name, dot),
        }
    }));

    use dialoguer::{theme::ColorfulTheme, Select};
    let sel = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Open which account?")
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(|e| anyhow!("picker failed: {e}"))?;

    match sel {
        Some(0) => open_all(paths),
        Some(i) => open_account(paths, &accounts[i - 1]),
        None => {
            println!("Cancelled.");
            Ok(())
        }
    }
}

fn list(paths: &Paths, json: bool) -> Result<()> {
    let accounts = account::list_accounts(paths)?;
    let ctrl = app::control();

    if json {
        let arr: Vec<serde_json::Value> = accounts
            .iter()
            .map(|a| {
                serde_json::json!({
                    "slot": a.meta.slot,
                    "name": a.meta.name,
                    "email": a.meta.email,
                    "created": a.meta.created,
                    "last_opened": a.meta.last_opened,
                    "running": ctrl.is_instance_running(&a.data_dir()).unwrap_or(false),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }

    if accounts.is_empty() {
        println!("No saved accounts yet. Run `guise add <name>`.");
        return Ok(());
    }
    for a in &accounts {
        let dot = if ctrl.is_instance_running(&a.data_dir()).unwrap_or(false) { "●" } else { " " };
        match &a.meta.email {
            Some(e) => println!("{} {}. {:<14} ·  {}", dot, a.meta.slot, a.meta.name, e),
            None => println!("{} {}. {}", dot, a.meta.slot, a.meta.name),
        }
    }
    Ok(())
}

fn config_cmd(paths: &Paths, action: Option<ConfigAction>) -> Result<()> {
    let mut cfg = ToolConfig::load(paths)?;
    match action {
        None => {
            println!("app-path = {}", cfg.app_path.as_deref().unwrap_or(crate::paths::DEFAULT_APP_PATH));
            Ok(())
        }
        Some(ConfigAction::Get { key }) => {
            match key.as_str() {
                "app-path" => println!("{}", cfg.app_path.as_deref().unwrap_or(crate::paths::DEFAULT_APP_PATH)),
                other => return Err(anyhow!("unknown config key: {other}")),
            }
            Ok(())
        }
        Some(ConfigAction::Set { key, value }) => {
            match key.as_str() {
                "app-path" => cfg.app_path = Some(value.clone()),
                other => return Err(anyhow!("unknown config key: {other}")),
            }
            cfg.save(paths)?;
            println!("✓ set {key} = {value}");
            Ok(())
        }
    }
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}
