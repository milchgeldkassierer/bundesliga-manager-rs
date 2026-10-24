//! `PIC\<id>.VGA` sprite images (run-length encoded, 8 bits per pixel).
//!
//! Full specification: `docs/re/vga.md`.
//!
//! Address conventions used below:
//! - "image" offsets are offsets into the *unpacked* load image of `BMMAIN.EXE`
//!   (the file is EXEPACK-compressed, header paragraph count 0x20).
//! - Ghidra addresses (`1000:7a06`) refer to the Ghidra project, which holds the
//!   still-packed image loaded at segment 0x1000. Code below image 0x35ad1 is stored
//!   verbatim there (Ghidra address = image offset + 0x10000); routines above that
//!   offset only exist as shifted literal copies inside the packed stream.
//!
//! File layout (all little endian):
//!
//! ```text
//! +0  u16 width
//! +2  u16 height
//! +4  u16 packed_len        number of RLE bytes that follow (terminator included)
//! +6  u8  rle[packed_len]   decodes to exactly width * height pixel bytes, row-major
//! ```

use std::fmt;

/// Size of the fixed file header in bytes.
pub const HEADER_LEN: usize = 6;

/// RLE control byte that ends the stream.
pub const RLE_END: u8 = 0x80;

/// Width value that makes the loader read a `.CP` full-screen file instead of a
/// `.VGA` sprite (`cmp word ptr [bp+8], 0x7530` at Ghidra `1000:7a29`).
pub const CP_WIDTH_MARKER: u16 = 30000;

/// Picture ids at or above this value are the 16-colour grey photos (`200.VGA` ..
/// `229.VGA`); the loader stores their width in a global (`cmp byte [bp+6], 0xc7`
/// at Ghidra `1000:7d14`).
pub const PHOTO_FIRST_ID: u8 = 200;

/// 16-entry photo palette in `0x0RGB` (4 bits per channel) form, copied from
/// DGROUP offset 0x9394 (image offset 0x55ec4). The grey level of entry `i` is the
/// 4-bit bit-reversal of `i`, so the photo pixel values are bit-reversed brightness.
/// The game installs it into palette entries 0..15 before drawing a photo
/// (caller at Ghidra `3000:f39f`, converter at image 0x39924).
pub const PHOTO_PALETTE_RGB12: [u16; 16] = [
    0x000, 0x888, 0x444, 0xccc, 0x222, 0xaaa, 0x666, 0xeee, 0x111, 0x999, 0x555, 0xddd, 0x333,
    0xbbb, 0x777, 0xfff,
];

/// Errors produced while parsing a `.VGA` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VgaError {
    /// The input is shorter than the 6-byte header.
    TruncatedHeader { len: usize },
    /// The header announces more packed bytes than the file contains.
    TruncatedData { packed_len: usize, available: usize },
    /// The RLE stream ran out of bytes before the end marker 0x80.
    MissingTerminator,
    /// The stream decoded to a different number of pixels than `width * height`.
    SizeMismatch { expected: usize, actual: usize },
}

impl fmt::Display for VgaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VgaError::TruncatedHeader { len } => {
                write!(f, "VGA file too short for header ({len} bytes)")
            }
            VgaError::TruncatedData {
                packed_len,
                available,
            } => write!(
                f,
                "VGA header announces {packed_len} packed bytes, only {available} present"
            ),
            VgaError::MissingTerminator => write!(f, "RLE stream has no 0x80 end marker"),
            VgaError::SizeMismatch { expected, actual } => {
                write!(
                    f,
                    "RLE stream decoded to {actual} pixels, expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for VgaError {}

/// The 6-byte `.VGA` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VgaHeader {
    pub width: u16,
    pub height: u16,
    /// Number of RLE bytes following the header, including the 0x80 terminator.
    pub packed_len: u16,
}

/// A decoded picture: `width * height` palette indices, row-major, no padding.
///
/// This mirrors the game's in-memory sprite object `{u16 w; u16 h; u8 far *pixels}`
/// built by `pic_load_image`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VgaImage {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

impl VgaImage {
    /// Palette index at (`x`, `y`), or `None` outside the image.
    pub fn pixel(&self, x: u16, y: u16) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get(usize::from(y) * usize::from(self.width) + usize::from(x))
            .copied()
    }
}

