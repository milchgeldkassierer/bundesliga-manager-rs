//! `PIC/<n>.CP` full-screen pictures (320x240, Mode X).
//!
//! A `.CP` file is a 6-byte header followed by one run-length encoded pixel
//! stream that covers a whole 320x240 Mode X screen. The game loads it with
//! the same routine as `.VGA` pictures (`load_pic_cp_or_vga`, selected by the
//! magic width argument 30000), parks the compressed bytes in the off-screen
//! VRAM page at `A000:4B00` and then expands them with the shared RLE decoder
//! straight into the visible Mode X page. See `docs/re/cp.md` for the full
//! specification and all addresses.
//!
//! Address note: `BMMAIN.EXE` is EXEPACK-compressed. Addresses written as
//! `seg:off` below are far addresses of the *unpacked* load image (segment 0 =
//! image start). In the shared Ghidra project (which holds the packed image,
//! loaded at segment 0x1000) the loader sits at `1000:7a06`, the decoder at
//! `4000:84dc` and its plane-advance helper at `4000:84c3`.

use std::fmt;

/// Width of a CP picture in pixels (Mode X screen width).
pub const CP_WIDTH: usize = 320;
/// Height of a CP picture in pixels (Mode X 320x240 screen).
pub const CP_HEIGHT: usize = 240;
/// Bytes per Mode X plane for one 320x240 page (`80 * 240`), the constant
/// `0x4B00` used throughout the loader.
pub const PLANE_BYTES: usize = 0x4B00;
/// Size of the CP header in bytes.
pub const HEADER_LEN: usize = 6;
/// Maximum number of compressed bytes the loader can park in VRAM
/// (four planes of [`PLANE_BYTES`]).
pub const MAX_PACKED_LEN: usize = 4 * PLANE_BYTES;
/// Width argument that makes the picture loader open `pic\<n>.cp` instead of
/// `pic\<n>.vga`.
pub const CP_WIDTH_MAGIC: u16 = 30000;

/// Picture numbers of all `.CP` files shipped with the game and where they are
/// loaded from (call site as a linear offset into the unpacked load image).
pub const CP_PICTURES: [(u8, &str); 8] = [
    (
        22,
        "BMMAIN.EXE linear 0xc792: marble background (main/office screens)",
    ),
    (45, "BMMAIN.EXE linear 0x346ca: high-score table"),
    (
        46,
        "BMMAIN.EXE linear 0x28fa0: advertising income/expenses screen",
    ),
    (47, "BMMAIN.EXE linear 0x9194: title picture (players)"),
    (48, "BMLOADER.EXE linear 0x0cdf: title picture (stadium)"),
    (
        49,
        "BMLOADER.EXE linear 0x0c82: 'eine KRON Produktion' logo",
    ),
    (
        50,
        "no literal call site found: replay/video recorder control panel",
    ),
    (
        51,
        "no literal call site found: 'Software 2000 praesentiert' logo",
    ),
];

/// Errors produced while parsing or decoding a CP picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpError {
    /// The file is shorter than the 6-byte header.
    TooShort(usize),
    /// The RLE stream ended (ran past the bytes the loader parks in VRAM)
    /// before the 0x80 terminator was read.
    UnexpectedEnd {
        /// Offset into the compressed stream at which more input was needed.
        offset: usize,
    },
    /// The stream produced more than 320*240 pixels; the original would start
    /// overwriting its own compressed source in VRAM page 1.
    Overflow {
        /// Offset into the compressed stream of the offending control byte.
        offset: usize,
    },
    /// The stream terminated before filling the screen.
    Incomplete {
        /// Number of pixels produced.
        pixels: usize,
    },
}

impl fmt::Display for CpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpError::TooShort(n) => write!(f, "CP file too short ({n} bytes)"),
            CpError::UnexpectedEnd { offset } => {
                write!(f, "CP RLE stream truncated at offset {offset}")
            }
            CpError::Overflow { offset } => {
                write!(f, "CP RLE stream overflows the screen at offset {offset}")
            }
            CpError::Incomplete { pixels } => {
                write!(f, "CP RLE stream ended after {pixels} of 76800 pixels")
            }
        }
    }
}

impl std::error::Error for CpError {}

/// The 6-byte CP header: three little-endian `u16` words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpHeader {
    /// Stored width (320 in every shipped file). Read by the loader but not
    /// used by the CP decode path.
    pub width: u16,
    /// Stored height (240 in every shipped file). Read but not used.
    pub height: u16,
    /// Length of the compressed stream including the 0x80 terminator. The
    /// loader uses it only to drive its chunked copy into VRAM.
    pub packed_len: u16,
}

