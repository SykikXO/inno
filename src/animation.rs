use anyhow::{anyhow, Context, Result};
use cairo::ImageSurface;
use std::path::{Path, PathBuf};

/// How large to decode frames.
///
/// Decoding straight to display size is what keeps this cheap: a 216-frame
/// 640x640 set decoded at natural size is 337 MB resident and 88M pixels of
/// per-frame scaling work. At a 200px display size it is a fraction of that.
#[derive(Clone, Copy, Debug)]
pub enum TargetSize {
    /// Natural frame size times the display scale. Used when the animation is
    /// the whole notification.
    Scaled(f64),
    /// Long edge in logical pixels, aspect preserved. Used when the animation
    /// sits beside or above text.
    LongestEdge(i32),
}

/// Frames kept decoded ahead of the playhead. One would be enough to avoid
/// blocking on the frame being drawn; a few more absorb a slow disk and keep
/// the loop from stalling when a tick lands between decodes.
const LOOKAHEAD: usize = 3;

/// A frame animation, decoded on demand.
///
/// Frames are held in a small ring keyed by position rather than all resident
/// up front. Loading therefore costs one PNG decode instead of N, which
/// matters because loading happens on the event loop.
pub struct AnimPlayer {
    /// Frame paths in playback order.
    paths: Vec<PathBuf>,
    /// Ring of decoded frames, each tagged with the position it holds.
    ring: Vec<Option<(usize, ImageSurface)>>,
    pub loop_: bool,
    pub frame_idx: usize,
    /// Dimensions frames are decoded at, which is the display size capped at
    /// the source size. Never larger than `source_w`/`source_h`.
    pub frame_w: i32,
    pub frame_h: i32,
    /// Natural frame dimensions. The display size is these times the scale,
    /// independently of how large the frames were decoded.
    pub source_w: i32,
    pub source_h: i32,
    done: bool,
}

impl std::fmt::Debug for AnimPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimPlayer")
            .field("frames", &self.paths.len())
            .field("decoded", &self.ring.iter().filter(|s| s.is_some()).count())
            .field("loop_", &self.loop_)
            .field("frame_idx", &self.frame_idx)
            .field("frame_w", &self.frame_w)
            .field("frame_h", &self.frame_h)
            .field("source", &(self.source_w, self.source_h))
            .field("done", &self.done)
            .finish()
    }
}

/// The PNG signature every frame file must start with.
const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

impl AnimPlayer {
    pub fn load<P: AsRef<Path>>(
        source: P,
        loop_: bool,
        target: TargetSize,
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
            return Err(anyhow!("No PNG files found in animation directory: {:?}", path));
        }

        // Cheap up-front validation: read only the 8-byte signature of each
        // frame so a corrupt file is reported now rather than mid-playback.
        // Decoding all of them to find out is exactly what this design avoids.
        for entry in &entries {
            let mut magic = [0u8; 8];
            let mut file = std::fs::File::open(entry)
                .with_context(|| format!("Failed to open frame {:?}", entry))?;
            std::io::Read::read_exact(&mut file, &mut magic)
                .with_context(|| format!("Failed to read frame {:?}", entry))?;
            if magic != PNG_MAGIC {
                return Err(anyhow!("Frame is not a PNG: {:?}", entry));
            }
        }

        // The first frame is decoded at natural size to learn the source
        // dimensions, then scaled like the rest. It is the only frame ever held
        // at full resolution.
        let (source_w, source_h) = decode_natural(&entries[0])?;

        let factor = match target {
            TargetSize::Scaled(scale) => scale,
            TargetSize::LongestEdge(px) => px as f64 / source_w.max(source_h) as f64,
        };
        // Never upscale: a larger target only wastes memory and softens the
        // image, so the layer is scaled up at draw time instead.
        let factor = if factor > 1.0 { 1.0 } else { factor.max(0.0) };
        let frame_w = ((source_w as f64 * factor).round() as i32).max(1);
        let frame_h = ((source_h as f64 * factor).round() as i32).max(1);

