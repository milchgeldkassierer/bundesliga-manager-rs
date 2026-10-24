//! Bitmap fonts and text rendering of `BMMAIN.EXE`.
//!
//! The game does not use the VGA BIOS font. Its three proportional bitmap fonts
//! (`micro4.font`, `flow.font`, `newbold.font`) are compiled into the data area of the
//! assembler graphics library segment (image segment `0x35ac`, Ghidra `45ac:xxxx` in the
//! unpacked program). Because `BMMAIN.EXE` is compressed with Microsoft EXEPACK and the
//! font data lies in the compressed part, the loader first unpacks the load image.
//!
//! See `docs/re/font.md` for the full specification. Ghidra addresses below refer to the
//! unpacked program `BMMAIN_UNPACKED.EXE` (load segment 0x1000, so Ghidra segment
//! `S` = image segment `S - 0x1000`).

use std::fmt;
use std::path::Path;

/// Image segment of the graphics library that holds the font tables (Ghidra `45ac`).
pub const FONT_SEGMENT: u32 = 0x35ac;
/// Offset (within [`FONT_SEGMENT`]) of the character translation table.
pub const CHAR_MAP_OFFSET: u16 = 0x03eb;
/// Offset of the font count word; the descriptor pointer table follows at +2.
pub const FONT_TABLE_OFFSET: u16 = 0x03fc;
/// Number of glyphs per font: character codes 0x20..=0x7e.
pub const GLYPH_COUNT: usize = 95;
/// Size of the expansion buffer at `45ac:0406` (it ends where the `newbold` descriptor
/// starts at `45ac:07ee`).
pub const GLYPH_BUFFER_SIZE: usize = 0x07ee - 0x0406;

/// MOD2026: the byte code of the euro sign in the mod's texts. It is 0xd5, where IBM code
/// page 858 (CP850 with the euro sign) puts it. In CP437 0xd5 is a box-drawing character the
/// game never prints, and it is none of the eight codes [`map_char`] translates, so v1.0.0
/// shows it as a space. Only text drawing with the mod flag maps it to [`euro_glyph`].
pub const EURO: u8 = 0xd5;

/// MOD2026: the euro glyphs of the three fonts as text art (`docs/mod2026/CHANGES.md`).
const EURO_GLYPHS: &str = include_str!("../data/euro_glyphs.txt");

/// Screen width used by [`Font::draw_text`].
pub const SCREEN_W: usize = 320;
/// Screen height used by [`Font::draw_text`].
pub const SCREEN_H: usize = 200;

/// Errors produced while loading the fonts.
#[derive(Debug)]
pub enum FontError {
    Io(std::io::Error),
    /// The executable is not an MZ file or not EXEPACK-compressed as expected.
    BadExe(&'static str),
    /// The font tables do not look like the ones of `BMMAIN.EXE`.
    BadFontTable(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Io(e) => write!(f, "I/O error: {e}"),
            FontError::BadExe(m) => write!(f, "bad executable: {m}"),
            FontError::BadFontTable(m) => write!(f, "bad font table: {m}"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<crate::exepack::ExepackError> for FontError {
    fn from(e: crate::exepack::ExepackError) -> Self {
        FontError::BadExe(e.0)
    }
}

impl From<std::io::Error> for FontError {
    fn from(e: std::io::Error) -> Self {
        FontError::Io(e)
    }
}

fn rd16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

/// One glyph: a 1-bit bitmap, MSB first, `ceil(width / 8)` bytes per row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// Advance width in pixels, which is also the bitmap width (spacing is built in).
    pub width: u8,
    /// `height * ceil(width / 8)` bitmap bytes.
    pub bits: Vec<u8>,
}

impl Glyph {
    /// Bytes per bitmap row.
    pub fn row_bytes(&self) -> usize {
        (self.width as usize).div_ceil(8)
    }

    /// Whether pixel (`col`, `row`) is set.
    pub fn pixel(&self, col: usize, row: usize) -> bool {
        let b = self.bits[row * self.row_bytes() + col / 8];
        b & (0x80 >> (col % 8)) != 0
    }
}

/// Font index as used by the graphics library (position in the table at `45ac:03fe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontId {
    /// `micro4.font`, 6 pixels high (library index 1, game handle slot 2).
    Micro4 = 1,
    /// `flow.font`, 8 pixels high (library index 2, game handle slot 1).
    Flow = 2,
    /// `newbold.font`, 16 pixels high, upper case only (library index 3, game slot 3).
    Newbold = 3,
}

impl FontId {
    /// All fonts that carry glyph data.
    pub const ALL: [FontId; 3] = [FontId::Micro4, FontId::Flow, FontId::Newbold];

    /// The file-like name stored in the descriptor.
    pub fn name(self) -> &'static str {
        match self {
            FontId::Micro4 => "micro4.font",
            FontId::Flow => "flow.font",
            FontId::Newbold => "newbold.font",
        }
    }
}

/// A font descriptor with its glyphs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    pub name: String,
    /// Glyph cell height in pixels (descriptor word +2).
    pub height: u16,
    /// Rows below the baseline (descriptor word +4).
    pub descent: u16,
    /// Glyphs for character codes 0x20..=0x7e (after [`map_char`]).
    pub glyphs: Vec<Glyph>,
}

