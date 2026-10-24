//! Graphics and UI library of `BMMAIN.EXE`: a bit-exact model of the assembly Mode X
//! library (Ghidra segment `45ac`), its C wrapper layer (`4930`: rastport, screen,
//! palette, mouse) and the game's UI primitives (`16c7`: buttons, frames, text, hit test).
//!
//! The video memory is modelled as the real VGA hardware sees it in Mode X: four planes
//! of 64 KiB. Pixel `(x, y)` of a page at offset `base` lives in plane `x & 3` at byte
//! `base + y * 80 + x / 4`. Page 0 is at offset 0, page 1 at `0x4B00`, the office
//! background cache ("page 2") at `0x9600` and the three palette slots at `0xE100`.
//! [`Gfx::to_vga`] copies the page the CRTC shows into a [`Vga`] for display.
//!
//! The software mouse pointer is drawn into VRAM exactly like the original: the
//! background under it is saved in a bitmap, and every pointer-aware drawing routine
//! hides it while it draws. Bitmaps are addressed through [`BitmapId`] handles, which
//! play the role of the original far pointers to `{u16 w; u16 h; u8 far *data}`
//! descriptors; a descriptor whose data pointer has segment `A000` is a VRAM bitmap.
//!
//! See `docs/re/logic/gfxlib.md` for the full specification with Ghidra addresses.

#![allow(dead_code)]

use bmp_formats::font::{EURO, Font, FontSet, Glyph, map_char};
use bmp_formats::vga::VgaImage;

use crate::display::{self, Block, Display, DisplayFrame};
use crate::video::{Mode, Vga, WIDTH};

/// Screen width in pixels.
pub const SCREEN_W: i16 = 320;
/// Screen height in pixels.
pub const SCREEN_H: i16 = 240;
/// Height of TEAMED.EXE's screen descriptor (`16b7:028A`, 0x149): the visible 242 lines
/// plus off-screen rows below.
pub const TEAMED_SCREEN_H: i16 = 0x149;
/// Lines TEAMED.EXE's CRTC table displays.
pub const TEAMED_VISIBLE_LINES: i16 = 242;
/// CRTC start of TEAMED.EXE's scrolled view (`vga_show_page(3)`): row 87.
pub const TEAMED_SCROLLED_START: u16 = 0x1b30;
/// Bytes per row and plane in Mode X.
pub const ROW_BYTES: u16 = 80;
/// Size of one 320x240 page in each plane (`0x4B00`).
pub const PAGE_SIZE: u16 = 0x4B00;
/// Offset of the off-screen office background cache ("page 2", `A000:9600`).
pub const PAGE2_OFFSET: u16 = 0x9600;
/// Offset of the palette slots kept in off-screen VRAM (`A000:E100`).
pub const PALETTE_SLOT_OFFSET: u16 = 0xE100;
/// Value every XOR operation of the library combines with (`0x1F`).
pub const XOR_VALUE: u8 = 0x1F;

/// Pointer state value meaning "pointer is not in VRAM" (`CS:0010 == 2`).
pub const POINTER_HIDDEN: u8 = 2;
/// Pointer state value meaning "drawn, hide on first overlap and keep hidden" (`1`).
pub const POINTER_LAZY: u8 = 1;
/// Pointer state value meaning "drawn and kept visible" (`0`).
pub const POINTER_VISIBLE: u8 = 0;

/// Built-in 11x11 arrow pointer (`45ac:02a8`, descriptor at `45ac:0298`).
pub const DEFAULT_POINTER: [u8; 121] = [
    0x11, 0x13, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0x12, 0x11, 0x13, 0x13, 0, 0, 0, 0, 0, 0, 0, //
    0, 0x12, 0x11, 0x11, 0x13, 0x13, 0, 0, 0, 0, 0, //
    0, 0x12, 0x11, 0x11, 0x11, 0x11, 0x13, 0x13, 0, 0, 0, //
    0, 0, 0x12, 0x11, 0x11, 0x11, 0x11, 0x11, 0x13, 0x13, 0, //
    0, 0, 0x12, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0, //
    0, 0, 0, 0x12, 0x11, 0x11, 0x11, 0x13, 0, 0, 0, //
    0, 0, 0, 0x12, 0x11, 0x11, 0x12, 0x11, 0x13, 0, 0, //
    0, 0, 0, 0, 0x12, 0x11, 0, 0x12, 0x11, 0x13, 0, //
    0, 0, 0, 0, 0x12, 0x11, 0, 0, 0x12, 0x11, 0x13, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0x12, 0x11, //
];

/// Handle of a bitmap descriptor (the original passes far pointers).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitmapId(pub usize);

/// Where a bitmap's pixels live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Linear memory, `width * height` bytes, row-major.
    Mem(Vec<u8>),
    /// VRAM (data segment `A000`) at the given offset. Offset 0 is "the screen".
    Vram(u16),
}

/// A bitmap descriptor `{u16 w; u16 h; u8 far *data}`.
///
/// `TEAMED.EXE`'s wrapper layer (segment `1a46`) uses a 12-byte descriptor with an origin
/// `{+8 x, +0A y}` that every rastport and blit wrapper adds to the coordinates (the
/// team editor's crest grid is a view of the screen starting at row 75). `BMMAIN.EXE`'s
/// descriptors have no origin; it is `(0, 0)` for every bitmap the game creates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: i16,
    pub height: i16,
    pub storage: Storage,
    /// Origin added by the TEAMED wrappers (`+8`, `+0A`); always `(0, 0)` in BMMAIN.
    pub origin: (i16, i16),
    /// MOD2026: the display side of a memory bitmap, one block per pixel; only bitmaps
    /// that received a block other than their doubled pixel while the display layer is
    /// on have one.
    blocks: Option<Vec<Block>>,
}

impl Bitmap {
    /// A zero-filled linear bitmap.
    pub fn new_mem(width: i16, height: i16) -> Self {
        let len = (width.max(0) as usize) * (height.max(0) as usize);
        Self {
            width,
            height,
            storage: Storage::Mem(vec![0; len]),
            origin: (0, 0),
            blocks: None,
        }
    }

    /// A linear bitmap holding a decoded `.VGA` sprite.
    pub fn from_image(img: &VgaImage) -> Self {
        Self {
            width: img.width as i16,
            height: img.height as i16,
            storage: Storage::Mem(img.pixels.clone()),
            origin: (0, 0),
            blocks: None,
        }
    }

    /// A VRAM bitmap descriptor.
    pub fn vram(width: i16, height: i16, offset: u16) -> Self {
        Self {
            width,
            height,
            storage: Storage::Vram(offset),
            origin: (0, 0),
            blocks: None,
        }
    }

    fn vram_base(&self) -> Option<u16> {
        match self.storage {
            Storage::Vram(o) => Some(o),
            Storage::Mem(_) => None,
        }
    }

    /// True for the screen descriptor `A000:0000` (the test `4930:0991` makes).
    pub fn is_screen(&self) -> bool {
        self.storage == Storage::Vram(0)
    }
}

/// Rastport: drawing state bound to a bitmap (`4930:05a4`, 16 bytes).
///
/// Layout in the original: `+0` far ptr bitmap, `+4` fg pen, `+5` bg pen, `+6` draw
/// mode (0 = normal: solid fills, transparent text; 2 = XOR; any other non-zero value
/// draws text opaquely), `+8`/`+0A` pen x/y, `+0C` far ptr to a font handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RastPort {
    pub bitmap: BitmapId,
    pub fg: u8,
    pub bg: u8,
    pub mode: u8,
    pub pen_x: i16,
    pub pen_y: i16,
    /// Library font index from the font handle (`None` = no font / handle -1).
    pub font: Option<u16>,
}

impl RastPort {
    /// Defaults: fg 0xFF, bg 0, mode 1, pen (0,0), no font.
    ///
    /// Origin: BMMAIN.EXE 4930:05a4 (rastport_init)
    pub fn new(bitmap: BitmapId) -> Self {
        Self {
            bitmap,
            fg: 0xff,
            bg: 0,
            mode: 1,
            pen_x: 0,
            pen_y: 0,
            font: None,
        }
    }
}

/// Font handle filled by `font_find_by_name` (6 bytes: index, height, descent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontHandle {
    pub index: u16,
    pub height: u16,
    pub descent: u16,
}

/// Names in the library font table `45ac:03fe` (index order).
pub const FONT_NAMES: [&str; 4] = ["normal.font", "micro4.font", "flow.font", "newbold.font"];

/// Software mouse pointer state (library CS variables).
#[derive(Clone, Debug)]
pub struct Pointer {
    /// `CS:0008`/`CS:000A`: pointer position.
    pub x: i16,
    pub y: i16,
    /// `CS:000C`/`CS:000E`: offsets added to the position to get the sprite origin.
    pub hot_x: i16,
    pub hot_y: i16,
    /// `CS:0010`: [`POINTER_VISIBLE`], [`POINTER_LAZY`] or [`POINTER_HIDDEN`].
    pub state: u8,
    /// `CS:0011`: hide nesting counter.
    pub hide_count: i16,
    /// `CS:0023..0026`: latched events left down, left up, right down, right up.
    pub events: [bool; 4],
    /// `CS:0027`: pointer sprite bitmap (0 = transparent).
    pub image: BitmapId,
    /// `CS:002B`: buffer holding the screen under the pointer.
    pub background: BitmapId,
    /// `CS:002F`: the screen bitmap the pointer is drawn to.
    pub screen: BitmapId,
    /// `CS:028E`: 1 when the driver only reports 0..319 (no x halving).
    pub native_320: bool,
    /// Copies of the position the library writes for the game: mouse struct
    /// `+2` (x) / `+0` (y) and screen struct `+6` (x) / `+4` (y).
    pub client_x: i16,
    pub client_y: i16,
}

/// The library state plus the emulated VGA.
pub struct Gfx {
    /// Four VGA planes of 64 KiB. Private so that every write goes through the
    /// [`Gfx::vram_write`] family, which keeps [`Gfx::display`] in step.
    planes: [Vec<u8>; 4],
    /// MOD2026: the 2x display buffer, `None` when the layer is off.
    display: Option<Display>,
    /// CRTC start address (page shown).
    pub crtc_start: u16,
    /// `CS:0033`: non-zero = draw to page 1 (`+0x4B00`).
    pub draw_page: u8,
    /// `CS:0036`: library busy flag. While set, the mouse event handler skips the x
    /// halving and the pointer move, but still latches button events.
    pub busy: bool,
    /// Video refresh disabled (`video_screen_off`).
    pub blanked: bool,
    /// True after `vga_set_mode_x_320x240`, false after `vga_set_text_mode`.
    pub mode_x: bool,
    /// Lines the CRTC displays: 240 (BMMAIN's register table), 242 in TEAMED.EXE.
    pub visible_lines: i16,
    /// Vertical range of the INT 33h driver (function 8) as TEAMED's `vga_show_page`
    /// sets it; `None` (BMMAIN) leaves the pointer coordinates as the host gives them.
    pub mouse_y_range: Option<(i16, i16)>,
    /// Mask of the start row in the VRAM address computations: BMMAIN's library
    /// multiplies the row byte by 80 (`MUL BL`, rows wrap at 256), TEAMED's the whole
    /// word (`MUL BX`, e.g. `16b7:1f36`), which its 329-row screen needs.
    pub row_mask: u16,
    /// VGA DAC, 6 bits per channel.
    pub dac: [[u8; 3]; 256],
    /// Screen palette arrays (`screen + 8`): R at 0..256, G at 0x100.., B at 0x200..
    pub palette: [u8; 0x300],
    pub pointer: Pointer,
    /// Fonts by library index (index 0, `normal.font`, has no glyph data).
    pub fonts: [Option<Font>; 4],
    /// MOD2026: the euro glyph of each font, installed only with the mod flag
    /// ([`Gfx::set_euro_glyphs`]); `None` everywhere is v1.0.0.
    pub euro: [Option<Glyph>; 4],
    /// MOD2026: the 2x glyphs of each font for the display layer by glyph index, `None`
    /// for a glyph whose cell is not exactly twice the installed one
    /// ([`Gfx::set_hires_fonts`]); empty everywhere is v1.0.0.
    hires: [Vec<Option<Glyph>>; 4],
    /// BIOS tick counter `0040:006C` (advanced by the host at 18.2 Hz).
    pub bios_ticks: u32,
    /// `CS:0038/003A`: timer deadline.
    pub deadline: u32,
    bitmaps: Vec<Option<Bitmap>>,
}

impl Default for Gfx {
    fn default() -> Self {
        Self::new()
    }
}

/// Font height, descent and per-character `(width, pixels, 2x display blocks)`
/// expansions; the blocks only with the display layer on and a 2x glyph.
type ExpandedText = (u16, u16, Vec<(u8, Vec<u8>, Option<Vec<Block>>)>);

/// MOD2026: the 2x glyph `big` of a `w x h` glyph as one display block per 1x pixel,
/// row by row, set pixels in `fg` and clear ones in `bg`.
fn hires_blocks(big: &Glyph, w: usize, h: usize, fg: u8, bg: u8) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(w * h);
    for row in 0..h {
        for col in 0..w {
            blocks.push(std::array::from_fn(|k| {
                if big.pixel(2 * col + k % 2, 2 * row + k / 2) {
                    fg
                } else {
                    bg
                }
            }));
        }
    }
    blocks
}

/// Result of clipping a blit: `(sx, sy, dx, dy, w, h)`.
type Clip = (i16, i16, i16, i16, i16, i16);

/// Clipping shared by `blit_rect` and `blit_rect_transparent` (signed 16-bit).
#[allow(clippy::too_many_arguments)]
fn clip_blit(
    src_w: i16,
    src_h: i16,
    mut sx: i16,
    mut sy: i16,
    dst_w: i16,
    dst_h: i16,
    mut dx: i16,
    mut dy: i16,
    mut w: i16,
    mut h: i16,
) -> Option<Clip> {
    if sx < 0 {
        w = w.wrapping_add(sx);
        dx = dx.wrapping_sub(sx);
        sx = 0;
        if w <= 0 {
            return None;
        }
    }
    if sy < 0 {
        h = h.wrapping_add(sy);
        dy = dy.wrapping_sub(sy);
        sy = 0;
        if h <= 0 {
            return None;
        }
    }
    if dx < 0 {
        w = w.wrapping_add(dx);
        sx = sx.wrapping_sub(dx);
        dx = 0;
        if w <= 0 {
            return None;
        }
    }
    if dy < 0 {
        h = h.wrapping_add(dy);
        sy = sy.wrapping_sub(dy);
        dy = 0;
        if h <= 0 {
            return None;
        }
    }
    if sx >= src_w || sy >= src_h || dx >= dst_w || dy >= dst_h {
        return None;
    }
    let e = dx.wrapping_add(w);
    if dst_w < e {
        w = w.wrapping_add(dst_w.wrapping_sub(e));
    }
    let e = dy.wrapping_add(h);
    if dst_h < e {
        h = h.wrapping_add(dst_h.wrapping_sub(e));
    }
    let e = sx.wrapping_add(w);
    if src_w < e {
        w = w.wrapping_add(src_w.wrapping_sub(e));
    }
    let e = sy.wrapping_add(h);
    if src_h < e {
        h = h.wrapping_add(src_h.wrapping_sub(e));
    }
    // Deviation (safety guard): 45ac:1d1c has no final check. A caller passing
    // `w <= 0` or `h <= 0` with in-range coordinates would reach the copy loops,
    // whose `DEC DX / JNZ` row loop then runs 65536 rows (h == 0). No such call is
    // known; the port draws nothing instead.
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((sx, sy, dx, dy, w, h))
}

/// Clipping of `fill_rect`, `fill_rect_xor` and `vga_restore_rect_from_page`:
/// returns `(x, y, w, h)`.
fn clip_fill(bw: i16, bh: i16, mut x: i16, mut y: i16, mut w: i16, mut h: i16) -> Option<Clip> {
    if x < 0 {
        w = w.wrapping_add(x);
        x = 0;
    }
    let e = x.wrapping_add(w);
    if e > bw {
        w = w.wrapping_sub(e.wrapping_sub(bw));
    }
    if w <= 0 {
        return None;
    }
    if y < 0 {
        h = h.wrapping_add(y);
        y = 0;
    }
    let e = y.wrapping_add(h);
    if e > bh {
        h = h.wrapping_sub(e.wrapping_sub(bh));
    }
    if h <= 0 {
        return None;
    }
    Some((x, y, 0, 0, w, h))
}

/// VRAM byte offset of pixel `(x, y)` of the page at `base` (no wrap of y).
fn page_offset(base: u16, x: i16, y: i16) -> u16 {
    base.wrapping_add((y as u16).wrapping_mul(ROW_BYTES))
        .wrapping_add(x as u16 >> 2)
}

/// VRAM byte offset of row `r` of a rect starting at `y0` and column `x` (as the
/// planar loops compute it: `(y0 & 0xFF) * 80` by an 8-bit `MUL`, then `+80` per row).
fn vram_off(base: u16, y0: i16, r: i16, x: i16, row_mask: u16) -> u16 {
    base.wrapping_add((y0 as u16 & row_mask).wrapping_mul(ROW_BYTES))
        .wrapping_add((r as u16).wrapping_mul(ROW_BYTES))
        .wrapping_add(x as u16 >> 2)
}

