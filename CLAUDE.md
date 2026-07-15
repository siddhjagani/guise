# CLAUDE.md — guise

Guidance for working in this repo with Claude Code.

## What this is

`guise` is a macOS CLI that switches the logged-in account of **Claude Desktop**
(`/Applications/Claude.app`) instantly, without logout/relogin. It snapshots each
account's full desktop session once, then swaps between them as an atomic,
backed-up transaction. See [README.md](README.md) and [PLAN.md](PLAN.md).

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
| `src/paths.rs` | Resolve userData dir / `Claude.app` / `~/.guise`; the matched-set list. |
| `src/config.rs` | `~/.guise/config.json` tool settings (active slot, prefs). |
| `src/keychain.rs` | `CredentialBackend` trait; macOS impl shells out to `/usr/bin/security`. Presence checks + `--with-cli` credential r/w. |
| `src/app.rs` | `AppControl` trait; detect/quit/relaunch Claude Desktop, poll LOCK release. |
| `src/profile.rs` | Capture/restore the matched set, `meta.json`, atomic fs primitives, plaintext identity detection. |
| `src/swap.rs` | The transactional switch (`Sys`-injected) + `add`/`undo`/`rm` + rollback. |
| `src/doctor.rs` | Read-only environment + integrity diagnostics. |
| `src/cli.rs` | clap surface, the ambiguity rule, dispatch, interactive picker. |
| `src/main.rs` | Thin entrypoint. |

## Testability seam

`swap::switch` / `add` / `undo` take a `Sys { app: &dyn AppControl, keychain:
&dyn CredentialBackend }`. Production passes `RealApp` + the real Keychain
backend; tests pass fakes so the full transaction (quit → backup → restore →
verify → relaunch) can run against a synthetic `HOME` **without touching the real
Claude Desktop or Keychain**. Keep this seam — it is how the swap logic is
tested. See the `#[cfg(test)]` modules in `swap.rs` and `profile.rs`.

## Working here

```sh
cargo build            # debug
cargo test             # unit + multi-account integration tests
cargo build --release  # optimized single binary
```

- Prefer adding new platform backends behind `CredentialBackend` / `AppControl`
  rather than `#[cfg]`-ing call sites.
- When testing anything that reads real Claude data, use a synthetic `HOME`
  (see the test helpers) — do not read the user's live session stores.
- macOS only for v1. Windows/Linux backends are stubbed (`Unsupported`).
