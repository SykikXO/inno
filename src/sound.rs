use std::process::Command;

/// Fire-and-forget sound player using paplay.
/// No background thread — each call spawns a detached paplay process.
pub struct SoundWorker;

impl SoundWorker {
    pub fn new() -> Self {
        Self
    }

    pub fn play(&self, path: &std::path::Path) {
        if !path.exists() {
            eprintln!("Sound: file not found: {:?}", path);
            return;
        }
        match Command::new("paplay").arg("--volume=65536").arg(path).spawn() {
            Ok(_) => {}
            Err(e) => eprintln!("Sound: paplay error: {}", e),
        }
    }
}