impl Gfx {
    /// Library state after loading: pointer hidden (`CS:0010 = 2`), the built-in
    /// arrow, all VRAM zero, the screen descriptor `CS:0286` (320x240 at `A000:0000`).
    pub fn new() -> Self {
        let mut g = Self {
            planes: [
                vec![0; 0x10000],
                vec![0; 0x10000],
                vec![0; 0x10000],
                vec![0; 0x10000],
            ],
            display: None,
            crtc_start: 0,
            draw_page: 0,
            busy: false,
            blanked: false,
            mode_x: false,
            visible_lines: SCREEN_H,
            mouse_y_range: None,
            row_mask: 0xff,
            dac: [[0; 3]; 256],
            palette: [0; 0x300],
            pointer: Pointer {
                x: 0,
                y: 0,
                hot_x: 0,
                hot_y: 0,
                state: POINTER_HIDDEN,
                hide_count: 0,
                events: [false; 4],
                image: BitmapId(0),
                background: BitmapId(0),
                screen: BitmapId(0),
                native_320: false,
                client_x: 0,
                client_y: 0,
            },
            fonts: [None, None, None, None],
            euro: [None, None, None, None],
            hires: Default::default(),
            bios_ticks: 0,
            deadline: 0,
            bitmaps: Vec::new(),
        };
        let screen = g.alloc(Bitmap::vram(SCREEN_W, SCREEN_H, 0));
        let image = g.alloc(Bitmap {
            width: 11,
            height: 11,
            storage: Storage::Mem(DEFAULT_POINTER.to_vec()),
            origin: (0, 0),
            blocks: None,
        });
        let bg = g.alloc(Bitmap::new_mem(11, 11));
        g.pointer.screen = screen;
        g.pointer.image = image;
        g.pointer.background = bg;
        g
    }

    /// Installs the three glyph fonts at their library indices 1..3.
    pub fn set_fonts(&mut self, set: &FontSet) {
        use bmp_formats::font::FontId;
        for id in FontId::ALL {
            self.fonts[id as usize] = Some(set.get(id).clone());
        }
    }

    /// MOD2026: gives the three glyph fonts the mod's euro glyph for the byte code [`EURO`],
    /// which v1.0.0 draws as a space (`docs/mod2026/CHANGES.md`).
    pub fn set_euro_glyphs(&mut self) {
        use bmp_formats::font::{FontId, euro_glyph};
        for id in FontId::ALL {
            self.euro[id as usize] = Some(euro_glyph(id));
        }
    }

    /// MOD2026: gives the installed glyph fonts their 2x glyphs for the display layer
    /// (`docs/mod2026/fonts/README.md`). A glyph of another `BMMAIN.EXE` whose cell does
    /// not match its 2x glyph keeps being drawn doubled.
    pub fn set_hires_fonts(&mut self) {
        use bmp_formats::font::FontId;
        for id in FontId::ALL {
            let Some(font) = &self.fonts[id as usize] else {
                continue;
            };
            let h = (font.height & 0xff) as usize;
            let glyphs = bmp_formats::hires_font::glyphs(id)
                .into_iter()
                .zip(&font.glyphs)
                .map(|(big, g)| {
                    let fits = big.width as usize == 2 * g.width as usize
                        && big.bits.len() == 2 * h * big.row_bytes();
                    fits.then_some(big)
                })
                .collect();
            self.hires[id as usize] = glyphs;
        }
    }

    /// The font slot byte `c` in font `font` draws: [`map_char`]'s, or `None` for
    /// [`EURO`] when the mod installed a euro glyph.
    fn glyph_slot(&self, font: u16, c: u8) -> Option<usize> {
        // MOD2026: the euro code has its own glyph only with the mod flag.
        match self.euro.get(font as usize) {
            Some(Some(_)) if c == EURO => None,
            _ => Some(map_char(c) as usize),
        }
    }

    /// MOD2026: the 2x glyph of byte `c` in font `font`, if it has one. The mod's euro
    /// glyph has none yet and is drawn doubled.
    fn hires_glyph(&self, font: u16, c: u8) -> Option<&Glyph> {
        self.hires
            .get(font as usize)?
            .get(self.glyph_slot(font, c)?)?
            .as_ref()
    }

    /// The glyph of byte `c` in font `font` (see [`Gfx::glyph_slot`]).
    fn glyph<'a>(&'a self, font: u16, f: &'a Font, c: u8) -> &'a Glyph {
        match self.glyph_slot(font, c) {
            Some(i) => &f.glyphs[i],
            None => self.euro[font as usize]
                .as_ref()
                .expect("installed euro glyph"),
        }
    }

    // ----- bitmap arena -------------------------------------------------------------

    /// Registers a bitmap descriptor and returns its handle.
    pub fn alloc(&mut self, bm: Bitmap) -> BitmapId {
        if let Some(i) = self.bitmaps.iter().position(Option::is_none) {
            self.bitmaps[i] = Some(bm);
            BitmapId(i)
        } else {
            self.bitmaps.push(Some(bm));
            BitmapId(self.bitmaps.len() - 1)
        }
    }

    /// Releases a bitmap (`bitmap_free` 176b:0dca).
    pub fn free(&mut self, id: BitmapId) {
        if let Some(slot) = self.bitmaps.get_mut(id.0) {
            *slot = None;
        }
    }

    pub fn bitmap(&self, id: BitmapId) -> &Bitmap {
        self.bitmaps[id.0].as_ref().expect("bitmap freed")
    }

    pub fn bitmap_mut(&mut self, id: BitmapId) -> &mut Bitmap {
        self.bitmaps[id.0].as_mut().expect("bitmap freed")
    }

    fn dims(&self, id: BitmapId) -> (i16, i16, Option<u16>) {
        let b = self.bitmap(id);
        (b.width, b.height, b.vram_base())
    }

    fn page_add(&self) -> u16 {
        if self.draw_page != 0 { PAGE_SIZE } else { 0 }
    }

    fn mem_get(&self, id: BitmapId, idx: usize) -> u8 {
        match &self.bitmap(id).storage {
            Storage::Mem(v) => v.get(idx).copied().unwrap_or(0),
            Storage::Vram(_) => 0,
        }
    }

    /// MOD2026: the display block of pixel `idx` of a memory bitmap, `None` without a
    /// display side.
    fn mem_block(&self, id: BitmapId, idx: usize) -> Option<Block> {
        self.bitmap(id).blocks.as_ref()?.get(idx).copied()
    }

    /// The display side of a memory bitmap, created from its pixels doubled.
    fn mem_side(&mut self, id: BitmapId) -> &mut Vec<Block> {
        let bm = self.bitmap_mut(id);
        let pixels = match &bm.storage {
            Storage::Mem(v) => v,
            Storage::Vram(_) => &Vec::new(),
        };
        bm.blocks
            .get_or_insert_with(|| pixels.iter().map(|&v| [v; 4]).collect())
    }

    /// Writes pixel `idx` of a memory bitmap with its display block (MOD2026: the block
    /// goes to the display side).
    fn mem_put(&mut self, id: BitmapId, idx: usize, v: u8, block: Block) {
        let Storage::Mem(px) = &mut self.bitmap_mut(id).storage else {
            return;
        };
        let Some(p) = px.get_mut(idx) else {
            return;
        };
        *p = v;
        let has_side = self.mem_block(id, idx).is_some();
        if self.display.is_some() && (has_side || block != [v; 4]) {
            self.mem_side(id)[idx] = block;
        }
    }

    /// Read-modify-write of pixel `idx` of a memory bitmap; a display side gets `op` on
    /// each of its pixels, like [`Display::update`].
    fn mem_update(&mut self, id: BitmapId, idx: usize, op: impl Fn(u8) -> u8) {
        let bm = self.bitmap_mut(id);
        if let Storage::Mem(px) = &mut bm.storage
            && let Some(p) = px.get_mut(idx)
        {
            *p = op(*p);
            if let Some(side) = &mut bm.blocks {
                side[idx] = side[idx].map(&op);
            }
        }
    }

    /// Reads plane `p` at `off`.
    pub fn vram_read(&self, plane: usize, off: u16) -> u8 {
        self.planes[plane & 3][off as usize]
    }

    /// Writes plane `p` at `off`; the display gets the value doubled.
    pub fn vram_write(&mut self, plane: usize, off: u16, v: u8) {
        self.vram_put(plane, off, v, [v; 4]);
    }

    fn vram_put(&mut self, plane: usize, off: u16, v: u8, block: Block) {
        self.planes[plane & 3][off as usize] = v;
        // MOD2026: the display layer mirrors every VRAM write.
        if let Some(display) = &mut self.display {
            display.put(plane, off, block);
        }
    }

    /// Read-modify-write of plane `p` at `off`; the display applies `op` to each of its
    /// pixels, so an XOR applied twice restores it exactly.
    fn vram_update(&mut self, plane: usize, off: u16, op: impl Fn(u8) -> u8) {
        let p = &mut self.planes[plane & 3][off as usize];
        *p = op(*p);
        if let Some(display) = &mut self.display {
            display.update(plane, off, op);
        }
    }

    /// The display block of plane `p` at `off`, `None` when the layer is off.
    fn display_block(&self, plane: usize, off: u16) -> Option<Block> {
        self.display.as_ref().map(|d| d.block(plane, off))
    }

    /// Copies one VRAM byte together with its display block.
    fn vram_copy(&mut self, src_plane: usize, src_off: u16, dst_plane: usize, dst_off: u16) {
        let v = self.vram_read(src_plane, src_off);
        let block = self.display_block(src_plane, src_off).unwrap_or([v; 4]);
        self.vram_put(dst_plane, dst_off, v, block);
    }

    /// Pixel `(x, y)` of the page at `base` (row-linear Mode X addressing, no wrap of y).
    pub fn page_pixel(&self, base: u16, x: i16, y: i16) -> u8 {
        self.vram_read((x & 3) as usize, page_offset(base, x, y))
    }

    // ----- mode, pages, display -----------------------------------------------------

    /// INT10 mode 13h, unchain (SEQ 4 = 06), clear `0x9600` bytes of all planes from the
    /// screen descriptor's offset (pages 0 and 1, page 2 survives), misc output `0xE3`,
    /// 17 CRTC registers from `45ac:039a` (320x240, 80 bytes per row). Points the
    /// pointer's screen bitmap at the library descriptor `CS:0286`.
    ///
    /// Origin: BMMAIN.EXE 45ac:1c80 (vga_set_mode_x_320x240)
    pub fn vga_set_mode_x_320x240(&mut self) {
        let screen = self.pointer.screen;
        *self.bitmap_mut(screen) = Bitmap::vram(SCREEN_W, SCREEN_H, 0);
        for p in 0..4 {
            for i in 0..2 * PAGE_SIZE {
                self.vram_write(p, i, 0);
            }
        }
        self.crtc_start = 0;
        self.mode_x = true;
        self.visible_lines = SCREEN_H;
    }

    /// TEAMED.EXE's copy of [`Gfx::vga_set_mode_x_320x240`]: the same code, but the
    /// library's screen descriptor (`CS:028A`) is 320x329 and the CRTC table at
    /// `CS:039E` sets the vertical display end (register 0x12) to `0xE3` instead of
    /// `0xDF`, so 242 lines are visible.
    ///
    /// Origin: TEAMED.EXE 16b7:1ca0 (vga_set_mode_x_320x240)
    pub fn vga_set_mode_x_teamed(&mut self) {
        self.vga_set_mode_x_320x240();
        let screen = self.pointer.screen;
        self.bitmap_mut(screen).height = TEAMED_SCREEN_H;
        self.visible_lines = TEAMED_VISIBLE_LINES;
        self.row_mask = 0xffff;
    }

    /// TEAMED.EXE's `vga_show_page`: modes 0 and 2 are BMMAIN's; mode 1 shows and draws
    /// page 0 and sets the driver's y range to 0..241; the extra mode 3 shows the screen
    /// from row 87 (CRTC start `0x1B30`, the scrolled UEFA list), draws page 0 and sets
    /// the y range to 87..328. The driver clamps the pointer into the new range without
    /// calling the event handler, so the library's client position is not updated.
    ///
    /// Origin: TEAMED.EXE 16b7:3231 (vga_show_page)
    pub fn vga_show_page_teamed(&mut self, n: u16) {
        match n {
            1 => {
                self.draw_page = 0;
                self.crtc_start = 0;
                self.mouse_y_range = Some((0, 0xf1));
            }
            3 => {
                self.draw_page = 0;
                self.crtc_start = TEAMED_SCROLLED_START;
                self.mouse_y_range = Some((0x57, 0x148));
            }
            _ => self.vga_show_page(n),
        }
    }

    /// A driver y coordinate (script, DOSBox) clamped to the range set by
    /// [`Gfx::vga_show_page_teamed`].
    pub fn mouse_clamp_y(&self, y: i16) -> i16 {
        match self.mouse_y_range {
            Some((lo, hi)) => y.clamp(lo, hi),
            None => y,
        }
    }

    /// The driver y coordinate for a window row: on real hardware the pointer moves
    /// relative and stays in the driver's range, which starts at the first row shown
    /// (0 or 87), so a window row maps to `row + range start`.
    pub fn mouse_window_y(&self, row: i16) -> i16 {
        match self.mouse_y_range {
            Some((lo, hi)) => row.wrapping_add(lo).clamp(lo, hi),
            None => row,
        }
    }

    /// Back to text mode via INT10.
    ///
    /// Origin: BMMAIN.EXE 45ac:1d08 (vga_set_text_mode)
    pub fn vga_set_text_mode(&mut self) {
        self.mode_x = false;
    }

    /// `n == 1`: show and draw page 0. `n == 2`: show and draw page 1. `n == 0`: flip:
    /// if drawing page 0, draw page 1 and show page 0, else draw page 0 and show page 1.
    /// Other values do nothing. No retrace wait.
    ///
    /// Origin: BMMAIN.EXE 45ac:327d (vga_show_page)
    pub fn vga_show_page(&mut self, n: u16) {
        match n {
            1 => {
                self.draw_page = 0;
                self.crtc_start = 0;
            }
            2 => {
                self.draw_page = 1;
                self.crtc_start = PAGE_SIZE;
            }
            0 => {
                if self.draw_page == 0 {
                    self.draw_page = 1;
                    self.crtc_start = 0;
                } else {
                    self.draw_page = 0;
                    self.crtc_start = PAGE_SIZE;
                }
            }
            _ => {}
        }
    }

    /// `CS:0033 = (n - 1) as u8`; any non-zero result selects page 1 (so `n == 0` too).
    ///
    /// Origin: BMMAIN.EXE 45ac:32fe (vga_set_draw_page)
    pub fn vga_set_draw_page(&mut self, n: u16) {
        self.draw_page = (n as u8).wrapping_sub(1);
    }

    /// Latched copy of `0x4B00` bytes in all four planes from `A000:src` to `A000:dst`,
    /// forward, byte by byte (offsets wrap at 64 KiB). Not pointer-aware: a visible
    /// pointer is copied along.
    ///
    /// Origin: BMMAIN.EXE 45ac:332b (vga_copy_page)
    pub fn vga_copy_page(&mut self, src: u16, dst: u16) {
        for i in 0..PAGE_SIZE {
            for p in 0..4 {
                self.vram_copy(p, src.wrapping_add(i), p, dst.wrapping_add(i));
            }
        }
    }

    /// Video refresh off (INT10/12 BL=36 AL=1).
    ///
    /// Origin: BMMAIN.EXE 45ac:2f93 (video_screen_off)
    pub fn video_screen_off(&mut self) {
        self.blanked = true;
    }

    /// Video refresh on (INT10/12 BL=36 AL=0).
    ///
    /// Origin: BMMAIN.EXE 45ac:2f9b (video_screen_on)
    pub fn video_screen_on(&mut self) {
        self.blanked = false;
    }

    /// Copies the page the CRTC shows (and the DAC) into `vga` for presentation.
    pub fn to_vga(&self, vga: &mut Vga) {
        vga.mode = if self.visible_lines == TEAMED_VISIBLE_LINES {
            Mode::ModeX242
        } else {
            Mode::ModeX
        };
        for y in 0..self.visible_lines {
            for x in 0..SCREEN_W {
                let v = if self.blanked {
                    0
                } else {
                    self.page_pixel(self.crtc_start, x, y)
                };
                vga.pixels[y as usize * WIDTH + x as usize] = v;
            }
        }
        vga.dac = self.shown_dac();
    }

    /// The DAC as the monitor sees it: black while the refresh is off.
    fn shown_dac(&self) -> [[u8; 3]; 256] {
        if self.blanked {
            [[0; 3]; 256]
        } else {
            self.dac
        }
    }

    /// MOD2026: switches the 2x display layer on, starting from the current planes.
    pub fn display_on(&mut self) {
        self.display = Some(Display::from_planes(&self.planes));
    }