/// Reads the header fields exactly as the loader does: three consecutive 2-byte
/// `read()` calls into width, height and packed length.
///
/// Origin: BMMAIN.EXE 1000:7ae3 (pic_load_image)
pub fn parse_header(data: &[u8]) -> Result<VgaHeader, VgaError> {
    if data.len() < HEADER_LEN {
        return Err(VgaError::TruncatedHeader { len: data.len() });
    }
    let word = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
    Ok(VgaHeader {
        width: word(0),
        height: word(2),
        packed_len: word(4),
    })
}

/// Decodes one RLE stream into a linear buffer (the non-`0xA000` destination path).
///
/// Control byte `c`:
/// - `c < 0x80`: copy the next `c + 1` bytes literally;
/// - `c == 0x80`: end of stream;
/// - `c > 0x80`: write the next byte `(c & 0x7f) + 1` times.
///
/// Returns the decoded bytes and the number of input bytes consumed (terminator
/// included). The original has no bounds checks; here running off the end of `src`
/// yields [`VgaError::MissingTerminator`].
///
/// Origin: BMMAIN.EXE image 0x38be0 = raw 35ac:3120, shifted copy in Ghidra at 4000:84dc (rle_decode)
pub fn decode_rle(src: &[u8]) -> Result<(Vec<u8>, usize), VgaError> {
    let mut out = Vec::new();
    let mut si = 0usize;
    loop {
        let al = *src.get(si).ok_or(VgaError::MissingTerminator)?;
        si += 1;
        if al & 0x80 == 0 {
            // mov cl, al; inc cx; rep movsb
            let n = usize::from(al) + 1;
            let lit = src.get(si..si + n).ok_or(VgaError::MissingTerminator)?;
            out.extend_from_slice(lit);
            si += n;
        } else {
            let count = al & 0x7f;
            if count == 0 {
                return Ok((out, si));
            }
            // mov cl, al; inc cx; lodsb; rep stosb
            let value = *src.get(si).ok_or(VgaError::MissingTerminator)?;
            si += 1;
            out.extend(std::iter::repeat_n(value, usize::from(count) + 1));
        }
    }
}

/// Decodes one RLE stream directly into unchained ("mode X") VGA memory: pixel `i`
/// goes to plane `i % 4` at byte offset `i / 4`. This is the `dst segment == 0xA000`
/// branch of the decoder, used for `.CP` screens. Returns the four planes and the
/// number of input bytes consumed.
///
/// The original additionally switches the read-map plane whenever the *source*
/// offset reaches 0x4b00 (when the packed data itself lives in VGA memory); that
/// source paging is not modelled here because `src` is a flat slice.
///
/// Origin: BMMAIN.EXE image 0x38c27 = raw 35ac:3147, shifted copy in Ghidra at 4000:8523 (rle_decode)
pub fn decode_rle_planar(src: &[u8]) -> Result<([Vec<u8>; 4], usize), VgaError> {
    let (linear, used) = decode_rle(src)?;
    let mut planes: [Vec<u8>; 4] = Default::default();
    for (i, &p) in linear.iter().enumerate() {
        planes[i % 4].push(p);
    }
    Ok((planes, used))
}

/// Parses a complete `.VGA` file. The RLE stream is taken from
/// `data[6 .. 6 + packed_len]`; bytes after it are ignored (none of the shipped files
/// has any).
///
/// Origin: BMMAIN.EXE 1000:7a06 (pic_load_image)
pub fn parse(data: &[u8]) -> Result<VgaImage, VgaError> {
    let header = parse_header(data)?;
    let packed_len = usize::from(header.packed_len);
    let available = data.len() - HEADER_LEN;
    if packed_len > available {
        return Err(VgaError::TruncatedData {
            packed_len,
            available,
        });
    }
    let (pixels, _used) = decode_rle(&data[HEADER_LEN..HEADER_LEN + packed_len])?;
    let expected = usize::from(header.width) * usize::from(header.height);
    if pixels.len() != expected {
        return Err(VgaError::SizeMismatch {
            expected,
            actual: pixels.len(),
        });
    }
    Ok(VgaImage {
        width: header.width,
        height: header.height,
        pixels,
    })
}

