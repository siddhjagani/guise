# guise — Claude Desktop account switcher

> Wear a different account's **guise** on Claude Desktop — switch logged-in
> accounts instantly, no logout/relogin. The desktop counterpart to
> [`meld`](https://github.com/siddhjagani/meld) (which unifies chat history) and
> to `claude-swap` (which switches the Claude Code *CLI*, not the desktop app).

## Context

**Problem.** Claude Desktop (the Electron app at `/Applications/Claude.app`) supports
only one live login at a time. When you hit a rate/usage limit mid-project, the only
way to keep working is to log out, re-enter credentials, and re-verify — every single
time, back and forth. `claude-swap` solves this for the Claude Code CLI but explicitly
has **no desktop mode**. There is no tool that switches the *desktop app's* account.

**Outcome.** A short, `meld`-style Rust CLI, `guise`, that snapshots each account's
full desktop session once, then swaps between them atomically in seconds. Same branding
values as `meld`: **local-only, atomic writes, non-destructive, always backed up, app
never running during a swap.** No token is ever decrypted or displayed.

## How Claude Desktop stores an account (verified on this machine)

- **App**: `/Applications/Claude.app`, bundle `com.anthropic.claudefordesktop`.
- **Electron userData dir**: `~/Library/Application Support/Claude/`.
- **The credential** is an OAuth token cache stored *encrypted* in
  `~/Library/Application Support/Claude/config.json` under keys `oauth:tokenCache` and
  `oauth:tokenCacheV2` (both `v10` Chromium `safeStorage` ciphertext blobs).
- **The decryption key** lives in the macOS login Keychain: service `Claude Safe Storage`,
  account `Claude Key`. **Critical fact: this key is app-wide and stable — NOT per-account.**
  So encrypted blobs captured under account A stay decryptable after switching to account B.
  → **`guise` never decrypts anything; it moves still-encrypted blobs around.**
- **Active-account markers** (plaintext, safe to read): `config.json → lastKnownAccountUuid`;
  identity cache in `Local Storage/leveldb` (`emailAddress`, `accountUuid`,
  `organizationUuid`, `billing_type`); `cowork-enabled-cli-ops.json → ownerAccountId`.
- **A full account = a matched set** that must move together, or the token desyncs from the
  cached webview session:
  1. `config.json` auth keys: `oauth:tokenCache`, `oauth:tokenCacheV2`, `lastKnownAccountUuid`
  2. `Cookies` (SQLite) + `Cookies-journal`
  3. `Local Storage/leveldb/` (entire dir)
  4. `Session Storage/` (entire dir)
  5. `IndexedDB/https_claude.ai_0.indexeddb.leveldb/` (+ its `.blob` dir)
- **Hard constraint**: the app holds LevelDB `LOCK` files and the Cookies SQLite open while
  running. **Claude Desktop must be fully quit before any read/write of these stores.**
- **Claude Code CLI is separate** (credential in Keychain `Claude Code-credentials` +
  a config-hash variant `-d9b66b90`; identity in `~/.claude.json → oauthAccount`). It is
  untouched by default; the optional `--with-cli` flag keeps it on the same account.

## Design

### Profile store
```
~/.guise/
  config.json                # tool settings (active slot, prefs, app path)
  profiles/
    1-siddh.jain@jwero.com/
      meta.json              # email, accountUuid, orgUuid, alias, created, lastUsed, billingType
      config.auth.json       # ONLY the 3 auth keys extracted from Claude's config.json
      Cookies  Cookies-journal
      Local Storage/ …  Session Storage/ …  IndexedDB/ …
      cli/                   # present only if captured --with-cli (see below)
    2-siddh.tanika@gmail.com/ …
  backups/
    <timestamp>-pre-switch/  # auto-snapshot of live state before each swap
```
- `meta.json` fields are all read from **plaintext** sources — no decryption needed to
  identify who a profile belongs to.
- Slots are numbered (`1`, `2`, …) like `claude-swap`, addressable by number, email, or alias.

### The swap algorithm (core of the tool)
Implemented as one guarded, atomic transaction:
1. **Preflight** (`doctor`-style checks): locate `Claude.app` + userData dir, verify the
   `Claude Safe Storage` keychain item exists, verify target profile is intact.
2. **Quit gate**: detect if Claude Desktop is running (match bundle
   `com.anthropic.claudefordesktop`). If running: prompt to quit (graceful
   `osascript -e 'quit app "Claude"'`, fall back to SIGTERM), then **poll until LevelDB
   `LOCK` is released** before proceeding. `--force` skips the prompt; `--no-quit` aborts if
   running.
3. **Auto-backup**: snapshot the current live matched-set into `~/.guise/backups/<ts>-pre-switch/`.
   (Also refreshes the *currently active* profile's snapshot so unsaved session drift isn't lost.)
4. **Restore target**: copy the target profile's matched-set into the userData dir using
   **write-to-temp + atomic rename** per file/dir; surgically merge the 3 auth keys back into
   the live `config.json` (never overwrite the whole file — it also holds locale/theme/allowlist
   caches that are not account-scoped).
5. **(optional) `--with-cli`**: swap the Keychain `Claude Code-credentials` entry and
   `~/.claude.json → oauthAccount` for the matching account. Requires having captured the CLI
   credential when the profile was added (`add --with-cli`).
6. **Relaunch**: `open -a Claude` unless `--no-launch`.
7. **Verify + record**: confirm `lastKnownAccountUuid` matches the target; update
   `config.json → active` and the profile's `lastUsed`.

Any failure before step 7 triggers rollback from the step-3 backup — nothing is left half-swapped.

### CLI surface — meld-simple: one bare command does the switch

The whole point is switching, so switching is the **bare command** — no `switch` verb, no
slot numbers, no flags. Everything else (quit the app, back up, restore, relaunch) happens
**automatically**. Only 3 everyday verbs beyond that.

```
guise                # interactive picker: shows accounts, arrow-pick, enter = switch.
                     #   (with no saved accounts yet, it just tells you to run `guise add`)
guise <name>         # THE switch. e.g. `guise work` → auto-quit app, swap, relaunch. Done.
guise add [name]     # save the CURRENTLY logged-in account. name optional (defaults to the
                     #   email prefix, e.g. siddh.jain@jwero.com → "jwero"). auto-detects
                     #   email/UUID from plaintext markers — no decryption, no slot to pick.
guise ls             # list saved accounts; ● marks the one that's live right now.
guise doctor         # environment + integrity diagnostics.
```

Occasional verbs (still one word, no ceremony):
```
guise undo           # revert the last switch — restores the previous account from auto-backup.
guise rm <name>      # forget a saved account (deletes only the saved profile, never the live app).
guise config         # show/set the few preferences (see below).
```

**Everything else is a default, not a flag:**
- Auto-quit + relaunch of Claude Desktop — automatic. (Turn off relaunch once via
  `guise config set relaunch off` if you prefer; not a per-command flag.)
- Auto-backup before every switch — automatic and silent; `guise undo` uses it.
- **CLI sync** is a **set-once preference**, not the per-command `--with-cli` flag:
  `guise config set sync-cli on` and from then on every switch keeps the Claude Code CLI
  credential on the same account. Default off.

Only two optional flags exist, and only where meld has them too:
- `--dry-run` on a switch (`guise work --dry-run`) — prints the exact quit/copy/relaunch plan, writes nothing.
- `--json` on `ls` / `doctor` — for scripting.

Ambiguity rule (documented): if the first arg matches a reserved verb (`add`, `ls`, `doctor`,
`undo`, `rm`, `config`) it's that command; otherwise it's an account name to switch to. A
`guise use <name>` alias is provided for scripts that want to be unambiguous.

## Stack & distribution (match `meld`)
- **Rust** single binary named `guise`. Suggested crates:
  - `clap` (derive) — CLI; `serde`/`serde_json` — profiles & config.
  - `security-framework` — read/write the macOS Keychain items (`Claude Safe Storage` presence
    check; `Claude Code-credentials` for `--with-cli`).
  - `fs_extra` or hand-rolled — recursive dir copy with atomic temp+rename.
  - `sysinfo` or `std::process` + `pgrep`/`osascript` — detect/quit/relaunch the app.
  - `rusqlite` (optional) — only to *validate* the Cookies DB is a well-formed SQLite file before
    swapping (we copy the file wholesale; we do not read cookie values).
- **Platform**: macOS first (Apple Silicon + Intel), matching where the research was verified.
  Structure the storage/keychain layer behind a `trait CredentialBackend` so Windows (DPAPI +
  `%APPDATA%/Claude`) / Linux (libsecret) can be added later — same as `claude-swap`'s
  platform-specific credential handling.
- **Install**: `install.sh` / `install.ps1` + Homebrew tap, exactly like `meld`.
- Repo scaffolding to mirror `meld`: `Cargo.toml`, `src/` (`main.rs`, `cli.rs`, `profile.rs`,
  `swap.rs`, `app.rs` for process control, `keychain.rs`, `paths.rs`, `doctor.rs`), `install.sh`,
  `install.ps1`, `README.md`, `CLAUDE.md`.

## Files to create (all new — greenfield repo)
- `src/paths.rs` — resolve userData dir, `Claude.app`, `~/.guise`; the matched-set path list.
- `src/app.rs` — is-running detection, graceful quit, LOCK-release polling, relaunch.
- `src/keychain.rs` — `Claude Safe Storage` presence check; `Claude Code-credentials` r/w for `--with-cli`.
- `src/profile.rs` — capture/restore matched set, `meta.json`, atomic temp+rename copy, config.json key merge.
- `src/swap.rs` — the transactional switch algorithm + rollback.
- `src/doctor.rs`, `src/cli.rs`, `src/main.rs`, `src/config.rs`.

## Safety guarantees (headline, `meld`-style)
- **Local-only** — nothing leaves the machine.
- **Never decrypts or prints tokens** — blobs move encrypted; the app-wide Keychain key is
  read for *presence* only, never exported.
- **App-quit enforced** before touching locked stores.
- **Atomic** temp+rename writes; **auto-backup** before every swap; **rollback** on any failure.
- **Non-destructive** — `remove` deletes only saved profiles, never the live session.
- Note during `doctor`: this machine already has a Keychain `claude-swap` item and a
  `Claude Code-credentials-d9b66b90` entry from prior experimentation — `doctor` should report
  them so state is transparent, and `guise` must not clobber them.

## Verification (end-to-end)
1. `cargo build` → run `guise doctor`: confirms it finds `Claude.app`, the userData dir, the
   `Claude Safe Storage` keychain item, and reports app running/quit + profile health.
2. With account A logged in: `guise add work` → assert `~/.guise/profiles/*work*/` contains
   all 5 matched-set items + a `meta.json` with the correct email/UUID (cross-check against
   `config.json → lastKnownAccountUuid`).
3. Log into account B in the app, `guise add personal`.
4. `guise work --dry-run` → prints the exact quit/copy/merge/relaunch plan, writes nothing.
5. `guise work` → app quits, swaps, relaunches; **manually confirm the app is now on account A**
   and chat/session matches. `guise personal`, confirm the reverse. `guise` (bare) → picker works.
6. `guise undo` → returns the app to the account active before the last switch.
7. Corruption guard: kill the tool mid-swap (or simulate a copy failure) and confirm auto-rollback
   returns the live app to its pre-swap account cleanly.
8. `guise config set sync-cli on` → then `guise work` → confirm `claude` CLI
   (`~/.claude.json → oauthAccount`) now reports account A too.
9. `guise ls --json` / `guise doctor --json` → valid JSON for scripting.

## Out of scope (v1)
- Windows/Linux backends (trait is stubbed; not implemented).
- Auto-switch-before-limit and usage-window polling (a `claude-swap`-style `auto` verb is a
  natural v2, but needs the desktop app's usage signal, which v1 does not read).
- Cross-machine profile export (safeStorage blobs are bound to this machine's Keychain key and
  won't decrypt elsewhere — document this explicitly rather than pretend portability).