/// Translation of a raw string byte to a glyph index (0..=94).
///
/// Codes below 0x20 become a space. Then an 8-entry pair table at `45ac:03eb`
/// substitutes CP437 German characters into the ASCII slots that hold their glyphs
/// (`0x5c`->`0x60`, `Ä`0x8e->`0x5b`, `Ö`0x99->`0x5c`, `Ü`0x9a->`0x5d`, `ä`0x84->`0x7b`,
/// `ö`0x94->`0x7c`, `ü`0x81->`0x7d`, `ß`0xe1->`0x7e`); a substituted code is used as is.
/// Any other code >= 0x7f becomes a space. The result is `code - 0x20`.
///
/// Origin: BMMAIN.EXE 45ac:16a0 (font_map_char_to_glyph)
pub fn map_char(c: u8) -> u8 {
    const PAIRS: [(u8, u8); 8] = [
        (0x5c, 0x60),
        (0x8e, 0x5b),
        (0x99, 0x5c),
        (0x9a, 0x5d),
        (0x84, 0x7b),
        (0x94, 0x7c),
        (0x81, 0x7d),
        (0xe1, 0x7e),
    ];
    let mut al = c;
    if al < 0x20 {
        al = 0x20;
    } else if let Some(&(_, to)) = PAIRS.iter().find(|&&(from, _)| from == al) {
        al = to;
    } else if al >= 0x7f {
        al = 0x20;
    }
    al.wrapping_sub(0x20)
}

/// Encodes a Rust string to the CP437 bytes the game stores its text in. Only the
/// characters the fonts can show are mapped (ASCII and the German umlauts/sharp s, and
/// the mod's euro sign to [`EURO`]); anything else becomes 0x7f, which renders as a space.
pub fn encode_cp437(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            '\u{20}'..='\u{7e}' => c as u8,
            // MOD2026: only fonts with the mod's euro glyph show it; v1.0.0 shows a space.
            '€' => EURO,
            'Ä' => 0x8e,
            'Ö' => 0x99,
            'Ü' => 0x9a,
            'ä' => 0x84,
            'ö' => 0x94,
            'ü' => 0x81,
            'ß' => 0xe1,
            _ => 0x7f,
        })
        .collect()
}