    #[cfg(test)]
    pub(crate) fn display(&self) -> Option<&Display> {
        self.display.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn display_mut(&mut self) -> Option<&mut Display> {
        self.display.as_mut()
    }

    /// MOD2026: [`Gfx::to_vga`] for the display layer; `false` (and `frame` untouched)
    /// when the layer is off.
    pub fn to_display(&self, frame: &mut DisplayFrame) -> bool {
        let Some(display) = &self.display else {
            return false;
        };
        frame.height = 2 * self.visible_lines as usize;
        for y in 0..self.visible_lines {
            for x in 0..SCREEN_W {
                let block = if self.blanked {
                    [0; 4]
                } else {
                    display.block((x & 3) as usize, page_offset(self.crtc_start, x, y))
                };
                frame.set_block(x as usize, y as usize, block);
            }
        }
        frame.dac = self.shown_dac();
        true
    }

    // ----- mouse pointer --------------------------------------------------------------

    fn pointer_origin(&self) -> (i16, i16) {
        (
            self.pointer.x.wrapping_add(self.pointer.hot_x),
            self.pointer.y.wrapping_add(self.pointer.hot_y),
        )
    }

    /// Whether rect `(x, y, w, h)` overlaps the pointer sprite rectangle (sprite origin
    /// = position + offsets, size = sprite bitmap size). Strict comparisons, signed.
    ///
    /// Origin: BMMAIN.EXE 45ac:16d0 (rect_hits_mouse_pointer)
    pub fn rect_hits_mouse_pointer(&self, x: i16, y: i16, w: i16, h: i16) -> bool {
        let (px, py) = self.pointer_origin();
        let img = self.bitmap(self.pointer.image);
        let hit_x = if x < px {
            px < x.wrapping_add(w)
        } else {
            x < px.wrapping_add(img.width)
        };
        if !hit_x {
            return false;
        }
        if y < py {
            py < y.wrapping_add(h)
        } else {
            y < py.wrapping_add(img.height)
        }
    }

    /// State = hidden; copy the saved background (its full size) back to the screen
    /// bitmap at the sprite origin.
    ///
    /// Origin: BMMAIN.EXE 45ac:1746 (mouse_pointer_restore_bg)
    pub fn mouse_pointer_restore_bg(&mut self) {
        self.pointer.state = POINTER_HIDDEN;
        let (px, py) = self.pointer_origin();
        let bg = self.pointer.background;
        let (bw, bh, _) = self.dims(bg);
        let scr = self.pointer.screen;
        self.blit_rect_impl(bg, 0, 0, scr, px, py, bw, bh, false);
    }

    /// State = hidden while drawing; save the screen under the sprite (background size)
    /// and blit the sprite transparently (with the background's size); state = visible.
    ///
    /// Origin: BMMAIN.EXE 45ac:17a0 (mouse_pointer_draw)
    pub fn mouse_pointer_draw(&mut self) {
        self.pointer.state = POINTER_HIDDEN;
        let (px, py) = self.pointer_origin();
        let bg = self.pointer.background;
        let (bw, bh, _) = self.dims(bg);
        let scr = self.pointer.screen;
        let img = self.pointer.image;
        self.blit_rect_impl(scr, px, py, bg, 0, 0, bw, bh, false);
        self.blit_rect_impl(img, 0, 0, scr, px, py, bw, bh, true);
        self.pointer.state = POINTER_VISIBLE;
    }

    /// The "hide if overlapping" prologue of every pointer-aware VRAM routine. Returns
    /// whether the pointer must be redrawn afterwards (`CS:027C`).
    fn pointer_hide_for_rect(&mut self, x: i16, y: i16, w: i16, h: i16) -> bool {
        let st = self.pointer.state;
        if st == POINTER_HIDDEN || !self.rect_hits_mouse_pointer(x, y, w, h) {
            return false;
        }
        self.mouse_pointer_restore_bg();
        st != POINTER_LAZY
    }

    fn set_client_pos(&mut self, x: i16, y: i16) {
        self.pointer.client_x = x;
        self.pointer.client_y = y;
    }

    /// Move without clamping (called by the event handler): hide if drawn, store the
    /// position and the client copies, redraw and restore the previous state.
    ///
    /// Origin: BMMAIN.EXE 45ac:1850 (mouse_pointer_move_internal)
    pub fn mouse_pointer_move_internal(&mut self, x: i16, y: i16) {
        let saved = self.pointer.state;
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_restore_bg();
        }
        self.pointer.x = x;
        self.pointer.y = y;
        self.set_client_pos(x, y);
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_draw();
            self.pointer.state = saved;
        }
    }

    /// INT33 event handler (mask 0x1F). The x coordinate is halved unless the driver
    /// reports 0..319. A move (bit 0) to a new position moves the pointer. Then the
    /// first set bit of left-down (1), left-up (2), right-down (3), right-up (4) latches
    /// its event flag. While the library is busy (`CS:0036 == 1`) the halving and the
    /// move are skipped (`JZ 45ac:18f6`), but the button flags are still latched.
    ///
    /// Origin: BMMAIN.EXE 45ac:18c2 (mouse_event_handler)
    pub fn mouse_event_handler(&mut self, mask: u16, cx: u16, dx: u16) {
        if !self.busy {
            let x = if self.pointer.native_320 { cx } else { cx >> 1 };
            if mask & 1 != 0 && (x as i16 != self.pointer.x || dx as i16 != self.pointer.y) {
                self.mouse_pointer_move_internal(x as i16, dx as i16);
            }
        }
        for (bit, flag) in [(2u16, 0usize), (4, 1), (8, 2), (0x10, 3)] {
            if mask & bit != 0 {
                self.pointer.events[flag] = true;
                break;
            }
        }
    }

    /// Returns and clears the first latched event: 1 left down, 2 left up, 3 right down,
    /// 4 right up, 0 none.
    ///
    /// Origin: BMMAIN.EXE 45ac:192d (mouse_pop_button_event)
    pub fn mouse_pop_button_event(&mut self) -> u16 {
        for i in 0..4 {
            if self.pointer.events[i] {
                self.pointer.events[i] = false;
                return i as u16 + 1;
            }
        }
        0
    }

    /// Clamp to `0..w-1` / `0..h-1` of the pointer screen bitmap (upper bound first),
    /// store, update the client copies, set the driver position (x doubled unless
    /// native) and redraw keeping the previous state.
    ///
    /// Origin: BMMAIN.EXE 45ac:1971 (mouse_set_position)
    pub fn mouse_set_position(&mut self, x: i16, y: i16) {
        let saved = self.pointer.state;
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_restore_bg();
        }
        let (sw, sh, _) = self.dims(self.pointer.screen);
        let mut cx = x;
        if cx >= sw {
            cx = sw.wrapping_sub(1);
        }
        if cx < 0 {
            cx = 0;
        }
        let mut cy = y;
        if cy >= sh {
            cy = sh.wrapping_sub(1);
        }
        if cy < 0 {
            cy = 0;
        }
        self.pointer.x = cx;
        self.pointer.y = cy;
        self.set_client_pos(cx, cy);
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_draw();
            self.pointer.state = saved;
        }
    }

    /// Set sprite, background buffer and origin offsets; redraw keeping the state.
    ///
    /// Origin: BMMAIN.EXE 45ac:1a19 (mouse_set_pointer_shape)
    pub fn mouse_set_pointer_shape(
        &mut self,
        image: BitmapId,
        background: BitmapId,
        hot_x: i16,
        hot_y: i16,
    ) {
        let saved = self.pointer.state;
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_restore_bg();
        }
        self.pointer.hot_x = hot_x;
        self.pointer.hot_y = hot_y;
        self.pointer.background = background;
        self.pointer.image = image;
        if saved != POINTER_HIDDEN {
            self.mouse_pointer_draw();
            self.pointer.state = saved;
        }
    }

    /// `lazy == 0`: remove the pointer now. Non-zero: keep it drawn but set the state
    /// to `lazy` (1 = remove on the first overlapping draw). Nothing changes when it is
    /// already hidden. The nesting counter is always incremented.
    ///
    /// Origin: BMMAIN.EXE 45ac:1a81 (mouse_pointer_hide)
    pub fn mouse_pointer_hide(&mut self, lazy: u8) {
        if self.pointer.state != POINTER_HIDDEN {
            if lazy == 0 {
                self.mouse_pointer_restore_bg();
            } else {
                self.pointer.state = lazy;
            }
        }
        self.pointer.hide_count = self.pointer.hide_count.wrapping_add(1);
    }

    /// Decrement the counter; at 0 draw the pointer if hidden, and set it visible.
    ///
    /// Origin: BMMAIN.EXE 45ac:1ac7 (mouse_pointer_show)
    pub fn mouse_pointer_show(&mut self) {
        self.pointer.hide_count = self.pointer.hide_count.wrapping_sub(1);
        if self.pointer.hide_count == 0 {
            if self.pointer.state == POINTER_HIDDEN {
                self.mouse_pointer_draw();
            }
            self.pointer.state = POINTER_VISIBLE;
        }
    }

    /// Driver reset; returns false without a driver. Probes the x range by setting
    /// x = 600 and reading it back: a driver that clamps to <= 350 is "native" 0..319.
    /// Sets ranges y 0..239, x 0..319 (native) or 0..639, centers the pointer at
    /// (160, 120), installs the built-in arrow with offsets (0, 0), state hidden,
    /// hide counter 1, clears the events and writes the client position copies.
    ///
    /// Origin: BMMAIN.EXE 45ac:1b08 (mouse_install)
    pub fn mouse_install(&mut self, driver_present: bool, driver_range_640: bool) -> bool {
        if self.pointer.state != POINTER_HIDDEN {
            self.mouse_pointer_restore_bg();
        }
        if !driver_present {
            return false;
        }
        self.pointer.native_320 = !driver_range_640;
        self.pointer.x = 160;
        self.pointer.y = 120;
        self.pointer.hot_x = 0;
        self.pointer.hot_y = 0;
        let image = self.alloc(Bitmap {
            width: 11,
            height: 11,
            storage: Storage::Mem(DEFAULT_POINTER.to_vec()),
            origin: (0, 0),
            blocks: None,
        });
        let bg = self.alloc(Bitmap::new_mem(11, 11));
        self.pointer.image = image;
        self.pointer.background = bg;
        self.pointer.state = POINTER_HIDDEN;
        self.pointer.hide_count = 1;
        self.pointer.events = [false; 4];
        self.set_client_pos(160, 120);
        true
    }

    /// Remove the pointer and reset the driver.
    ///
    /// Origin: BMMAIN.EXE 45ac:1c4b (mouse_uninstall)
    pub fn mouse_uninstall(&mut self) {
        if self.pointer.state != POINTER_HIDDEN {
            self.mouse_pointer_restore_bg();
        }
    }

    // ----- blits ----------------------------------------------------------------------

    /// Clipped rectangle copy (see [`clip_blit`]). Linear/linear: plain copy; VRAM
    /// source (screen to memory) or VRAM destination (memory to screen): Mode X
    /// addressing from the descriptor offset plus the draw page, pointer hidden while
    /// copying if it overlaps the VRAM rectangle. `transparent` skips source zeros.
    ///
    /// VRAM to VRAM is not supported by the original (it would write through the
    /// planar window linearly); the wrappers route it through a temporary buffer, and
    /// this model copies pixel-exactly.
    #[allow(clippy::too_many_arguments)]
    fn blit_rect_impl(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
        transparent: bool,
    ) {
        let (sw, sh, sv) = self.dims(src);
        let (dw, dh, dv) = self.dims(dst);
        let Some((sx, sy, dx, dy, w, h)) = clip_blit(sw, sh, sx, sy, dw, dh, dx, dy, w, h) else {
            return;
        };
        let page = self.page_add();
        let redraw = if sv.is_some() {
            self.pointer_hide_for_rect(sx, sy, w, h)
        } else if dv.is_some() {
            self.pointer_hide_for_rect(dx, dy, w, h)
        } else {
            false
        };
        if src == dst && sv.is_none() {
            // rep movsb inside one buffer: forward byte order.
            for r in 0..h {
                for c in 0..w {
                    let s = (sy + r) as usize * sw as usize + (sx + c) as usize;
                    let d = (dy + r) as usize * dw as usize + (dx + c) as usize;
                    let v = self.mem_get(src, s);
                    let block = self.mem_block(src, s);
                    self.mem_blit_put(dst, d, v, block, transparent);
                }
            }
        } else {
            // Each pixel with its display block (MOD2026), `None` for a doubled one.
            let mut buf: Vec<(u8, Option<Block>)> = Vec::with_capacity(w as usize * h as usize);
            for r in 0..h {
                for c in 0..w {
                    let px = match sv {
                        Some(base) => {
                            let x = sx + c;
                            let plane = x as usize & 3;
                            let off = vram_off(base.wrapping_add(page), sy, r, x, self.row_mask);
                            (self.vram_read(plane, off), self.display_block(plane, off))
                        }
                        None => {
                            let i = (sy + r) as usize * sw as usize + (sx + c) as usize;
                            (self.mem_get(src, i), self.mem_block(src, i))
                        }
                    };
                    buf.push(px);
                }
            }
            for r in 0..h {
                for c in 0..w {
                    let (v, block) = buf[r as usize * w as usize + c as usize];
                    match dv {
                        Some(base) => {
                            let x = dx + c;
                            let off = vram_off(base.wrapping_add(page), dy, r, x, self.row_mask);
                            self.vram_blit_put(x as usize & 3, off, v, block, transparent);
                        }
                        None => {
                            let d = (dy + r) as usize * dw as usize + (dx + c) as usize;
                            self.mem_blit_put(dst, d, v, block, transparent);
                        }
                    }
                }
            }
        }
        if redraw {
            self.mouse_pointer_draw();
        }
    }

    /// One destination pixel of a VRAM blit. Transparent: a source 0 keeps the logical
    /// pixel, and (MOD2026) the display keeps every display pixel the block has as 0.
    fn vram_blit_put(
        &mut self,
        plane: usize,
        off: u16,
        v: u8,
        block: Option<Block>,
        transparent: bool,
    ) {
        let block = block.unwrap_or([v; 4]);
        if !transparent {
            self.vram_put(plane, off, v, block);
            return;
        }
        if v != 0 {
            self.planes[plane & 3][off as usize] = v;
        }
        if let Some(display) = &mut self.display {
            display.merge(plane, off, block);
        }
    }

    /// One destination pixel of a blit into a memory bitmap, like [`Gfx::vram_blit_put`].
    fn mem_blit_put(
        &mut self,
        id: BitmapId,
        idx: usize,
        v: u8,
        block: Option<Block>,
        transparent: bool,
    ) {
        let block = block.unwrap_or([v; 4]);
        if !transparent {
            self.mem_put(id, idx, v, block);
            return;
        }
        if block == [0; 4] {
            return;
        }
        let old = self.mem_get(id, idx);
        let new = if v != 0 { v } else { old };
        let mut merged = self.mem_block(id, idx).unwrap_or([old; 4]);
        display::merge(&mut merged, block);
        self.mem_put(id, idx, new, merged);
    }

    /// Opaque clipped rectangle copy.
    ///
    /// Origin: BMMAIN.EXE 45ac:1d1c (blit_rect)
    #[allow(clippy::too_many_arguments)]
    pub fn blit_rect(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        self.blit_rect_impl(src, sx, sy, dst, dx, dy, w, h, false);
    }

    /// Clipped rectangle copy that skips source pixels of value 0.
    ///
    /// Origin: BMMAIN.EXE 45ac:20c8 (blit_rect_transparent)
    #[allow(clippy::too_many_arguments)]
    pub fn blit_rect_transparent(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        self.blit_rect_impl(src, sx, sy, dst, dx, dy, w, h, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn blit_bitmap_impl(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
        transparent: bool,
    ) {
        // TEAMED.EXE 1a46:09a8/0878 add both bitmaps' origins (zero in BMMAIN).
        let (sx, sy) = self.with_origin(src, sx, sy);
        let (dx, dy) = self.with_origin(dst, dx, dy);
        if self.bitmap(src).is_screen() && self.bitmap(dst).is_screen() {
            // Temporary w x h buffer (mem_alloc without zero fill in the original).
            let tmp = self.alloc(Bitmap::new_mem(w, h));
            self.blit_rect_impl(src, sx, sy, tmp, 0, 0, w, h, false);
            self.blit_rect_impl(tmp, 0, 0, dst, dx, dy, w, h, transparent);
            self.free(tmp);
        } else {
            self.blit_rect_impl(src, sx, sy, dst, dx, dy, w, h, transparent);
        }
    }

    /// `blit_rect`, through a temporary buffer when both bitmaps are the screen
    /// (`A000:0000`). In the original `dst` is a rastport (its bitmap is used).
    ///
    /// Origin: BMMAIN.EXE 4930:0991 (blit_bitmap)
    #[allow(clippy::too_many_arguments)]
    pub fn blit_bitmap(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        self.blit_bitmap_impl(src, sx, sy, dst, dx, dy, w, h, false);
    }

    /// Transparent variant of [`Gfx::blit_bitmap`] (`dst` is a bitmap here).
    ///
    /// Origin: BMMAIN.EXE 4930:0aa5 (blit_bitmap_transparent)
    #[allow(clippy::too_many_arguments)]
    pub fn blit_bitmap_transparent(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        self.blit_bitmap_impl(src, sx, sy, dst, dx, dy, w, h, true);
    }

    /// If `src` is not the screen and is 240 rows high (the 320x240 work bitmap, whose
    /// pixels really live in VRAM page 2), copy the rectangle `(sx, sy, w, h)` from page
    /// 2 to page 0 at the same position, ignoring `dst`, `dx` and `dy`. Otherwise
    /// behave like [`Gfx::blit_bitmap`].
    ///
    /// Origin: BMMAIN.EXE 4930:0849 (blit_bitmap_or_restore_page2)
    #[allow(clippy::too_many_arguments)]
    pub fn blit_bitmap_or_restore_page2(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        let s = self.bitmap(src);
        if !s.is_screen() && s.height == 0xf0 {
            self.vga_restore_rect_from_page(SCREEN_W, SCREEN_H, sx, sy, w, h, 3);
        } else {
            self.blit_bitmap_impl(src, sx, sy, dst, dx, dy, w, h, false);
        }
    }

    /// Latched (write mode 1) copy of a clipped rectangle from VRAM page 2 (`sel == 3`,
    /// `A000:9600`) or page 1 (any other value, `A000:4B00`) to page 0 at the same
    /// position. The destination ignores the draw page. Pointer-aware like `fill_rect`.
    ///
    /// Origin: BMMAIN.EXE 45ac:34fc (vga_restore_rect_from_page)
    #[allow(clippy::too_many_arguments)]
    pub fn vga_restore_rect_from_page(
        &mut self,
        clip_w: i16,
        clip_h: i16,
        x: i16,
        y: i16,
        w: i16,
        h: i16,
        sel: u16,
    ) {
        let Some((x, y, _, _, w, h)) = clip_fill(clip_w, clip_h, x, y, w, h) else {
            return;
        };
        let redraw = self.pointer_hide_for_rect(x, y, w, h);
        let src_base = if sel == 3 { PAGE2_OFFSET } else { PAGE_SIZE };
        for r in 0..h {
            for c in 0..w {
                let px = x + c;
                let off = vram_off(0, y, r, px, self.row_mask);
                let plane = px as usize & 3;
                self.vram_copy(plane, off.wrapping_add(src_base), plane, off);
            }
        }
        if redraw {
            self.mouse_pointer_draw();
        }
    }

    // ----- pixels, lines, rectangles ---------------------------------------------------

    /// Index into the pointer buffers for a screen pixel covered by the pointer.
    ///
    /// The original (45ac:2507 / 2607 / 2715) multiplies the row by word `+2` of the
    /// background descriptor, which is its HEIGHT, not its width. This is only
    /// equivalent for square pointers (all shipped shapes are 11x11 or 1x1); it is
    /// reproduced as is. 16-bit `MUL`, wrapping.
    fn pointer_buf_index(&self, x: i16, y: i16) -> usize {
        let (px, py) = self.pointer_origin();
        let bx = x.wrapping_sub(px) as u16;
        let cy = y.wrapping_sub(py) as u16;
        let stride = self.bitmap(self.pointer.background).height as u16;
        cy.wrapping_mul(stride).wrapping_add(bx) as usize
    }

    fn pixel_common(&mut self, x: i16, y: i16, bm: BitmapId, op: impl Fn(u8) -> u8) {
        let (w, h, v) = self.dims(bm);
        if x < 0 || x >= w || y < 0 || y >= h {
            return;
        }
        match v {
            None => {
                let idx = (y as u16).wrapping_mul(w as u16).wrapping_add(x as u16) as usize;
                self.mem_update(bm, idx, op);
            }
            Some(_) => {
                if self.pointer.state != POINTER_HIDDEN && self.rect_hits_mouse_pointer(x, y, 1, 1)
                {
                    let i = self.pointer_buf_index(x, y);
                    let bg = self.pointer.background;
                    self.mem_update(bg, i, &op);
                    if self.mem_get(self.pointer.image, i) != 0 {
                        return;
                    }
                }
                // The descriptor offset is ignored: page base only.
                let off = ((y as u16 & self.row_mask).wrapping_mul(ROW_BYTES))
                    .wrapping_add(x as u16 >> 2)
                    .wrapping_add(self.page_add());
                self.vram_update(x as usize & 3, off, op);
            }
        }
    }

    /// Set a pixel (bounds-checked, signed). On VRAM the descriptor offset is ignored;
    /// a pixel under the visible pointer is written into the saved background and only
    /// reaches VRAM where the sprite is transparent.
    ///
    /// Origin: BMMAIN.EXE 45ac:2486 (put_pixel)
    pub fn put_pixel(&mut self, x: i16, y: i16, bm: BitmapId, color: u8) {
        self.pixel_common(x, y, bm, |_| color);
    }

    /// XOR a pixel with `0x1F` (VRAM: GC function XOR on the latches); same pointer
    /// handling as [`Gfx::put_pixel`].
    ///
    /// Origin: BMMAIN.EXE 45ac:2586 (xor_pixel)
    pub fn xor_pixel(&mut self, x: i16, y: i16, bm: BitmapId) {
        self.pixel_common(x, y, bm, |o| o ^ XOR_VALUE);
    }

    /// Read a pixel; `0xFFFF` outside the bitmap. Under the visible pointer the saved
    /// background is returned. On VRAM the descriptor offset is ignored.
    ///
    /// Origin: BMMAIN.EXE 45ac:2694 (get_pixel)
    pub fn get_pixel(&self, x: i16, y: i16, bm: BitmapId) -> u16 {
        let (w, h, v) = self.dims(bm);
        if x < 0 || x >= w || y < 0 || y >= h {
            return 0xffff;
        }
        match v {
            None => {
                let idx = (y as u16).wrapping_mul(w as u16).wrapping_add(x as u16) as usize;
                self.mem_get(bm, idx) as u16
            }
            Some(_) => {
                if self.pointer.state != POINTER_HIDDEN && self.rect_hits_mouse_pointer(x, y, 1, 1)
                {
                    return self.mem_get(self.pointer.background, self.pointer_buf_index(x, y))
                        as u16;
                }
                let off = ((y as u16 & self.row_mask).wrapping_mul(ROW_BYTES))
                    .wrapping_add(x as u16 >> 2)
                    .wrapping_add(self.page_add());
                self.vram_read(x as usize & 3, off) as u16
            }
        }
    }

    /// Bresenham as in `45ac:278b`: plots `(x0, y0)`, then steps the major axis
    /// (`|dy| >= |dx|` makes y major, so a point is y-major) with an unsigned error
    /// accumulator, plotting after each step; `max(|dx|,|dy|) + 1` points in total.
    fn line_impl(
        &mut self,
        x0: i16,
        y0: i16,
        x1: i16,
        y1: i16,
        mut plot: impl FnMut(&mut Self, i16, i16),
    ) {
        let mut ddx = x1.wrapping_sub(x0);
        let sxs: i16 = if ddx < 0 {
            ddx = ddx.wrapping_neg();
            -1
        } else {
            1
        };
        let mut ddy = y1.wrapping_sub(y0);
        let sys: i16 = if ddy < 0 {
            ddy = ddy.wrapping_neg();
            -1
        } else {
            1
        };
        let (mut x, mut y) = (x0, y0);
        let mut err: u16 = 0;
        plot(self, x, y);
        if ddy < ddx {
            let mut n = ddx;
            loop {
                n = n.wrapping_sub(1);
                if n < 0 {
                    break;
                }
                x = x.wrapping_add(sxs);
                err = err.wrapping_add(ddy as u16);
                if err >= ddx as u16 {
                    err = err.wrapping_sub(ddx as u16);
                    y = y.wrapping_add(sys);
                }
                plot(self, x, y);
            }
        } else {
            let mut n = ddy;
            loop {
                n = n.wrapping_sub(1);
                if n < 0 {
                    break;
                }
                y = y.wrapping_add(sys);
                err = err.wrapping_add(ddx as u16);
                if err >= ddy as u16 {
                    err = err.wrapping_sub(ddy as u16);
                    x = x.wrapping_add(sxs);
                }
                plot(self, x, y);
            }
        }
    }

    /// Bresenham line with [`Gfx::put_pixel`].
    ///
    /// Origin: BMMAIN.EXE 45ac:278b (draw_line)
    pub fn draw_line(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8, bm: BitmapId) {
        self.line_impl(x0, y0, x1, y1, |g, x, y| g.put_pixel(x, y, bm, color));
    }

    /// Bresenham line with [`Gfx::xor_pixel`].
    ///
    /// Origin: BMMAIN.EXE 45ac:2898 (draw_line_xor)
    pub fn draw_line_xor(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, bm: BitmapId) {
        self.line_impl(x0, y0, x1, y1, |g, x, y| g.xor_pixel(x, y, bm));
    }

    fn fill_impl(&mut self, bm: BitmapId, x: i16, y: i16, w: i16, h: i16, op: impl Fn(u8) -> u8) {
        let (bw, bh, v) = self.dims(bm);
        let Some((x, y, _, _, w, h)) = clip_fill(bw, bh, x, y, w, h) else {
            return;
        };
        match v {
            None => {
                for r in 0..h {
                    for c in 0..w {
                        let idx = (y + r) as usize * bw as usize + (x + c) as usize;
                        self.mem_update(bm, idx, &op);
                    }
                }
            }
            Some(base) => {
                let redraw = self.pointer_hide_for_rect(x, y, w, h);
                let base = base.wrapping_add(self.page_add());
                for r in 0..h {
                    for c in 0..w {
                        let px = x + c;
                        let off = vram_off(base, y, r, px, self.row_mask);
                        self.vram_update(px as usize & 3, off, &op);
                    }
                }
                if redraw {
                    self.mouse_pointer_draw();
                }
            }
        }
    }

    /// Clipped solid fill of `(x, y, w, h)`. Clipping: left (`w += x; x = 0`), right,
    /// reject `w <= 0`, then the same for y.
    ///
    /// Origin: BMMAIN.EXE 45ac:29a4 (fill_rect)
    pub fn fill_rect(&mut self, bm: BitmapId, x: i16, y: i16, w: i16, h: i16, color: u8) {
        self.fill_impl(bm, x, y, w, h, |_| color);
    }

    /// Clipped XOR-`0x1F` fill.
    ///
    /// Origin: BMMAIN.EXE 45ac:2b3e (fill_rect_xor)
    pub fn fill_rect_xor(&mut self, bm: BitmapId, x: i16, y: i16, w: i16, h: i16) {
        self.fill_impl(bm, x, y, w, h, |o| o ^ XOR_VALUE);
    }

    /// Linear: fill `(w * h) as u16` bytes. VRAM: clear one page (`0x4B00` bytes in all
    /// planes) at the pointer screen descriptor's offset plus the draw page; the
    /// argument's offset is ignored. The pointer is removed first unless hidden, and
    /// redrawn afterwards unless it was in lazy state.
    ///
    /// Origin: BMMAIN.EXE 45ac:2cf0 (clear_bitmap)
    pub fn clear_bitmap(&mut self, bm: BitmapId, color: u8) {
        let (w, h, v) = self.dims(bm);
        match v {
            None => {
                let n = (w as u16).wrapping_mul(h as u16) as usize;
                let bitmap = self.bitmap_mut(bm);
                if let Storage::Mem(d) = &mut bitmap.storage {
                    let n = n.min(d.len());
                    d[..n].fill(color);
                }
                if let Some(side) = &mut bitmap.blocks {
                    let n = n.min(side.len());
                    side[..n].fill([color; 4]);
                }
            }
            Some(_) => {
                let st = self.pointer.state;
                let mut redraw = false;
                if st != POINTER_HIDDEN {
                    redraw = st != POINTER_LAZY;
                    self.mouse_pointer_restore_bg();
                }
                let base = self
                    .bitmap(self.pointer.screen)
                    .vram_base()
                    .unwrap_or(0)
                    .wrapping_add(self.page_add());
                for p in 0..4 {
                    for i in 0..PAGE_SIZE {
                        self.vram_write(p, base.wrapping_add(i), color);
                    }
                }
                if redraw {
                    self.mouse_pointer_draw();
                }
            }
        }
    }

    // ----- text -----------------------------------------------------------------------

    /// Draws `text` with the baseline at `y` in font `font`. The cell top is
    /// `y - (height - descent - 1)`. Each glyph is expanded into a `width x height`
    /// buffer (`fg` for set bits, `bg` or 0 when `mode == 0` for clear bits) and blitted
    /// transparently when `mode == 0`, opaquely otherwise. Stops when the text ends or
    /// before a glyph whose `x >= bitmap width`. On the screen a visible pointer is put
    /// into lazy-hide state for the duration and redrawn at the end if it was removed.
    /// Returns the final pen x (the original returns nothing).
    ///
    /// MOD2026: with the display layer on, a glyph with a 2x glyph
    /// ([`Gfx::set_hires_fonts`]) gives its buffer the 2x glyph as display side, so the
    /// display shows the 2x glyph and the logical frame stays the same.
    ///
    /// Origin: BMMAIN.EXE 45ac:2d83 (draw_text)
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &mut self,
        bm: BitmapId,
        mut x: i16,
        y: i16,
        font: u16,
        fg: u8,
        bg: u8,
        mode: u8,
        text: &[u8],
    ) -> i16 {
        let (bw, _, v) = self.dims(bm);
        let mut lazy = false;
        if v.is_some() && self.pointer.state == POINTER_VISIBLE {
            lazy = true;
            self.pointer.state = POINTER_LAZY;
        }
        let bgc = if mode == 0 { 0 } else { bg };
        let glyph_data: Option<ExpandedText> = self
            .fonts
            .get(font as usize)
            .and_then(|f| f.as_ref())
            .map(|f| {
                let gl = text
                    .iter()
                    .map(|&c| {
                        let g = self.glyph(font, f, c);
                        let w = g.width as usize;
                        let h = (f.height & 0xff) as usize;
                        let mut px = Vec::with_capacity(w * h);
                        for row in 0..h {
                            for col in 0..w {
                                px.push(if g.pixel(col, row) { 1 } else { 0 });
                            }
                        }
                        // MOD2026: the 2x glyph.
                        let hires = self
                            .display
                            .as_ref()
                            .and_then(|_| self.hires_glyph(font, c))
                            .map(|big| hires_blocks(big, w, h, fg, bgc));
                        (g.width, px, hires)
                    })
                    .collect();
                (f.height, f.descent, gl)
            });
        if let Some((height, descent, glyphs)) = glyph_data {
            let top = y.wrapping_sub(height.wrapping_sub(descent).wrapping_sub(1) as i16);
            let gh = (height & 0xff) as i16;
            for (width, bits, hires) in glyphs {
                if x >= bw {
                    break;
                }
                // Deviation: the original expands a width-0 glyph with `DEC DL` from
                // 0 (256 columns per row), overrunning the `CS:0406` buffer. No
                // shipped font has a width-0 glyph; the port skips it.
                if width == 0 {
                    continue;
                }
                let gw = width as i16;
                let buf: Vec<u8> = bits
                    .iter()
                    .map(|&b| if b != 0 { fg } else { bgc })
                    .collect();
                let tmp = self.alloc(Bitmap {
                    width: gw,
                    height: gh,
                    storage: Storage::Mem(buf),
                    origin: (0, 0),
                    blocks: None,
                });
                if let Some(blocks) = hires {
                    *self.mem_side(tmp) = blocks;
                }
                self.blit_rect_impl(tmp, 0, 0, bm, x, top, gw, gh, mode == 0);
                self.free(tmp);
                x = x.wrapping_add(gw);
            }
        }
        if lazy {
            if self.pointer.state == POINTER_HIDDEN {
                self.mouse_pointer_draw();
            }
            self.pointer.state = POINTER_VISIBLE;
        }
        x
    }

    /// Sum of glyph widths (16-bit wrapping).
    ///
    /// Origin: BMMAIN.EXE 45ac:2eb4 (text_width)
    pub fn text_width(&self, font: u16, text: &[u8]) -> u16 {
        match self.fonts.get(font as usize).and_then(|f| f.as_ref()) {
            Some(f) => text.iter().fold(0u16, |a, &c| {
                a.wrapping_add(self.glyph(font, f, c).width as u16)
            }),
            None => 0,
        }
    }

    /// Exact name lookup in the library font table; fills index, height and descent,
    /// or returns `None` (handle word -1).
    ///
    /// Origin: BMMAIN.EXE 45ac:2f16 (font_find_by_name)
    pub fn font_find_by_name(&self, name: &str) -> Option<FontHandle> {
        let index = FONT_NAMES.iter().position(|&n| n == name)? as u16;
        let (height, descent) = match &self.fonts[index as usize] {
            Some(f) => (f.height, f.descent),
            None => (0, 0),
        };
        Some(FontHandle {
            index,
            height,
            descent,
        })
    }

    // ----- palette --------------------------------------------------------------------

    /// INT10/1010: one DAC register (6-bit channels).
    ///
    /// Origin: BMMAIN.EXE 45ac:2fa3 (vga_set_dac_color)
    pub fn vga_set_dac_color(&mut self, index: u8, r: u8, g: u8, b: u8) {
        self.dac[index as usize] = [r & 0x3f, g & 0x3f, b & 0x3f];
    }

    /// Reads `count` interleaved RGB triplets for entries `first..` from plane
    /// `slot & 3` at `0xE100 + 3 * first`, stores them in `arrays` (R/G/B at
    /// `+0/+0x100/+0x200`, index `first + i`) and loads them into the DAC.
    ///
    /// Origin: BMMAIN.EXE 45ac:2fd8 (vga_restore_palette_slot)
    pub fn vga_restore_palette_slot(
        &mut self,
        first: u16,
        count: u16,
        slot: u16,
        arrays: &mut [u8; 0x300],
    ) {
        let plane = (slot & 3) as usize;
        let src = PALETTE_SLOT_OFFSET.wrapping_add(first.wrapping_mul(3));
        for i in 0..count {
            let mut rgb = [0u8; 3];
            for (c, v) in rgb.iter_mut().enumerate() {
                *v = self.vram_read(
                    plane,
                    src.wrapping_add(i.wrapping_mul(3)).wrapping_add(c as u16),
                );
                let idx = (c * 0x100 + first as usize + i as usize) % 0x300;
                arrays[idx] = *v;
            }
            let e = first.wrapping_add(i) as usize & 0xff;
            self.dac[e] = [rgb[0] & 0x3f, rgb[1] & 0x3f, rgb[2] & 0x3f];
        }
    }

    /// Converts 32 `0x0RGB` words to DAC values (`R*4, G*4, B*4`) and fades DAC
    /// entries 0..31 in (`fade_in`) from black or out to black: 60 steps of
    /// `acc += target; if acc >= 60 { acc -= 60; out ± 1 }` per channel; the DAC is
    /// loaded (after waiting for vertical retrace) after every second step. Returns
    /// the 30 uploaded frames; the DAC holds the last one afterwards.
    ///
    /// Origin: BMMAIN.EXE 45ac:33ca (vga_set_palette_rgb4)
    pub fn vga_set_palette_rgb4(
        &mut self,
        colors: &[u16; 32],
        fade_in: bool,
    ) -> Vec<[[u8; 3]; 32]> {
        let mut target = [0u8; 96];
        for (i, &w) in colors.iter().enumerate() {
            target[i * 3] = ((w >> 8) as u8) << 2;
            target[i * 3 + 1] = ((w as u8) & 0xf0) >> 2;
            target[i * 3 + 2] = ((w as u8) & 0x0f) << 2;
        }
        let mut out = if fade_in { [0u8; 96] } else { target };
        let mut acc = [0i8; 96];
        let mut frames = Vec::with_capacity(30);
        let mut cx: u16 = 0x3c;
        loop {
            for i in 0..96 {
                acc[i] = acc[i].wrapping_add(target[i] as i8);
                if acc[i] >= 0x3c {
                    acc[i] -= 0x3c;
                    if fade_in {
                        out[i] = out[i].wrapping_add(1);
                    } else {
                        out[i] = out[i].wrapping_sub(1);
                    }
                }
            }
            if cx & 1 == 0 {
                cx -= 1;
                continue;
            }
            let mut f = [[0u8; 3]; 32];
            for (e, fe) in f.iter_mut().enumerate() {
                *fe = [out[e * 3], out[e * 3 + 1], out[e * 3 + 2]];
                self.dac[e] = [fe[0] & 0x3f, fe[1] & 0x3f, fe[2] & 0x3f];
            }
            frames.push(f);
            cx -= 1;
            if cx == 0 {
                break;
            }
        }
        frames
    }

    /// Wait for vertical retrace: nothing to do in the model.
    ///
    /// Origin: BMMAIN.EXE 45ac:34ef (vga_wait_vretrace)
    pub fn vga_wait_vretrace(&self) {}

    /// Store one entry in the screen palette arrays and program the DAC.
    ///
    /// Origin: BMMAIN.EXE 4930:05dd (palette_set_entry)
    pub fn palette_set_entry(&mut self, index: u8, r: u8, g: u8, b: u8) {
        let i = index as usize;
        self.palette[i] = r;
        self.palette[i + 0x100] = g;
        self.palette[i + 0x200] = b;
        self.vga_set_dac_color(index, r, g, b);
    }

    /// Load `count` entries from 12-bit `0x0RGB` words (R = `(w >> 6) & 0x3C`,
    /// G = `(w >> 2) & 0x3C`, B = `(w & 0xF) << 2`) into the screen palette and DAC.
    ///
    /// Origin: BMMAIN.EXE 4930:0624 (palette_load_rgb4)
    pub fn palette_load_rgb4(&mut self, words: &[u16], count: i16) {
        let mut i: i16 = 0;
        while i < count {
            let w = words[i as usize];
            let r = ((w >> 6) as u8) & 0x3c;
            let g = ((w >> 2) as u8) & 0x3c;
            let b = ((w as u8) & 0xf) << 2;
            self.palette_set_entry(i as u8, r, g, b);
            i += 1;
        }
    }

    /// The `palette_load_rgb4` branch for a "pointer" value below 4 (signed 32-bit):
    /// restore palette slot `slot` (entries `0..count`) from VRAM.
    ///
    /// Origin: BMMAIN.EXE 4930:0624 (palette_load_rgb4)
    pub fn palette_load_slot(&mut self, slot: u16, count: u16) {
        let mut arr = self.palette;
        self.vga_restore_palette_slot(0, count, slot, &mut arr);
        self.palette = arr;
    }

    /// `((R << 4 | G) << 2) | (B >> 2)` from the screen palette (exact `0x0RGB` when
    /// the channels are multiples of 4).
    ///
    /// Origin: BMMAIN.EXE 4930:06d5 (palette_get_rgb4)
    pub fn palette_get_rgb4(&self, index: u8) -> u16 {
        let i = index as usize;
        let r = self.palette[i] as u16;
        let g = self.palette[i + 0x100] as u16;
        let b = self.palette[i + 0x200];
        (((r << 4) | g) << 2) | (b >> 2) as u16
    }

    // ----- timer ----------------------------------------------------------------------

    /// Deadline = BIOS tick count + `n` (32-bit).
    ///
    /// Origin: BMMAIN.EXE 45ac:307a (timer_set_deadline)
    pub fn timer_set_deadline(&mut self, n: u16) {
        self.deadline = self.bios_ticks.wrapping_add(n as u32);
    }

    /// True when ticks >= deadline (signed compare of the high words, unsigned of the
    /// low words).
    ///
    /// Origin: BMMAIN.EXE 45ac:30bc (timer_deadline_reached)
    pub fn timer_deadline_reached(&self) -> bool {
        let (th, tl) = ((self.bios_ticks >> 16) as i16, self.bios_ticks as u16);
        let (dh, dl) = ((self.deadline >> 16) as i16, self.deadline as u16);
        if dh < th {
            true
        } else if dh > th {
            false
        } else {
            tl >= dl
        }
    }

    /// Sets the deadline to `n / 3` ticks (signed 32-bit division truncating toward
    /// zero, low word used); the caller then waits for [`Gfx::timer_deadline_reached`].
    ///
    /// Origin: BMMAIN.EXE 4930:01e7 (delay_ticks)
    pub fn delay_ticks(&mut self, n: i32) {
        self.timer_set_deadline((n / 3) as u16);
    }

    // ----- rastport layer (4930) ------------------------------------------------------

    /// `(x, y)` plus the bitmap's origin: what TEAMED's wrappers (`1a46`) pass to the
    /// assembly library. BMMAIN's bitmaps have origin `(0, 0)`, so this is the identity
    /// for the game.
    ///
    /// Origin: TEAMED.EXE 1a46:0215 (rastport_draw_text, `ES:[BX+8]`/`ES:[BX+0A]` added)
    pub fn with_origin(&self, bm: BitmapId, x: i16, y: i16) -> (i16, i16) {
        let (ox, oy) = self.bitmap(bm).origin;
        (x.wrapping_add(ox), y.wrapping_add(oy))
    }

    /// Attach a font handle; refused (returns -1) when the handle word is -1.
    ///
    /// Origin: BMMAIN.EXE 4930:0261 (rastport_set_font)
    pub fn rastport_set_font(rp: &mut RastPort, handle: Option<FontHandle>) -> i16 {
        match handle {
            Some(h) => {
                rp.font = Some(h.index);
                0
            }
            None => -1,
        }
    }

    /// `draw_text` at the pen with the rastport's fg/bg/mode. The pen does not move.
    ///
    /// Origin: BMMAIN.EXE 4930:0291 (rastport_draw_text)
    pub fn rastport_draw_text(&mut self, rp: &RastPort, text: &[u8]) {
        if let Some(font) = rp.font {
            let (x, y) = self.with_origin(rp.bitmap, rp.pen_x, rp.pen_y);
            self.draw_text(rp.bitmap, x, y, font, rp.fg, rp.bg, rp.mode, text);
        }
    }

    /// Width of `text` in the rastport font.
    ///
    /// Origin: BMMAIN.EXE 4930:02f6 (rastport_text_width)
    pub fn rastport_text_width(&self, rp: &RastPort, text: &[u8]) -> i16 {
        rp.font.map_or(0, |f| self.text_width(f, text) as i16)
    }

    /// Origin: BMMAIN.EXE 4930:031c (rastport_move_to)
    pub fn rastport_move_to(rp: &mut RastPort, x: i16, y: i16) {
        rp.pen_x = x;
        rp.pen_y = y;
    }

    /// Line from the pen to `(x, y)` (XOR line in mode 2), then pen = `(x, y)`.
    ///
    /// Origin: BMMAIN.EXE 4930:033b (rastport_line_to)
    pub fn rastport_line_to(&mut self, rp: &mut RastPort, x: i16, y: i16) {
        let (x0, y0) = self.with_origin(rp.bitmap, rp.pen_x, rp.pen_y);
        let (x1, y1) = self.with_origin(rp.bitmap, x, y);
        if rp.mode == 2 {
            self.draw_line_xor(x0, y0, x1, y1, rp.bitmap);
        } else {
            self.draw_line(x0, y0, x1, y1, rp.fg, rp.bitmap);
        }
        rp.pen_x = x;
        rp.pen_y = y;
    }

    /// Plot in the fg pen (XOR in mode 2).
    ///
    /// Origin: BMMAIN.EXE 4930:03ce (rastport_put_pixel)
    pub fn rastport_put_pixel(&mut self, rp: &RastPort, x: i16, y: i16) {
        let (x, y) = self.with_origin(rp.bitmap, x, y);
        if rp.mode == 2 {
            self.xor_pixel(x, y, rp.bitmap);
        } else {
            self.put_pixel(x, y, rp.bitmap, rp.fg);
        }
    }

    /// Origin: BMMAIN.EXE 4930:043a (rastport_get_pixel)
    pub fn rastport_get_pixel(&self, rp: &RastPort, x: i16, y: i16) -> u16 {
        let (x, y) = self.with_origin(rp.bitmap, x, y);
        self.get_pixel(x, y, rp.bitmap)
    }

    /// Fill the inclusive rectangle `(x0, y0)-(x1, y1)` (`w = x1 - x0 + 1`, 16-bit) in
    /// the fg pen, or XOR it in mode 2.
    ///
    /// Origin: BMMAIN.EXE 4930:0471 (rastport_fill_rect)
    pub fn rastport_fill_rect(&mut self, rp: &RastPort, x0: i16, y0: i16, x1: i16, y1: i16) {
        let w = x1.wrapping_sub(x0).wrapping_add(1);
        let h = y1.wrapping_sub(y0).wrapping_add(1);
        let (x0, y0) = self.with_origin(rp.bitmap, x0, y0);
        if rp.mode == 2 {
            self.fill_rect_xor(rp.bitmap, x0, y0, w, h);
        } else {
            self.fill_rect(rp.bitmap, x0, y0, w, h, rp.fg);
        }
    }

    /// Origin: BMMAIN.EXE 4930:04fd (rastport_set_color)
    pub fn rastport_set_color(rp: &mut RastPort, c: u8) {
        rp.fg = c;
    }

    /// Origin: BMMAIN.EXE 4930:0515 (rastport_set_bg_color)
    pub fn rastport_set_bg_color(rp: &mut RastPort, c: u8) {
        rp.bg = c;
    }

    /// Origin: BMMAIN.EXE 4930:052d (rastport_set_draw_mode)
    pub fn rastport_set_draw_mode(rp: &mut RastPort, m: u8) {
        rp.mode = m;
    }

    /// Origin: BMMAIN.EXE 4930:0545 (rastport_clear)
    pub fn rastport_clear(&mut self, rp: &RastPort, color: u8) {
        self.clear_bitmap(rp.bitmap, color);
    }

    /// Allocates the screen (320x240 at `A000:0000`, the rastport at screen `+0x0C`
    /// using it) and switches to Mode X. Returns the screen rastport.
    ///
    /// Origin: BMMAIN.EXE 4930:0712 (screen_open)
    pub fn screen_open(&mut self) -> RastPort {
        let bm = self.alloc(Bitmap::vram(SCREEN_W, SCREEN_H, 0));
        self.vga_set_mode_x_320x240();
        // The 0x28-byte block comes from mem_alloc without zero fill; the game sets
        // the draw mode explicitly before drawing.
        RastPort {
            bitmap: bm,
            fg: 0,
            bg: 0,
            mode: 0,
            pen_x: 0,
            pen_y: 0,
            font: None,
        }
    }

    /// Origin: BMMAIN.EXE 4930:0819 (screen_close)
    pub fn screen_close(&mut self) {
        self.vga_set_text_mode();
    }

    /// Allocate the pointer state and install the mouse (see [`Gfx::mouse_install`]).
    ///
    /// Origin: BMMAIN.EXE 4930:077e (mouse_open)
    pub fn mouse_open(&mut self, driver_present: bool, driver_range_640: bool) -> bool {
        self.mouse_install(driver_present, driver_range_640)
    }

    /// Origin: BMMAIN.EXE 4930:0826 (mouse_close)
    pub fn mouse_close(&mut self) {
        self.mouse_uninstall();
    }

    /// The 8 symmetric points around `(cx, cy)` in this order: `(cx+a, cy+b)`,
    /// `(cx+a, cy-b)`, `(cx-a, cy+b)`, `(cx-a, cy-b)`, `(cx+b, cy+a)`, `(cx+b, cy-a)`,
    /// `(cx-b, cy+a)`, `(cx-b, cy-a)` (order matters in XOR mode).
    ///
    /// Origin: BMMAIN.EXE 4930:0bad (draw_circle_octants)
    pub fn draw_circle_octants(&mut self, rp: &RastPort, a: i16, b: i16, cx: i16, cy: i16) {
        let pts = [
            (cx.wrapping_add(a), cy.wrapping_add(b)),
            (cx.wrapping_add(a), cy.wrapping_sub(b)),
            (cx.wrapping_sub(a), cy.wrapping_add(b)),
            (cx.wrapping_sub(a), cy.wrapping_sub(b)),
            (cx.wrapping_add(b), cy.wrapping_add(a)),
            (cx.wrapping_add(b), cy.wrapping_sub(a)),
            (cx.wrapping_sub(b), cy.wrapping_add(a)),
            (cx.wrapping_sub(b), cy.wrapping_sub(a)),
        ];
        for (x, y) in pts {
            self.rastport_put_pixel(rp, x, y);
        }
    }

    /// Midpoint circle: `x = 0, y = r, d = 3 - 2r`; while `y > x`: octants, then
    /// `d < 0 ? d += 4x + 6 : (d += 4(x - y) + 10, y -= 1)`, `x += 1`; finally octants
    /// once more when `y == x`.
    ///
    /// Origin: BMMAIN.EXE 4930:0c90 (draw_circle)
    pub fn draw_circle(&mut self, rp: &RastPort, cx: i16, cy: i16, r: i16) {
        let mut x: i16 = 0;
        let mut y = r;
        let mut d = (r.wrapping_shl(1).wrapping_sub(3)).wrapping_neg();
        while y > x {
            self.draw_circle_octants(rp, x, y, cx, cy);
            if d < 0 {
                d = d.wrapping_add(x.wrapping_shl(2).wrapping_add(6));
            } else {
                d = d.wrapping_add(x.wrapping_sub(y).wrapping_shl(2).wrapping_add(10));
                y = y.wrapping_sub(1);
            }
            x = x.wrapping_add(1);
        }
        if y == x {
            self.draw_circle_octants(rp, x, y, cx, cy);
        }
    }
}