impl CpHeader {
    /// Parse the header from the start of a `.CP` file.
    ///
    /// Origin: BMMAIN.EXE 076b:0356 (load_pic_cp_or_vga), the three 2-byte
    /// reads at unpacked 076b:0433..0461 into `[bp+8]`, `[bp+0Ah]`, `[bp-5Ah]`.
    pub fn parse(data: &[u8]) -> Result<CpHeader, CpError> {
        if data.len() < HEADER_LEN {
            return Err(CpError::TooShort(data.len()));
        }
        let word = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
        Ok(CpHeader {
            width: word(0),
            height: word(2),
            packed_len: word(4),
        })
    }
}

/// A decoded CP picture: 320x240 palette indices in row-major order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpImage {
    /// The parsed header.
    pub header: CpHeader,
    /// `CP_WIDTH * CP_HEIGHT` palette indices, row-major.
    pub pixels: Vec<u8>,
    /// Number of compressed bytes consumed, including the terminator.
    pub consumed: usize,
}

impl CpImage {
    /// Pixel at `(x, y)`.
    pub fn pixel(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * CP_WIDTH + x]
    }

    /// Split the picture into the four Mode X planes exactly as the decoder
    /// writes VRAM: pixel `i` goes to plane `i & 3` at offset `i >> 2`.
    ///
    /// Origin: BMMAIN.EXE 35ac:3120 (FUN_4000_84dc), Mode X branch
    /// (unpacked 35ac:3167..31c7; plane = `cl`, offset = `di`).
    pub fn to_planes(&self) -> [Vec<u8>; 4] {
        let mut planes: [Vec<u8>; 4] = Default::default();
        for p in planes.iter_mut() {
            p.reserve(PLANE_BYTES);
        }
        for (i, &px) in self.pixels.iter().enumerate() {
            planes[i & 3].push(px);
        }
        planes
    }
}

/// The bytes of a CP file that the loader copies into VRAM page 1
/// (`A000:4B00`, planes 0..3, 0x4B00 bytes each): the file body after the
/// header, at most [`MAX_PACKED_LEN`] bytes.
///
/// The loader reads the body in 0x4B00-byte chunks, one per plane, with a
/// `fread` that simply returns fewer bytes at end of file; the header's
/// `packed_len` only decides whether a chunk is the last one. For every
/// possible `packed_len` the net effect is "copy the body up to four planes".
///
/// Origin: BMMAIN.EXE 076b:0356 (load_pic_cp_or_vga), CP branch at unpacked
/// 076b:0472..052e (Sequencer map mask loop, `_fread`, far copy 35ac:304a).
pub fn vram_source(data: &[u8]) -> Result<&[u8], CpError> {
    if data.len() < HEADER_LEN {
        return Err(CpError::TooShort(data.len()));
    }
    let body = &data[HEADER_LEN..];
    Ok(&body[..body.len().min(MAX_PACKED_LEN)])
}

/// Expand the CP RLE stream into a 320x240 pixel buffer.
///
/// Control byte `c`:
/// * `c < 0x80`: copy the next `c + 1` bytes literally;
/// * `c == 0x80`: end of picture;
/// * `c > 0x80`: repeat the next byte `(c & 0x7F) + 1` times.
///
/// The source is read sequentially (the original walks VRAM plane 0, then 1,
/// 2, 3 via `cp_vram_source_advance_plane`, which is the same byte order as
/// the file). Output pixel `i` lands in plane `i & 3` at offset `i >> 2`,
/// which for a 320-pixel-wide Mode X page is simply row-major order.
///
/// Origin: BMMAIN.EXE 35ac:3120 (FUN_4000_84dc), Mode X branch taken when
/// the destination segment is 0xA000; source advance helper 35ac:3107
/// (cp_vram_source_advance_plane).
pub fn decode_rle(src: &[u8]) -> Result<(Vec<u8>, usize), CpError> {
    let total = CP_WIDTH * CP_HEIGHT;
    let mut out = Vec::with_capacity(total);
    let mut i = 0usize;
    let next = |i: &mut usize| -> Result<u8, CpError> {
        let b = *src.get(*i).ok_or(CpError::UnexpectedEnd { offset: *i })?;
        *i += 1;
        Ok(b)
    };
    loop {
        let at = i;
        let c = next(&mut i)?;
        if c & 0x80 == 0 {
            // Literal run: bh = c + 1 bytes copied one pixel at a time.
            let n = usize::from(c) + 1;
            if out.len() + n > total {
                return Err(CpError::Overflow { offset: at });
            }
            for _ in 0..n {
                let b = next(&mut i)?;
                out.push(b);
            }
        } else {
            let n = c & 0x7F;
            if n == 0 {
                break;
            }
            let n = usize::from(n) + 1;
            let v = next(&mut i)?;
            if out.len() + n > total {
                return Err(CpError::Overflow { offset: at });
            }
            out.extend(std::iter::repeat_n(v, n));
        }
    }
    Ok((out, i))
}

