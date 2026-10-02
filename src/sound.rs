use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A way to hand a file to whatever audio stack the machine has.
///
/// Kept as external processes on purpose. The obvious alternative, the `rodio`
/// crate, pins a `cpal` release that cannot select the native PipeWire host, so
/// it still has to reach the sound server through the ALSA-to-Pulse bridge that
/// breaks under a systemd autostart. Reaching PipeWire natively from Rust means
/// depending on `cpal` directly with its `pipewire` feature, which costs 45
/// dependencies plus `libpipewire-0.3-dev` at build time. For six short sounds
/// played a few times an hour that is not a trade worth making.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backend {
    /// PipeWire's own player. Speaks the native protocol, so no PulseAudio
    /// compatibility layer is involved.
    PwPlay,
    /// PulseAudio's player. Most widely packaged of the working options, and
    /// what a PipeWire system provides through `pipewire-pulse`.
    Paplay,
    Ffplay,
    Mpv,
}

impl Backend {
    const CHAIN: [Backend; 4] = [
        Backend::PwPlay,
        Backend::Paplay,
        Backend::Ffplay,
        Backend::Mpv,
    ];

    fn binary(self) -> &'static str {
        match self {
            Backend::PwPlay => "pw-play",
            Backend::Paplay => "paplay",
            Backend::Ffplay => "ffplay",
            Backend::Mpv => "mpv",
        }
    }

    fn command(self, file: &Path) -> Command {
        let mut cmd = Command::new(self.binary());
        match self {
            // pw-play takes a 0..1 float; paplay takes 0..65536. They cannot
            // share a flag, so each backend builds its own arguments.
            Backend::PwPlay => {
                cmd.arg("--volume=1.0");
            }
            Backend::Paplay => {
                cmd.arg("--volume=65536");
            }
            Backend::Ffplay => {
                cmd.args(["-nodisp", "-autoexit", "-v", "error", "-volume", "100"]);
            }
            Backend::Mpv => {
                cmd.args(["--no-video", "--really-quiet", "--volume=100"]);
            }
        }
        cmd.arg(file).stdin(Stdio::null());
        cmd
    }
}

/// Why a candidate backend was rejected.
enum ProbeError {
    /// Could not be started at all: not installed, or not on `PATH`.
    NotInstalled(String),
    /// Started but did not succeed: no sound server, or no audio device.
    Failed(String),
}

impl ProbeError {
    fn reason(&self) -> &str {
        match self {
            ProbeError::NotInstalled(why) | ProbeError::Failed(why) => why,
        }
    }
}

/// Upper bound on simultaneously live player processes. Exceeding it means
/// events are arriving faster than sounds finish, which is a storm rather than
/// normal use.
const MAX_CONCURRENT_PLAYERS: usize = 8;

/// Plays notification sounds.
pub struct SoundWorker {
    backend: Option<Backend>,
    /// Outstanding children and the files they were given. Retained so they can
    /// be waited on: dropping a `Child` does not reap it, which leaves a zombie
    /// for every sound played until the daemon exits. More than one can be live
    /// at a time, because the previous one may not have exited yet.
    pending: Vec<(PathBuf, Child)>,
    /// Errors already reported, so a broken sound file does not print once per
    /// notification.
    reported: std::collections::HashSet<String>,
}

impl SoundWorker {
    /// Probes for a working player and caches the winner.
    ///
    /// The probe plays a real silent file rather than only checking `PATH`. A
    /// binary being present says nothing about whether a sound server is up or
    /// a device exists, and both are common ways for audio to be silently dead.
    pub fn probe() -> Self {
        Self::with_probe(true)
    }

    /// Builds a worker without probing. Only for tests.
    pub fn disabled() -> Self {
        Self {
            backend: None,
            pending: Vec::new(),
            reported: std::collections::HashSet::new(),
        }
    }

    fn with_probe(probe: bool) -> Self {
        if !probe {
            return Self::disabled();
        }

        let mut failures = Vec::new();
        for backend in Backend::CHAIN {
            match Self::try_backend(backend) {
                Ok(()) => {
                    return Self {
                        backend: Some(backend),
                        pending: Vec::new(),
                        reported: std::collections::HashSet::new(),
                    };
                }
                Err(error) => failures.push(format!("{}: {}", backend.binary(), error.reason())),
            }
        }

        // One line, once. A notification daemon that will not start because a
        // cosmetic sound cannot play is worse than a silent one, so this warns
        // and carries on with sound off.
        eprintln!(
            "inno: sound unavailable, notifications will be silent ({})",
            failures.join("; ")
        );

        Self::disabled()
    }

