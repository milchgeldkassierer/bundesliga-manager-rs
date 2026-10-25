//! VGA palettes: `PIC/1.PAL`, the BMINTRO.EXE start-up palette, the 12-bit
//! "0x0RGB" colour words used by the palette routines, and the 32-colour fade.
//!
//! The full specification (with Ghidra addresses) is in `docs/re/pal.md`.
//! Addresses below refer to the EXEPACK-unpacked BMMAIN.EXE image as imported
//! in the shared Ghidra project (`/unpacked/BMMAIN_UNPACKED.EXE`, load segment
//! 0x1000).

use std::fmt;
use std::path::Path;

/// Number of DAC entries in a VGA palette.
pub const PALETTE_ENTRIES: usize = 256;
/// Size of one palette in bytes (256 x R,G,B).
pub const PALETTE_BYTES: usize = PALETTE_ENTRIES * 3;
/// Number of palettes stored in `PIC/1.PAL`.
pub const PAL_FILE_PALETTES: usize = 3;
/// Exact size of `PIC/1.PAL`.
pub const PAL_FILE_BYTES: usize = PALETTE_BYTES * PAL_FILE_PALETTES;
/// Path of the palette file relative to the game directory (the EXE uses `pic\1.pal`).
pub const PAL_FILE_PATH: &str = "PIC/1.PAL";

/// Palette index selected at start-up (title / first screen), DGROUP:0x0234 = 2.
pub const PALETTE_STARTUP: usize = 2;
/// Palette index used by most game screens, DGROUP:0x0794 = 0.
pub const PALETTE_MAIN: usize = 0;
/// Palette index used by the screen built in FUN_362f_2be8, DGROUP:0x0798 = 1.
pub const PALETTE_ALT: usize = 1;

/// Number of DAC entries affected by the fade routine.
pub const FADE_COLORS: usize = 32;
/// Number of accumulation steps of the fade routine.
pub const FADE_STEPS: usize = 60;
/// Number of visible DAC updates of one fade (one per two steps).
pub const FADE_FRAMES: usize = FADE_STEPS / 2;

/// Offset of the start-up palette inside BMINTRO.EXE's data segment (DS = 0x00BE).
pub const BMINTRO_PALETTE_DS_OFFSET: usize = 0x1382;
/// Load-image offset of BMINTRO.EXE's data segment (segment 0x00BE).
pub const BMINTRO_DATA_SEGMENT: usize = 0x00be;

/// Errors produced by the palette loaders.
#[derive(Debug)]
pub enum PalError {
    /// The input has the wrong length.
    BadLength { expected: usize, actual: usize },
    /// The input is not a valid MZ executable or too short.
    BadExecutable,
    /// I/O error while reading a file.
    Io(std::io::Error),
}

impl fmt::Display for PalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PalError::BadLength { expected, actual } => {
                write!(f, "bad palette length: expected {expected}, got {actual}")
            }
            PalError::BadExecutable => write!(f, "not a usable MZ executable"),
            PalError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for PalError {}

impl From<std::io::Error> for PalError {
    fn from(e: std::io::Error) -> Self {
        PalError::Io(e)
    }
}

/// Expands a 6-bit VGA DAC value to 8 bits exactly like DOSBox does:
/// `(v << 2) | (v >> 4)`. Only the low 6 bits of `v` are used, as on real hardware.
pub fn dac6_to_8(v: u8) -> u8 {
    let v = v & 0x3f;
    (v << 2) | (v >> 4)
}

/// A 256-entry VGA palette of 6-bit R, G, B values (0..=63).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    /// Entries as `[r, g, b]`, each 0..=63.
    pub entries: [[u8; 3]; PALETTE_ENTRIES],
}

impl Default for Palette {
    fn default() -> Self {
        Self::black()
    }
}

impl Palette {
    /// An all-black palette.
    pub fn black() -> Self {
        Palette {
            entries: [[0; 3]; PALETTE_ENTRIES],
        }
    }

    /// Builds a palette from 768 interleaved R,G,B bytes as they would be sent to
    /// the DAC data port. The DAC only latches the low 6 bits of every byte, so the
    /// values are masked with 0x3F (this matters for the BMINTRO palette, which
    /// contains 0xFF bytes).
    pub fn from_dac_bytes(bytes: &[u8]) -> Result<Self, PalError> {
        if bytes.len() != PALETTE_BYTES {
            return Err(PalError::BadLength {
                expected: PALETTE_BYTES,
                actual: bytes.len(),
            });
        }
        let mut p = Palette::black();
        for (i, e) in p.entries.iter_mut().enumerate() {
            for (c, v) in e.iter_mut().enumerate() {
                *v = bytes[i * 3 + c] & 0x3f;
            }
        }
        Ok(p)
    }

