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
//! through such a side (`docs/mod2026/fonts/README.md`). A transparent blit is transparent
//! per display pixel: display pixels 0 of the source keep the destination's.

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

    /// A fresh, not yet existing directory below the system temp directory.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bmp-display-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The data and display layer a scenario runs with.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Setup {
        /// Mod 2026 data built from `BMP_DATA`, with the display layer but without the 2x
        /// fonts.
        ModData,
        /// A copy of the original data, whose game is the frozen v1.0.0, with the display
        /// layer as the SDL host switches it on ([`Host::enable_display_layer`]).
        OriginalWithHiresFonts,
        /// Mod 2026 data built from `BMP_DATA`, with the display layer as the SDL host
        /// switches it on ([`Host::enable_display_layer`]): what a player of the mod sees.
        ModDataWithHiresFonts,
    }

    /// Runs `tests/visual/<name>.port.txt` like `tools/visual-check.sh` with the display
    /// layer on; returns the logical and the display frame of every screenshot.
    fn scenario_shots(data: &Path, name: &str, setup: Setup) -> Vec<(String, Vga, DisplayFrame)> {
        let path = format!(
            "{}/../../tests/visual/{name}.port.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let header = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("# {key}:")))
                .map(|v| v.split_whitespace().next().unwrap_or("").to_string())
        };
        let seed = |key: &str| header(key).map(|v| v.parse::<i32>().unwrap());
        let teamed = header("command").as_deref() == Some("teamed");
        // Tests running in parallel may run the same scenario with another setup.
        let run_data = temp_dir(&format!("{name}-{setup:?}-data"));
        let shots = temp_dir(&format!("{name}-{setup:?}-shots"));
        match setup {
            Setup::ModData | Setup::ModDataWithHiresFonts => {
                bmp_game::mod2026::build(data, &run_data).unwrap()
            }
            Setup::OriginalWithHiresFonts => bmp_game::mod2026::copy_tree(data, &run_data).unwrap(),
        }
        std::fs::create_dir_all(&shots).unwrap();
        let steps = parse_script(&text).unwrap();
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

    /// The newspaper of the match-day scenario with `setup` is exactly the doubled logical
    /// frame plus the 2x detail of `tests/display/<golden>` (`tests/display/README.md`).
    /// `BMP_DISPLAY_GOLDEN_WRITE=1` rewrites the golden first.
    fn check_newspaper_golden(data: &Path, setup: Setup, golden: &str) {
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/display")
            .join(golden);
        let (_, vga, frame) = scenario_shots(data, "matchday", setup)
            .into_iter()
            .find(|(shot, ..)| shot == "m08-newspaper")
            .expect("no m08-newspaper shot");
        assert_eq!(frame.height, 2 * vga.height());
        assert!(frame.dac == vga.dac);
        let got = delta(&vga, &frame);
        assert!(got.iter().any(|&(_, differs)| differs), "no 2x detail");
        if std::env::var_os("BMP_DISPLAY_GOLDEN_WRITE").is_some() {
            write_delta(&golden, frame.height, &got);
        }
        let (w, h, want) = read_delta(&golden);
        assert_eq!((w as usize, h as usize), (DISPLAY_W, frame.height));
        let wrong: Vec<usize> = (0..got.len()).filter(|&i| got[i] != want[i]).collect();
        assert!(
            wrong.is_empty(),
            "{} display pixels differ from the golden, first at {:?}",
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
            Setup::ModDataWithHiresFonts,
            "matchday-m08-newspaper-mod.png",
        );
    }
}
