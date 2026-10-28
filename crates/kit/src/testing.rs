//! Helpers for tests in every crate.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A new empty folder for one test. Tests run in parallel, so each gets its own (the process id
/// and a counter keep them apart).
pub fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("tvl-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the temp folder is writable");
    dir
}
