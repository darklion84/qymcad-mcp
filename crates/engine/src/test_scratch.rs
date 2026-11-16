//! One scratch directory per test process, `<temp>/qymcad-mcp-tests/<pid>/`, swept when its process is gone.
//! Shared by the engine's unit tests, the engine's integration tests and the server's integration tests (`#[path]`).
//!
//! Each process holds an exclusive lock on `<pid>.lock` for its whole life; the OS drops the lock when the process ends,
//! however it ends. The first use in a process removes every sibling whose lock can be taken (its process is gone) and
//! empties its own directory, which an earlier process with the same pid may have left behind.
#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// A lock file this young may belong to a process that created it and has not locked it yet.
const FRESH: Duration = Duration::from_secs(60);

/// This process's scratch directory, created (empty) on first use.
pub fn dir() -> &'static Path {
    static DIR: OnceLock<(PathBuf, File)> = OnceLock::new();
    &DIR.get_or_init(|| {
        let root = std::env::temp_dir().join("qymcad-mcp-tests");
        fs::create_dir_all(&root).unwrap();
        let pid = std::process::id().to_string();
        let lock = File::create(root.join(format!("{pid}.lock"))).unwrap();
        lock.lock().unwrap();
        sweep(&root, &pid);
        let dir = root.join(&pid);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (dir, lock)
    })
    .0
}

/// `name` inside this process's scratch directory. Names must be unique within the process.
pub fn path(name: &str) -> PathBuf {
    dir().join(name)
}

/// Remove the directories of processes that no longer hold their lock.
fn sweep(root: &Path, own: &str) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_suffix(".lock")) else { continue };
        if pid == own {
            continue;
        }
        let fresh =
            entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < FRESH);
        if fresh {
            continue;
        }
        let Ok(file) = File::open(entry.path()) else { continue };
        if file.try_lock().is_ok() {
            let _ = fs::remove_dir_all(root.join(pid));
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[test]
fn sweep_removes_the_directories_of_gone_processes_and_keeps_live_and_fresh_ones() {
    let root = path("sweep-root");
    fs::create_dir_all(&root).unwrap();
    let old = SystemTime::now() - 2 * FRESH;
    let make = |pid: &str, modified: SystemTime| {
        fs::create_dir_all(root.join(pid)).unwrap();
        fs::write(root.join(pid).join("file"), b"x").unwrap();
        let lock = File::create(root.join(format!("{pid}.lock"))).unwrap();
        lock.set_modified(modified).unwrap();
        lock
    };
    drop(make("1", old)); // its process is gone: nobody holds the lock
    let live = make("2", old);
    live.lock().unwrap();
    drop(make("3", SystemTime::now())); // created a moment ago, maybe not locked yet
    drop(make("4", old)); // this process's own
    sweep(&root, "4");
    assert!(!root.join("1").exists() && !root.join("1.lock").exists());
    for kept in ["2", "3", "4"] {
        assert!(root.join(kept).join("file").exists() && root.join(format!("{kept}.lock")).exists(), "{kept} must stay");
    }
}
