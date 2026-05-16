use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

/// Channel-backed sound worker — single thread, single OutputStream
pub struct SoundWorker {
    tx: Option<std::sync::mpsc::Sender<PathBuf>>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl SoundWorker {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();

        let handle = std::thread::spawn(move || {
            let Ok((stream, stream_handle)) = rodio::OutputStream::try_default() else {
                eprintln!("Sound: failed to create OutputStream");
                return;
            };
            let sink = match rodio::Sink::try_new(&stream_handle) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Sound: failed to create Sink: {}", e);
                    return;
                }
            };
            let _stream = stream;

            while let Ok(path) = rx.recv() {
                if !path.exists() {
                    eprintln!("Sound file not found: {:?}", path);
                    continue;
                }
                if let Ok(file) = File::open(&path) {
                    let reader = BufReader::new(file);
                    if let Ok(source) = rodio::Decoder::new(reader) {
                        sink.append(source);
                        sink.sleep_until_end();
                    }
                }
            }
        });

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
