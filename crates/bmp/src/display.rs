//! MOD2026: the 2x display layer (`docs/mod2026/PRD.md`, R5b). With Mod 2026 data the SDL
//! host presents a 640-pixel-wide indexed frame instead of the logical 320x240 one.
//!
//! The display buffer mirrors VRAM: every VRAM byte, i.e. one logical pixel of one plane,
//! owns a 2x2 block of display pixels. Keying the blocks by the same `(plane, offset)` as
//! the planes keeps pages, the page-2 cache, offset wrap-around, page copies and CRTC
//! scrolling identical for both buffers. [`crate::gfx::Gfx`] writes both on every VRAM
//! write; reads (hit tests, `get_pixel`, screenshots) stay on the logical planes.
//!
//! While the layer is on, a bitmap in memory gets a display side of one block per pixel as
//! soon as a block other than its doubled pixel is written into it, so a round trip through
//! memory (the pointer background, for one) keeps the blocks. Text draws its 2x glyphs
//! through such a side (`docs/mod2026/fonts/README.md`), and a mod crest picture gets its 2x
//! version as its side when it is loaded ([`hires_crest_side`]). A transparent blit is transparent
//! per display pixel: display pixels 0 of the source keep the destination's.

use bmp_formats::vga::VgaImage;

use crate::video::{self, MAX_HEIGHT, WIDTH};

/// Width of the display frame in pixels.
pub const DISPLAY_W: usize = 2 * WIDTH;

/// The display pixels of one logical pixel: top-left, top-right, bottom-left, bottom-right.
pub type Block = [u8; 4];

/// One [`Block`] per VRAM byte of the four 64 KiB planes.
pub struct Display {
    blocks: Vec<Block>,
}

fn index(plane: usize, off: u16) -> usize {
    (plane & 3) << 16 | off as usize
}

impl Display {
    /// A display holding the logical planes doubled.
    pub fn from_planes(planes: &[Vec<u8>; 4]) -> Self {
        Self {
            blocks: planes.iter().flatten().map(|&v| [v; 4]).collect(),
        }
    }

    pub fn block(&self, plane: usize, off: u16) -> Block {
        self.blocks[index(plane, off)]
    }

    pub fn put(&mut self, plane: usize, off: u16, block: Block) {
        self.blocks[index(plane, off)] = block;
    }

    /// Writes the non-zero display pixels of `block` (a transparent write).
    pub fn merge(&mut self, plane: usize, off: u16, block: Block) {
        merge(&mut self.blocks[index(plane, off)], block);
    }

    /// Applies a raster operation to every display pixel of a block.
    pub fn update(&mut self, plane: usize, off: u16, op: impl Fn(u8) -> u8) {
        for v in &mut self.blocks[index(plane, off)] {
            *v = op(*v);
        }
    }
}

/// Writes the non-zero display pixels of `src` into `dst`.
pub fn merge(dst: &mut Block, src: Block) {
    for (d, s) in dst.iter_mut().zip(src) {
        if s != 0 {
            *d = s;
        }
    }
}

/// The display side of the loaded picture `n` (pixels `img`): one block of its 2x version per
/// pixel, when `img` is the mod crest `n` ([`bmp_game::mod2026::hires_crest`]).
pub(crate) fn hires_crest_side(n: u16, img: &VgaImage) -> Option<Vec<Block>> {
    let hires = bmp_game::mod2026::hires_crest(n, img)?;
    let (w, h) = (usize::from(img.width), usize::from(img.height));
    let at = |x: usize, y: usize| hires.pixels[y * 2 * w + x];
    Some(
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                [
                    at(2 * x, 2 * y),
                    at(2 * x + 1, 2 * y),
                    at(2 * x, 2 * y + 1),
                    at(2 * x + 1, 2 * y + 1),
                ]
            })
            .collect(),
    )
}

/// A presented display frame: `height` rows of [`DISPLAY_W`] indexed pixels and the DAC.
#[derive(Clone)]
pub struct DisplayFrame {
    pub height: usize,
    pub pixels: Vec<u8>,
    pub dac: [[u8; 3]; 256],
}

impl Default for DisplayFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayFrame {
    pub fn new() -> Self {
        Self {
            height: 2 * MAX_HEIGHT,
            pixels: vec![0; DISPLAY_W * 2 * MAX_HEIGHT],
            dac: [[0; 3]; 256],
        }
    }

