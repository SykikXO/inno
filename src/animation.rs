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
    use crate::config::DisplayMode;
    use crate::testutil::TempDir;
    use std::cmp::Ordering;

    const FPS: u64 = 30;

    /// Builds a player directly. The state machine is pure index arithmetic, so
    /// most of its tests need no filesystem at all. `done` is private to this
    /// module, which is why the constructor lives here rather than in testutil.
    fn anim_player(frames: usize, fps: u64, loop_: bool) -> AnimPlayer {
        AnimPlayer {
            frames: (0..frames)
                .map(|_| cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap())
                .collect(),
            fps,
            loop_,
            display: DisplayMode::Anim,
            frame_idx: 0,
            frame_w: 2,
            frame_h: 2,
            done: false,
        }
    }

    fn load(dir: &Path, fps: u64, loop_: bool) -> AnimPlayer {
        AnimPlayer::load(dir, fps, loop_, DisplayMode::Anim).unwrap()
    }

    // --- loading ---------------------------------------------------------

    #[test]
    fn test_load_reads_every_png_in_order() {
        let dir = TempDir::new("anim-load");
        dir.write_frames(5);

        let player = load(dir.path(), FPS, true);
        assert_eq!(player.frames.len(), 5);
        assert_eq!(player.fps, FPS);
        assert!(player.loop_);
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_load_takes_dimensions_from_first_frame() {
        let dir = TempDir::new("anim-dims");
        dir.write_frames(2);

        let player = load(dir.path(), FPS, true);
        assert_eq!((player.frame_w, player.frame_h), (2, 2));
    }

    #[test]
    fn test_load_keeps_first_dimensions_when_a_frame_differs() {
        // Mismatched frames warn and keep the first frame's size rather than
        // failing, so one bad export does not kill the whole animation.
        let dir = TempDir::new("anim-mismatch");
        dir.write_frames(1);
        let bigger = cairo::ImageSurface::create(cairo::Format::ARgb32, 8, 8).unwrap();
        bigger
            .write_to_png(&mut std::fs::File::create(dir.join("frame_0001.png")).unwrap())
            .unwrap();

        let player = load(dir.path(), FPS, true);
        assert_eq!(player.frames.len(), 2);
        assert_eq!((player.frame_w, player.frame_h), (2, 2));
    }

    #[test]
    fn test_load_clamps_zero_fps_to_one() {
        // A zero fps would make the frame period divide to zero and spin the
        // event loop, so the loader clamps instead.
        let dir = TempDir::new("anim-fps0");
        dir.write_frames(2);

        assert_eq!(load(dir.path(), 0, true).fps, 1);
    }

    #[test]
    fn test_load_ignores_non_png_entries() {
        let dir = TempDir::new("anim-filter");
        dir.write_frames(2);
        std::fs::write(dir.join("notes.txt"), b"ignore me").unwrap();
        std::fs::write(dir.join("frame_0002.jpg"), b"ignore me").unwrap();
        // A dotfile named `.png` has no extension at all, so it is not a frame.
        std::fs::write(dir.join(".png"), b"ignore me").unwrap();
        std::fs::create_dir(dir.join("subdir")).unwrap();

        assert_eq!(load(dir.path(), FPS, true).frames.len(), 2);
    }

    #[test]
    fn test_load_accepts_uppercase_extension() {
        let dir = TempDir::new("anim-upper");
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
        surface
            .write_to_png(&mut std::fs::File::create(dir.join("a.PNG")).unwrap())
            .unwrap();
        std::fs::copy(dir.join("a.PNG"), dir.join("b.png")).unwrap();

        assert_eq!(load(dir.path(), FPS, true).frames.len(), 2);
    }

    #[test]
    fn test_load_rejects_missing_directory() {
        let dir = TempDir::new("anim-absent");
        let missing = dir.join("nope");
        let err = AnimPlayer::load(&missing, FPS, true, DisplayMode::Anim).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "got: {}", err);
    }

    #[test]
    fn test_load_rejects_directory_with_no_frames() {
        let dir = TempDir::new("anim-empty");
        let err = AnimPlayer::load(dir.path(), FPS, true, DisplayMode::Anim).unwrap_err();
        assert!(err.to_string().contains("No PNG files"), "got: {}", err);
    }

    #[test]
    fn test_load_rejects_a_corrupt_frame() {
        let dir = TempDir::new("anim-corrupt");
        dir.write_frames(2);
        std::fs::write(dir.join("frame_0001.png"), b"not a png").unwrap();

        let err = AnimPlayer::load(dir.path(), FPS, true, DisplayMode::Anim).unwrap_err();
        assert!(err.to_string().contains("frame_0001"), "got: {}", err);
    }

    // --- looping state machine ------------------------------------------

    #[test]
    fn test_tick_wraps_when_looping() {
        let mut player = anim_player(3, FPS, true);
        for expected in [1, 2, 0, 1, 2] {
            player.tick();
            assert_eq!(player.frame_idx, expected);
        }
        assert!(!player.is_done());
    }

    #[test]
    fn test_tick_stops_at_last_frame_when_not_looping() {
        let mut player = anim_player(3, FPS, false);
        for expected in [1, 2] {
            player.tick();
            assert_eq!(player.frame_idx, expected);
        }
        assert!(!player.is_done());

        player.tick();
        assert_eq!(player.frame_idx, 2);
        assert!(player.is_done());
    }

    #[test]
    fn test_tick_is_a_noop_once_done() {
        let mut player = anim_player(2, FPS, false);
        player.tick();
        player.tick();
        assert!(player.is_done());

        player.tick();
        assert_eq!(player.frame_idx, 1);
    }

    #[test]
    fn test_single_frame_looping_never_completes() {
        let mut player = anim_player(1, FPS, true);
        for _ in 0..5 {
            player.tick();
        }
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_single_frame_non_looping_completes_on_first_tick() {
        let mut player = anim_player(1, FPS, false);
        player.tick();
        assert_eq!(player.frame_idx, 0);
        assert!(player.is_done());
    }

    #[test]
    fn test_reset_rearms_a_completed_animation() {
        let mut player = anim_player(3, FPS, false);
        for _ in 0..5 {
            player.tick();
        }
        assert!(player.is_done());

        player.reset();
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_large_frame_count_cycles_without_drift() {
        let mut player = anim_player(256, FPS, true);
        for i in 1..=256 {
            player.tick();
            assert_eq!(player.frame_idx, i % 256);
        }
    }

    #[test]
    fn test_current_frame_is_always_in_bounds() {
        let mut player = anim_player(7, FPS, true);
        for _ in 0..100 {
            player.tick();
            assert!(player.frame_idx < player.frames.len());
            let frame = player.current_frame();
            assert!(frame.width() > 0);
        }
    }

    // --- filename ordering ------------------------------------------------

    #[test]
    fn test_nat_compare_orders_numerically_not_lexicographically() {
        // The one case that separates a natural sort from a string sort.
        assert_eq!(
            nat_compare(Path::new("frame_9.png"), Path::new("frame_10.png")),
            Ordering::Less
        );
    }

    #[test]
    fn test_nat_compare_is_a_total_order_over_mixed_names() {
        // Comparing only a trailing number and falling back to string compare
        // admits the cycle b1 < a9 < aa < b1, so no sorted order exists for
        // this set. Sorting it and re-checking the comparator is what proves
        // the fix.
        let mut names = vec![
            "b1.png", "a9.png", "aa.png", "frame_2.png", "frame_10.png", "frame_1.png",
            "z.png", "a10.png", "a1.png",
        ];
        names.sort_by(|a, b| nat_compare(Path::new(a), Path::new(b)));
        for pair in names.windows(2) {
            let ordering = nat_compare(Path::new(&pair[0]), Path::new(&pair[1]));
            assert_ne!(
                ordering,
                Ordering::Greater,
                "{} should not sort after {} (result {:?})",
                pair[0],
                pair[1],
                names
            );
        }
    }

    #[test]
    fn test_nat_compare_handles_digit_runs_too_long_for_u64() {
        let huge = Path::new("frame_99999999999999999999999999.png");
        let small = Path::new("frame_1.png");
        assert_eq!(nat_compare(huge, small), Ordering::Greater);
        assert_eq!(nat_compare(small, huge), Ordering::Less);
    }

    /// Writes a 2x2 PNG whose red channel encodes `value`, so the sort order
    /// can be read back off the decoded frames.
    fn write_tagged_frame(dir: &Path, name: &str, value: u8) {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgba(value as f64 / 255.0, 0.0, 0.0, 1.0);
            cr.paint().unwrap();
        }
        surface.flush();
        surface
            .write_to_png(&mut std::fs::File::create(dir.join(name)).unwrap())
            .unwrap();
    }

    fn red_channel(surface: &mut cairo::ImageSurface) -> u8 {
        // ARGB32 in native byte order, so on little-endian this is index 2.
        surface.data().unwrap()[2]
    }

    #[test]
    fn test_load_sorts_frames_naturally() {
        let dir = TempDir::new("anim-sort");
        write_tagged_frame(dir.path(), "f_10.png", 10);
        write_tagged_frame(dir.path(), "f_2.png", 2);
        write_tagged_frame(dir.path(), "f_1.png", 1);

        let mut player = load(dir.path(), FPS, true);
        let order: Vec<u8> = (0..player.frames.len())
            .map(|i| red_channel(&mut player.frames[i]))
            .collect();
        assert_eq!(order, vec![1, 2, 10], "frames should sort numerically");
    }
}