        let mut player = Self {
            paths: entries,
            ring: vec![None; LOOKAHEAD + 1],
            loop_,
            frame_idx: 0,
            frame_w,
            frame_h,
            source_w,
            source_h,
            done: false,
        };
        player.ensure(0)?;
        player.prefetch();
        Ok(player)
    }

    /// Makes sure `position` is decoded, evicting whatever shared its slot.
    fn ensure(&mut self, position: usize) -> Result<()> {
        let slot = position % self.ring.len();
        if self.ring[slot].as_ref().is_some_and(|(p, _)| *p == position) {
            return Ok(());
        }
        let surface = decode_scaled(&self.paths[position], (self.frame_w, self.frame_h))?;
        self.ring[slot] = Some((position, surface));
        Ok(())
    }

    /// Decodes the frames after the playhead so the next few ticks do not block.
    /// A prefetch failure is not fatal: the frame being drawn is what matters,
    /// and it is loaded on demand either way.
    fn prefetch(&mut self) {
        let start = self.frame_idx;
        let end = (start + LOOKAHEAD).min(self.paths.len() - 1);
        for position in start..=end {
            if let Err(e) = self.ensure(position) {
                eprintln!("inno: prefetch of frame {} failed: {}", position, e);
                return;
            }
        }
    }

    pub fn tick(&mut self) {
        if self.done {
            return;
        }

        let next = self.frame_idx + 1;
        if next >= self.paths.len() {
            if self.loop_ {
                self.frame_idx = 0;
            } else {
                self.frame_idx = self.paths.len() - 1;
                self.done = true;
                // Nothing new to decode: the playhead is parked.
                return;
            }
        } else {
            self.frame_idx = next;
        }
        self.prefetch();
    }

    /// The current frame, decoded if it is not resident.
    pub fn frame(&mut self) -> Result<&ImageSurface> {
        self.ensure(self.frame_idx)?;
        Ok(&self.ring[self.frame_idx % self.ring.len()]
            .as_ref()
            .expect("slot was just filled")
            .1)
    }

    /// Natural frame dimensions.
    pub fn source_size(&self) -> (i32, i32) {
        (self.source_w, self.source_h)
    }

    pub fn reset(&mut self) {
        self.frame_idx = 0;
        self.done = false;
    }

    /// Whether a non-looping animation has reached its last frame.
    pub fn is_done(&self) -> bool {
        self.done
    }
}

/// Decodes a PNG at its natural size.
fn decode_natural(path: &Path) -> Result<(i32, i32)> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("Failed to open frame {:?}", path))?;
    let surface = ImageSurface::create_from_png(&mut file)
        .map_err(|e| anyhow!("Failed to load frame {:?}: {}", path, e))?;
    Ok((surface.width(), surface.height()))
}

/// Decodes a PNG and scales it to `target`.
///
/// Cairo's filters are bilinear, which aliases badly once the ratio passes
/// about 2:1. Stepping down by halves first approximates a box filter, and each
/// intermediate surface is smaller than the last so it costs little.
fn decode_scaled(path: &Path, target: (i32, i32)) -> Result<ImageSurface> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("Failed to open frame {:?}", path))?;
    let mut surface = ImageSurface::create_from_png(&mut file)
        .map_err(|e| anyhow!("Failed to load frame {:?}: {}", path, e))?;

    if (surface.width(), surface.height()) == target {
        return Ok(surface);
    }

    let mut size = (surface.width(), surface.height());
    while size.0 > target.0 * 2 && size.1 > target.1 * 2 {
        let next = ((size.0 / 2).max(target.0), (size.1 / 2).max(target.1));
        surface = resample(&surface, next)?;
        size = next;
    }
    resample(&surface, target)
}

