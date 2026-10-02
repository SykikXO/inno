//! Helpers shared by unit tests. Not compiled into the binary.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop, including during a panic unwind.
///
/// The name carries the pid and a per-process counter so that two concurrent
/// `cargo test` runs on one machine, or parallel threads within one run, cannot
/// delete each other's frames mid-test.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "inno-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        // create_dir, not create_dir_all: a collision should fail loudly rather
        // than silently hand back a directory another run is still using.
        std::fs::create_dir(&path).expect("failed to create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes `count` 2x2 PNG frames named `frame_0000.png`, `frame_0001.png`, ...
    pub fn write_frames(&self, count: usize) {
        for i in 0..count {
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
            let mut file =
                std::fs::File::create(self.0.join(format!("frame_{i:04}.png"))).unwrap();
            surface.write_to_png(&mut file).unwrap();
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