    /// Returns the palette as 768 interleaved R,G,B bytes (6-bit values).
    pub fn to_dac_bytes(&self) -> [u8; PALETTE_BYTES] {
        let mut out = [0u8; PALETTE_BYTES];
        for (i, e) in self.entries.iter().enumerate() {
            out[i * 3..i * 3 + 3].copy_from_slice(e);
        }
        out
    }

    /// Returns the palette in the game's planar shadow layout: R[256], G[256], B[256].
    ///
    /// Origin: BMMAIN.EXE 4930:05dd (pal_set_color) - layout of the shadow buffer
    /// written at offsets +0x000/+0x100/+0x200.
    pub fn to_planar(&self) -> [u8; PALETTE_BYTES] {
        let mut out = [0u8; PALETTE_BYTES];
        for (i, e) in self.entries.iter().enumerate() {
            out[i] = e[0];
            out[0x100 + i] = e[1];
            out[0x200 + i] = e[2];
        }
        out
    }

    /// Converts every entry to 8-bit RGB with the DOSBox expansion `(v << 2) | (v >> 4)`.
    pub fn to_rgb8(&self) -> [[u8; 3]; PALETTE_ENTRIES] {
        let mut out = [[0u8; 3]; PALETTE_ENTRIES];
        for (o, e) in out.iter_mut().zip(self.entries.iter()) {
            *o = [dac6_to_8(e[0]), dac6_to_8(e[1]), dac6_to_8(e[2])];
        }
        out
    }

    /// Sets one entry (6-bit values are stored unmasked, as the game does).
    ///
    /// Origin: BMMAIN.EXE 4930:05dd (pal_set_color)
    pub fn set_color(&mut self, index: u8, r: u8, g: u8, b: u8) {
        self.entries[index as usize] = [r, g, b];
    }

    /// Packs entry `index` into the game's 16-bit colour word,
    /// `((r << 4 | g) << 2) | (b >> 2)`. The fields overlap, so the result is only
    /// a clean 0x0RGB word when all three components are multiples of 4.
    ///
    /// Origin: BMMAIN.EXE 4930:06d5 (pal_get_color_12bit)
    pub fn get_color_word(&self, index: u8) -> u16 {
        let [r, g, b] = self.entries[index as usize];
        let mut ax = (r as u16) << 4;
        ax |= g as u16;
        ax = ax.wrapping_shl(2);
        ax | ((b >> 2) as u16)
    }

    /// Loads `words.len()` entries starting at 0 from 0x0RGB colour words.
    ///
    /// Origin: BMMAIN.EXE 4930:0624 (pal_set_palette), pointer branch
    pub fn set_from_words(&mut self, words: &[u16]) {
        for (i, &w) in words.iter().enumerate().take(PALETTE_ENTRIES) {
            let [r, g, b] = word_to_rgb6(w);
            self.entries[i] = [r, g, b];
        }
    }
}

/// Decodes a colour word the way `pal_set_palette` does:
/// `R = (w >> 6) & 0x3C`, `G = (w >> 2) & 0x3C`, `B = (w & 0x0F) << 2`.
///
/// Origin: BMMAIN.EXE 4930:0624 (pal_set_palette)
pub fn word_to_rgb6(w: u16) -> [u8; 3] {
    let r = ((w >> 6) as u8) & 0x3c;
    let g = ((w >> 2) as u8) & 0x3c;
    let b = ((w as u8) & 0x0f) << 2;
    [r, g, b]
}

/// Decodes a colour word the way the fade routine does (byte arithmetic on
/// AH/AL): `R = AH << 2`, `G = (AL & 0xF0) >> 2`, `B = (AL & 0x0F) << 2`.
/// Identical to [`word_to_rgb6`] for words below 0x1000.
///
/// Origin: BMMAIN.EXE 45ac:33ca (vga_fade_32_colors), conversion loop
pub fn fade_word_to_rgb6(w: u16) -> [u8; 3] {
    let ah = (w >> 8) as u8;
    let al = w as u8;
    [ah.wrapping_shl(2), (al & 0xf0) >> 2, (al & 0x0f) << 2]
}

