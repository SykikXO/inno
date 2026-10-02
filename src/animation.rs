use anyhow::{anyhow, Context, Result};
use cairo::ImageSurface;
use std::path::{Path, PathBuf};

/// Loaded frame animation ready for playback
pub struct AnimPlayer {
    pub frames: Vec<ImageSurface>,
    pub fps: u64,
    pub loop_: bool,
    pub display: crate::config::DisplayMode,
    pub frame_idx: usize,
    pub frame_w: i32,
    pub frame_h: i32,
    done: bool,
}

impl std::fmt::Debug for AnimPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimPlayer")
            .field("frames", &self.frames.len())
            .field("fps", &self.fps)
            .field("loop_", &self.loop_)
            .field("display", &self.display)
            .field("frame_idx", &self.frame_idx)
            .field("frame_w", &self.frame_w)
            .field("frame_h", &self.frame_h)
            .field("done", &self.done)
            .finish()
    }
}

impl AnimPlayer {
    pub fn load<P: AsRef<Path>>(
        source: P,
        fps: u64,
        loop_: bool,
        display: crate::config::DisplayMode,
    ) -> Result<Self> {
        let path = source.as_ref();
        if !path.is_dir() {
            return Err(anyhow!("Animation source is not a directory: {:?}", path));
        }

        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .context("Failed to read animation directory")?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            })
            .collect();

        entries.sort_by(|a, b| nat_compare(a, b));

        if entries.is_empty() {
            return Err(anyhow!(
                "No PNG files found in animation directory: {:?}",
                path
            ));
        }

        let mut frames = Vec::with_capacity(entries.len());
        let mut dims: Option<(i32, i32)> = None;

        for entry in &entries {
            let mut file = std::fs::File::open(entry)
                .with_context(|| format!("Failed to open frame {:?}", entry))?;
            let surface = ImageSurface::create_from_png(&mut file)
                .map_err(|e| anyhow!("Failed to load frame {:?}: {}", entry, e))?;

            let (w, h) = (surface.width(), surface.height());
            match dims {
                None => dims = Some((w, h)),
                Some((ref_w, ref_h)) if w != ref_w || h != ref_h => {
                    // Warn but don't fail — use first frame dimensions
                    eprintln!(
                        "Warning: frame {:?} dimensions ({}x{}) differ from first frame ({}x{})",
                        entry, w, h, ref_w, ref_h
                    );
                }
                _ => {}
            }
            frames.push(surface);
        }

        let (frame_w, frame_h) = dims.unwrap_or((1, 1));
        // A zero fps would make the frame period divide to zero downstream.
        let fps = fps.max(1);

        Ok(Self {
            frames,
            fps,
            loop_,
            display,
            frame_idx: 0,
            frame_w,
            frame_h,
            done: false,
        })
    }

    pub fn tick(&mut self) {
        if self.done {
            return;
        }

        let next = self.frame_idx + 1;
        if next >= self.frames.len() {
            if self.loop_ {
                self.frame_idx = 0;
            } else {
                self.frame_idx = self.frames.len().saturating_sub(1);
                self.done = true;
            }
        } else {
            self.frame_idx = next;
        }
    }

    pub fn current_frame(&self) -> &ImageSurface {
        &self.frames[self.frame_idx]
    }

    pub fn reset(&mut self) {
        self.frame_idx = 0;
        self.done = false;
    }

    /// Whether a non-looping animation has completed
    pub fn is_done(&self) -> bool {
        self.done
    }
}

