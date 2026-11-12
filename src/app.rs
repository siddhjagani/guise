//! Claude Desktop process control for the per-account instance model.
//!
//! Each account runs as its own Claude process launched with
//! `--user-data-dir=<account data dir>`. We detect a specific account's
//! instance by matching that flag in the process command line, launch it
//! detached, bring it to the foreground, and quit a specific instance.
//!
//! Behind an `AppControl` trait so the logic is testable with a fake.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// How long to wait for an instance to disappear after a forced kill.
const KILL_WAIT: Duration = Duration::from_secs(10);
/// Windows: how long a polite close gets before the instance is terminated.
const WINDOWS_GRACE: Duration = Duration::from_secs(3);

/// Control over Claude Desktop instances. Testable via a fake.
pub trait AppControl {
    /// Launch a Claude instance bound to `data_dir`. `app` is the `.app`
    /// bundle (macOS) or the Claude executable (Windows). Fully detached from the
    /// caller's terminal.
    fn launch_instance(&self, app: &Path, data_dir: &Path) -> Result<()>;
    /// Is a Claude instance for exactly this `data_dir` currently running?
    fn is_instance_running(&self, data_dir: &Path) -> Result<bool>;
    /// Bring the running instance bound to `data_dir` to the foreground.
    fn activate(&self, app: &Path, data_dir: &Path) -> Result<()>;
    /// Quit the Claude instance bound to `data_dir` (gracefully, then firmly),
    /// returning once its processes are gone.
    fn quit_instance(&self, data_dir: &Path, timeout: Duration) -> Result<()>;
}

/// Production implementation talking to the real OS.
pub struct RealApp;

impl AppControl for RealApp {
    fn launch_instance(&self, app: &Path, data_dir: &Path) -> Result<()> {
        platform_launch(app, data_dir)
    }

    fn is_instance_running(&self, data_dir: &Path) -> Result<bool> {
        Ok(!platform_pids(data_dir)?.is_empty())
    }

    fn activate(&self, app: &Path, data_dir: &Path) -> Result<()> {
        platform_activate(app, data_dir)
    }

    fn quit_instance(&self, data_dir: &Path, timeout: Duration) -> Result<()> {
        // On Windows a polite close only hides Claude's window (it keeps
        // running), so don't wait long before terminating it.
        let timeout = if cfg!(windows) {
            timeout.min(WINDOWS_GRACE)
        } else {
            timeout
        };
        for force in [false, true] {
            let pids = platform_pids(data_dir)?;
            if pids.is_empty() {
                return Ok(());
            }
            for pid in pids {
                platform_stop(pid, force);
            }
            if wait_until_gone(data_dir, if force { KILL_WAIT } else { timeout })? {
                return Ok(());
            }
        }
        Err(anyhow!(
            "Claude for {} did not exit; close it and try again",
            data_dir.display()
        ))
    }
}

/// Construct the production app controller.
pub fn control() -> RealApp {
    RealApp
}

/// Poll until no process is bound to `data_dir`; `false` if `timeout` passes
/// first. Waiting matters on Windows, where a profile can't be deleted while
/// the exiting app still holds its files open.
fn wait_until_gone(data_dir: &Path, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while !platform_pids(data_dir)?.is_empty() {
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(Duration::from_millis(200));
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// macOS implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn platform_launch(app_bundle: &Path, data_dir: &Path) -> Result<()> {
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
        return Err(anyhow!(
            "failed to launch Claude for {}",
            data_dir.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_activate(app_bundle: &Path, data_dir: &Path) -> Result<()> {
    // Activate this account's own instance by PID: `open -a` alone would focus
    // whichever Claude instance LaunchServices picks. Best-effort.
    let main = macos_processes(data_dir)?
        .into_iter()
        .find(|(_, cmd)| !cmd.contains(" --type="));
    if let Some((pid, _)) = main {
        let script = format!(
            "ObjC.import('AppKit'); $.NSRunningApplication\
             .runningApplicationWithProcessIdentifier({pid}).activateWithOptions(3)"
        );
        let ok = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", &script])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            return Ok(());
        }
    }
    let _ = Command::new("open").arg("-a").arg(app_bundle).status();
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_stop(pid: u32, force: bool) {
    let signal = if force { "-KILL" } else { "-TERM" };
    let _ = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .output();
}

/// PIDs whose command line binds them to exactly this userData dir, excluding
/// guise itself.
#[cfg(target_os = "macos")]
fn platform_pids(data_dir: &Path) -> Result<Vec<u32>> {
    Ok(macos_processes(data_dir)?
        .into_iter()
        .map(|(pid, _)| pid)
        .collect())
}

/// `(pid, command)` of every process carrying `--user-data-dir=<data_dir>`.
/// Plain text matching on `ps` output: pgrep would read the path as a regex.
#[cfg(target_os = "macos")]
fn macos_processes(data_dir: &Path) -> Result<Vec<(u32, String)>> {
    let out = Command::new("ps")
        .args(["-axww", "-o", "pid=,command="])
        .output()
        .context("running ps")?;
    let me = std::process::id();
    let needle = format!("--user-data-dir={}", data_dir.display());
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, cmd) = line.trim_start().split_once(' ')?;
            Some((pid.parse::<u32>().ok()?, cmd.to_string()))
        })
        .filter(|(pid, cmd)| *pid != me && carries_flag(cmd, &needle))
        .collect())
}