// ----- UI primitives (segment 16c7) -------------------------------------------------------

/// Number of button slots (`FAR:539E + 12*i`, 20 entries). The slot search can return
/// index 20, which in the original overlaps the frame bitmap pointers at `FAR:548E`;
/// the model keeps a 21st slot instead.
pub const BUTTON_SLOTS: usize = 20;

/// One registered button (`FAR:539E + 12*i`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonSlot {
    /// `+0` x.
    pub x: i16,
    /// `+2` width + 2; 0 = free.
    pub w: i16,
    /// `+4` (y - 2) as byte.
    pub y: u8,
    /// `+5` (h + 2) as byte.
    pub h: u8,
    /// `+6` pixel at `(x + 2, y + 2)` before drawing (low byte of `get_pixel`).
    pub pixel: u8,
    /// `+8` saved background (bitmap handle), if requested.
    pub saved: Option<BitmapId>,
}

/// UI globals used by the 16c7 primitives.
#[derive(Clone, Debug)]
pub struct Ui {
    pub slots: [ButtonSlot; BUTTON_SLOTS + 1],
    /// `FAR:548E/5492`: the two 50x41 button frame pictures (0x18, 0x19).
    pub frames: [Option<BitmapId>; 2],
    /// `FAR:2F3C`: icon sheet (6 icons of 33x24 per row).
    pub icons: Option<BitmapId>,
    /// `FAR:57C0`: picture used for icon code 0x23 (after remapping).
    pub icon_alt: Option<BitmapId>,
    /// Picture 37 (`37.VGA`), loaded by the original for icon code 0xC8 each time.
    pub picture_37: Option<BitmapId>,
    /// `FAR:56FA`: 320x240 work bitmap (a stand-in for VRAM page 2).
    pub work: Option<BitmapId>,
    /// `FAR:A6CA`: the screen bitmap.
    pub screen: BitmapId,
}