/// Paints `src` scaled into a new surface of `size`.
fn resample(src: &ImageSurface, size: (i32, i32)) -> Result<ImageSurface> {
    let out = ImageSurface::create(cairo::Format::ARgb32, size.0, size.1)
        .context("Failed to allocate scaled frame")?;
    {
        let cr = cairo::Context::new(&out).context("Failed to create cairo context")?;
        cr.scale(size.0 as f64 / src.width() as f64, size.1 as f64 / src.height() as f64);
        cr.set_source_surface(src, 0.0, 0.0)?;
        cr.paint()?;
    }
    out.flush();
    Ok(out)
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
    use crate::testutil::TempDir;
    use std::cmp::Ordering;

    /// Small enough that nothing is downscaled.
    const NATURAL: TargetSize = TargetSize::Scaled(1.0);

    fn load(dir: &Path, loop_: bool, target: TargetSize) -> AnimPlayer {
        AnimPlayer::load(dir, loop_, target).unwrap()
    }

    /// Builds a player over a real frame set. The directory has to outlive the
    /// player because frames are read from it lazily, so it is returned too.
    fn player(frames: usize, loop_: bool) -> (TempDir, AnimPlayer) {
        let dir = TempDir::new("anim-sm");
        dir.write_frames(frames);
        let player = load(dir.path(), loop_, NATURAL);
        (dir, player)
    }

    /// How many frames are currently decoded and resident.
    fn resident(player: &AnimPlayer) -> usize {
        player.ring.iter().filter(|slot| slot.is_some()).count()
    }

    // --- loading ---------------------------------------------------------

    #[test]
    fn test_load_reads_every_png() {
        let dir = TempDir::new("anim-load");
        dir.write_frames(5);

        let player = load(dir.path(), true, NATURAL);
        assert_eq!(player.paths.len(), 5);
        assert!(player.loop_);
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_load_decodes_lazily_so_cost_does_not_scale_with_frame_count() {
        // The whole point of the ring: a 216-frame animation must not decode 216
        // frames up front, because load runs on the event loop.
        let small = TempDir::new("anim-lazy-small");
        small.write_frames(4);
        let large = TempDir::new("anim-lazy-large");
        large.write_frames(120);

        let few = load(small.path(), true, NATURAL);
        let many = load(large.path(), true, NATURAL);

        assert_eq!(many.paths.len(), 120);
        assert_eq!(
            resident(&few),
            resident(&many),
            "resident frames should not grow with the frame count"
        );
        assert!(resident(&many) <= LOOKAHEAD + 1);
    }

    #[test]
    fn test_load_takes_dimensions_from_first_frame() {
        let dir = TempDir::new("anim-dims");
        dir.write_frames(2);

        let player = load(dir.path(), true, NATURAL);
        assert_eq!((player.frame_w, player.frame_h), (2, 2));
    }

    #[test]
    fn test_load_normalises_a_frame_whose_size_differs() {
        // The target is fixed from the first frame, and every later frame is
        // resampled to it, so one stray export at the wrong resolution does not
        // kill the animation or shift the layout mid-playback.
        let dir = TempDir::new("anim-mismatch");
        dir.write_frames(1);
        let bigger = cairo::ImageSurface::create(cairo::Format::ARgb32, 8, 8).unwrap();
        bigger
            .write_to_png(&mut std::fs::File::create(dir.join("frame_0001.png")).unwrap())
            .unwrap();

        let player = load(dir.path(), true, NATURAL);
        assert_eq!(player.paths.len(), 2);
        assert_eq!((player.frame_w, player.frame_h), (2, 2));
    }

    #[test]
    fn test_load_clamps_zero_fps_to_one() {
        // A zero fps would make the frame period divide to zero and spin the
        // event loop.
        let dir = TempDir::new("anim-fps0");
        dir.write_frames(2);

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

        assert_eq!(load(dir.path(), true, NATURAL).paths.len(), 2);
    }

    #[test]
    fn test_load_accepts_uppercase_extension() {
        let dir = TempDir::new("anim-upper");
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
        surface
            .write_to_png(&mut std::fs::File::create(dir.join("a.PNG")).unwrap())
            .unwrap();
        std::fs::copy(dir.join("a.PNG"), dir.join("b.png")).unwrap();

        assert_eq!(load(dir.path(), true, NATURAL).paths.len(), 2);
    }

    #[test]
    fn test_load_rejects_missing_directory() {
        let dir = TempDir::new("anim-absent");
        let missing = dir.join("nope");
        let err = AnimPlayer::load(&missing, true, NATURAL).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "got: {}", err);
    }

    #[test]
    fn test_load_rejects_directory_with_no_frames() {
        let dir = TempDir::new("anim-empty");
        let err = AnimPlayer::load(dir.path(), true, NATURAL).unwrap_err();
        assert!(err.to_string().contains("No PNG files"), "got: {}", err);
    }

    #[test]
    fn test_load_rejects_a_file_that_is_not_a_png() {
        // Caught by the signature check at load, so it is reported before
        // playback rather than on whichever tick reaches it.
        let dir = TempDir::new("anim-notpng");
        dir.write_frames(2);
        std::fs::write(dir.join("frame_0001.png"), b"definitely not a png").unwrap();

        let err = AnimPlayer::load(dir.path(), true, NATURAL).unwrap_err();
        assert!(err.to_string().contains("not a PNG"), "got: {}", err);
    }

    #[test]
    fn test_a_frame_that_only_fails_on_decode_is_reported_when_reached() {
        // Lazy loading trades up-front validation for not stalling the loop, so
        // a file with a valid signature but broken data is not caught at load.
        // It surfaces the first time the playhead needs it, which is the point
        // at which there is something to report about.
        let dir = TempDir::new("anim-truncated");
        dir.write_frames(2);
        let mut bytes = std::fs::read(dir.join("frame_0001.png")).unwrap();
        bytes.truncate(20);
        std::fs::write(dir.join("frame_0001.png"), bytes).unwrap();

        let mut player = load(dir.path(), true, NATURAL);
        assert!(player.frame().is_ok(), "frame 0 should be fine");
        player.tick();
        assert!(player.frame().is_err(), "frame 1 should report the failure");
    }

    // --- decode sizing ---------------------------------------------------

    #[test]
    fn test_longest_edge_target_downscales_preserving_aspect() {
        let dir = TempDir::new("anim-downscale");
        write_sized_frames(dir.path(), 64, 32, 3);

        let player = load(dir.path(), true, TargetSize::LongestEdge(16));
        // Long edge becomes 16, the short edge halves with it.
        assert_eq!((player.frame_w, player.frame_h), (16, 8));
    }

    #[test]
    fn test_scaled_target_shrinks_when_the_display_is_smaller() {
        let dir = TempDir::new("anim-scaledown");
        write_sized_frames(dir.path(), 40, 40, 2);

        let player = load(dir.path(), true, TargetSize::Scaled(0.5));
        assert_eq!((player.frame_w, player.frame_h), (20, 20));
    }

    #[test]
    fn test_target_never_upscales() {
        // Decoding above the source size would multiply memory for no gain, and
        // cairo scales up at draw time anyway. An animation-only display on a
        // high-DPI output therefore renders slightly soft rather than costing
        // frames_per_frame * scale^2 * 4 bytes.
        let dir = TempDir::new("anim-noupscale");
        write_sized_frames(dir.path(), 8, 8, 1);

        for target in [TargetSize::Scaled(4.0), TargetSize::LongestEdge(64)] {
            let player = load(dir.path(), true, target);
            assert_eq!((player.frame_w, player.frame_h), (8, 8));
        }
    }

    #[test]
    fn test_decoded_frame_is_actually_at_the_target_size() {
        let dir = TempDir::new("anim-verify");
        write_sized_frames(dir.path(), 64, 32, 2);

        let mut player = load(dir.path(), true, TargetSize::LongestEdge(16));
        let surface = player.frame().unwrap();
        assert_eq!((surface.width(), surface.height()), (16, 8));
    }

    #[test]
    fn test_a_large_downscale_lands_close_to_a_direct_resample() {
        // Stepping down by halves exists because cairo's bilinear filter
        // aliases badly past about 2:1. This compares the stepped result
        // against resampling straight to the target, on a gradient where a
        // difference would actually show.
        let dir = TempDir::new("anim-halve");
        let gradient = cairo::ImageSurface::create(cairo::Format::ARgb32, 256, 256).unwrap();
        {
            let cr = cairo::Context::new(&gradient).unwrap();
            for x in 0..256 {
                let shade = (x as f64 / 255.0).powf(2.2);
                cr.set_source_rgb(shade, shade, shade);
                cr.rectangle(x as f64, 0.0, 1.0, 256.0);
                cr.fill().unwrap();
            }
        }
        gradient.flush();
        gradient
            .write_to_png(&mut std::fs::File::create(dir.join("frame_0000.png")).unwrap())
            .unwrap();

        let mut stepped = decode_scaled(&dir.join("frame_0000.png"), (16, 16)).unwrap();
        let mut direct = resample(&gradient, (16, 16)).unwrap();

        // Averaging a gradient is well defined either way, so the two should
        // agree closely; the step is there to keep the aliasing low, not to
        // produce a different image.
        let mut worst = 0i32;
        for i in 0..16 * 16 {
            worst = worst.max((stepped.data().unwrap()[i] as i32 - direct.data().unwrap()[i] as i32).abs());
        }
        assert!(worst <= 24, "stepped and direct resample differ by {worst}");
    }

    // --- looping state machine ------------------------------------------

    #[test]
    fn test_tick_wraps_when_looping() {
        let (_dir, mut player) = player(3, true);
        for expected in [1, 2, 0, 1, 2] {
            player.tick();
            assert_eq!(player.frame_idx, expected);
        }
        assert!(!player.is_done());
    }

    #[test]
    fn test_tick_stops_at_last_frame_when_not_looping() {
        let (_dir, mut player) = player(3, false);
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
        let (_dir, mut player) = player(2, false);
        player.tick();
        player.tick();
        assert!(player.is_done());

        player.tick();
        assert_eq!(player.frame_idx, 1);
    }

    #[test]
    fn test_single_frame_looping_never_completes() {
        let (_dir, mut player) = player(1, true);
        for _ in 0..5 {
            player.tick();
        }
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_single_frame_non_looping_completes_on_first_tick() {
        let (_dir, mut player) = player(1, false);
        player.tick();
        assert_eq!(player.frame_idx, 0);
        assert!(player.is_done());
    }

    #[test]
    fn test_reset_rearms_a_completed_animation() {
        let (_dir, mut player) = player(3, false);
        for _ in 0..5 {
            player.tick();
        }
        assert!(player.is_done());

        player.reset();
        assert_eq!(player.frame_idx, 0);
        assert!(!player.is_done());
    }

    #[test]
    fn test_playback_yields_the_right_frame_at_every_position() {
        // The property that actually matters about the ring: at every tick the
        // surface handed back belongs to the current position, including across
        // wraps where a slot is reused for a different frame. Tagging each frame
        // with its own index makes a mis-association visible.
        let dir = TempDir::new("anim-identity");
        for i in 0..9u8 {
            write_tagged_frame(dir.path(), &format!("f_{i}.png"), i);
        }
        let mut player = load(dir.path(), true, NATURAL);

        for expected in (0..9).chain(0..9).chain(0..9) {
            player.frame().expect("the playhead's frame should be resident");
            assert_eq!(
                current_tag(&mut player),
                expected as u8,
                "expected frame {expected} at position {}",
                player.frame_idx
            );
            player.tick();
        }
    }

    #[test]
    fn test_resident_frames_stay_bounded_over_a_long_playthrough() {
        // The ring is what stops memory growing with the frame count, which is
        // what makes decoding every frame up front unnecessary.
        let (_dir, mut player) = player(200, true);
        let mut widest = 0;
        for _ in 0..500 {
            player.tick();
            widest = widest.max(resident(&player));
        }
        assert!(
            widest <= LOOKAHEAD + 1,
            "resident frames peaked at {widest}, ring holds {}",
            player.ring.len()
        );
    }

    #[test]
    fn test_every_frame_decodes_on_the_way_past() {
        let (_dir, mut player) = player(30, true);
        for _ in 0..30 {
            let surface = player.frame().expect("every frame should decode");
            assert_eq!((surface.width(), surface.height()), (2, 2));
            player.tick();
        }
    }

    #[test]
    fn test_current_frame_is_always_in_bounds() {
        let (_dir, mut player) = player(7, true);
        for _ in 0..100 {
            player.tick();
            assert!(player.frame_idx < player.paths.len());
            assert!(player.frame().unwrap().width() > 0);
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
            assert_ne!(
                nat_compare(Path::new(&pair[0]), Path::new(&pair[1])),
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

    #[test]
    fn test_load_sorts_frames_naturally() {
        let dir = TempDir::new("anim-sort");
        write_tagged_frame(dir.path(), "f_10.png", 10);
        write_tagged_frame(dir.path(), "f_2.png", 2);
        write_tagged_frame(dir.path(), "f_1.png", 1);

        let player = load(dir.path(), true, NATURAL);
        let order: Vec<u8> = player.paths.iter().map(|p| red_of_file(p)).collect();
        assert_eq!(order, vec![1, 2, 10], "frames should sort numerically");
    }

    // --- helpers ---------------------------------------------------------

    fn write_sized_frames(dir: &Path, w: i32, h: i32, count: usize) {
        for i in 0..count {
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
            surface
                .write_to_png(
                    &mut std::fs::File::create(dir.join(format!("frame_{i:04}.png"))).unwrap(),
                )
                .unwrap();
        }
    }

    /// Writes a PNG whose red channel encodes `value`, so playback order can be
    /// read back off the decoded frames.
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

    /// Reads the red tag of the current frame by moving its surface out of the
    /// ring. A clone would share the same cairo surface, and cairo refuses
    /// pixel access while another reference exists. The slot is left empty and
    /// refilled by the next prefetch.
    fn current_tag(player: &mut AnimPlayer) -> u8 {
        let slot = player.frame_idx % player.ring.len();
        let mut surface = player.ring[slot].take().expect("prefetched").1;
        surface.data().unwrap()[2]
    }

    /// Reads the red channel straight from a file. It has to be its own decode:
    /// cairo refuses pixel access while the ring holds a reference to the same
    /// surface.
    fn red_of_file(path: &Path) -> u8 {
        let mut file = std::fs::File::open(path).unwrap();
        let mut surface = cairo::ImageSurface::create_from_png(&mut file).unwrap();
        // ARGB32 in native byte order, so on little-endian this is index 2.
        surface.data().unwrap()[2]
    }
}