/// The game's RLE compressor, reproduced instruction by instruction. It produces
/// byte-identical streams for every shipped `.VGA` file.
///
/// The original compares `src[si]` with `src[si + 1]` at the start of every packet,
/// and inside a literal packet, which can read one byte past the input; `past_end`
/// supplies that byte (`None` means "never equal"). Only the very last input byte
/// is affected.
///
/// Origin: BMMAIN.EXE image 0x38c96 = raw 35ac:31d6, shifted copy in Ghidra at 4000:8592 (rle_encode)
pub fn encode_rle_with_past_end(src: &[u8], past_end: Option<u8>) -> Vec<u8> {
    let at = |i: usize| -> Option<u8> {
        if i < src.len() {
            Some(src[i])
        } else if i == src.len() {
            past_end
        } else {
            None
        }
    };
    let mut out = Vec::with_capacity(src.len() + src.len() / 64 + 2);
    if src.is_empty() {
        // CX = 0 would make the original loop 65536 times; emit an empty stream.
        out.push(RLE_END);
        return out;
    }
    let mut si = 0usize;
    let mut cx = src.len();
    loop {
        // 85b3: DX = 0; BX = DI; DI++
        let mut dx: usize = 0;
        let bx = out.len();
        out.push(0);
        let al = src[si];
        let mut run = at(si + 1) == Some(al);
        if run {
            // 85e7: dec cx; jnz 85ed; inc cx; jmp literal
            cx -= 1;
            if cx == 0 {
                cx = 1;
                run = false;
            }
        }
        if run {
            // 85ed: stosb (value byte)
            out.push(al);
            loop {
                // 85f1
                si += 1;
                dx += 1;
                cx -= 1;
                if cx == 0 {
                    // 8609
                    out.push(RLE_END);
                    out[bx] = (dx as u8) | 0x80;
                    return out;
                }
                if dx == 0x7f || at(si + 1) != Some(al) {
                    break;
                }
            }
            // 8600
            out[bx] = (dx as u8) | 0x80;
            si += 1;
            continue;
        }
        // 85bf: literal packet
        loop {
            out.push(src[si]);
            si += 1;
            dx += 1;
            cx -= 1;
            if cx == 0 {
                // 85db
                out.push(RLE_END);
                out[bx] = (dx - 1) as u8;
                return out;
            }
            if dx == 0x80 {
                break;
            }
            if at(si) == at(si + 1) {
                break;
            }
        }
        // 85d5
        out[bx] = (dx - 1) as u8;
    }
}

/// [`encode_rle_with_past_end`] with no matching byte past the end of the input.
pub fn encode_rle(src: &[u8]) -> Vec<u8> {
    encode_rle_with_past_end(src, None)
}

/// Serializes an image into the `.VGA` file layout using the game's compressor.
///
/// Panics if the packed stream exceeds 65535 bytes (impossible to represent in
/// the 16-bit length field).
pub fn encode(image: &VgaImage) -> Vec<u8> {
    let rle = encode_rle(&image.pixels);
    let packed_len = u16::try_from(rle.len()).expect("packed VGA stream exceeds 64 KiB");
    let mut out = Vec::with_capacity(HEADER_LEN + rle.len());
    out.extend_from_slice(&image.width.to_le_bytes());
    out.extend_from_slice(&image.height.to_le_bytes());
    out.extend_from_slice(&packed_len.to_le_bytes());
    out.extend_from_slice(&rle);
    out
}

/// File name the game builds for picture `id`: `"pic\\" + itoa(id) + ".vga"`
/// (strings at DGROUP 0x192 and 0x19b). Returned with a forward slash and the
/// upper-case spelling of the shipped files.
///
/// Origin: BMMAIN.EXE 1000:7a73 (pic_load_image)
pub fn file_name(id: u8) -> String {
    format!("PIC/{id}.VGA")
}

/// Converts one `0x0RGB` colour to 6-bit DAC values `[r, g, b]`, each channel
/// shifted left by two (`(w >> 6) & 0x3c`, `(w >> 2) & 0x3c`, `(w & 0x0f) << 2`).
///
/// Origin: BMMAIN.EXE image 0x39924 = raw 3930:0624, shifted copy in Ghidra at 4000:9220 (palette_set_rgb12)
pub fn rgb12_to_dac(w: u16) -> [u8; 3] {
    [
        ((w >> 6) & 0x3c) as u8,
        ((w >> 2) & 0x3c) as u8,
        ((w & 0x0f) << 2) as u8,
    ]
}