impl Ui {
    pub fn new(screen: BitmapId) -> Self {
        Self {
            slots: [ButtonSlot::default(); BUTTON_SLOTS + 1],
            frames: [None, None],
            icons: None,
            icon_alt: None,
            picture_37: None,
            work: None,
            screen,
        }
    }

    /// Return the 1-based index of the first rect `[x, y, w, h]` that strictly contains
    /// the mouse (`x < mx < x + w`, `y < my < y + h`), or 0. `click` is `None` when no
    /// click is required, else the event value `FAR:2EA2` after polling (must be 1).
    /// The loop counter is a signed byte stepping by 4 through `n * 4`, so only the
    /// first 32 rects can be tested (beyond that the original loops forever when nothing
    /// is hit; the model stops).
    ///
    /// Origin: BMMAIN.EXE 16c7:0002 (ui_hit_test_buttons)
    pub fn hit_test_buttons(
        rects: &[[i16; 4]],
        n: i16,
        mouse_x: i16,
        mouse_y: i16,
        click: Option<i16>,
    ) -> i16 {
        if let Some(ev) = click
            && ev != 1
        {
            return 0;
        }
        let limit = n.wrapping_mul(4);
        let mut c: i8 = 0;
        while (c as i16) < limit {
            if c < 0 {
                return 0;
            }
            let Some(r) = rects.get(c as usize / 4) else {
                return 0;
            };
            if r[0] < mouse_x
                && r[0].wrapping_add(r[2]) > mouse_x
                && r[1] < mouse_y
                && r[1].wrapping_add(r[3]) > mouse_y
            {
                return ((c >> 2) as i16) + 1;
            }
            c = c.wrapping_add(4);
        }
        0
    }