/// Decode a complete `.CP` file into a 320x240 picture.
///
/// Origin: BMMAIN.EXE 076b:0356 (load_pic_cp_or_vga) with width == 30000,
/// followed by the decode call at unpacked 076b:062f into 35ac:3120
/// (FUN_4000_84dc).
pub fn decode(data: &[u8]) -> Result<CpImage, CpError> {
    let header = CpHeader::parse(data)?;
    let src = vram_source(data)?;
    let (pixels, consumed) = decode_rle(src)?;
    if pixels.len() != CP_WIDTH * CP_HEIGHT {
        return Err(CpError::Incomplete {
            pixels: pixels.len(),
        });
    }
    Ok(CpImage {
        header,
        pixels,
        consumed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv1a(data: &[u8]) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for &b in data {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        h
    }

    /// Build a synthetic file: header + RLE that fills the screen.
    fn synthetic() -> Vec<u8> {
        let mut body = vec![0x02, 7, 8, 9]; // literal 3 bytes
        let mut left = CP_WIDTH * CP_HEIGHT - 3;
        while left > 0 {
            let n = left.min(128);
            body.push(0x80 | (n as u8 - 1));
            body.push(5);
            left -= n;
        }
        body.push(0x80);
        let mut f = Vec::new();
        f.extend_from_slice(&320u16.to_le_bytes());
        f.extend_from_slice(&240u16.to_le_bytes());
        f.extend_from_slice(&(body.len() as u16).to_le_bytes());
        f.extend_from_slice(&body);
        f
    }

    #[test]
    fn decodes_synthetic_stream() {
        let f = synthetic();
        let img = decode(&f).unwrap();
        assert_eq!(img.header.width, 320);
        assert_eq!(img.header.height, 240);
        assert_eq!(&img.pixels[..4], &[7, 8, 9, 5]);
        assert_eq!(img.pixel(319, 239), 5);
        assert_eq!(img.consumed, f.len() - HEADER_LEN);
        let planes = img.to_planes();
        assert!(planes.iter().all(|p| p.len() == PLANE_BYTES));
        assert_eq!(planes[0][0], 7);
        assert_eq!(planes[3][0], 5);
    }

    #[test]
    fn literal_and_run_lengths() {
        // 0x7F = 128 literal bytes, 0xFF = run of 128.
        let mut s = vec![0x7F];
        s.extend(0..128u8);
        s.extend_from_slice(&[0xFF, 0xAA, 0x80]);
        let (out, used) = decode_rle(&s).unwrap();
        assert_eq!(out.len(), 256);
        assert_eq!(out[127], 127);
        assert!(out[128..].iter().all(|&b| b == 0xAA));
        assert_eq!(used, s.len());
    }

    #[test]
    fn errors() {
        assert_eq!(decode(&[1, 2, 3]), Err(CpError::TooShort(3)));
        assert_eq!(
            decode_rle(&[0x81]),
            Err(CpError::UnexpectedEnd { offset: 1 })
        );
        let mut f = synthetic();
        let n = f.len();
        f[n - 1] = 0x81; // replace terminator by a run: overflows
        f.push(0);
        assert!(matches!(decode(&f), Err(CpError::Overflow { .. })));
        let short = [0, 0, 0, 0, 0, 0, 0x81, 1, 0x80];
        assert_eq!(decode(&short), Err(CpError::Incomplete { pixels: 2 }));
    }

    #[test]
    fn vram_source_is_capped_at_four_planes() {
        let mut f = vec![0u8; HEADER_LEN + MAX_PACKED_LEN + 10];
        f[0] = 1;
        assert_eq!(vram_source(&f).unwrap().len(), MAX_PACKED_LEN);
    }

    #[test]
    fn decodes_all_original_files() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        // (picture, fnv1a of pixels, consumed bytes, file body length)
        let expected: [(u8, u32, usize, usize); 8] = [
            (22, 0x297d_ffcc, 58374, 58374),
            (45, 0x703b_07e0, 22994, 22994),
            (46, 0x51c1_63f8, 38516, 38516),
            (47, 0xde9d_ba7a, 53807, 53807),
            (48, 0x5681_8a00, 57056, 57056),
            (49, 0x4b9f_6d9f, 5893, 5893),
            (50, 0x8350_8be2, 13996, 14092),
            (51, 0x5456_c0d5, 5105, 5105),
        ];
        for (n, hash, consumed, body) in expected {
            let path = dir.join("PIC").join(format!("{n}.CP"));
            let data = std::fs::read(&path).unwrap();
            assert_eq!(data.len() - HEADER_LEN, body, "{n}.CP body length");
            let img = decode(&data).unwrap();
            assert_eq!(img.header.width, 320);
            assert_eq!(img.header.height, 240);
            assert_eq!(usize::from(img.header.packed_len), consumed);
            assert_eq!(img.consumed, consumed, "{n}.CP consumed");
            assert_eq!(fnv1a(&img.pixels), hash, "{n}.CP pixel hash");
        }
    }
}