/// Does the space-joined `cmd` contain `needle` as a whole argument (so
/// `…/1-work/data` doesn't match `…/1-work/data2`)?
#[cfg(any(target_os = "macos", test))]
fn carries_flag(cmd: &str, needle: &str) -> bool {
    cmd.match_indices(needle).any(|(i, _)| {
        cmd[..i].ends_with(' ')
            && cmd[i + needle.len()..]
                .chars()
                .next()
                .is_none_or(|c| c == ' ')
    })
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn platform_launch(app_exe: &Path, data_dir: &Path) -> Result<()> {
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
    };
    let mut flag = std::ffi::OsString::from("--user-data-dir=");
    flag.push(data_dir);
    // Detached from this console, and out of the terminal's job object when it
    // allows that, so closing the terminal can't take Claude down with it.
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    win::spawn_detached(app_exe, &flag, detached | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| win::spawn_detached(app_exe, &flag, detached))
        .with_context(|| {
            format!(
                "launching Claude ({}) for {}",
                app_exe.display(),
                data_dir.display()
            )
        })
}

#[cfg(target_os = "windows")]
fn platform_activate(app_exe: &Path, data_dir: &Path) -> Result<()> {
    // Claude holds Electron's single-instance lock per userData dir, so a
    // second launch for the same dir hands off to the running instance, which
    // brings its window forward (the same as clicking Claude's shortcut).
    platform_launch(app_exe, data_dir)
}

#[cfg(target_os = "windows")]
fn platform_stop(pid: u32, force: bool) {
    // `/T` includes the instance's helper processes. Without `/F`, taskkill
    // asks the windows to close (like clicking ✕); with `/F` it terminates.
    let mut cmd = Command::new("taskkill");
    if force {
        cmd.arg("/F");
    }
    let _ = cmd.arg("/T").arg("/PID").arg(pid.to_string()).output();
}

/// PIDs whose command line binds them to exactly this userData dir, excluding
/// guise itself.
#[cfg(target_os = "windows")]
fn platform_pids(data_dir: &Path) -> Result<Vec<u32>> {
    let me = std::process::id();
    Ok(win::command_lines()
        .context("listing processes")?
        .into_iter()
        .filter(|(pid, cmdline)| *pid != me && targets_data_dir(cmdline, data_dir))
        .map(|(pid, _)| pid)
        .collect())
}

/// Does this Windows command line run Claude bound to exactly `data_dir`?
/// Accepts both `"--user-data-dir=C:\a b"` (how guise quotes it) and
/// `--user-data-dir="C:\a b"` (how Chromium passes it to helper processes).
#[cfg(any(target_os = "windows", test))]
fn targets_data_dir(cmdline: &str, data_dir: &Path) -> bool {
    let want = normalize_windows_path(&data_dir.to_string_lossy());
    split_command_line(cmdline)
        .iter()
        .skip(1)
        .filter_map(|arg| arg.strip_prefix("--user-data-dir="))
        .any(|dir| normalize_windows_path(dir) == want)
}