    /// Draws a button and returns `(slot index, rect [x, y, w, h])`.
    ///
    /// `w != 0` (bevelled panel, registered in a slot): slot = `{x, (y-2) as u8,
    /// w+2, (h+2) as u8}`; optionally saves `(w+4) x (h+5)` screen pixels from
    /// `(x, y-2)`; fills shadows `(x+2, y+h-1)-(x+w+2, y+h+2)` and
    /// `(x+w, y)-(x+w+2, y+h-2)` in colour 0, the face `(x, y)-(x+w, y+h)` in `color`,
    /// then lines top (2), right (7), left (1), bottom (6) with byte y coordinates.
    ///
    /// `w == 0` (icon button 50x41, not registered): `h` is the icon code with flags
    /// 0x2000 (transparent frame, no icon), 0x4000 (no frame), 0x8000 (second frame
    /// picture), 0x1000 (grey backdrop for code 0x29); the low byte selects the icon.
    ///
    /// Origin: BMMAIN.EXE 16c7:00a7 (ui_draw_button)
    #[allow(clippy::too_many_arguments)]
    pub fn draw_button(
        &mut self,
        g: &mut Gfx,
        rp: &mut RastPort,
        x: i16,
        y: i16,
        w: i16,
        h: i16,
        save_bg: bool,
        color: u8,
    ) -> (u8, [i16; 4]) {
        let mut i: usize = 0;
        while self.slots[i].w != 0 {
            if i >= BUTTON_SLOTS {
                break;
            }
            i += 1;
        }
        self.slots[i].pixel = g.rastport_get_pixel(rp, x.wrapping_add(2), y.wrapping_add(2)) as u8;
        if w != 0 {
            let sy = (y as u8).wrapping_sub(2);
            let sw = w.wrapping_add(2);
            let sh = (h as u8).wrapping_add(2);
            let rect = [x, sy as i16, sw, sh as i16];
            self.slots[i].x = x;
            self.slots[i].y = sy;
            self.slots[i].w = sw;
            self.slots[i].h = sh;
            if save_bg {
                let tmp = g.alloc(Bitmap::new_mem(w.wrapping_add(5), h.wrapping_add(5)));
                g.blit_bitmap(
                    self.screen,
                    x,
                    sy as i16,
                    tmp,
                    0,
                    0,
                    sw.wrapping_add(2),
                    sh as i16 + 3,
                );
                self.slots[i].saved = Some(tmp);
            }
            Gfx::rastport_set_color(rp, 0);
            let yh = y.wrapping_add(h);
            let xw = x.wrapping_add(w);
            g.rastport_fill_rect(
                rp,
                x.wrapping_add(2),
                yh.wrapping_sub(1),
                xw.wrapping_add(2),
                yh.wrapping_add(2),
            );
            g.rastport_fill_rect(rp, xw, y, xw.wrapping_add(2), yh.wrapping_sub(2));
            Gfx::rastport_set_color(rp, color);
            g.rastport_fill_rect(rp, x, y, xw, yh);
            let yb = y as u8;
            let yhb = (h as u8).wrapping_add(y as u8);
            ui_draw_line(g, rp, x, yb, xw, yb, 2);
            ui_draw_line(g, rp, xw, yb, xw, yhb, 7);
            ui_draw_line(g, rp, x, yb, x, yhb, 1);
            ui_draw_line(g, rp, x, yhb, xw, yhb, 6);
            return (i as u8, rect);
        }
        let rect = [x, y, 0x32, 0x29];
        self.slots[i].w = 0;
        let no_frame = h & 0x4000 != 0;
        let transparent_frame = h & 0x2000 != 0;
        let frame = self.frames[((h >> 15) & 1) as usize];
        if transparent_frame {
            if let Some(f) = frame {
                g.blit_bitmap_transparent(f, 0, 0, rp.bitmap, x, y, 0x32, 0x29);
            }
        } else if !no_frame && let Some(f) = frame {
            g.blit_bitmap(f, 0, 0, rp.bitmap, x, y, 0x32, 0x29);
        }
        let orig = h & 0xff;
        let mut code = orig;
        if code == 0x22 || code == 0x23 {
            code += 1;
        }
        if code == 100 {
            code = 0x22;
        }
        if code == 0x96 {
            code = 0x26;
        }
        let isy = (code / 6) * 0x18;
        let isx = (code % 6) * 0x21;
        if transparent_frame {
            return (i as u8, rect);
        }
        let (ix, iy) = (x.wrapping_add(9), y.wrapping_add(9));
        if orig == 0x29 {
            if h & 0x1000 != 0 {
                ui_fill_rect(g, rp, ix, iy, x.wrapping_add(0x28), y.wrapping_add(0x1f), 8);
            }
            if let Some(ic) = self.icons {
                g.blit_bitmap_transparent(ic, isx, isy, rp.bitmap, ix, iy, 0x20, 0x17);
            }
        } else if orig == 0xc8 {
            if let Some(p) = self.picture_37 {
                g.blit_bitmap(p, 0, 0x1f, rp.bitmap, ix, iy, 0x20, 0x15);
            }
        } else if code == 0x23 {
            if let Some(p) = self.icon_alt {
                g.blit_bitmap(p, 0, 0, rp.bitmap, ix, iy, 0x20, 0x17);
            }
        } else if let Some(ic) = self.icons {
            g.blit_bitmap(ic, isx, isy, rp.bitmap, ix, iy, 0x20, 0x17);
        }
        (i as u8, rect)
    }

    /// Removes registered buttons: slot `index` (0x7F = all 20) whose stored width
    /// equals `w_filter` (0 = any). Without a saved background the face
    /// `(x, y)-(x+w, y+h)` is filled with 0 and, if the remembered pixel is non-zero,
    /// `(w+2) x (h+4)` is restored from the work bitmap (VRAM page 2); with one, it is
    /// blitted back `(w+1) x (h+3)` and freed. The slot is then cleared.
    ///
    /// Origin: BMMAIN.EXE 16c7:0563 (ui_redraw_buttons)
    pub fn redraw_buttons(&mut self, g: &mut Gfx, rp: &mut RastPort, index: u8, w_filter: i16) {
        let (lo, hi) = if index == 0x7f {
            (0i8, 0x13i8)
        } else {
            (index as i8, index as i8)
        };
        let mut i = lo;
        while (hi as i16 + 1) > i as i16 {
            let idx = i as isize;
            if !(0..=BUTTON_SLOTS as isize).contains(&idx) {
                i = i.wrapping_add(1);
                continue;
            }
            let s = self.slots[idx as usize];
            if s.w != 0 && (s.w == w_filter || w_filter == 0) {
                match s.saved {
                    None => {
                        ui_fill_rect(
                            g,
                            rp,
                            s.x,
                            s.y as i16,
                            s.w.wrapping_add(s.x),
                            s.h as i16 + s.y as i16,
                            0,
                        );
                        if s.pixel != 0
                            && let Some(work) = self.work
                        {
                            g.blit_bitmap_or_restore_page2(
                                work,
                                s.x,
                                s.y as i16,
                                self.screen,
                                s.x,
                                s.y as i16,
                                s.w.wrapping_add(2),
                                s.h as i16 + 4,
                            );
                        }
                    }
                    Some(b) => {
                        g.blit_bitmap(
                            b,
                            0,
                            0,
                            rp.bitmap,
                            s.x,
                            s.y as i16,
                            s.w.wrapping_add(1),
                            s.h as i16 + 3,
                        );
                        g.free(b);
                    }
                }
                let sl = &mut self.slots[idx as usize];
                sl.w = 0;
                sl.saved = None;
            }
            i = i.wrapping_add(1);
        }
    }
}

/// Fill `(x0, y0)-(x1, y1)` in `fill`, then outline it in `frame`.
///
/// Origin: BMMAIN.EXE 16c7:0701 (ui_draw_filled_frame)
#[allow(clippy::too_many_arguments)]
pub fn ui_draw_filled_frame(
    g: &mut Gfx,
    rp: &mut RastPort,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    frame: u8,
    fill: u8,
) {
    Gfx::rastport_set_color(rp, fill);
    g.rastport_fill_rect(rp, x0, y0, x1, y1);
    ui_draw_frame(g, rp, x0, y0, x1, y1, frame);
}

/// Outline via move_to/line_to: `(x0,y0)->(x1,y0)->(x1,y1)->(x0,y1)->(x0,y0)`.
///
/// Origin: BMMAIN.EXE 16c7:0753 (ui_draw_frame)
pub fn ui_draw_frame(
    g: &mut Gfx,
    rp: &mut RastPort,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    color: u8,
) {
    Gfx::rastport_set_color(rp, color);
    Gfx::rastport_move_to(rp, x0, y0);
    g.rastport_line_to(rp, x1, y0);
    g.rastport_line_to(rp, x1, y1);
    g.rastport_line_to(rp, x0, y1);
    g.rastport_line_to(rp, x0, y0);
}

/// Draw `text` with the baseline at `(x, y)` in `color` (0 becomes 0xFF).
///
/// Origin: BMMAIN.EXE 16c7:07d5 (ui_draw_text_at)
pub fn ui_draw_text_at(g: &mut Gfx, rp: &mut RastPort, text: &[u8], x: i16, y: i16, color: i16) {
    let c = if color == 0 { 0xff } else { color };
    Gfx::rastport_set_color(rp, c as u8);
    Gfx::rastport_move_to(rp, x, y);
    g.rastport_draw_text(rp, text);
}

/// Fill the inclusive rectangle in `color` (XOR in mode 2).
///
/// Origin: BMMAIN.EXE 16c7:083c (ui_fill_rect)
pub fn ui_fill_rect(g: &mut Gfx, rp: &mut RastPort, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    Gfx::rastport_set_color(rp, color);
    g.rastport_fill_rect(rp, x0, y0, x1, y1);
}

/// Centered text: `x = ((x1 - x0) / 2 - width / 2) + x0 + 1` (C division toward 0).
/// If `shadow != 0xFF` the shadow (0 becomes 0xFF) is drawn at `(x, y)`. The text
/// (colour 0 becomes 0xFF) is drawn at `(x - 1, y)`, or at `(0, y)` when
/// `x + width > 319` or `x < 0`.
///
/// Origin: BMMAIN.EXE 16c7:087c (ui_draw_text_centered_shadow)
#[allow(clippy::too_many_arguments)]
pub fn ui_draw_text_centered_shadow(
    g: &mut Gfx,
    rp: &mut RastPort,
    text: &[u8],
    x0: i16,
    x1: i16,
    y: i16,
    color: u8,
    shadow: u8,
) {
    let tw = g.rastport_text_width(rp, text);
    let mut x = (x1.wrapping_sub(x0) / 2)
        .wrapping_sub(tw / 2)
        .wrapping_add(x0)
        .wrapping_add(1);
    if shadow != 0xff {
        let s = if shadow == 0 { 0xff } else { shadow };
        Gfx::rastport_set_color(rp, s);
        Gfx::rastport_move_to(rp, x, y);
        g.rastport_draw_text(rp, text);
    }
    let c = if color == 0 { 0xff } else { color };
    Gfx::rastport_set_color(rp, c);
    let tw = g.rastport_text_width(rp, text);
    if tw.wrapping_add(x) > 0x13f || x < 0 {
        x = 1;
    }
    Gfx::rastport_move_to(rp, x.wrapping_sub(1), y);
    g.rastport_draw_text(rp, text);
}

/// Line `(x0, y0)-(x1, y1)` in `color`; the y coordinates are bytes (0..255).
///
/// Origin: BMMAIN.EXE 16c7:09b0 (ui_draw_line)
pub fn ui_draw_line(g: &mut Gfx, rp: &mut RastPort, x0: i16, y0: u8, x1: i16, y1: u8, color: u8) {
    Gfx::rastport_move_to(rp, x0, y0 as i16);
    Gfx::rastport_set_color(rp, color);
    g.rastport_line_to(rp, x1, y1 as i16);
}