/// MOD2026: the euro glyph drawn for the mod in the style of font `id`
/// (`data/euro_glyphs.txt`). Its height is the font's cell height and the last row is the
/// descent, like the original glyphs.
pub fn euro_glyph(id: FontId) -> Glyph {
    let mut rows = Vec::new();
    let mut found = false;
    for line in EURO_GLYPHS.lines() {
        let line = line.trim();
        // Comments start with "# "; pixel rows hold only '#' and '.'.
        if line.is_empty() || line.starts_with("# ") {
            continue;
        }
        if let Some(name) = line.strip_prefix("font ") {
            if found {
                break;
            }
            found = name == id.name();
        } else if found {
            rows.push(line.as_bytes());
        }
    }
    assert!(!rows.is_empty(), "no euro glyph for {}", id.name());
    let width = rows[0].len();
    assert!(
        rows.iter()
            .all(|r| r.len() == width && r.iter().all(|p| b"#.".contains(p)))
            && (1..=0xff).contains(&width),
        "ragged euro glyph for {}",
        id.name()
    );
    let mut bits = vec![0u8; rows.len() * width.div_ceil(8)];
    for (y, row) in rows.iter().enumerate() {
        for (x, &p) in row.iter().enumerate() {
            if p == b'#' {
                bits[y * width.div_ceil(8) + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    Glyph {
        width: width as u8,
        bits,
    }
}

/// All glyph-carrying fonts of `BMMAIN.EXE`.
#[derive(Clone, Debug)]
pub struct FontSet {
    fonts: Vec<Font>,
}

impl FontSet {
    /// Loads the fonts from `<data_dir>/BMMAIN.EXE`.
    pub fn load(data_dir: &Path) -> Result<FontSet, FontError> {
        let exe = std::fs::read(data_dir.join("BMMAIN.EXE"))?;
        FontSet::from_exe(&exe)
    }

    /// Loads the fonts from the bytes of the (packed) `BMMAIN.EXE`.
    pub fn from_exe(exe: &[u8]) -> Result<FontSet, FontError> {
        let image = crate::exepack::unpack(exe)?;
        FontSet::from_image(&image)
    }

    /// Loads the fonts from an unpacked load image.
    ///
    /// Reads the descriptor pointer table at `45ac:03fc` exactly as
    /// `font_find_by_name` does and parses each descriptor
    /// (`+0` name pointer, `+2` height, `+4` descent, `+6` 95 glyph pointers).
    ///
    /// Origin: BMMAIN.EXE 45ac:2f16 (font_find_by_name)
    pub fn from_image(image: &[u8]) -> Result<FontSet, FontError> {
        let seg = (FONT_SEGMENT * 16) as usize;
        let s = image
            .get(seg..(seg + 0x10000).min(image.len()))
            .ok_or(FontError::BadFontTable("image too short"))?;
        let w =
            |off: u16| rd16(s, off as usize).ok_or(FontError::BadFontTable("pointer out of range"));
        let count = w(FONT_TABLE_OFFSET)?;
        if count != 4 {
            return Err(FontError::BadFontTable("expected 4 font descriptors"));
        }
        let mut fonts = Vec::with_capacity(3);
        for id in FontId::ALL {
            let desc = w(FONT_TABLE_OFFSET + 2 + 2 * id as u16)?;
            let name_ptr = w(desc)? as usize;
            let name_end = s[name_ptr..]
                .iter()
                .position(|&b| b == 0)
                .ok_or(FontError::BadFontTable("unterminated name"))?;
            let name = String::from_utf8_lossy(&s[name_ptr..name_ptr + name_end]).into_owned();
            if name != id.name() {
                return Err(FontError::BadFontTable("unexpected font name"));
            }
            let height = w(desc + 2)?;
            let descent = w(desc + 4)?;
            let mut glyphs = Vec::with_capacity(GLYPH_COUNT);
            for i in 0..GLYPH_COUNT as u16 {
                let p = w(desc + 6 + 2 * i)? as usize;
                let width = *s
                    .get(p)
                    .ok_or(FontError::BadFontTable("glyph out of range"))?;
                let len = (width as usize).div_ceil(8) * height as usize;
                let bits = s
                    .get(p + 1..p + 1 + len)
                    .ok_or(FontError::BadFontTable("glyph out of range"))?
                    .to_vec();
                glyphs.push(Glyph { width, bits });
            }
            fonts.push(Font {
                name,
                height,
                descent,
                glyphs,
            });
        }
        Ok(FontSet { fonts })
    }

    /// The font with the given library index.
    pub fn get(&self, id: FontId) -> &Font {
        &self.fonts[id as usize - 1]
    }
}

/// A destination bitmap: `width * height` bytes, one palette index per pixel.
pub struct Bitmap<'a> {
    pub width: i16,
    pub height: i16,
    pub pixels: &'a mut [u8],
}

/// Clipped rectangle copy from `src` (`src_w` x `src_h`, linear) into `dst`.
/// `transparent` skips source bytes equal to 0.
///
/// The clipping order and signed 16-bit arithmetic follow both blitters: negative
/// source origin, negative destination origin, origin outside either bitmap (reject),
/// then right/bottom trimming against the destination and then the source.
///
/// Origin: BMMAIN.EXE 45ac:20c8 (FUN_45ac_20c8, transparent) and 45ac:1d1c (FUN_45ac_1d1c, opaque)
#[allow(clippy::too_many_arguments)]
fn blit(
    src: &[u8],
    src_w: i16,
    src_h: i16,
    mut sx: i16,
    mut sy: i16,
    dst: &mut Bitmap<'_>,
    mut dx: i16,
    mut dy: i16,
    mut w: i16,
    mut h: i16,
    transparent: bool,
) {
    if sx < 0 {
        w = w.wrapping_add(sx);
        dx = dx.wrapping_sub(sx);
        sx = 0;
        if w <= 0 {
            return;
        }
    }
    if sy < 0 {
        h = h.wrapping_add(sy);
        dy = dy.wrapping_sub(sy);
        sy = 0;
        if h <= 0 {
            return;
        }
    }
    if dx < 0 {
        w = w.wrapping_add(dx);
        sx = sx.wrapping_sub(dx);
        dx = 0;
        if w <= 0 {
            return;
        }
    }
    if dy < 0 {
        h = h.wrapping_add(dy);
        sy = sy.wrapping_sub(dy);
        dy = 0;
        if h <= 0 {
            return;
        }
    }
    if !(sx < src_w && sy < src_h && dx < dst.width && dy < dst.height) {
        return;
    }
    let end = dx.wrapping_add(w);
    if dst.width < end {
        w = w.wrapping_add(dst.width.wrapping_sub(end));
    }
    let end = dy.wrapping_add(h);
    if dst.height < end {
        h = h.wrapping_add(dst.height.wrapping_sub(end));
    }
    let end = sx.wrapping_add(w);
    if src_w < end {
        w = w.wrapping_add(src_w.wrapping_sub(end));
    }
    let end = sy.wrapping_add(h);
    if src_h < end {
        h = h.wrapping_add(src_h.wrapping_sub(end));
    }
    if w <= 0 || h <= 0 {
        return;
    }
    for row in 0..h as usize {
        let s0 = (sy as usize + row) * src_w as usize + sx as usize;
        let d0 = (dy as usize + row) * dst.width as usize + dx as usize;
        for col in 0..w as usize {
            let v = src[s0 + col];
            if (!transparent || v != 0)
                && let Some(p) = dst.pixels.get_mut(d0 + col)
            {
                *p = v;
            }
        }
    }
}

impl Font {
    /// Sum of the advance widths of `text` (16-bit wrapping, no clipping).
    ///
    /// Origin: BMMAIN.EXE 45ac:2eb4 (font_text_length)
    pub fn text_length(&self, text: &[u8]) -> u16 {
        text.iter().fold(0u16, |acc, &c| {
            acc.wrapping_add(self.glyphs[map_char(c) as usize].width as u16)
        })
    }

    /// Draws `text` with its baseline at `y`, starting at `x`.
    ///
    /// The cell top is `y - (height - descent - 1)`. Each glyph is expanded into a
    /// `width * height` buffer with `fg` for set bits and, when `opaque`, `bg` for clear
    /// bits (otherwise 0), then blitted: transparently (zero bytes skipped) when not
    /// `opaque`, as an opaque copy otherwise. `x` advances by the glyph width. Drawing
    /// stops when the string is exhausted or before a glyph whose `x >= dst.width`.
    /// Returns the final pen x (the original routine does not return it).
    ///
    /// Origin: BMMAIN.EXE 45ac:2d83 (font_draw_text)
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_raw(
        &self,
        dst: &mut Bitmap<'_>,
        mut x: i16,
        y: i16,
        fg: u8,
        bg: u8,
        opaque: bool,
        text: &[u8],
    ) -> i16 {
        let top = y.wrapping_sub((self.height.wrapping_sub(self.descent).wrapping_sub(1)) as i16);
        let bg = if opaque { bg } else { 0 };
        let h = (self.height & 0xff) as usize;
        let mut buf = Vec::with_capacity(GLYPH_BUFFER_SIZE);
        for &c in text {
            if x >= dst.width {
                break;
            }
            let g = &self.glyphs[map_char(c) as usize];
            if g.width == 0 {
                // Not present in the data; the original would loop 256 pixels per row.
                continue;
            }
            let w = g.width as usize;
            buf.clear();
            for row in 0..h {
                for col in 0..w {
                    buf.push(if g.pixel(col, row) { fg } else { bg });
                }
            }
            blit(
                &buf, w as i16, h as i16, 0, 0, dst, x, top, w as i16, h as i16, !opaque,
            );
            x = x.wrapping_add(w as i16);
        }
        x
    }

    /// Draws `text` transparently in `color` onto a 320x200 framebuffer with the
    /// baseline at `y`. This is what the game's `print_text_at` does (with a rastport
    /// in transparent mode); note that caller maps color 0 to 0xff beforehand.
    ///
    /// Origin: BMMAIN.EXE 16c7:07d5 (print_text_at)
    pub fn draw_text(&self, fb: &mut [u8], x: i16, y: i16, color: u8, text: &str) -> i16 {
        let len = fb.len().min(SCREEN_W * SCREEN_H);
        let mut bm = Bitmap {
            width: SCREEN_W as i16,
            height: SCREEN_H as i16,
            pixels: &mut fb[..len],
        };
        self.draw_text_raw(&mut bm, x, y, color, 0, false, &encode_cp437(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Option<FontSet> {
        let dir = crate::data_dir()?;
        Some(FontSet::load(&dir).expect("fonts load"))
    }

    #[test]
    fn char_map() {
        assert_eq!(map_char(b' '), 0);
        assert_eq!(map_char(b'A'), 0x21);
        assert_eq!(map_char(0x05), 0);
        assert_eq!(map_char(0x7f), 0);
        assert_eq!(map_char(0xff), 0);
        assert_eq!(map_char(0x8e), 0x5b - 0x20);
        assert_eq!(map_char(0xe1), 0x7e - 0x20);
        assert_eq!(map_char(b'\\'), 0x60 - 0x20);
        assert_eq!(map_char(b'['), 0x5b - 0x20);
    }

    #[test]
    fn cp437() {
        assert_eq!(encode_cp437("Mühle ß"), b"M\x81hle \xe1".to_vec());
        assert_eq!(encode_cp437("1.000 €"), b"1.000 \xd5".to_vec());
    }

    /// `map_char` is v1.0.0's for every byte: the euro code stays a space without the mod.
    #[test]
    fn char_map_is_unchanged_for_every_byte() {
        const PAIRS: [(u8, u8); 8] = [
            (0x5c, 0x60),
            (0x8e, 0x5b),
            (0x99, 0x5c),
            (0x9a, 0x5d),
            (0x84, 0x7b),
            (0x94, 0x7c),
            (0x81, 0x7d),
            (0xe1, 0x7e),
        ];
        for c in 0..=0xffu8 {
            let want = match PAIRS.iter().find(|&&(from, _)| from == c) {
                Some(&(_, to)) => to - 0x20,
                None if (0x20..0x7f).contains(&c) => c - 0x20,
                None => 0,
            };
            assert_eq!(map_char(c), want, "{c:#04x}");
        }
        assert_eq!(map_char(EURO), map_char(b' '));
    }

    /// MOD2026: the euro code is free: not printable ASCII and none of the translated codes.
    #[test]
    fn euro_code_is_free() {
        assert!(!(0x20..=0x7e).contains(&EURO));
        assert!(![0x5c, 0x8e, 0x99, 0x9a, 0x84, 0x94, 0x81, 0xe1].contains(&EURO));
    }

    /// MOD2026: every font's euro glyph has the font's cell height (`docs/re/font.md`), a
    /// width within the font's original width range and at least one set pixel; its
    /// spacing column and descent row are blank like the original glyphs'.
    #[test]
    fn euro_glyphs_fit_their_fonts() {
        for (id, height, widths) in [
            (FontId::Micro4, 6, 2..=6),
            (FontId::Flow, 8, 3..=8),
            (FontId::Newbold, 16, 5..=23),
        ] {
            let g = euro_glyph(id);
            let w = g.width as usize;
            assert!(widths.contains(&g.width), "{}", id.name());
            assert_eq!(g.bits.len(), height * g.row_bytes(), "{}", id.name());
            let set = |col, row| g.pixel(col, row);
            assert!(
                (0..height).any(|r| (0..w).any(|c| set(c, r))),
                "{}",
                id.name()
            );
            assert!((0..height).all(|r| !set(w - 1, r)), "{}", id.name());
            assert!((0..w).all(|c| !set(c, height - 1)), "{}", id.name());
        }
        // The cell heights and width ranges above are the shipped fonts'.
        let Some(f) = fonts() else { return };
        for id in FontId::ALL {
            let font = f.get(id);
            let g = euro_glyph(id);
            assert_eq!(g.bits.len(), font.height as usize * g.row_bytes());
            let ws = font.glyphs.iter().map(|g| g.width);
            let (lo, hi) = (ws.clone().min().unwrap(), ws.max().unwrap());
            assert!((lo..=hi).contains(&g.width), "{}: {lo}..={hi}", id.name());
        }
    }

    #[test]
    fn metrics() {
        let Some(f) = fonts() else { return };
        let m = f.get(FontId::Micro4);
        assert_eq!((m.height, m.descent), (6, 1));
        let fl = f.get(FontId::Flow);
        assert_eq!((fl.height, fl.descent), (8, 1));
        let nb = f.get(FontId::Newbold);
        assert_eq!((nb.height, nb.descent), (16, 1));
        // Advance widths of a few glyphs taken from the data.
        assert_eq!(m.glyphs[0].width, 2);
        assert_eq!(fl.glyphs[(b'A' - 0x20) as usize].width, 7);
        assert_eq!(nb.glyphs[(b'W' - 0x20) as usize].width, 23);
        // newbold has no lower case: 'a' shares the bitmap of 'A'.
        assert_eq!(
            nb.glyphs[(b'a' - 0x20) as usize],
            nb.glyphs[(b'A' - 0x20) as usize]
        );
        assert_eq!(fl.text_length(b"AB"), 14);
    }

    #[test]
    fn draw_clips_and_advances() {
        let Some(f) = fonts() else { return };
        let fl = f.get(FontId::Flow);
        let mut fb = vec![0u8; SCREEN_W * SCREEN_H];
        let end = fl.draw_text(&mut fb, 10, 20, 15, "Hi");
        assert_eq!(end, 10 + fl.text_length(b"Hi") as i16);
        // All lit pixels are within the cell rows 20-6..=20+1 and use color 15.
        for (i, &p) in fb.iter().enumerate() {
            if p != 0 {
                let (px, py) = (i % SCREEN_W, i / SCREEN_W);
                assert_eq!(p, 15);
                assert!((14..=21).contains(&py) && (10..end as usize).contains(&px));
            }
        }
        assert!(fb.iter().any(|&p| p != 0));
        // Drawing off-screen must not panic and must clip.
        fl.draw_text(&mut fb, -5, 3, 1, "XYZ");
        fl.draw_text(&mut fb, 315, 199, 1, "XYZ");
        let end = fl.draw_text(&mut fb, 400, 50, 1, "X");
        assert_eq!(end, 400);
    }

    #[test]
    fn opaque_fills_background() {
        let Some(f) = fonts() else { return };
        let m = f.get(FontId::Micro4);
        let mut px = vec![9u8; 40 * 10];
        let mut bm = Bitmap {
            width: 40,
            height: 10,
            pixels: &mut px,
        };
        // Baseline 4: cell top = 4 - (6 - 1 - 1) = 0.
        m.draw_text_raw(&mut bm, 0, 4, 3, 0, true, b" ");
        // A space is all background: the 2x6 cell at rows 0..6 becomes 0.
        for row in 0..6 {
            assert_eq!(&px[row * 40..row * 40 + 2], &[0, 0]);
            assert_eq!(px[row * 40 + 2], 9);
        }
    }

    /// Writes a 320x200 raw indexed sample to `$BMP_FONT_SAMPLE_OUT` when that is set
    /// (inspection aid; converted to PNG by an external script).
    #[test]
    fn render_sample() {
        let Some(f) = fonts() else { return };
        let Some(out) = std::env::var_os("BMP_FONT_SAMPLE_OUT") else {
            return;
        };
        let mut fb = vec![0u8; SCREEN_W * SCREEN_H];
        let nb = f.get(FontId::Newbold);
        nb.draw_text(&mut fb, 4, 16, 15, "BUNDESLIGA MANAGER");
        nb.draw_text(&mut fb, 4, 34, 14, "ÄÖÜ ß 0123456789");
        let fl = f.get(FontId::Flow);
        fl.draw_text(&mut fb, 4, 50, 15, "Kickfelder Mühle - Spieltag 12: 3:1");
        fl.draw_text(&mut fb, 4, 60, 10, "ABCDEFGHIJKLMNOPQRSTUVWXYZ ÄÖÜ");
        fl.draw_text(&mut fb, 4, 70, 10, "abcdefghijklmnopqrstuvwxyz äöüß");
        fl.draw_text(&mut fb, 4, 80, 12, "0123456789 !\"#$%&'()*+,-./:;<=>?@");
        let m = f.get(FontId::Micro4);
        m.draw_text(
            &mut fb,
            4,
            92,
            15,
            "micro4: ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz",
        );
        m.draw_text(&mut fb, 4, 100, 15, "0123456789 ÄÖÜ äöüß !?.,:;()");
        let mut px = vec![1u8; 120 * 30];
        let mut bm = Bitmap {
            width: 120,
            height: 30,
            pixels: &mut px,
        };
        fl.draw_text_raw(&mut bm, 2, 10, 15, 4, true, b"opaque");
        for row in 0..30 {
            let d = (110 + row) * SCREEN_W + 4;
            fb[d..d + 120].copy_from_slice(&px[row * 120..row * 120 + 120]);
        }
        nb.draw_text(&mut fb, 300, 180, 15, "CLIPPED");
        std::fs::write(out, &fb).expect("write sample");
    }
}
