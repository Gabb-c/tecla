use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        // tests run in parallel and several modules reuse names like "missing"
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tecla-test-{}-{n}-{name}", std::process::id()));
        // leftovers from an aborted run; usually nothing to remove
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // best effort; a leftover temp dir is harmless
        let _ = fs::remove_dir_all(&self.0);
    }
}