/// Copy-protection trap: when the protection was not passed (`DS:05AC == 0`) and
/// `random_range(0, 100) == 0`, the original loops forever calling
/// `screen_fade(random_range(0, 1))`. Returns true when that hang starts (the
/// caller must then freeze); `random_range` is consumed exactly as in the original
/// (one call, or none when the protection was passed).
///
/// Origin: BMMAIN.EXE 16c7:09f9 (copy_protection_penalty)
pub fn copy_protection_penalty(
    protection_passed: bool,
    mut random_range: impl FnMut(i16, i16) -> i16,
) -> bool {
    !protection_passed && random_range(0, 100) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gfx_with_screen() -> (Gfx, RastPort) {
        let mut g = Gfx::new();
        let mut rp = g.screen_open();
        Gfx::rastport_set_draw_mode(&mut rp, 0);
        (g, rp)
    }

    fn screen_px(g: &Gfx, x: i16, y: i16) -> u8 {
        g.page_pixel(if g.draw_page != 0 { PAGE_SIZE } else { 0 }, x, y)
    }

    #[test]
    fn mode_x_addressing() {
        let (mut g, rp) = gfx_with_screen();
        g.put_pixel(5, 3, rp.bitmap, 9);
        assert_eq!(g.planes[1][3 * 80 + 1], 9);
        assert_eq!(g.get_pixel(5, 3, rp.bitmap), 9);
        assert_eq!(g.get_pixel(-1, 3, rp.bitmap), 0xffff);
        assert_eq!(g.get_pixel(320, 3, rp.bitmap), 0xffff);
        g.vga_set_draw_page(2);
        g.put_pixel(5, 3, rp.bitmap, 7);
        assert_eq!(g.planes[1][(PAGE_SIZE + 3 * 80 + 1) as usize], 7);
    }

    #[test]
    fn fill_rect_clips_and_is_exact() {
        let (mut g, rp) = gfx_with_screen();
        g.fill_rect(rp.bitmap, -3, 1, 6, 2, 4);
        for y in 0..4 {
            for x in 0..6 {
                let want = if (1..3).contains(&y) && x < 3 { 4 } else { 0 };
                assert_eq!(screen_px(&g, x, y), want, "{x},{y}");
            }
        }
        g.fill_rect(rp.bitmap, 318, 238, 10, 10, 5);
        assert_eq!(screen_px(&g, 319, 239), 5);
        assert_eq!(screen_px(&g, 317, 239), 0);
        // Single-group case inside one 4-pixel byte.
        g.fill_rect(rp.bitmap, 9, 10, 2, 1, 6);
        assert_eq!(
            (8..12).map(|x| screen_px(&g, x, 10)).collect::<Vec<_>>(),
            vec![0, 6, 6, 0]
        );
        g.fill_rect_xor(rp.bitmap, 8, 10, 3, 1);
        assert_eq!(
            (8..12).map(|x| screen_px(&g, x, 10)).collect::<Vec<_>>(),
            vec![0x1f, 6 ^ 0x1f, 6 ^ 0x1f, 0]
        );
    }

    #[test]
    fn rastport_fill_is_inclusive() {
        let (mut g, mut rp) = gfx_with_screen();
        ui_fill_rect(&mut g, &mut rp, 2, 2, 4, 3, 1);
        let n = (0..10)
            .flat_map(|y| (0..10).map(move |x| (x, y)))
            .filter(|&(x, y)| screen_px(&g, x, y) == 1)
            .count();
        assert_eq!(n, 3 * 2);
    }

    #[test]
    fn teamed_rows_beyond_255_and_the_scrolled_page() {
        let mut g = Gfx::new();
        // BMMAIN's library wraps the start row at 256 (MUL BL).
        let bm = g.alloc(Bitmap::vram(SCREEN_W, 300, 0));
        g.fill_rect(bm, 0, 260, 4, 1, 9);
        assert_eq!(g.page_pixel(0, 0, 4), 9);
        let mut g = Gfx::new();
        g.vga_set_mode_x_teamed();
        let bm = g.pointer.screen;
        g.fill_rect(bm, 0, 300, 4, 1, 9);
        assert_eq!(g.page_pixel(0, 0, 300), 9);
        assert_eq!(g.page_pixel(0, 0, 44), 0);
        g.vga_show_page_teamed(3);
        assert_eq!(g.crtc_start, TEAMED_SCROLLED_START);
        // Row 300 is visible line 213 of the scrolled view.
        assert_eq!(g.page_pixel(g.crtc_start, 0, 300 - 87), 9);
        assert_eq!(g.mouse_clamp_y(20), 87);
        assert_eq!(g.mouse_clamp_y(400), 328);
        assert_eq!(g.mouse_window_y(10), 97);
        g.vga_show_page_teamed(1);
        assert_eq!((g.crtc_start, g.mouse_window_y(10)), (0, 10));
        assert_eq!(g.mouse_clamp_y(300), 0xf1);
    }

    #[test]
    fn teamed_bitmap_origin_offsets_rastport_and_blits() {
        let mut g = Gfx::new();
        g.vga_set_mode_x_teamed();
        assert_eq!(g.visible_lines, TEAMED_VISIBLE_LINES);
        let mut view = Bitmap::vram(SCREEN_W, TEAMED_SCREEN_H, 0);
        view.origin = (2, 75);
        let view = g.alloc(view);
        let mut rp = RastPort::new(view);
        Gfx::rastport_set_color(&mut rp, 7);
        g.rastport_fill_rect(&rp, 0, 0, 0, 0);
        assert_eq!(g.page_pixel(0, 2, 75), 7);
        assert_eq!(g.rastport_get_pixel(&rp, 0, 0), 7);
        let src = g.alloc(Bitmap {
            width: 1,
            height: 1,
            storage: Storage::Mem(vec![9]),
            origin: (0, 0),
            blocks: None,
        });
        g.blit_bitmap(src, 0, 0, view, 1, 0, 1, 1);
        assert_eq!(g.page_pixel(0, 3, 75), 9);
        let mut vga = Vga::new();
        g.to_vga(&mut vga);
        assert_eq!(vga.height(), 242);
    }

    #[test]
    fn blit_clipping_and_transparency() {
        let mut g = Gfx::new();
        let src = g.alloc(Bitmap {
            width: 3,
            height: 2,
            storage: Storage::Mem(vec![1, 0, 3, 4, 5, 0]),
            origin: (0, 0),
            blocks: None,
        });
        let dst = g.alloc(Bitmap {
            width: 4,
            height: 3,
            storage: Storage::Mem(vec![9; 12]),
            origin: (0, 0),
            blocks: None,
        });
        g.blit_rect_transparent(src, 0, 0, dst, -1, 1, 3, 2);
        let Storage::Mem(d) = &g.bitmap(dst).storage else {
            panic!()
        };
        assert_eq!(d, &vec![9, 9, 9, 9, 9, 3, 9, 9, 5, 9, 9, 9]);
        g.blit_rect(src, 1, 0, dst, 3, 0, 5, 5);
        let Storage::Mem(d) = &g.bitmap(dst).storage else {
            panic!()
        };
        assert_eq!(&d[..4], &[9, 9, 9, 0]);
        // Rejected: source origin outside.
        g.blit_rect(src, 3, 0, dst, 0, 0, 1, 1);
    }

    #[test]
    fn blit_to_and_from_screen() {
        let (mut g, rp) = gfx_with_screen();
        let img = VgaImage {
            width: 5,
            height: 2,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 0, 9, 10],
        };
        let spr = g.alloc(Bitmap::from_image(&img));
        g.blit_rect_transparent(spr, 0, 0, rp.bitmap, 2, 1, 5, 2);
        assert_eq!(
            (2..7).map(|x| screen_px(&g, x, 2)).collect::<Vec<_>>(),
            vec![6, 7, 0, 9, 10]
        );
        let back = g.alloc(Bitmap::new_mem(5, 2));
        g.blit_bitmap(rp.bitmap, 2, 1, back, 0, 0, 5, 2);
        let Storage::Mem(d) = &g.bitmap(back).storage else {
            panic!()
        };
        assert_eq!(d, &vec![1, 2, 3, 4, 5, 6, 7, 0, 9, 10]);
        // Screen to screen goes through a temporary buffer.
        g.blit_bitmap(rp.bitmap, 2, 1, rp.bitmap, 100, 50, 5, 2);
        assert_eq!(screen_px(&g, 104, 51), 10);
    }

    #[test]
    fn lines_match_bresenham_of_the_original() {
        let (mut g, rp) = gfx_with_screen();
        g.draw_line(0, 0, 4, 2, 1, rp.bitmap);
        let pts: Vec<_> = (0..5)
            .flat_map(|x| (0..3).map(move |y| (x, y)))
            .filter(|&(x, y)| screen_px(&g, x, y) == 1)
            .collect();
        // err += 2 per x step; y steps when err >= 4.
        assert_eq!(pts, vec![(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)]);
        g.draw_line(10, 10, 10, 10, 2, rp.bitmap);
        assert_eq!(screen_px(&g, 10, 10), 2);
        // XOR frame: corners are XORed twice and cancel.
        let mut rp2 = rp;
        Gfx::rastport_set_draw_mode(&mut rp2, 2);
        ui_draw_frame(&mut g, &mut rp2, 20, 20, 24, 23, 0);
        assert_eq!(screen_px(&g, 22, 20), 0x1f);
        assert_eq!(screen_px(&g, 20, 20), 0);
    }

    #[test]
    fn pages_and_copy() {
        let (mut g, rp) = gfx_with_screen();
        g.vga_show_page(0);
        assert_eq!((g.draw_page, g.crtc_start), (1, 0));
        g.fill_rect(rp.bitmap, 0, 0, 4, 1, 3);
        assert_eq!(g.page_pixel(PAGE_SIZE, 1, 0), 3);
        assert_eq!(g.page_pixel(0, 1, 0), 0);
        g.vga_show_page(0);
        assert_eq!((g.draw_page, g.crtc_start), (0, PAGE_SIZE));
        let mut vga = Vga::new();
        g.to_vga(&mut vga);
        assert_eq!(vga.pixels[1], 3);
        g.vga_copy_page(PAGE_SIZE, PAGE2_OFFSET);
        g.vga_restore_rect_from_page(320, 240, 1, 0, 2, 1, 3);
        assert_eq!(g.page_pixel(0, 1, 0), 3);
        assert_eq!(g.page_pixel(0, 0, 0), 0);
        g.vga_show_page(2);
        assert_eq!((g.draw_page, g.crtc_start), (1, PAGE_SIZE));
    }

    #[test]
    fn pointer_is_drawn_and_preserved() {
        let (mut g, rp) = gfx_with_screen();
        assert!(g.mouse_open(true, true));
        g.fill_rect(rp.bitmap, 0, 0, 320, 240, 7);
        g.mouse_pointer_show();
        assert_eq!(g.pointer.state, POINTER_VISIBLE);
        assert_eq!(screen_px(&g, 160, 120), 0x11);
        // Drawing under the pointer keeps the sprite and updates the saved background.
        g.put_pixel(160, 120, rp.bitmap, 9);
        assert_eq!(screen_px(&g, 160, 120), 0x11);
        assert_eq!(g.get_pixel(160, 120, rp.bitmap), 9);
        // Transparent sprite pixel: goes to VRAM.
        g.put_pixel(170, 120, rp.bitmap, 9);
        assert_eq!(screen_px(&g, 170, 120), 9);
        g.fill_rect(rp.bitmap, 150, 110, 30, 30, 4);
        assert_eq!(screen_px(&g, 160, 120), 0x11);
        assert_eq!(screen_px(&g, 170, 120), 4);
        g.mouse_event_handler(1, 20, 5);
        assert_eq!((g.pointer.x, g.pointer.y), (10, 5));
        assert_eq!(screen_px(&g, 160, 120), 4);
        assert_eq!(screen_px(&g, 10, 5), 0x11);
        g.mouse_event_handler(2 | 4, 20, 5);
        assert_eq!(g.mouse_pop_button_event(), 1);
        assert_eq!(g.mouse_pop_button_event(), 0);
        g.mouse_set_position(400, -3);
        assert_eq!((g.pointer.x, g.pointer.y), (319, 0));
        g.mouse_pointer_hide(0);
        assert_eq!(g.pointer.state, POINTER_HIDDEN);
        assert_eq!(screen_px(&g, 319, 0), 7);
    }

    #[test]
    fn lazy_hide_removes_on_overlap_only() {
        let (mut g, rp) = gfx_with_screen();
        g.mouse_open(true, false);
        g.mouse_pointer_show();
        g.mouse_pointer_hide(1);
        g.fill_rect(rp.bitmap, 0, 0, 10, 10, 3);
        assert_eq!(g.pointer.state, POINTER_LAZY);
        g.fill_rect(rp.bitmap, 150, 110, 30, 30, 4);
        assert_eq!(g.pointer.state, POINTER_HIDDEN);
        assert_eq!(screen_px(&g, 160, 120), 4);
        g.mouse_pointer_show();
        assert_eq!(screen_px(&g, 160, 120), 0x11);
    }

    #[test]
    fn busy_handler_still_latches_buttons() {
        let (mut g, _rp) = gfx_with_screen();
        g.mouse_open(true, true);
        let (x0, y0) = (g.pointer.x, g.pointer.y);
        g.busy = true;
        // Move + left down while busy: no move, but the click is latched.
        g.mouse_event_handler(1 | 2, 20, 5);
        assert_eq!((g.pointer.x, g.pointer.y), (x0, y0));
        assert_eq!(g.mouse_pop_button_event(), 1);
        g.busy = false;
        g.mouse_event_handler(1 | 8, 20, 5);
        assert_eq!((g.pointer.x, g.pointer.y), (10, 5));
        assert_eq!(g.mouse_pop_button_event(), 3);
    }

    #[test]
    fn pointer_buffer_stride_uses_height() {
        let (mut g, rp) = gfx_with_screen();
        g.mouse_open(true, true);
        // 4 wide x 2 high pointer, fully transparent.
        let img = g.alloc(Bitmap::new_mem(4, 2));
        let bg = g.alloc(Bitmap::new_mem(4, 2));
        g.mouse_set_pointer_shape(img, bg, 0, 0);
        g.mouse_set_position(100, 50);
        g.mouse_pointer_show();
        // Pixel (x+1, y+1) under the pointer: original index = 1*h + 1 = 3.
        g.put_pixel(101, 51, rp.bitmap, 9);
        let Storage::Mem(ref b) = g.bitmap(bg).storage else {
            panic!("background must be linear");
        };
        assert_eq!(b[3], 9);
        assert_eq!(b[5], 0);
        assert_eq!(g.get_pixel(101, 51, rp.bitmap), 9);
    }

    #[test]
    fn hit_test_is_strict() {
        let rects = [[10, 10, 20, 10], [40, 0, 5, 5]];
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 11, 11, None), 1);
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 10, 11, None), 0);
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 30, 11, None), 0);
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 42, 2, None), 2);
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 42, 2, Some(2)), 0);
        assert_eq!(Ui::hit_test_buttons(&rects, 2, 42, 2, Some(1)), 2);
    }

    #[test]
    fn panel_button_registers_and_restores() {
        let (mut g, mut rp) = gfx_with_screen();
        g.fill_rect(rp.bitmap, 0, 0, 320, 240, 5);
        let mut ui = Ui::new(rp.bitmap);
        let (slot, rect) = ui.draw_button(&mut g, &mut rp, 20, 30, 40, 10, true, 9);
        assert_eq!(slot, 0);
        assert_eq!(rect, [20, 28, 42, 12]);
        assert_eq!(ui.slots[0].pixel, 5);
        assert_eq!(screen_px(&g, 30, 30), 2); // top edge
        assert_eq!(screen_px(&g, 60, 35), 7); // right edge
        assert_eq!(screen_px(&g, 20, 35), 1); // left edge
        assert_eq!(screen_px(&g, 30, 40), 6); // bottom edge
        assert_eq!(screen_px(&g, 30, 35), 9); // face
        assert_eq!(screen_px(&g, 61, 35), 0); // right shadow
        assert_eq!(screen_px(&g, 30, 42), 0); // bottom shadow
        let (slot2, _) = ui.draw_button(&mut g, &mut rp, 100, 30, 10, 10, false, 9);
        assert_eq!(slot2, 1);
        ui.redraw_buttons(&mut g, &mut rp, 0x7f, 42);
        assert_eq!(ui.slots[0].w, 0);
        assert_eq!(ui.slots[1].w, 12);
        for (x, y) in [(30, 30), (60, 35), (30, 42), (61, 35), (20, 28)] {
            assert_eq!(screen_px(&g, x, y), 5, "{x},{y}");
        }
    }

    #[test]
    fn centered_text_position() {
        let (mut g, mut rp) = gfx_with_screen();
        // Synthetic font: every glyph 3 wide, 2 high, descent 0, top row set.
        let glyph = bmp_formats::font::Glyph {
            width: 3,
            bits: vec![0xe0, 0x00],
        };
        g.fonts[1] = Some(Font {
            name: "micro4.font".into(),
            height: 2,
            descent: 0,
            glyphs: vec![glyph; 95],
        });
        rp.font = Some(1);
        // width 6; x = (100 - 0) / 2 - 3 + 0 + 1 = 48; text at 47, shadow at 48.
        ui_draw_text_centered_shadow(&mut g, &mut rp, b"AB", 0, 100, 10, 0, 4);
        // Baseline 10, top = 10 - (2 - 0 - 1) = 9.
        assert_eq!(screen_px(&g, 46, 9), 0);
        assert_eq!(screen_px(&g, 47, 9), 0xff);
        assert_eq!(screen_px(&g, 52, 9), 0xff);
        assert_eq!(screen_px(&g, 53, 9), 4);
        assert_eq!(screen_px(&g, 47, 10), 0);
        assert_eq!(g.text_width(1, b"AB"), 6);
        // Opaque mode writes the background.
        Gfx::rastport_set_draw_mode(&mut rp, 1);
        Gfx::rastport_set_bg_color(&mut rp, 3);
        ui_draw_text_at(&mut g, &mut rp, b"A", 200, 10, 8);
        assert_eq!(screen_px(&g, 200, 9), 8);
        assert_eq!(screen_px(&g, 200, 10), 3);
    }

    #[test]
    fn circle_points() {
        let (mut g, mut rp) = gfx_with_screen();
        Gfx::rastport_set_color(&mut rp, 1);
        g.draw_circle(&rp, 50, 50, 3);
        for (x, y) in [(50, 53), (53, 50), (47, 50), (50, 47), (52, 52), (48, 48)] {
            assert_eq!(screen_px(&g, x, y), 1, "{x},{y}");
        }
        assert_eq!(screen_px(&g, 50, 50), 0);
    }

    #[test]
    fn palette_conversions_and_fade() {
        let mut g = Gfx::new();
        g.palette_load_rgb4(&[0x0f84], 1);
        assert_eq!(g.dac[0], [0x3c, 0x20, 0x10]);
        assert_eq!(g.palette_get_rgb4(0), 0x0f84);
        let mut cols = [0u16; 32];
        cols[1] = 0x0f00;
        cols[2] = 0x0001;
        let frames = g.vga_set_palette_rgb4(&cols, true);
        assert_eq!(frames.len(), 30);
        assert_eq!(frames[0][1][0], 2); // 60 * 2 / 60 after two steps
        assert_eq!(frames[0][2][2], 0); // 4 * 2 / 60
        assert_eq!(frames[29][1][0], 0x3c);
        assert_eq!(frames[29][2][2], 4);
        let out = g.vga_set_palette_rgb4(&cols, false);
        assert_eq!(out[29][1][0], 0);
        assert_eq!(g.dac[1], [0, 0, 0]);
        // Palette slot in VRAM plane 1.
        g.planes[1][0xE100 + 3] = 5;
        g.planes[1][0xE100 + 4] = 6;
        g.planes[1][0xE100 + 5] = 7;
        g.palette_load_slot(1, 2);
        assert_eq!(g.dac[1], [5, 6, 7]);
        assert_eq!(g.palette[0x101], 6);
    }

    #[test]
    fn timer_compare() {
        let mut g = Gfx::new();
        g.bios_ticks = 0xfffe;
        g.delay_ticks(10);
        assert_eq!(g.deadline, 0x10001);
        assert!(!g.timer_deadline_reached());
        g.bios_ticks = 0x10001;
        assert!(g.timer_deadline_reached());
    }

    #[test]
    fn copy_protection_consumes_one_random_call() {
        let mut calls = 0;
        assert!(!copy_protection_penalty(true, |_, _| {
            calls += 1;
            0
        }));
        assert_eq!(calls, 0);
        assert!(copy_protection_penalty(false, |_, _| 0));
        assert!(!copy_protection_penalty(false, |_, _| 5));
    }

    /// An icon button blits the real 50x41 frame picture (24.VGA) opaquely.
    #[test]
    fn real_button_frame_picture() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let data = std::fs::read(dir.join(bmp_formats::vga::file_name(0x18))).expect("24.VGA");
        let img = bmp_formats::vga::parse(&data).expect("parse");
        let (mut g, mut rp) = gfx_with_screen();
        let mut ui = Ui::new(rp.bitmap);
        ui.frames[0] = Some(g.alloc(Bitmap::from_image(&img)));
        // Flag 0x4000 off, 0x2000 off: opaque frame; code 0x29 without icons.
        let (_, rect) = ui.draw_button(&mut g, &mut rp, 30, 40, 0, 0x29, false, 0);
        assert_eq!(rect, [30, 40, 0x32, 0x29]);
        let w = (img.width as i16).min(0x32);
        let h = (img.height as i16).min(0x29);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    screen_px(&g, 30 + x, 40 + y),
                    img.pixel(x as u16, y as u16).unwrap(),
                    "{x},{y}"
                );
            }
        }
    }

    /// Without the mod flag the euro code is v1.0.0's: in every font it draws the same
    /// pixels as a space, transparently and opaquely, and has the space's width.
    #[test]
    fn euro_code_is_a_space_without_the_mod() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let set = FontSet::load(&dir).expect("fonts");
        for font in 1..=3u16 {
            for mode in [0, 1] {
                let draw = |text: &[u8]| {
                    let (mut g, rp) = gfx_with_screen();
                    g.set_fonts(&set);
                    g.draw_text(rp.bitmap, 10, 20, font, 15, 4, mode, text);
                    let x = g.draw_text(rp.bitmap, 40, 60, font, 15, 4, mode, text);
                    let px: Vec<u8> = (0..200)
                        .flat_map(|y| (0..320).map(move |x| (x, y)))
                        .map(|(x, y)| screen_px(&g, x, y))
                        .collect();
                    (x, g.text_width(font, text), px)
                };
                assert!(draw(&[EURO]) == draw(b" "), "font {font} mode {mode}");
            }
        }
    }

    /// MOD2026: with the euro glyphs installed the euro code draws the mod's glyph, and
    /// every other byte still draws v1.0.0's glyph.
    #[test]
    fn euro_code_draws_the_mod_glyph() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let set = FontSet::load(&dir).expect("fonts");
        for id in bmp_formats::font::FontId::ALL {
            let font = id as u16;
            let f = set.get(id);
            let euro = bmp_formats::font::euro_glyph(id);
            let (mut g, rp) = gfx_with_screen();
            g.set_fonts(&set);
            g.set_euro_glyphs();
            assert_eq!(g.text_width(font, &[EURO]), euro.width as u16);
            assert_eq!(g.text_width(font, b"1 DM"), f.text_length(b"1 DM"));
            let top = 20 - (f.height - f.descent - 1) as i16;
            let x = g.draw_text(rp.bitmap, 10, 20, font, 15, 4, 1, &[EURO]);
            assert_eq!(x, 10 + euro.width as i16);
            for row in 0..f.height as usize {
                for col in 0..euro.width as usize {
                    let want = if euro.pixel(col, row) { 15 } else { 4 };
                    let got = screen_px(&g, 10 + col as i16, top + row as i16);
                    assert_eq!(got, want, "{} {col},{row}", f.name);
                }
            }
        }
    }

    /// Writes the euro glyph sample (`1.000.000 DM` and `1.000.000 €` without the mod on
    /// the left, `1.000.000 €` with it on the right, in micro4, flow and newbold) as a PNG,
    /// scaled 3x, to `$BMP_EURO_SAMPLE_OUT` when that is set (evidence aid).
    #[test]
    fn render_euro_sample() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let Some(out) = std::env::var_os("BMP_EURO_SAMPLE_OUT") else {
            return;
        };
        let set = FontSet::load(&dir).expect("fonts");
        let (mut off, rp_off) = gfx_with_screen();
        off.set_fonts(&set);
        let (mut on, rp_on) = gfx_with_screen();
        on.set_fonts(&set);
        on.set_euro_glyphs();
        let euro = bmp_formats::font::encode_cp437("1.000.000 €");
        for (font, y, gap) in [(1u16, 10i16, 9i16), (2, 33, 11), (3, 68, 19)] {
            off.draw_text(rp_off.bitmap, 6, y, font, 15, 0, 0, b"1.000.000 DM");
            off.draw_text(rp_off.bitmap, 6, y + gap, font, 14, 0, 0, &euro);
            on.draw_text(rp_on.bitmap, 166, y, font, 10, 0, 0, &euro);
        }
        let (w, h, scale) = (320usize, 92usize, 3usize);
        let mut rgb = Vec::with_capacity(w * h * scale * scale * 3);
        for y in 0..h * scale {
            for x in 0..w * scale {
                let (px, py) = ((x / scale) as i16, (y / scale) as i16);
                let g = if px < 160 { &off } else { &on };
                rgb.extend_from_slice(match screen_px(g, px, py) {
                    0 if px == 160 => &[96, 96, 96],
                    0 => &[0, 0, 48],
                    10 => &[96, 255, 96],
                    14 => &[255, 255, 96],
                    _ => &[255, 255, 255],
                });
            }
        }
        crate::video::write_rgb_png(
            std::path::Path::new(&out),
            (w * scale) as u32,
            (h * scale) as u32,
            &rgb,
        )
        .expect("write sample");
    }

    /// Text drawn through the library matches the font crate's reference renderer.
    #[test]
    fn real_fonts_match_reference_renderer() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let set = FontSet::load(&dir).expect("fonts");
        let (mut g, mut rp) = gfx_with_screen();
        g.set_fonts(&set);
        let h = g.font_find_by_name("flow.font").expect("flow");
        assert_eq!((h.index, h.height, h.descent), (2, 8, 1));
        assert!(g.font_find_by_name("flow").is_none());
        Gfx::rastport_set_font(&mut rp, Some(h));
        ui_draw_text_at(&mut g, &mut rp, b"Bundesliga 1993 \x81", 10, 20, 0x0f);
        let mut fb = vec![0u8; 320 * 200];
        set.get(bmp_formats::font::FontId::Flow).draw_text(
            &mut fb,
            10,
            20,
            0x0f,
            "Bundesliga 1993 ü",
        );
        for y in 0..30 {
            for x in 0..200 {
                assert_eq!(
                    screen_px(&g, x, y),
                    fb[y as usize * 320 + x as usize],
                    "{x},{y}"
                );
            }
        }
    }

    /// MOD2026: a screen with the fonts, the display layer on and, when `hires`, the 2x
    /// fonts installed. `None` without `BMP_DATA`.
    fn gfx_with_fonts(display: bool, hires: bool) -> Option<(Gfx, RastPort, FontSet)> {
        let set = FontSet::load(&bmp_formats::data_dir()?).expect("fonts");
        let (mut g, rp) = gfx_with_screen();
        g.set_fonts(&set);
        if display {
            g.display_on();
        }
        if hires {
            g.set_hires_fonts();
        }
        Some((g, rp, set))
    }

    fn page0_block(g: &Gfx, x: i16, y: i16) -> Block {
        g.display()
            .unwrap()
            .block(x as usize & 3, page_offset(0, x, y))
    }

    fn page0_frame(g: &Gfx) -> Vec<u8> {
        (0..240)
            .flat_map(|y| (0..320).map(move |x| (x, y)))
            .map(|(x, y)| screen_px(g, x, y))
            .collect()
    }

    /// The expected display block of 1x pixel `(col, row)` of a glyph with 2x glyph `big`.
    fn want_block(big: &Glyph, col: usize, row: usize, set: u8, clear: u8) -> Block {
        std::array::from_fn(|k| {
            if big.pixel(2 * col + k % 2, 2 * row + k / 2) {
                set
            } else {
                clear
            }
        })
    }

    const HIRES_SAMPLE: &str = "Bundesliga Manager 1993: ÄÖÜ äöü ß 0123456789";

    /// MOD2026: the 2x fonts change neither the pen, nor the text width, nor any logical
    /// pixel (needs `BMP_DATA`).
    #[test]
    fn hires_text_keeps_the_logical_frame_and_widths() {
        let text = bmp_formats::font::encode_cp437(HIRES_SAMPLE);
        for id in bmp_formats::font::FontId::ALL {
            let font = id as u16;
            for mode in [0, 1] {
                let draw = |display: bool| {
                    let (mut g, rp, set) = gfx_with_fonts(display, display)?;
                    g.fill_rect(rp.bitmap, 0, 0, 320, 240, 7);
                    let x = g.draw_text(rp.bitmap, 3, 40, font, 15, 4, mode, &text);
                    let width = g.text_width(font, &text);
                    assert_eq!(width, set.get(id).text_length(&text), "{}", id.name());
                    Some((x, width, page0_frame(&g)))
                };
                let Some(plain) = draw(false) else { return };
                assert!(draw(true) == Some(plain), "{} mode {mode}", id.name());
            }
        }
    }

    /// MOD2026: opaque text puts the 2x glyph into the display, set pixels in the
    /// foreground and clear ones in the background colour (needs `BMP_DATA`).
    #[test]
    fn opaque_text_draws_the_2x_glyph() {
        for id in bmp_formats::font::FontId::ALL {
            let Some((mut g, rp, set)) = gfx_with_fonts(true, true) else {
                return;
            };
            let f = set.get(id);
            let big = &bmp_formats::hires_font::glyphs(id)[map_char(b'A') as usize];
            let top = 30 - (f.height - f.descent - 1) as i16;
            g.draw_text(rp.bitmap, 10, 30, id as u16, 15, 4, 1, b"A");
            for row in 0..f.height as usize {
                for col in 0..f.glyphs[map_char(b'A') as usize].width as usize {
                    let (x, y) = (10 + col as i16, top + row as i16);
                    assert_eq!(
                        page0_block(&g, x, y),
                        want_block(big, col, row, 15, 4),
                        "{} {col},{row}",
                        id.name()
                    );
                }
            }
        }
    }

    /// MOD2026: transparent text sets only the display pixels of its 2x glyph, including
    /// those of 1x pixels it leaves clear in the logical frame (needs `BMP_DATA`).
    #[test]
    fn transparent_text_keeps_the_background_between_2x_pixels() {
        let mut grown = 0;
        for id in bmp_formats::font::FontId::ALL {
            let Some((mut g, rp, set)) = gfx_with_fonts(true, true) else {
                return;
            };
            let f = set.get(id);
            g.fill_rect(rp.bitmap, 0, 0, 320, 240, 7);
            let text = b"AVWO";
            g.draw_text(rp.bitmap, 10, 30, id as u16, 15, 0, 0, text);
            let top = 30 - (f.height - f.descent - 1) as i16;
            let mut x = 10;
            for &c in text {
                let orig = &f.glyphs[map_char(c) as usize];
                let big = &bmp_formats::hires_font::glyphs(id)[map_char(c) as usize];
                for row in 0..f.height as usize {
                    for col in 0..orig.width as usize {
                        let (px, py) = (x + col as i16, top + row as i16);
                        let logical = if orig.pixel(col, row) { 15 } else { 7 };
                        assert_eq!(screen_px(&g, px, py), logical);
                        let want = want_block(big, col, row, 15, 7);
                        assert_eq!(page0_block(&g, px, py), want, "{} {px},{py}", id.name());
                        grown += usize::from(logical == 7 && want != [7; 4]);
                    }
                }
                x += orig.width as i16;
            }
        }
        if bmp_formats::data_dir().is_some() {
            assert!(grown > 0, "no 2x pixel outside the 1x glyph was checked");
        }
    }

    /// MOD2026: the pointer saves and restores the display blocks under it, so passing
    /// over text keeps its 2x detail (needs `BMP_DATA`).
    #[test]
    fn pointer_round_trip_keeps_text_detail() {
        let Some((mut g, rp, _)) = gfx_with_fonts(true, true) else {
            return;
        };
        g.fill_rect(rp.bitmap, 0, 0, 320, 240, 7);
        g.draw_text(rp.bitmap, 20, 50, 3, 15, 0, 0, b"BUNDESLIGA");
        let area = |g: &Gfx| -> Vec<Block> {
            (30..60)
                .flat_map(|y| (15..140).map(move |x| (x, y)))
                .map(|(x, y)| page0_block(g, x, y))
                .collect()
        };
        let before = area(&g);
        assert!(before.iter().any(|b| b.iter().any(|&p| p != b[0])));
        assert!(g.mouse_open(true, true));
        g.mouse_event_handler(1, 2 * 30, 40);
        g.mouse_pointer_show();
        for x in (30..130).step_by(7) {
            g.mouse_event_handler(1, 2 * x, 42);
        }
        g.mouse_event_handler(1, 2 * 250, 200);
        g.mouse_pointer_hide(0);
        assert!(area(&g) == before);
    }

    /// MOD2026: text clipped at the screen edges draws like without the 2x fonts
    /// (needs `BMP_DATA`).
    #[test]
    fn clipped_text_with_hires_glyphs() {
        for (x, y) in [(-3, 20), (300, 120), (100, 238), (-30, 3)] {
            for mode in [0, 1] {
                let draw = |on: bool| {
                    let (mut g, rp, _) = gfx_with_fonts(on, on)?;
                    let x = g.draw_text(rp.bitmap, x, y, 3, 15, 4, mode, b"CLIPPED TEXT");
                    Some((x, page0_frame(&g)))
                };
                let Some(plain) = draw(false) else { return };
                assert!(draw(true) == Some(plain), "{x},{y} mode {mode}");
            }
        }
    }

    /// MOD2026: a glyph clipped at the left edge shows the 2x glyph of its visible
    /// columns (needs `BMP_DATA`).
    #[test]
    fn left_clipped_text_keeps_its_2x_glyph() {
        let mut checked = 0;
        for id in bmp_formats::font::FontId::ALL {
            let Some((mut g, rp, set)) = gfx_with_fonts(true, true) else {
                return;
            };
            let f = set.get(id);
            let w = f.glyphs[map_char(b'W') as usize].width as usize;
            let big = &bmp_formats::hires_font::glyphs(id)[map_char(b'W') as usize];
            let top = 30 - (f.height - f.descent - 1) as i16;
            g.draw_text(rp.bitmap, -2, 30, id as u16, 15, 4, 1, b"W");
            for row in 0..f.height as usize {
                for col in 2..w {
                    let (x, y) = (col as i16 - 2, top + row as i16);
                    assert_eq!(
                        page0_block(&g, x, y),
                        want_block(big, col, row, 15, 4),
                        "{} {col},{row}",
                        id.name()
                    );
                    checked += 1;
                }
            }
        }
        if bmp_formats::data_dir().is_some() {
            assert!(checked > 0, "no visible column checked");
        }
    }

    /// MOD2026: the euro sign has no 2x glyph yet; it is drawn as its 1x glyph doubled
    /// (#205; needs `BMP_DATA`).
    #[test]
    fn euro_has_no_hires_glyph_yet() {
        let Some((mut g, rp, set)) = gfx_with_fonts(true, true) else {
            return;
        };
        g.set_euro_glyphs();
        for id in bmp_formats::font::FontId::ALL {
            let f = set.get(id);
            let euro = bmp_formats::font::euro_glyph(id);
            let top = 100 - (f.height - f.descent - 1) as i16;
            g.draw_text(rp.bitmap, 10, 100, id as u16, 15, 4, 1, &[EURO]);
            for row in 0..f.height as i16 {
                for col in 0..euro.width as i16 {
                    let v = screen_px(&g, 10 + col, top + row);
                    assert_eq!(page0_block(&g, 10 + col, top + row), [v; 4]);
                }
            }
        }
    }
}