/// DAC values (6 bit) of palette entries 0..15 while a photo (`id >= 200`) is shown.
pub fn photo_palette_dac() -> [[u8; 3]; 16] {
    PHOTO_PALETTE_RGB12.map(rgb12_to_dac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_literal_run_and_end() {
        let src = [0x01, 7, 8, 0x82, 5, 0x80];
        let (out, used) = decode_rle(&src).unwrap();
        assert_eq!(out, vec![7, 8, 5, 5, 5]);
        assert_eq!(used, src.len());
    }

    #[test]
    fn decode_reports_missing_terminator() {
        assert_eq!(decode_rle(&[0x00, 1]), Err(VgaError::MissingTerminator));
        assert_eq!(decode_rle(&[0x85]), Err(VgaError::MissingTerminator));
    }

    #[test]
    fn parse_small_file() {
        let file = [2, 0, 2, 0, 3, 0, 0x83, 9, 0x80];
        let img = parse(&file).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixels, vec![9; 4]);
        assert_eq!(img.pixel(1, 1), Some(9));
        assert_eq!(img.pixel(2, 0), None);
        assert_eq!(encode(&img), file);
    }

    #[test]
    fn parse_rejects_size_mismatch() {
        let file = [3, 0, 2, 0, 3, 0, 0x83, 9, 0x80];
        assert_eq!(
            parse(&file),
            Err(VgaError::SizeMismatch {
                expected: 6,
                actual: 4
            })
        );
    }

    #[test]
    fn encoder_packet_limits() {
        // 200 equal bytes: run of 128 (0xff) then run of 72 ending the stream.
        let enc = encode_rle(&[4u8; 200]);
        assert_eq!(enc, vec![0xff, 4, 0xc7, 4, 0x80]);
        // 130 distinct-neighbour bytes: literal of 128 (0x7f) then literal of 2.
        let src: Vec<u8> = (0..130u32).map(|i| (i % 2) as u8).collect();
        let enc = encode_rle(&src);
        assert_eq!(enc[0], 0x7f);
        assert_eq!(enc[129], 0x01);
        assert_eq!(*enc.last().unwrap(), RLE_END);
        assert_eq!(decode_rle(&enc).unwrap().0, src);
    }

    #[test]
    fn encoder_roundtrips_synthetic_data() {
        let mut src = Vec::new();
        let mut x: u32 = 12345;
        for _ in 0..5000 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            let v = ((x >> 16) % 4) as u8;
            let n = 1 + ((x >> 8) % 7) as usize;
            src.extend(std::iter::repeat_n(v, n));
        }
        for past in [None, Some(0), Some(src[src.len() - 1])] {
            let enc = encode_rle_with_past_end(&src, past);
            let (dec, used) = decode_rle(&enc).unwrap();
            assert_eq!(dec, src);
            assert_eq!(used, enc.len());
        }
    }

    #[test]
    fn photo_palette_is_bit_reversed_grey() {
        let pal = photo_palette_dac();
        for (i, rgb) in pal.iter().enumerate() {
            let level = ((i as u8).reverse_bits() >> 4) << 2;
            assert_eq!(*rgb, [level; 3]);
        }
        assert_eq!(rgb12_to_dac(0xf00), [60, 0, 0]);
        assert_eq!(rgb12_to_dac(0x0f0), [0, 60, 0]);
    }

    #[test]
    fn planar_split() {
        let (planes, _) = decode_rle_planar(&[0x04, 0, 1, 2, 3, 4, 0x80]).unwrap();
        assert_eq!(planes[0], vec![0, 4]);
        assert_eq!(planes[1], vec![1]);
        assert_eq!(planes[3], vec![3]);
    }

    /// Every shipped `.VGA` file decodes, consumes exactly its bytes, has a
    /// plausible size, and re-encodes byte-identically with the game's compressor.
    #[test]
    fn all_original_files() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let pic = dir.join("PIC");
        let mut count = 0;
        for entry in std::fs::read_dir(&pic).expect("PIC directory") {
            let path = entry.unwrap().path();
            let is_vga = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("vga"));
            if !is_vga {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let name = path.display();
            let header = parse_header(&data).unwrap();
            assert_eq!(
                data.len(),
                HEADER_LEN + usize::from(header.packed_len),
                "{name}: trailing bytes"
            );
            let (_, used) = decode_rle(&data[HEADER_LEN..]).unwrap();
            assert_eq!(used, usize::from(header.packed_len), "{name}: consumed");
            let img = parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                (1..=320).contains(&img.width),
                "{name}: width {}",
                img.width
            );
            assert!(
                (1..=240).contains(&img.height),
                "{name}: height {}",
                img.height
            );
            assert_eq!(encode(&img), data, "{name}: re-encode differs");
            let id: u8 = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse().ok())
                .expect("numeric file name");
            let max = img.pixels.iter().copied().max().unwrap_or(0);
            if id >= PHOTO_FIRST_ID {
                assert!(max < 16, "{name}: photo uses index {max}");
            } else {
                assert!(max < 32, "{name}: sprite uses index {max}");
            }
            count += 1;
        }
        assert_eq!(count, 154, "number of .VGA files");
    }
}