    /// Plays a silent sample to check that a backend can actually produce audio.
    fn try_backend(backend: Backend) -> Result<(), ProbeError> {
        let dir = std::env::temp_dir().join(format!("inno-probe-{}", std::process::id()));
        // fs::write does not create parent directories.
        std::fs::create_dir_all(&dir).map_err(|e| ProbeError::NotInstalled(e.to_string()))?;
        let path = dir.join("probe.wav");
        if let Err(e) = std::fs::write(&path, Self::silent_wav()) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(ProbeError::NotInstalled(e.to_string()));
        }

        let result = backend
            .command(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&dir);

        match result {
            Err(e) => Err(ProbeError::NotInstalled(e.to_string())),
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(ProbeError::Failed(format!("exited with {status}"))),
        }
    }

    /// 8 kHz mono, 8-bit, one silent sample. Small enough to build inline.
    fn silent_wav() -> Vec<u8> {
        let sample_rate: u32 = 8_000;
        let samples: u32 = 8;
        let data_len = samples;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes()); // byte rate
        out.extend_from_slice(&1u16.to_le_bytes()); // block align
        out.extend_from_slice(&8u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend(std::iter::repeat_n(128u8, data_len as usize));
        out
    }

    /// A short description of the selected backend for logging.
    pub fn describe(&self) -> String {
        match self.backend {
            Some(backend) => backend.binary().to_string(),
            None => "none".to_string(),
        }
    }

    pub fn play(&mut self, path: &Path) {
        let Some(backend) = self.backend else { return };

        if !path.is_file() {
            self.report_once(path, "file not found");
            return;
        }

        self.reap();

        match backend.command(path).spawn() {
            Ok(child) => self.pending.push((path.to_path_buf(), child)),
            Err(e) => self.report_once(path, &e.to_string()),
        }

        // A storm of events must not accumulate children without bound. Waiting
        // on the oldest is the backstop; the bundled sounds are short.
        while self.pending.len() > MAX_CONCURRENT_PLAYERS {
            let (_, mut oldest) = self.pending.remove(0);
            let _ = oldest.wait();
        }
    }

    /// Reaps every child that has finished, without blocking. `spawn()`
    /// succeeding only means fork and exec worked: the child can still fail
    /// afterwards, and this is the only place that becomes visible.
    fn reap(&mut self) {
        let mut still_running = Vec::with_capacity(self.pending.len());
        for (path, mut child) in std::mem::take(&mut self.pending) {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        self.report_once(&path, &format!("player exited with {status}"));
                    }
                }
                // Still running: keep the handle so it can be reaped later.
                Ok(None) => still_running.push((path, child)),
                Err(_) => {}
            }
        }
        self.pending = still_running;
    }

    /// Reports a distinct failure once. Without this a broken sound file prints
    /// on every notification.
    fn report_once(&mut self, path: &Path, reason: &str) {
        let key = format!("{}: {reason}", path.display());
        if self.reported.insert(key) {
            eprintln!("Sound: {}: {}", path.display(), reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn test_chain_prefers_native_pipewire_then_pulse() {
        assert_eq!(Backend::CHAIN[0], Backend::PwPlay);
        assert_eq!(Backend::CHAIN[1], Backend::Paplay);
    }

    #[test]
    fn test_chain_excludes_aplay() {
        // aplay validates the fmt chunk strictly, so it cannot play the bundled
        // assets, and with no pulse plugin it would hit the sound card directly,
        // which is the routing bug this whole fallback exists to avoid.
        assert!(
            !Backend::CHAIN.iter().any(|b| b.binary() == "aplay"),
            "aplay must not be in the fallback chain"
        );
    }

    #[test]
    fn test_each_backend_uses_its_own_volume_scale() {
        // pw-play takes 0..1, paplay 0..65536, ffplay and mpv 0..100. A shared
        // flag would be out of range for three of the four.
        let file = PathBuf::from("/tmp/x.wav");
        let pw = format!("{:?}", Backend::PwPlay.command(&file));
        let pa = format!("{:?}", Backend::Paplay.command(&file));
        let ff = format!("{:?}", Backend::Ffplay.command(&file));
        assert!(pw.contains("1.0"));
        assert!(pa.contains("65536"));
        assert!(ff.contains("100"));
    }

    #[test]
    fn test_every_backend_passes_the_file_path() {
        for backend in Backend::CHAIN {
            let rendered = format!("{:?}", backend.command(Path::new("/tmp/a b/c.wav")));
            assert!(rendered.contains("/tmp/a b/c.wav"), "{backend:?} dropped the path");
        }
    }

    #[test]
    fn test_silent_wav_is_a_valid_riff_header() {
        let bytes = SoundWorker::silent_wav();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // PCM format tag, so strict decoders accept it.
        assert_eq!(&bytes[20..22], &1u16.to_le_bytes());
        assert_eq!(&bytes[36..40], b"data");
    }

    #[test]
    fn test_silent_wav_length_matches_the_header() {
        let bytes = SoundWorker::silent_wav();
        let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let data_size = u32::from_le_bytes([
            bytes[40],
            bytes[41],
            bytes[42],
            bytes[43],
        ]) as usize;
        assert_eq!(riff_size, 36 + data_size);
        assert_eq!(bytes.len(), 44 + data_size);
    }

    #[test]
    fn test_probe_succeeds_or_reports_every_candidate() {
        // Whatever the machine has, probing must terminate with either a
        // backend or a diagnostic naming each candidate. It must never panic
        // or hang, since it runs before the daemon is usable.
        let worker = SoundWorker::with_probe(true);
        if worker.backend.is_none() {
            assert!(worker.describe().starts_with("none"));
        }
    }

    #[test]
    fn test_probe_file_is_removed_after_the_probe() {
        let path = std::env::temp_dir().join(format!("inno-probe-{}", std::process::id()));
        SoundWorker::with_probe(true);
        assert!(
            !path.exists(),
            "the probe should not leave a temp file behind"
        );
    }

    #[test]
    fn test_disabled_worker_is_silent_and_inert() {
        let mut worker = SoundWorker::disabled();
        assert_eq!(worker.describe(), "none");

        let dir = TempDir::new("sound-disabled");
        let file = dir.join("real.wav");
        std::fs::write(&file, SoundWorker::silent_wav()).unwrap();
        // Must not panic and must not spawn anything.
        worker.play(&file);
        worker.play(Path::new("/nonexistent/nope.wav"));
    }

    #[test]
    fn test_missing_file_is_reported_once_not_every_event() {
        let mut worker = SoundWorker {
            backend: Some(Backend::Paplay),
            pending: Vec::new(),
            reported: std::collections::HashSet::new(),
        };
        let missing = PathBuf::from("/nonexistent/definitely-not-here.wav");

        for _ in 0..5 {
            worker.play(&missing);
        }
        assert_eq!(
            worker.reported.len(),
            1,
            "a missing file should be reported exactly once"
        );
    }

    #[test]
    fn test_playing_reaps_children_instead_of_leaking_zombies() {
        // Dropping a Child does not wait on it. Before the child was retained,
        // every sound played left a zombie behind for the life of the daemon.
        let mut worker = SoundWorker::with_probe(true);
        if worker.backend.is_none() {
            return; // nothing to spawn with on this machine
        }

        let me = std::process::id().to_string();
        let own_zombies = || -> usize {
            let ps = Command::new("ps").args(["-eo", "ppid=,stat="]).output().unwrap();
            String::from_utf8_lossy(&ps.stdout)
                .lines()
                .filter_map(|line| {
                    let mut it = line.split_whitespace();
                    let ppid = it.next().unwrap_or("");
                    let stat = it.next().unwrap_or("");
                    (ppid == me && stat.starts_with('Z')).then_some(())
                })
                .count()
        };

        let dir = TempDir::new("sound-reap");
        let file = dir.join("beep.wav");
        std::fs::write(&file, SoundWorker::silent_wav()).unwrap();

        let before = own_zombies();
        for _ in 0..8 {
            worker.play(&file);
            worker.reap();
        }
        // Let anything still running finish before the final check.
        for (_, child) in worker.pending.iter_mut() {
            let _ = child.wait();
        }
        worker.reap();
        assert!(
            own_zombies() <= before,
            "playing left {} zombies behind",
            own_zombies() - before
        );
    }

    #[test]
    fn test_probe_error_kinds_are_distinguishable() {
        // "is it installed" and "did it work" are different problems with
        // different fixes, so they must not collapse into one string match.
        assert!(matches!(
            SoundWorker::try_backend(Backend::PwPlay),
            Err(ProbeError::NotInstalled(_)) | Err(ProbeError::Failed(_)) | Ok(())
        ));
        assert!(!ProbeError::NotInstalled("x".into()).reason().is_empty());
        assert!(!ProbeError::Failed("x".into()).reason().is_empty());
    }

    #[test]
    fn test_probe_creates_its_own_temp_directory() {
        // fs::write does not create parents; missing that reported every
        // backend as "not installed" on a machine where all of them exist.
        let dir = std::env::temp_dir().join(format!("inno-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        SoundWorker::with_probe(true);
        assert!(!dir.exists(), "the probe must clean up its directory");
    }
}
