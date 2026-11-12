//! Stand-in for Claude Desktop in guise's Windows end-to-end test. Like
//! Electron's single-instance lock, one process holds each --user-data-dir: a
//! second launch for the same dir leaves a `focused` marker and exits.
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::{thread, time::Duration};

fn main() {
    let dir: PathBuf = std::env::args()
        .find_map(|a| a.strip_prefix("--user-data-dir=").map(PathBuf::from))
        .expect("no --user-data-dir");
    fs::create_dir_all(&dir).unwrap();
    // share_mode(0): held exclusively while this process lives, free once it dies.
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .share_mode(0)
        .open(dir.join("lockfile"));
    let Ok(_lock) = lock else {
        fs::write(dir.join("focused"), "").unwrap();
        return;
    };
    fs::write(dir.join("config.json"), "{}").unwrap();
    thread::sleep(Duration::from_secs(600));
}