/// The 16-entry grey ramp used before showing the photos 200..229.VGA
/// (DGROUP:0x9394). Entry `i` is the grey level `reverse4(i)` replicated into
/// all three nibbles (0x000, 0x888, 0x444, 0xCCC, ...).
///
/// Origin: BMMAIN.EXE 3e3a:0fdf (FUN_3e3a_0fdf) - table passed to pal_set_palette
pub fn grey_ramp_words() -> [u16; 16] {
    let mut out = [0u16; 16];
    for (i, o) in out.iter_mut().enumerate() {
        let r = ((i & 1) << 3) | ((i & 2) << 1) | ((i & 4) >> 1) | ((i & 8) >> 3);
        *o = (r as u16) * 0x111;
    }
    out
}

/// The contents of `PIC/1.PAL`: three consecutive 768-byte DAC palettes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PalFile {
    /// Palettes exactly as stored in the file.
    pub palettes: [Palette; PAL_FILE_PALETTES],
}

impl PalFile {
    /// Parses the 2304 bytes of `PIC/1.PAL`.
    pub fn parse(bytes: &[u8]) -> Result<Self, PalError> {
        if bytes.len() != PAL_FILE_BYTES {
            return Err(PalError::BadLength {
                expected: PAL_FILE_BYTES,
                actual: bytes.len(),
            });
        }
        let p0 = Palette::from_dac_bytes(&bytes[0..PALETTE_BYTES])?;
        let p1 = Palette::from_dac_bytes(&bytes[PALETTE_BYTES..2 * PALETTE_BYTES])?;
        let p2 = Palette::from_dac_bytes(&bytes[2 * PALETTE_BYTES..])?;
        Ok(PalFile {
            palettes: [p0, p1, p2],
        })
    }

    /// Reads and parses `PIC/1.PAL` below the given game directory.
    pub fn load(game_dir: &Path) -> Result<Self, PalError> {
        let bytes = std::fs::read(game_dir.join(PAL_FILE_PATH))?;
        Self::parse(&bytes)
    }

    /// Returns palette `index` as the game keeps it in VRAM: identical to the file
    /// except that entry 255 is forced to black (0, 0, 0).
    ///
    /// Origin: BMMAIN.EXE 18bc:0004 (gfx_init_load_palettes)
    pub fn vram_palette(&self, index: usize) -> Palette {
        let mut p = self.palettes[index];
        p.entries[255] = [0, 0, 0];
        p
    }
}

/// Extracts the 256-colour palette that BMINTRO.EXE programs (via ports
/// 0x3C8/0x3C9) right after switching to mode 13h. Bytes are masked to 6 bits as
/// the DAC does; entries 87..255 are stored as 0xFF and therefore become 63,63,63.
///
/// Origin: BMINTRO.EXE 0000:0aba (palette upload) called from 0000:0b39 (mode 13h init)
pub fn bmintro_palette(exe: &[u8]) -> Result<Palette, PalError> {
    if exe.len() < 0x1c || exe[0] != b'M' || exe[1] != b'Z' {
        return Err(PalError::BadExecutable);
    }
    let header_paragraphs = u16::from_le_bytes([exe[8], exe[9]]) as usize;
    let start = header_paragraphs * 16 + BMINTRO_DATA_SEGMENT * 16 + BMINTRO_PALETTE_DS_OFFSET;
    let bytes = exe
        .get(start..start + PALETTE_BYTES)
        .ok_or(PalError::BadExecutable)?;
    Palette::from_dac_bytes(bytes)
}

/// Direction of a fade.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FadeDirection {
    /// From black to the target colours (routine flag != 0).
    In,
    /// From the target colours to black (routine flag == 0).
    Out,
}

/// Computes the DAC contents of entries 0..31 for every visible step of the
/// game's fade. Returns [`FADE_FRAMES`] (30) frames; frame `k` is what the DAC
/// holds after the `k`-th vertical-retrace-synchronised upload. The last frame
/// equals the target (fade in) or black (fade out).
///
/// Origin: BMMAIN.EXE 45ac:33ca (vga_fade_32_colors)
pub fn fade_frames(words: &[u16; FADE_COLORS], dir: FadeDirection) -> Vec<[[u8; 3]; FADE_COLORS]> {
    // Target bytes at CS:0x00FC, current bytes at CS:0x003C, accumulators at CS:0x01BC.
    let mut target = [0u8; FADE_COLORS * 3];
    for (i, &w) in words.iter().enumerate() {
        target[i * 3..i * 3 + 3].copy_from_slice(&fade_word_to_rgb6(w));
    }
    let mut cur = match dir {
        FadeDirection::In => [0u8; FADE_COLORS * 3],
        FadeDirection::Out => target,
    };
    let mut acc = [0u8; FADE_COLORS * 3];
    let mut frames = Vec::with_capacity(FADE_FRAMES);
    let mut cx: u16 = FADE_STEPS as u16;
    while cx != 0 {
        for ((a, &t), c) in acc.iter_mut().zip(target.iter()).zip(cur.iter_mut()) {
            *a = a.wrapping_add(t);
            // `cmp [si+0xc0],ah ; jl` is a signed byte comparison against 60.
            if (*a as i8) >= 0x3c {
                *a = a.wrapping_sub(0x3c);
                *c = match dir {
                    FadeDirection::In => c.wrapping_add(1),
                    FadeDirection::Out => c.wrapping_sub(1),
                };
            }
        }
        if cx & 1 != 0 {
            let mut f = [[0u8; 3]; FADE_COLORS];
            for (i, e) in f.iter_mut().enumerate() {
                e.copy_from_slice(&cur[i * 3..i * 3 + 3]);
            }
            frames.push(f);
        }
        cx -= 1;
    }
    frames
}