/// Orders filenames naturally: digit runs compare as numbers so `frame_9`
/// precedes `frame_10`. Splits each stem into digit and non-digit runs and
/// compares run by run, which yields a total order. Comparing only a trailing
/// number does not: `b1 < a9 < aa < b1` is a cycle.
fn nat_compare(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a_name = a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let b_name = b.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    let (mut a_chars, mut b_chars) = (a_name.chars().peekable(), b_name.chars().peekable());

    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ac), Some(bc)) => {
                let ordering = if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let an: String = a_chars.by_ref().take_while(char::is_ascii_digit).collect();
                    let bn: String = b_chars.by_ref().take_while(char::is_ascii_digit).collect();
                    // Compare by length first so arbitrarily long digit runs
                    // never overflow u64.
                    an.len().cmp(&bn.len()).then_with(|| an.cmp(&bn))
                } else {
                    match ac.cmp(&bc) {
                        std::cmp::Ordering::Equal => {
                            a_chars.next();
                            b_chars.next();
                            continue;
                        }
                        other => other,
                    }
                };
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_frames(dir: &Path, count: usize) {
        fs::create_dir_all(dir).unwrap();
        for i in 0..count {
            let path = dir.join(format!("frame_{:04}.png", i));
            // Create minimal valid 2x2 PNG
            let surface =
                ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
            {
                let mut f = fs::File::create(&path).unwrap();
                surface.write_to_png(&mut f).unwrap();
            }
        }
    }

    #[test]
    fn test_load_frames() {
        let dir = std::env::temp_dir().join("inno_test_anim_load");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 5);

        let player = AnimPlayer::load(&dir, 30, true, crate::config::DisplayMode::Anim).unwrap();
        assert_eq!(player.frames.len(), 5);
        assert_eq!(player.fps, 30);
        assert!(player.loop_);
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tick_looping() {
        let dir = std::env::temp_dir().join("inno_test_anim_loop");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 3);

        let mut player = AnimPlayer::load(&dir, 30, true, crate::config::DisplayMode::Anim).unwrap();

        assert_eq!(player.frame_idx, 0);
        player.tick();
        assert_eq!(player.frame_idx, 1);
        player.tick();
        assert_eq!(player.frame_idx, 2);
        // Should wrap
        player.tick();
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tick_non_looping() {
        let dir = std::env::temp_dir().join("inno_test_anim_nonloop");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 3);

        let mut player =
            AnimPlayer::load(&dir, 30, false, crate::config::DisplayMode::Anim).unwrap();

        player.tick();
        assert_eq!(player.frame_idx, 1);
        player.tick();
        assert_eq!(player.frame_idx, 2);
        assert!(!player.is_done());
        // Advance past end
        player.tick();
        assert_eq!(player.frame_idx, 2);
        assert!(player.is_done());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_reset() {
        let dir = std::env::temp_dir().join("inno_test_anim_reset");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 5);

        let mut player =
            AnimPlayer::load(&dir, 30, false, crate::config::DisplayMode::Anim).unwrap();

        for _ in 0..10 {
            player.tick();
        }
        assert!(player.is_done());
        assert_eq!(player.frame_idx, 4);

        player.reset();
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_nat_compare() {
        let a = Path::new("frame_0001.png");
        let b = Path::new("frame_0002.png");
        let c = Path::new("frame_0010.png");

        assert_eq!(nat_compare(a, b), std::cmp::Ordering::Less);
        assert_eq!(nat_compare(b, c), std::cmp::Ordering::Less);
        assert_eq!(nat_compare(a, a), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_load_missing_dir() {
        let result = AnimPlayer::load("/nonexistent/path", 30, true, crate::config::DisplayMode::Anim);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_empty_dir() {
        let dir = std::env::temp_dir().join("inno_test_anim_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let result = AnimPlayer::load(&dir, 30, true, crate::config::DisplayMode::Anim);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tick_idempotent_when_done() {
        let dir = std::env::temp_dir().join("inno_test_anim_done");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 2);

        let mut player =
            AnimPlayer::load(&dir, 30, false, crate::config::DisplayMode::Anim).unwrap();

        player.tick();
        player.tick(); // now done (idx=1, last frame)
        assert!(player.is_done());
        let idx_before = player.frame_idx;
        player.tick(); // should be no-op
        assert_eq!(player.frame_idx, idx_before);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_current_frame_accessible() {
        let dir = std::env::temp_dir().join("inno_test_anim_current");
        let _ = fs::remove_dir_all(&dir);
        create_test_frames(&dir, 3);

        let mut player =
            AnimPlayer::load(&dir, 30, true, crate::config::DisplayMode::Anim).unwrap();

        // Should return a valid surface for each frame
        let surface = player.current_frame();
        assert!(surface.width() > 0);
        assert!(surface.height() > 0);

        player.tick();
        let surface2 = player.current_frame();
        assert!(surface2.width() > 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