/// Windows paths compare case-insensitively, with either slash and no
/// trailing one.
#[cfg(any(target_os = "windows", test))]
fn normalize_windows_path(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Split a Windows command line into arguments the way programs receive them
/// (the `CommandLineToArgvW` rules).
#[cfg(any(target_os = "windows", test))]
fn split_command_line(cmdline: &str) -> Vec<String> {
    let mut chars = cmdline.chars().peekable();
    // argv[0] is special: quotes only delimit it; backslashes are literal.
    let mut arg0 = String::new();
    let mut quoted = false;
    while let Some(c) = chars.next_if(|&c| quoted || !matches!(c, ' ' | '\t')) {
        if c == '"' {
            quoted = !quoted;
        } else {
            arg0.push(c);
        }
    }
    let mut args = vec![arg0];
    loop {
        while chars.next_if(|&c| matches!(c, ' ' | '\t')).is_some() {}
        if chars.peek().is_none() {
            return args;
        }
        let mut arg = String::new();
        let mut quoted = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let mut slashes = 1;
                    while chars.next_if_eq(&'\\').is_some() {
                        slashes += 1;
                    }
                    if chars.peek() == Some(&'"') {
                        // 2n backslashes + quote: n backslashes, then the quote
                        // toggles quoting; 2n+1: n backslashes and a literal quote.
                        arg.push_str(&"\\".repeat(slashes / 2));
                        if slashes % 2 == 1 {
                            arg.push('"');
                            chars.next();
                        }
                    } else {
                        arg.push_str(&"\\".repeat(slashes));
                    }
                }
                '"' if quoted && chars.next_if_eq(&'"').is_some() => arg.push('"'),
                '"' => quoted = !quoted,
                ' ' | '\t' if !quoted => break,
                _ => arg.push(c),
            }
        }
        args.push(arg);
    }
}

/// Win32 process listing: each process's command line read directly, as Task
/// Manager does — no shell-out, so no quoting or text encoding to get wrong.
#[cfg(target_os = "windows")]
mod win {
    use std::ffi::OsStr;
    use std::path::Path;
    use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, UNICODE_STRING};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, OpenProcess, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        STARTUPINFOW,
    };

    /// `PROCESSINFOCLASS::ProcessCommandLineInformation` (Windows 8.1+).
    const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

    /// Start `exe arg` with no inherited handles. std's `Command` always lets
    /// the child inherit guise's inheritable handles — including a caller's
    /// stdout pipe, so `$x = guise open work` would hang until Claude quit.
    pub fn spawn_detached(exe: &Path, arg: &OsStr, flags: u32) -> std::io::Result<()> {
        use std::os::windows::ffi::OsStrExt;
        let wide = |s: &OsStr| s.encode_wide().collect::<Vec<u16>>();
        let mut exe_w = wide(exe.as_os_str());
        // Command line: "<exe>" "<arg>" — neither can contain a quote; double
        // any trailing backslashes so they don't escape the closing quote.
        let mut cmdline = Vec::new();
        for part in [exe.as_os_str(), arg] {
            let w = wide(part);
            let trailing = w
                .iter()
                .rev()
                .take_while(|&&c| c == u16::from(b'\\'))
                .count();
            cmdline.push(u16::from(b'"'));
            cmdline.extend(&w);
            cmdline.extend(std::iter::repeat_n(u16::from(b'\\'), trailing));
            cmdline.extend([u16::from(b'"'), u16::from(b' ')]);
        }
        cmdline.push(0);
        exe_w.push(0);
        // SAFETY: valid NUL-terminated buffers; both returned handles are closed.
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                exe_w.as_ptr(),
                cmdline.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0, // bInheritHandles = FALSE
                flags,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut info,
            );
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            CloseHandle(info.hProcess);
            CloseHandle(info.hThread);
        }
        Ok(())
    }

    /// `(pid, command line)` for every process this user may query.
    pub fn command_lines() -> std::io::Result<Vec<(u32, String)>> {
        // SAFETY: plain Win32 calls on a zeroed, correctly sized entry; the
        // snapshot handle is closed before returning.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut out = Vec::new();
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if let Some(cmdline) = command_line(entry.th32ProcessID) {
                    out.push((entry.th32ProcessID, cmdline));
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            Ok(out)
        }
    }

    /// One process's command line, or `None` when it can't be read (it
    /// exited, or belongs to another user or a protected process).
    fn command_line(pid: u32) -> Option<String> {
        // SAFETY: the handle is closed before returning, and the
        // UNICODE_STRING the kernel writes points into `buf`, which outlives
        // the read. `u64` elements keep that header aligned.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let query = |buf: &mut [u64], needed: &mut u32| {
                NtQueryInformationProcess(
                    process,
                    PROCESS_COMMAND_LINE_INFORMATION,
                    buf.as_mut_ptr().cast(),
                    std::mem::size_of_val(buf) as u32,
                    needed,
                )
            };
            let mut buf = vec![0u64; 512];
            let mut needed = 0u32;
            let mut status = query(&mut buf, &mut needed);
            if status == STATUS_INFO_LENGTH_MISMATCH {
                buf = vec![0u64; (needed as usize).div_ceil(8)];
                status = query(&mut buf, &mut needed);
            }
            CloseHandle(process);
            if status < 0 {
                return None;
            }
            let text = &*buf.as_ptr().cast::<UNICODE_STRING>();
            if text.Buffer.is_null() {
                return Some(String::new());
            }
            let units = std::slice::from_raw_parts(text.Buffer, usize::from(text.Length / 2));
            Some(String::from_utf16_lossy(units))
        }
    }
}

