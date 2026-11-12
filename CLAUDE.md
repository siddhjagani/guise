# CLAUDE.md — guise

Guidance for working in this repo with Claude Code.

## What this is

`guise` is a macOS and Windows CLI that keeps several **Claude Desktop** accounts
logged in side by side: each account is its own permanent userData directory,
and guise launches Claude against it with `--user-data-dir`. See
[README.md](README.md) and [PLAN.md](PLAN.md) (PLAN.md describes the original
snapshot-and-swap design).

## Core invariants — do not break these

- **Never decrypt or print tokens.** The OAuth token cache in Claude's
  `config.json` (`oauth:tokenCache`, `oauth:tokenCacheV2`) is moved as opaque,
  still-encrypted bytes. The `Claude Safe Storage` Keychain key is checked for
  **presence only** (item attributes via `/usr/bin/security`, never `-w`), never
  retrieved or exported.
- **Never touch the live session on a running app.** Claude Desktop holds the
  LevelDB `LOCK` and the Cookies SQLite open. Always quit the app and wait for
  the LOCK to release before any read/write of the matched-set stores.
- **The matched set moves together** (auth keys + Cookies + Local Storage +
  Session Storage + IndexedDB). Splitting it desyncs the token from the cached
  webview session. See `paths::matched_set()`.
- **Only the 3 auth keys are merged into the live `config.json`** — never
  overwrite the whole file; it also holds locale/theme/allowlist caches that are
  not account-scoped. See `profile::merge_auth_keys`.
- **Every write is atomic** (temp + rename) and **every switch auto-backs-up**
  first, with **rollback on any failure**. See `profile::write_bytes_atomic`,
  `profile::stage_and_replace_dir`, and the rollback path in `swap::switch`.
- **Never clobber foreign Keychain items** (`claude-swap`,
  `Claude Code-credentials-d9b66b90`). `doctor` reports them; nothing writes them.

## Module map

| File | Responsibility |
|---|---|
| `src/paths.rs` | Home dir, Claude Desktop location (`Claude.app`; on Windows the detected `claude.exe`), the `~/.config/guise` store (+ legacy `~/.guise` migration). |
| `src/config.rs` | `config.json` tool settings (`app-path`, chat sharing). |
| `src/account.rs` | Saved accounts (`meta.json` + a `data/` userData dir each), atomic writes, the meld `claude-code-sessions` link (symlink on macOS, junction on Windows). |
| `src/app.rs` | `AppControl` trait: launch/detect/focus/quit one account's instance by its `--user-data-dir`. macOS: `open`/`pgrep`/`kill`. Windows: direct detached spawn, native process command-line listing, `taskkill`. |
| `src/doctor.rs` | Read-only environment + account diagnostics. |
| `src/cli.rs` | clap surface, the ambiguity rule, dispatch, interactive picker. |
| `src/main.rs` | Thin entrypoint. |

## Testability seam

Process control sits behind the `AppControl` trait (`RealApp` in production).
Account/link tests run against a synthetic `HOME` from `temp_paths` in
`account.rs` — they never touch the real Claude Desktop or its profiles. The
Windows command-line matcher in `app.rs` is pure and tested on every platform.

## Working here

```sh
cargo build            # debug
cargo test             # unit tests
cargo build --release  # optimized single binary
cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings  # check Windows code from a Mac
pwsh tests/windows/e2e.ps1   # Windows only, after a release build: drives guise.exe against a stand-in Claude
```

- Prefer adding new platform backends behind `AppControl` rather than
  `#[cfg]`-ing call sites.
- When testing anything that reads real Claude data, use a synthetic `HOME`
  (see the test helpers) — do not read the user's live session stores.
- macOS and Windows are supported (CI runs both). Linux remains unsupported
  (`Unsupported`).
