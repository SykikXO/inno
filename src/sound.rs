use std::path::PathBuf;
use std::process::Command;

/// Channel-backed sound worker — single thread, uses paplay for reliable PipeWire routing
pub struct SoundWorker {
    tx: Option<std::sync::mpsc::Sender<PathBuf>>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl SoundWorker {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();

        let handle = std::thread::Builder::new()
            .name("sound-worker".into())
            .spawn(move || {
                while let Ok(path) = rx.recv() {
                    if !path.exists() {
                        eprintln!("Sound: file not found: {:?}", path);
                        continue;
                    }
                    match Command::new("paplay")
                        .arg("--volume=65536")
                        .arg(&path)
                        .output()
                    {
                        Ok(out) => {
                            if !out.status.success() {
                                let stderr = String::from_utf8_lossy(&out.stderr);
                                eprintln!("Sound: paplay failed: {}", stderr);
                            }
                        }
                        Err(e) => eprintln!("Sound: paplay error: {}", e),
                    }
                }
            })
            .expect("Failed to spawn sound thread");

        Self {
            tx: Some(tx),
            _handle: Some(handle),
        }
    }

    pub fn play(&self, path: &std::path::Path) {
        if let Some(ref tx) = self.tx {
            let _ = tx.send(path.to_path_buf());
        }
    }
}