// ---------------------------------------------------------------------------
// Unsupported platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_launch(_app: &Path, _data_dir: &Path) -> Result<()> {
    Err(anyhow!("guise is not supported on this platform"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_activate(_app: &Path, _data_dir: &Path) -> Result<()> {
    Err(anyhow!("guise is not supported on this platform"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_stop(_pid: u32, _force: bool) {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_pids(_data_dir: &Path) -> Result<Vec<u32>> {
    Err(anyhow!("guise is not supported on this platform"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_windows_command_lines() {
        assert_eq!(
            split_command_line(r#""C:\Program Files\Claude\claude.exe" --a "b c"  d"#),
            [r"C:\Program Files\Claude\claude.exe", "--a", "b c", "d"]
        );
        // Backslashes are literal unless they precede a quote.
        assert_eq!(
            split_command_line(r#"x a\\b "c d\\" e\"f "g""h""#),
            ["x", r"a\\b", r"c d\", r#"e"f"#, r#"g"h"#]
        );
        assert_eq!(split_command_line(""), [""]);
    }

    #[test]
    fn matches_only_the_exact_data_dir() {
        let dir = Path::new(r"C:\Users\Jo Doe\.config\guise\accounts\1-work\data");
        let hit = |cmdline: &str| targets_data_dir(cmdline, dir);
        // How guise launches it (the whole argument is quoted for the space)…
        assert!(hit(
            r#"claude.exe "--user-data-dir=C:\Users\Jo Doe\.config\guise\accounts\1-work\data""#
        ));
        // …how Chromium hands it to helper processes…
        assert!(hit(
            r#""C:\x\claude.exe" --type=renderer --user-data-dir="C:\Users\Jo Doe\.config\guise\accounts\1-work\data" --lang=en"#
        ));
        // …and case or slash differences are the same Windows path.
        assert!(hit(
            r#"claude.exe "--user-data-dir=c:/users/jo doe/.config/guise/accounts/1-work/data/""#
        ));
        // Other accounts, look-alike paths and look-alike flags never match.
        assert!(!hit(
            r#"claude.exe "--user-data-dir=C:\Users\Jo Doe\.config\guise\accounts\11-work\data""#
        ));
        assert!(!hit(
            r#"claude.exe "--user-data-dir=C:\Users\Jo Doe\.config\guise\accounts\1-work\data2""#
        ));
        assert!(!hit(
            r#"claude.exe "--user-data-dir-x=C:\Users\Jo Doe\.config\guise\accounts\1-work\data""#
        ));
        assert!(!hit("claude.exe"));
        // A different program merely mentioning the path in one argument.
        assert!(!hit(
            r#"notepad.exe "C:\Users\Jo Doe\.config\guise\accounts\1-work\data\--user-data-dir=""#
        ));
    }

    #[test]
    fn macos_flag_match_is_whole_argument() {
        let n = "--user-data-dir=/u/jo (x)/a/1-work/data";
        assert!(carries_flag(&format!("/A/Claude {n}"), n));
        assert!(carries_flag(
            &format!("/A/Claude Helper --type=gpu {n} --x"),
            n
        ));
        assert!(!carries_flag(&format!("/A/Claude {n}2"), n));
        assert!(!carries_flag(&format!("/A/Claude x{n}"), n));
        assert!(!carries_flag("/A/Claude", n));
    }

    /// Our own command line comes back from the native process listing.
    #[cfg(target_os = "windows")]
    #[test]
    fn lists_process_command_lines() {
        let me = std::process::id();
        let all = win::command_lines().unwrap();
        let (_, cmdline) = all
            .iter()
            .find(|(pid, _)| *pid == me)
            .expect("own process listed");
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(cmdline.to_lowercase().contains(&name), "{cmdline}");
    }

    /// A stand-in instance (a sleeping `cmd` carrying the flag) is detected,
    /// then quit, even with a space in its data dir.
    #[cfg(target_os = "windows")]
    #[test]
    fn detects_and_quits_an_instance() {
        let dir = std::env::temp_dir().join(format!("guise app test {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut flag = std::ffi::OsString::from("--user-data-dir=");
        flag.push(&dir);
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1 >NUL & rem"])
            .arg(&flag)
            .spawn()
            .unwrap();
        let app = RealApp;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.is_instance_running(&dir).unwrap() {
            assert!(Instant::now() < deadline, "stand-in instance not detected");
            sleep(Duration::from_millis(100));
        }
        app.quit_instance(&dir, Duration::from_secs(2)).unwrap();
        assert!(!app.is_instance_running(&dir).unwrap());
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