/// MOD2026: [`fade_frames`] for any number of 6-bit DAC colours, the same 60 steps of
/// `acc += target; if acc >= 60 { acc -= 60; out ± 1 }` per channel and an upload after every
/// second step. The mod fades its crest colours (entries 64..127) with it, alongside the
/// original's 32 entries.
///
/// The original's colours never exceed 60 (4-bit words times 4); above that the signed
/// accumulator overflows and the fade stalls. So a value above 60 steps as 60, and a fade in
/// ends on the exact colours.
pub fn fade_frames_rgb6(targets: &[[u8; 3]], dir: FadeDirection) -> Vec<Vec<[u8; 3]>> {
    let exact = targets;
    let stepped: Vec<[u8; 3]> = targets.iter().map(|c| c.map(|v| v.min(0x3c))).collect();
    let targets = stepped.as_slice();
    let mut cur: Vec<[u8; 3]> = match dir {
        FadeDirection::In => vec![[0; 3]; targets.len()],
        FadeDirection::Out => stepped.clone(),
    };
    let mut acc = vec![[0u8; 3]; targets.len()];
    let mut frames = Vec::with_capacity(FADE_FRAMES);
    for cx in (1..=FADE_STEPS as u16).rev() {
        for ((a, t), c) in acc.iter_mut().zip(targets).zip(cur.iter_mut()) {
            for k in 0..3 {
                a[k] = a[k].wrapping_add(t[k]);
                if (a[k] as i8) >= 0x3c {
                    a[k] = a[k].wrapping_sub(0x3c);
                    c[k] = match dir {
                        FadeDirection::In => c[k].wrapping_add(1),
                        FadeDirection::Out => c[k].wrapping_sub(1),
                    };
                }
            }
        }
        if cx & 1 != 0 {
            frames.push(cur.clone());
        }
    }
    if let (Some(last), FadeDirection::In) = (frames.last_mut(), dir) {
        last.copy_from_slice(exact);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dac_expansion_matches_dosbox() {
        assert_eq!(dac6_to_8(0), 0);
        assert_eq!(dac6_to_8(63), 255);
        assert_eq!(dac6_to_8(32), 130);
        assert_eq!(dac6_to_8(0xff), 255);
    }

    #[test]
    fn word_roundtrip_for_4bit_colors() {
        let mut p = Palette::black();
        p.set_color(5, 0x3c, 0x14, 0x08);
        let w = p.get_color_word(5);
        assert_eq!(w, 0x0f52);
        assert_eq!(word_to_rgb6(w), [0x3c, 0x14, 0x08]);
        assert_eq!(fade_word_to_rgb6(w), [0x3c, 0x14, 0x08]);
    }

    #[test]
    fn word_packing_is_lossy_for_6bit_colors() {
        let mut p = Palette::black();
        p.set_color(0, 1, 2, 3);
        // ((1 << 4 | 2) << 2) | (3 >> 2) = 0x48
        assert_eq!(p.get_color_word(0), 0x48);
        assert_eq!(word_to_rgb6(0x48), [0, 0x10, 0x20]);
    }

    #[test]
    fn grey_ramp_matches_known_order() {
        let g = grey_ramp_words();
        assert_eq!(&g[..4], &[0x000, 0x888, 0x444, 0xccc]);
        assert_eq!(g[15], 0xfff);
    }

    #[test]
    fn fade_ends_exactly_and_has_30_frames() {
        let mut words = [0u16; FADE_COLORS];
        for (i, w) in words.iter_mut().enumerate() {
            *w = (i as u16 * 0x123) & 0x0fff;
        }
        let fin = fade_frames(&words, FadeDirection::In);
        assert_eq!(fin.len(), FADE_FRAMES);
        for (i, &w) in words.iter().enumerate() {
            assert_eq!(fin[FADE_FRAMES - 1][i], fade_word_to_rgb6(w));
        }
        let fout = fade_frames(&words, FadeDirection::Out);
        assert!(fout[FADE_FRAMES - 1].iter().all(|e| *e == [0, 0, 0]));
        // Fade-in, white (60) component: after 2 steps 60*2/60 = 2.
        let white = [0x0fffu16; FADE_COLORS];
        let f = fade_frames(&white, FadeDirection::In);
        assert_eq!(f[0][0], [2, 2, 2]);
        assert_eq!(f[14][0], [30, 30, 30]);
    }

    #[test]
    fn fade_partial_values() {
        // Target 4 (nibble 1): increments at steps 15, 30, 45, 60.
        let words = [0x0111u16; FADE_COLORS];
        let f = fade_frames(&words, FadeDirection::In);
        // Frame k is after step 2(k+1).
        assert_eq!(f[6][0], [0, 0, 0]); // after step 14
        assert_eq!(f[7][0], [1, 1, 1]); // after step 16
        let o = fade_frames(&words, FadeDirection::Out);
        assert_eq!(o[6][0], [4, 4, 4]);
        assert_eq!(o[7][0], [3, 3, 3]);
    }

    #[test]
    fn parse_rejects_bad_length() {
        assert!(PalFile::parse(&[0u8; 100]).is_err());
    }

    #[test]
    fn original_pal_file() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let pal = PalFile::load(&dir).expect("1.PAL");
        for (k, p) in pal.palettes.iter().enumerate() {
            assert!(p.entries.iter().flatten().all(|&v| v <= 63));
            // File stores white at 255; the game forces it to black.
            assert_eq!(p.entries[255], [63, 63, 63]);
            assert_eq!(pal.vram_palette(k).entries[255], [0, 0, 0]);
            // The fade only works losslessly because entries 0..31 are 4-bit exact.
            for i in 0..FADE_COLORS as u8 {
                let w = p.get_color_word(i);
                assert_eq!(word_to_rgb6(w), p.entries[i as usize]);
            }
            assert_eq!(p.entries[0], [0, 0, 0]);
        }
    }

    #[test]
    fn original_bmintro_palette() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let exe = std::fs::read(dir.join("BMINTRO.EXE")).expect("BMINTRO.EXE");
        let p = bmintro_palette(&exe).expect("palette");
        assert_eq!(p.entries[0], [0, 0, 0]);
        assert_eq!(p.entries[1], [0x37, 0x37, 0x37]);
        assert_eq!(p.entries[2], [2, 2, 2]);
        assert_eq!(p.entries[19], [0x2d, 0, 0]);
        assert_eq!(p.entries[255], [63, 63, 63]);
    }

    #[test]
    fn original_grey_ramp_in_exe() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        // BMMAIN.EXE is EXEPACK-compressed, but this table survives as literal
        // bytes; search for it instead of hard-coding a file offset.
        let exe = std::fs::read(dir.join("BMMAIN.EXE")).expect("BMMAIN.EXE");
        let needle: Vec<u8> = grey_ramp_words()
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert!(exe.windows(needle.len()).any(|w| w == needle.as_slice()));
    }

    #[test]
    fn fade_frames_rgb6_matches_the_32_colour_fade() {
        let words: [u16; FADE_COLORS] = std::array::from_fn(|i| (i as u16 * 0x123) & 0xfff);
        let targets: Vec<[u8; 3]> = words.iter().map(|&w| fade_word_to_rgb6(w)).collect();
        for dir in [FadeDirection::In, FadeDirection::Out] {
            let original = fade_frames(&words, dir);
            let generic = fade_frames_rgb6(&targets, dir);
            assert_eq!(generic.len(), FADE_FRAMES);
            for (o, g) in original.iter().zip(&generic) {
                assert_eq!(&o[..], &g[..]);
            }
        }
        // A 6-bit value above 60 still ends exactly on its colour, or on black.
        let fade_in = fade_frames_rgb6(&[[63, 61, 1]], FadeDirection::In);
        assert_eq!(fade_in.last().unwrap()[0], [63, 61, 1]);
        let fade_out = fade_frames_rgb6(&[[63, 61, 1]], FadeDirection::Out);
        assert_eq!(fade_out.last().unwrap()[0], [0, 0, 0]);
    }
}