    /// Stores logical pixel `(x, y)` as its 2x2 block.
    pub fn set_block(&mut self, x: usize, y: usize, block: Block) {
        let top = 2 * y * DISPLAY_W + 2 * x;
        self.pixels[top..top + 2].copy_from_slice(&block[..2]);
        self.pixels[top + DISPLAY_W..top + DISPLAY_W + 2].copy_from_slice(&block[2..]);
    }

    /// Renders the visible rows to packed RGB24.
    pub fn to_rgb24(&self) -> Vec<u8> {
        video::rgb24(&self.pixels[..DISPLAY_W * self.height], &self.dac)
    }
}

/// Whether `frame` is the nearest-neighbour 2x scale of `vga`, palette included.
#[cfg(test)]
pub(crate) fn is_doubled(vga: &video::Vga, frame: &DisplayFrame) -> bool {
    frame.height == 2 * vga.height()
        && frame.dac == vga.dac
        && frame.pixels[..DISPLAY_W * frame.height]
            .chunks(DISPLAY_W)
            .enumerate()
            .all(|(y, row)| {
                let logical = &vga.pixels[(y / 2) * WIDTH..(y / 2 + 1) * WIDTH];
                row.iter().enumerate().all(|(x, &v)| v == logical[x / 2])
            })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::gfx::{Bitmap, Gfx, PAGE_SIZE, PAGE2_OFFSET, RastPort};
    use crate::host::{Host, Quit, parse_script};
    use crate::video::Vga;

    fn gfx_with_display() -> (Gfx, RastPort) {
        let mut g = Gfx::new();
        let rp = g.screen_open();
        g.display_on();
        (g, rp)
    }

    fn assert_doubled(g: &Gfx, what: &str) {
        let mut vga = Vga::new();
        let mut frame = DisplayFrame::new();
        g.to_vga(&mut vga);
        assert!(g.to_display(&mut frame), "display off");
        assert!(
            is_doubled(&vga, &frame),
            "{what}: display is not the doubled frame"
        );
    }

    /// The VRAM address of screen pixel `(x, y)` on page 0.
    fn at(x: u16, y: u16) -> (usize, u16) {
        (x as usize & 3, y * 80 + x / 4)
    }

    #[test]
    fn xor_twice_restores_the_display_buffer() {
        let (mut g, rp) = gfx_with_display();
        let s = rp.bitmap;
        g.fill_rect(s, 0, 0, 40, 30, 5);
        let (p, off) = at(10, 10);
        g.display_mut().unwrap().put(p, off, [1, 2, 3, 4]);
        let snapshot = g.display().unwrap().blocks.clone();
        let xor_all = |g: &mut Gfx| {
            g.fill_rect_xor(s, 5, 5, 20, 20);
            g.xor_pixel(10, 10, s);
            g.draw_line_xor(0, 10, 30, 10, s);
        };
        xor_all(&mut g);
        assert_ne!(g.display().unwrap().blocks, snapshot);
        assert_ne!(g.display().unwrap().block(p, off), [1, 2, 3, 4]);
        xor_all(&mut g);
        assert!(g.display().unwrap().blocks == snapshot);
    }

    #[test]
    fn get_pixel_reads_the_logical_frame() {
        let (mut g, rp) = gfx_with_display();
        g.put_pixel(10, 10, rp.bitmap, 7);
        let (p, off) = at(10, 10);
        g.display_mut().unwrap().put(p, off, [1, 2, 3, 4]);
        assert_eq!(g.get_pixel(10, 10, rp.bitmap), 7);
        let mut vga = Vga::new();
        g.to_vga(&mut vga);
        assert_eq!(vga.pixels[10 * WIDTH + 10], 7);
    }

    #[test]
    fn display_on_starts_from_the_current_planes() {
        let mut g = Gfx::new();
        let rp = g.screen_open();
        g.fill_rect(rp.bitmap, 3, 4, 50, 60, 9);
        g.display_on();
        assert_doubled(&g, "switched on after drawing");
    }

    #[test]
    fn primitives_draw_doubled() {
        let (mut g, mut rp) = gfx_with_display();
        let s = rp.bitmap;
        let sprite = g.alloc(Bitmap::new_mem(8, 8));
        for i in 0..64 {
            g.put_pixel(i % 8, i / 8, sprite, (i % 3) as u8 * 7);
        }
        g.blit_rect(sprite, 0, 0, s, 3, 3, 8, 8);
        g.blit_rect_transparent(sprite, 0, 0, s, 30, 7, 8, 8);
        g.blit_bitmap(s, 0, 0, s, 101, 50, 40, 20);
        assert_doubled(&g, "blits");
        g.fill_rect(s, 200, 100, 33, 17, 12);
        g.put_pixel(319, 239, s, 13);
        g.draw_line(0, 239, 319, 0, 14, s);
        Gfx::rastport_set_color(&mut rp, 15);
        g.draw_circle(&rp, 160, 120, 40);
        assert_doubled(&g, "fill, pixel, line, circle");
        g.vga_show_page(0);
        g.clear_bitmap(s, 3);
        g.fill_rect(s, 10, 10, 20, 20, 8);
        g.vga_show_page(0);
        assert_doubled(&g, "page 1 after the flip");
        g.vga_copy_page(PAGE_SIZE, 0);
        g.vga_show_page(1);
        assert_doubled(&g, "page copy");
        g.vga_copy_page(PAGE_SIZE, PAGE2_OFFSET);
        g.vga_restore_rect_from_page(320, 240, 0, 0, 100, 100, 3);
        assert_doubled(&g, "restore from page 2");
        assert!(g.mouse_open(true, true));
        g.mouse_pointer_show();
        g.mouse_event_handler(1, 100, 50);
        assert_doubled(&g, "pointer shown and moved");
        g.mouse_pointer_hide(0);
        assert_doubled(&g, "pointer hidden");
        g.vga_set_mode_x_teamed();
        g.fill_rect(g.pointer.screen, 0, 250, 320, 40, 6);
        g.vga_show_page_teamed(3);
        assert_doubled(&g, "TEAMED's scrolled view");
        g.video_screen_off();
        assert_doubled(&g, "blanked");
    }

    #[test]
    fn copies_carry_display_blocks() {
        let (mut g, rp) = gfx_with_display();
        let detail = [1, 2, 3, 4];
        let (p, off) = at(9, 5);
        g.display_mut().unwrap().put(p, off + PAGE_SIZE, detail);
        g.vga_copy_page(PAGE_SIZE, PAGE2_OFFSET);
        assert_eq!(g.display().unwrap().block(p, off + PAGE2_OFFSET), detail);
        g.vga_restore_rect_from_page(320, 240, 0, 0, 20, 20, 3);
        assert_eq!(g.display().unwrap().block(p, off), detail);
        g.blit_bitmap(rp.bitmap, 9, 5, rp.bitmap, 9, 50, 1, 1);
        assert_eq!(g.display().unwrap().block(p, 50 * 80 + 2), detail);
        g.blit_rect(rp.bitmap, 9, 5, rp.bitmap, 13, 60, 1, 1);
        assert_eq!(g.display().unwrap().block(p, 60 * 80 + 3), detail);
    }

    /// The `w x h` logical pixels of `vga` at `(x, y)`.
    fn logical_rect(vga: &Vga, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        (y..y + h)
            .flat_map(|row| vga.pixels[row * WIDTH + x..row * WIDTH + x + w].to_vec())
            .collect()
    }

    /// The `2w x 2h` display pixels of `frame` that logical pixel `(x, y)` starts.
    fn display_rect(frame: &DisplayFrame, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        (2 * y..2 * (y + h))
            .flat_map(|row| {
                frame.pixels[row * DISPLAY_W + 2 * x..row * DISPLAY_W + 2 * (x + w)].to_vec()
            })
            .collect()
    }

    /// The decoded PNG of picture `n` in `table` (`CRESTS` or `HIRES_CRESTS`).
    fn crest_png(table: &[(u8, &[u8])], n: u8) -> VgaImage {
        let &(_, png) = table.iter().find(|&&(m, _)| m == n).unwrap();
        bmp_game::mod2026::decode_crest(png).unwrap()
    }

    /// MOD2026: a mod crest blitted to the screen, directly and through a memory cache and
    /// a screen-to-screen copy, is its 2x version in the display and the crest in the
    /// logical frame.
    #[test]
    fn hires_crest_goes_to_the_display_and_the_crest_to_the_frame() {
        use bmp_game::mod2026::{CRESTS, HIRES_CRESTS};
        for &(n, png) in CRESTS {
            let low = bmp_game::mod2026::decode_crest(png).unwrap();
            let hires = crest_png(HIRES_CRESTS, n);
            let (w, h) = (low.width as i16, low.height as i16);
            let (mut g, rp) = gfx_with_display();
            let s = rp.bitmap;
            g.fill_rect(s, 0, 0, 320, 240, 3);
            let crest = g.alloc(Bitmap::from_image(&low));
            g.set_display_side(crest, hires_crest_side(n.into(), &low).unwrap());
            g.blit_bitmap(crest, 0, 0, s, 101, 35, w, h);
            let cache = g.alloc(Bitmap::new_mem(4 * 41, h));
            g.blit_bitmap(crest, 0, 0, cache, 2 * 41, 0, w, h);
            g.blit_rect(cache, 2 * 41, 0, s, 7, 150, w, h);
            g.blit_bitmap(s, 7, 150, s, 187, 60, w, h);
            let mut vga = Vga::new();
            let mut frame = DisplayFrame::new();
            g.to_vga(&mut vga);
            assert!(g.to_display(&mut frame));
            let (w, h) = (w as usize, h as usize);
            let places = [(101, 35), (7, 150), (187, 60)];
            for (x, y) in places {
                assert_eq!(
                    logical_rect(&vga, x, y, w, h),
                    low.pixels,
                    "crest {n} at ({x}, {y})"
                );
                assert_eq!(
                    display_rect(&frame, x, y, w, h),
                    hires.pixels,
                    "2x crest {n} at ({x}, {y})"
                );
            }
            let inside = |i: usize| {
                let (x, y) = (i % DISPLAY_W / 2, i / DISPLAY_W / 2);
                places
                    .iter()
                    .any(|&(px, py)| (px..px + w).contains(&x) && (py..py + h).contains(&y))
            };
            let stray = delta(&vga, &frame)
                .iter()
                .enumerate()
                .filter(|&(i, &(_, differs))| differs && !inside(i))
                .count();
            assert_eq!(stray, 0, "crest {n}: 2x detail outside the crests");
        }
        let mut g = Gfx::new();
        let rp = g.screen_open();
        let (n, png) = CRESTS[0];
        let low = bmp_game::mod2026::decode_crest(png).unwrap();
        let crest = g.alloc(Bitmap::from_image(&low));
        g.set_display_side(crest, hires_crest_side(n.into(), &low).unwrap());
        g.blit_bitmap(crest, 0, 0, rp.bitmap, 101, 35, 38, 38);
        assert!(
            !g.to_display(&mut DisplayFrame::new()),
            "display switched on"
        );
        let mut vga = Vga::new();
        g.to_vga(&mut vga);
        assert_eq!(logical_rect(&vga, 101, 35, 38, 38), low.pixels);
    }

    /// MOD2026: `Host::load_picture` gives the mod crest its 2x version only on mod data with
    /// the display layer as the SDL host switches it on. Needs `BMP_DATA`.
    #[test]
    fn load_picture_gives_mod_crests_their_2x_version() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let (n, png) = bmp_game::mod2026::CRESTS[0];
        let low = bmp_game::mod2026::decode_crest(png).unwrap();
        let hires = crest_png(bmp_game::mod2026::HIRES_CRESTS, n);
        let mod_data = temp_dir("load-picture-mod");
        bmp_game::mod2026::build(&data, &mod_data).unwrap();
        let original = temp_dir("load-picture-original");
        bmp_game::mod2026::copy_tree(&data, &original).unwrap();
        let shots = temp_dir("load-picture-shots");
        std::fs::create_dir_all(&shots).unwrap();
        // The logical and the display frame with crest `n` drawn at (101, 35).
        let draw = |dir: &Path, hires_fonts: bool| {
            let mut host = Host::with_script(dir, Vec::new(), &shots, false).unwrap();
            if hires_fonts {
                host.enable_display_layer();
            } else {
                host.g.display_on();
            }
            let crest = host.load_picture(n.into()).unwrap();
            host.g
                .blit_bitmap(crest, 0, 0, host.rp.bitmap, 101, 35, 38, 38);
            let mut vga = Vga::new();
            let mut frame = DisplayFrame::new();
            host.g.to_vga(&mut vga);
            assert!(host.g.to_display(&mut frame));
            (vga, frame)
        };
        let (vga, frame) = draw(&mod_data, true);
        assert_eq!(logical_rect(&vga, 101, 35, 38, 38), low.pixels);
        assert_eq!(
            display_rect(&frame, 101, 35, 38, 38),
            hires.pixels,
            "mod data, display layer of the SDL host"
        );
        let (vga, frame) = draw(&mod_data, false);
        assert_eq!(logical_rect(&vga, 101, 35, 38, 38), low.pixels);
        assert!(
            doubled_at(&vga, &frame, 101, 35, 38, 38),
            "mod data, display layer only"
        );
        let (vga, frame) = draw(&original, true);
        assert_ne!(
            logical_rect(&vga, 101, 35, 38, 38),
            low.pixels,
            "original crest {n} is the mod crest"
        );
        assert!(doubled_at(&vga, &frame, 101, 35, 38, 38), "original data");
        for dir in [&mod_data, &original, &shots] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// A fresh, not yet existing directory below the system temp directory, unique per call:
    /// tests running in parallel may run the same scenario with the same setup.
    fn temp_dir(tag: &str) -> PathBuf {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("bmp-display-{tag}-{}-{call}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The data and display layer a scenario runs with.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Setup {
        /// Mod 2026 data built from `BMP_DATA`, with the display layer but without the 2x
        /// fonts and crests.
        ModData,
        /// A copy of the original data, whose game is the frozen v1.0.0, with the display
        /// layer as the SDL host switches it on ([`Host::enable_display_layer`]).
        OriginalWithHiresFonts,
        /// Mod 2026 data built from `BMP_DATA`, with the display layer as the SDL host
        /// switches it on ([`Host::enable_display_layer`]), with the 2x fonts and crests:
        /// what a player of the mod sees.
        ModDataWithHiresFonts,
    }

    /// Runs `tests/visual/<name>.port.txt` like `tools/visual-check.sh` with the display
    /// layer on; returns the logical and the display frame of every screenshot.
    fn scenario_shots(data: &Path, name: &str, setup: Setup) -> Vec<(String, Vga, DisplayFrame)> {
        script_shots(data, name, &scenario_text(name), setup)
    }

    /// The text of `tests/visual/<name>.port.txt`.
    fn scenario_text(name: &str) -> String {
        let path = format!(
            "{}/../../tests/visual/{name}.port.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap()
    }

    /// MOD2026: the text of `tests/mod2026/matchday_2026.port.txt`, the match-day scenario
    /// on the game of the Mod 2026 setups.
    fn mod_matchday_text() -> String {
        let path = format!(
            "{}/../../tests/mod2026/matchday_2026.port.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap()
    }

    /// [`scenario_shots`] for the script `text`; `name` tells its temp directories apart.
    fn script_shots(
        data: &Path,
        name: &str,
        text: &str,
        setup: Setup,
    ) -> Vec<(String, Vga, DisplayFrame)> {
        let header = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("# {key}:")))
                .map(|v| v.split_whitespace().next().unwrap_or("").to_string())
        };
        let seed = |key: &str| header(key).map(|v| v.parse::<i32>().unwrap());
        let teamed = header("command").as_deref() == Some("teamed");
        let run_data = temp_dir(&format!("{name}-{setup:?}-data"));
        let shots = temp_dir(&format!("{name}-{setup:?}-shots"));
        match setup {
            Setup::ModData | Setup::ModDataWithHiresFonts => {
                bmp_game::mod2026::build(data, &run_data).unwrap();
                // The scenarios load 07072019.MAN, and an original save is refused on Mod
                // 2026 data (#218): they load a game the port wrote on this data instead.
                let mana = std::fs::read(run_data.join("MANA.DAT")).unwrap();
                let mana = bmp_formats::mana::ManaDat::parse(&mana).unwrap();
                std::fs::write(
                    run_data.join("07072019.MAN"),
                    crate::game::load_game::tests::mod_save_bytes(&mana),
                )
                .unwrap();
            }
            Setup::OriginalWithHiresFonts => bmp_game::mod2026::copy_tree(data, &run_data).unwrap(),
        }
        std::fs::create_dir_all(&shots).unwrap();
        let steps = parse_script(text).unwrap();
        let mut host = Host::with_script(&run_data, steps, &shots, !teamed).unwrap();
        match setup {
            Setup::ModData => host.g.display_on(),
            Setup::OriginalWithHiresFonts | Setup::ModDataWithHiresFonts => {
                host.enable_display_layer()
            }
        }
        if teamed {
            let _ = crate::teamed::run(&mut host).unwrap();
        } else if crate::intro::run(&mut host).unwrap() != Err(Quit) {
            let time = seed("seed").expect("run scenarios need a seed");
            let time2 = seed("seed2").or(Some(time));
            let _ = crate::game::run(&mut host, time, time2).unwrap();
        }
        let _ = std::fs::remove_dir_all(&run_data);
        let _ = std::fs::remove_dir_all(&shots);
        host.display_shots
    }

    /// [`scenario_shots`] on Mod 2026 data; whether each screenshot's display is the
    /// doubled logical frame.
    fn run_scenario(data: &Path, name: &str) -> Vec<(String, bool)> {
        scenario_shots(data, name, Setup::ModData)
            .into_iter()
            .map(|(shot, vga, frame)| (shot, is_doubled(&vga, &frame)))
            .collect()
    }

    /// Needs `BMP_DATA`.
    #[test]
    fn visual_scenarios_show_the_doubled_logical_frame() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        for name in [
            "start_menu",
            "office",
            "squad",
            "cups",
            "teamed_start",
            "teamed_uefa",
        ] {
            let checks = run_scenario(&data, name);
            assert!(!checks.is_empty(), "{name}: no screenshot checked");
            for (shot, doubled) in checks {
                assert!(doubled, "{name}/{shot}: display is not the doubled frame");
            }
        }
    }

    /// The display pixels of `frame` that differ from the doubled logical frame `vga`, as
    /// `(palette index, true)` per differing display pixel and `(0, false)` elsewhere.
    fn delta(vga: &Vga, frame: &DisplayFrame) -> Vec<(u8, bool)> {
        (0..DISPLAY_W * frame.height)
            .map(|i| {
                let (x, y) = (i % DISPLAY_W, i / DISPLAY_W);
                let v = frame.pixels[i];
                if v == vga.pixels[(y / 2) * WIDTH + x / 2] {
                    (0, false)
                } else {
                    (v, true)
                }
            })
            .collect()
    }

    fn write_delta(path: &Path, height: usize, delta: &[(u8, bool)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(file, DISPLAY_W as u32, height as u32);
        enc.set_color(png::ColorType::GrayscaleAlpha);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = delta
            .iter()
            .flat_map(|&(v, differs)| [v, if differs { 0xff } else { 0 }])
            .collect();
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
    }

    /// The golden delta: its width, height and [`delta`] pixels.
    fn read_delta(path: &Path) -> (u32, u32, Vec<(u8, bool)>) {
        let file = std::fs::File::open(path)
            .unwrap_or_else(|e| panic!("{}: {e}; see tests/display/README.md", path.display()));
        let mut reader = png::Decoder::new(file).read_info().unwrap();
        let mut data = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut data).unwrap();
        assert_eq!(
            (info.color_type, info.bit_depth),
            (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight)
        );
        let delta = data[..info.buffer_size()]
            .chunks(2)
            .map(|p| (p[0], p[1] != 0))
            .collect();
        (info.width, info.height, delta)
    }

    /// The logical and the display frame of screenshot `name` in `shots`.
    fn shot<'a>(
        shots: &'a [(String, Vga, DisplayFrame)],
        name: &str,
    ) -> (&'a Vga, &'a DisplayFrame) {
        shots
            .iter()
            .find(|(shot, ..)| shot == name)
            .map(|(_, vga, frame)| (vga, frame))
            .unwrap_or_else(|| panic!("no {name} shot"))
    }

    /// The newspaper of the match-day scenario `text` with `setup` matches
    /// `tests/display/<golden>`.
    fn check_newspaper_golden(data: &Path, text: &str, setup: Setup, golden: &str) {
        let shots = script_shots(data, "matchday", text, setup);
        let (vga, frame) = shot(&shots, "m08-newspaper");
        assert_display_golden(vga, frame, golden);
    }

    /// `frame` is exactly the doubled logical frame `vga` plus the 2x detail of
    /// `tests/display/<golden>` (`tests/display/README.md`). `BMP_DISPLAY_GOLDEN_WRITE=1`
    /// rewrites the golden first.
    fn assert_display_golden(vga: &Vga, frame: &DisplayFrame, golden: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/display")
            .join(golden);
        assert_eq!(frame.height, 2 * vga.height());
        assert!(frame.dac == vga.dac);
        let got = delta(vga, frame);
        assert!(
            got.iter().any(|&(_, differs)| differs),
            "{golden}: no 2x detail"
        );
        if std::env::var_os("BMP_DISPLAY_GOLDEN_WRITE").is_some() {
            write_delta(&path, frame.height, &got);
        }
        let (w, h, want) = read_delta(&path);
        assert_eq!((w as usize, h as usize), (DISPLAY_W, frame.height));
        let wrong: Vec<usize> = (0..got.len()).filter(|&i| got[i] != want[i]).collect();
        assert!(
            wrong.is_empty(),
            "{golden}: {} display pixels differ from the golden, first at {:?}",
            wrong.len(),
            wrong
                .iter()
                .take(5)
                .map(|&i| (i % DISPLAY_W, i / DISPLAY_W))
                .collect::<Vec<_>>()
        );
    }

    /// MOD2026: the newspaper with the 2x fonts on the original data, whose newspaper text
    /// is v1.0.0's, so that changes to the mod's game do not change this golden. Needs
    /// `BMP_DATA`.
    #[test]
    fn newspaper_display_matches_the_golden_frame() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        check_newspaper_golden(
            &data,
            &scenario_text("matchday"),
            Setup::OriginalWithHiresFonts,
            "matchday-m08-newspaper.png",
        );
    }

    /// MOD2026: the newspaper with the 2x fonts on Mod 2026 data, as a player of the mod
    /// sees it. Its text comes from the mod's game, so a change to the mod's league,
    /// clubs or squads that changes this newspaper needs a regenerated golden. Needs
    /// `BMP_DATA`.
    #[test]
    fn mod_newspaper_display_matches_the_golden_frame() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        check_newspaper_golden(
            &data,
            &mod_matchday_text(),
            Setup::ModDataWithHiresFonts,
            "matchday-m08-newspaper-mod.png",
        );
    }

    /// MOD2026: the office and the 2. Bundesliga table of the match day on Mod 2026 data with
    /// the 2x fonts and crests, as a player of the mod sees them; the office shows the 2x
    /// crest of the manager's club. Like the mod newspaper, a change to the mod's game that
    /// changes these screens needs regenerated goldens. Needs `BMP_DATA`.
    #[test]
    fn mod_office_and_table_display_match_the_golden_frames() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let shots = script_shots(
            &data,
            "matchday",
            &mod_matchday_text(),
            Setup::ModDataWithHiresFonts,
        );
        let (vga, frame) = shot(&shots, "m01-office");
        assert!(hires_crest_at(vga, frame, 187, 60).is_some(), "no 2x crest");
        assert_display_golden(vga, frame, "matchday-m01-office-mod.png");
        let (vga, frame) = shot(&shots, "m07-table");
        assert_display_golden(vga, frame, "matchday-m07-table-mod.png");
    }

    /// The mod crest whose pixels the logical frame `vga` shows at `(x, y)`, when `frame`
    /// shows its 2x version there.
    fn hires_crest_at(vga: &Vga, frame: &DisplayFrame, x: usize, y: usize) -> Option<u8> {
        bmp_game::mod2026::CRESTS.iter().find_map(|&(n, png)| {
            let low = bmp_game::mod2026::decode_crest(png).unwrap();
            let (w, h) = (usize::from(low.width), usize::from(low.height));
            (logical_rect(vga, x, y, w, h) == low.pixels
                && display_rect(frame, x, y, w, h)
                    == crest_png(bmp_game::mod2026::HIRES_CRESTS, n).pixels)
                .then_some(n)
        })
    }

    /// Whether `frame` shows the `w x h` logical pixels of `vga` at `(x, y)` doubled.
    fn doubled_at(vga: &Vga, frame: &DisplayFrame, x: usize, y: usize, w: usize, h: usize) -> bool {
        let logical = logical_rect(vga, x, y, w, h);
        let display = display_rect(frame, x, y, w, h);
        (0..4 * w * h).all(|i| display[i] == logical[(i / (2 * w) / 2) * w + i % (2 * w) / 2])
    }

    /// The origins of the crests of the start menu's carousel strip, `cells` of 0..6.
    fn strip(cells: &[usize]) -> Vec<(usize, usize)> {
        cells.iter().map(|&k| (18 + 41 * k, 48)).collect()
    }

    /// The origins of the 32 crest cells of the team editor's grid.
    fn grid() -> Vec<(usize, usize)> {
        (0..32).map(|i| (39 * (i % 8), 87 + 39 * (i / 8))).collect()
    }

    /// MOD2026: every crest site of `docs/mod2026/crest-sites.md` that the visual scenarios
    /// reach on Mod 2026 data shows the 2x crest in the display (row ids in the comments);
    /// the enlarged editor crest stays doubled. Shots whose pointer covers a crest leave that
    /// crest out. Needs `BMP_DATA`.
    #[test]
    fn hires_crests_reach_the_crest_screens() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let start_menu = scenario_text("start_menu");
        let start = &start_menu[..start_menu.find("shot r02-start").unwrap()];
        // The strip scrolled right (`press` on its right half), then left.
        let scrolling = format!(
            "{start}press 250 60\nwait 600\nrelease\nwait 300\nshot c2-right\n\
             press 60 60\nwait 300\nrelease\nwait 300\nshot c2-left\n"
        );
        type Sites = Vec<(&'static str, Vec<(usize, usize)>)>;
        let runs: Vec<(&str, String, Sites)> = vec![
            (
                "start_menu",
                start_menu,
                vec![
                    ("r02-start", strip(&[0, 1, 2, 3, 4, 5, 6])), // C1
                    ("r05-options", vec![(22, 114)]),             // C4
                ],
            ),
            (
                "scrolling",
                scrolling,
                vec![
                    ("c2-right", strip(&[0, 1, 2, 3, 4, 6])), // C2
                    ("c2-left", strip(&[0, 2, 3, 4, 5, 6])),  // C2
                ],
            ),
            (
                "new_game",
                scenario_text("new_game"),
                vec![("r03-office", vec![(187, 60)])], // X3, O1
            ),
            (
                "add_manager",
                scenario_text("add_manager"),
                vec![
                    ("r03-start-menu", vec![(22, 114)]),    // X4, C3
                    ("r06-office-second", vec![(187, 60)]), // X3, O1
                ],
            ),
            (
                "office",
                scenario_text("office"),
                vec![("r03-office", vec![(187, 60)])], // X4, O1
            ),
            (
                "matchday",
                mod_matchday_text(),
                vec![
                    // M1, M2 (a computer club: X5) and the event panel M11, M12.
                    ("m02-live", vec![(22, 35), (101, 35), (20, 118), (261, 118)]),
                    // Redrawn from the badge cache X5.
                    ("m05-live", vec![(22, 35), (101, 35)]),
                ],
            ),
            (
                "teamed_start",
                scenario_text("teamed_start"),
                vec![("t03-panel-cleared", grid()), ("t04-liga-page2", grid())], // T1, T2
            ),
            (
                "teamed_club",
                scenario_text("teamed_club"),
                vec![("c01-selected", vec![(2, 46), (115, 46), (159, 202)])], // T3, T4, T6
            ),
        ];
        for (name, text, sites) in &runs {
            let shots = script_shots(&data, name, text, Setup::ModDataWithHiresFonts);
            for (shot_name, origins) in sites {
                let (vga, frame) = shot(&shots, shot_name);
                for &(x, y) in origins {
                    assert!(
                        hires_crest_at(vga, frame, x, y).is_some(),
                        "{name}/{shot_name}: no 2x crest at ({x}, {y})"
                    );
                }
            }
            if *name == "teamed_club" {
                let (vga, frame) = shot(&shots, "c01-selected");
                assert!(doubled_at(vga, frame, 1, 88, 152, 152), "T5 not doubled");
            }
        }
    }

    /// MOD2026: the manager header (X1; X2 is its copy in the season end) copies the 2x
    /// crest from the crest cache. The scenarios reach no header on Mod 2026 data. Needs
    /// `BMP_DATA`.
    #[test]
    fn manager_header_shows_the_2x_crest() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let mod_data = temp_dir("header-mod");
        bmp_game::mod2026::build(&data, &mod_data).unwrap();
        let shots = temp_dir("header-shots");
        std::fs::create_dir_all(&shots).unwrap();
        let mut host = Host::with_script(&mod_data, Vec::new(), &shots, false).unwrap();
        host.enable_display_layer();
        let (n, _) = bmp_game::mod2026::CRESTS[5];
        let crest = host.load_picture(n.into()).unwrap();
        let cache = host.g.alloc(Bitmap::new_mem(4 * 41, 38));
        let portraits = host.g.alloc(Bitmap::new_mem(4 * 41, 40));
        host.g.blit_bitmap(crest, 0, 0, cache, 2 * 41, 0, 38, 38);
        crate::game::widgets::ui_draw_manager_header(&mut host, 2, b"ADI", portraits, cache);
        let mut vga = Vga::new();
        let mut frame = DisplayFrame::new();
        host.g.to_vga(&mut vga);
        assert!(host.g.to_display(&mut frame));
        assert_eq!(hires_crest_at(&vga, &frame, 267, 62), Some(n));
        for dir in [&mod_data, &shots] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
