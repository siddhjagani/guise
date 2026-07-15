//! Saved accounts: each is a permanent, independent Claude Desktop userData
//! directory plus a little guise metadata. Nothing here reads or moves session
//! bytes — guise only creates the directory and lets Claude own its contents.

use crate::paths::{self, Paths};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Per-account metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub slot: u32,
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    pub created: String,
    #[serde(default)]
    pub last_opened: Option<String>,
}

/// A saved account on disk.
#[derive(Debug, Clone)]
pub struct Account {
    /// `~/.guise/accounts/<slot>-<name>/`
    pub dir: PathBuf,
    pub meta: Meta,
}

impl Account {
    pub fn meta_path(dir: &Path) -> PathBuf {
        dir.join("meta.json")
    }

    /// The directory passed to Claude via `--user-data-dir`. Claude owns
    /// everything inside it; guise never touches it after creation.
    pub fn data_dir(&self) -> PathBuf {
        self.dir.join("data")
    }

    pub fn load(dir: &Path) -> Result<Account> {
        let mp = Account::meta_path(dir);
        let raw = std::fs::read_to_string(&mp).with_context(|| format!("reading {}", mp.display()))?;
        let meta: Meta = serde_json::from_str(&raw).with_context(|| format!("parsing {}", mp.display()))?;
        Ok(Account { dir: dir.to_path_buf(), meta })
    }

    pub fn save_meta(&self) -> Result<()> {
        let body = serde_json::to_vec_pretty(&self.meta)?;
        write_bytes_atomic(&Account::meta_path(&self.dir), &body)
    }
}

/// List all saved accounts, sorted by slot.
pub fn list_accounts(paths: &Paths) -> Result<Vec<Account>> {
    let dir = paths.accounts_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let p = entry.path();
        if Account::meta_path(&p).exists() {
            if let Ok(acct) = Account::load(&p) {
                out.push(acct);
            }
        }
    }
    out.sort_by_key(|a| a.meta.slot);
    Ok(out)
}

/// Resolve an account by slot number, name, email, or unique prefix.
pub fn resolve_account(paths: &Paths, query: &str) -> Result<Account> {
    let accounts = list_accounts(paths)?;
    if accounts.is_empty() {
        return Err(anyhow!("no saved accounts yet — run `guise add <name>` first"));
    }
    if let Ok(n) = query.parse::<u32>() {
        if let Some(a) = accounts.iter().find(|a| a.meta.slot == n) {
            return Ok(a.clone());
        }
    }
    let q = query.to_lowercase();
    if let Some(a) = accounts.iter().find(|a| a.meta.name.to_lowercase() == q) {
        return Ok(a.clone());
    }
    if let Some(a) = accounts
        .iter()
        .find(|a| a.meta.email.as_deref().map(|e| e.to_lowercase()) == Some(q.clone()))
    {
        return Ok(a.clone());
    }
    let matches: Vec<&Account> = accounts
        .iter()
        .filter(|a| a.meta.name.to_lowercase().starts_with(&q))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(anyhow!("no saved account matches \"{query}\" (try `guise ls`)")),
        many => {
            let names: Vec<String> = many.iter().map(|a| a.meta.name.clone()).collect();
            Err(anyhow!("\"{query}\" is ambiguous — matches: {}", names.join(", ")))
        }
    }
}

/// Next free slot number.
pub fn next_slot(paths: &Paths) -> Result<u32> {
    Ok(list_accounts(paths)?.iter().map(|a| a.meta.slot).max().unwrap_or(0) + 1)
}

/// Directory name for a slot + name, sanitized.
pub fn account_dirname(slot: u32, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '@') { c } else { '_' })
        .collect();
    format!("{slot}-{safe}")
}

/// Create a brand-new empty account: its `data/` directory (for Claude to fill
/// on first login) and its `meta.json`. Fails if the name is already taken.
pub fn create_account(paths: &Paths, name: &str, email: Option<String>, created: String) -> Result<Account> {
    if list_accounts(paths)?.iter().any(|a| a.meta.name.eq_ignore_ascii_case(name)) {
        return Err(anyhow!("an account named \"{name}\" already exists (use `guise open {name}`)"));
    }
    let slot = next_slot(paths)?;
    let dir = paths.accounts_dir().join(account_dirname(slot, name));
    let account = Account {
        dir: dir.clone(),
        meta: Meta { slot, name: name.to_string(), email, created, last_opened: None },
    };
    std::fs::create_dir_all(account.data_dir())
        .with_context(|| format!("creating {}", account.data_dir().display()))?;
    account.save_meta()?;
    Ok(account)
}

/// Delete a saved account's directory entirely (its login and history).
pub fn delete_account(account: &Account) -> Result<()> {
    if paths::exists(&account.dir) {
        std::fs::remove_dir_all(&account.dir)
            .with_context(|| format!("removing {}", account.dir.display()))?;
    }
    Ok(())
}

/// Atomic write (temp + rename) — the one filesystem primitive guise still needs.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn temp_paths(tag: &str) -> Paths {
        let n = N.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("guise-acct-{}-{}-{}", std::process::id(), tag, n));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        Paths { home: home.clone(), app: root.join("Claude.app"), guise_root: home.join(".guise") }
    }

    #[test]
    fn create_list_resolve() {
        let paths = temp_paths("crl");
        let w = create_account(&paths, "work", Some("w@co.com".into()), "t1".into()).unwrap();
        let p = create_account(&paths, "personal", None, "t2".into()).unwrap();
        assert_eq!(w.meta.slot, 1);
        assert_eq!(p.meta.slot, 2);
        assert!(w.data_dir().is_dir(), "data dir created for Claude to fill");

        let all = list_accounts(&paths).unwrap();
        assert_eq!(all.len(), 2);

        assert_eq!(resolve_account(&paths, "1").unwrap().meta.name, "work");
        assert_eq!(resolve_account(&paths, "personal").unwrap().meta.slot, 2);
        assert_eq!(resolve_account(&paths, "w@co.com").unwrap().meta.name, "work");
        assert_eq!(resolve_account(&paths, "wo").unwrap().meta.name, "work"); // prefix
        assert!(resolve_account(&paths, "nope").is_err());
    }

    #[test]
    fn duplicate_name_rejected() {
        let paths = temp_paths("dup");
        create_account(&paths, "work", None, "t".into()).unwrap();
        assert!(create_account(&paths, "WORK", None, "t".into()).is_err());
    }

    #[test]
    fn delete_removes_dir() {
        let paths = temp_paths("del");
        let a = create_account(&paths, "work", None, "t".into()).unwrap();
        assert!(a.dir.is_dir());
        delete_account(&a).unwrap();
        assert!(!a.dir.exists());
        assert!(list_accounts(&paths).unwrap().is_empty());
    }

    #[test]
    fn dirname_sanitizes() {
        assert_eq!(account_dirname(1, "work"), "1-work");
        assert_eq!(account_dirname(2, "we ird/x"), "2-we_ird_x");
    }
}
