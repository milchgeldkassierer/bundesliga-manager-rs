//! IFF pictures (`FORM ILBM` / `FORM PBM `) as TEAMED.EXE's crest import reads them.
//!
//! Full specification: `docs/re/teamed/crest.md` ("IFF import").
//!
//! The editor's reader (`1000:0268`) is a minimal chunk walker: it accepts any `FORM`
//! of type `ILBM` or `PBM `, keeps the 20-byte `BMHD`, reads `CMAP` and `CAMG` into
//! buffers it never uses (the palette of the picture is ignored, the pixel values are
//! drawn with the editor's own palette) and stops at `BODY`. The picture is accepted
//! when it is at most 320 x 200 pixels. The decoder (`16b7:382b`) does not look at the
//! form type either: 8 bit planes mean chunky rows of `width` bytes (`PBM `), any other
//! plane count means interleaved bit planes of `width / 8` bytes per row (no padding to
//! a word boundary, so only widths that are a multiple of 16 decode like a real ILBM).
//! Compression 1 is ByteRun1, any other value means uncompressed rows.

/// Size of the `BMHD` chunk the reader copies (`DS:1DE2`, 20 bytes).
pub const BMHD_LEN: usize = 20;
/// Largest accepted picture (`1000:047c`, signed compares).
pub const MAX_WIDTH: i16 = 0x140;
pub const MAX_HEIGHT: i16 = 0xc8;

/// What [`read_picture`] keeps of an accepted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IffPicture {
    /// The raw (big-endian) `BMHD` chunk.
    pub bmhd: [u8; BMHD_LEN],
    /// The `BODY` chunk.
    pub body: Vec<u8>,
}

impl IffPicture {
    /// `BMHD` width (`bytes_reverse2`, `1000:00b4`).
    pub fn width(&self) -> i16 {
        i16::from_be_bytes([self.bmhd[0], self.bmhd[1]])
    }

    /// `BMHD` height.
    pub fn height(&self) -> i16 {
        i16::from_be_bytes([self.bmhd[2], self.bmhd[3]])
    }

    /// `BMHD` number of bit planes (`+8`).
    pub fn planes(&self) -> u8 {
        self.bmhd[8]
    }

    /// `BMHD` compression byte (`+0x0A`) is 1 (ByteRun1).
    pub fn compressed(&self) -> bool {
        self.bmhd[10] == 1
    }

    /// The chunky `width * height` pixels ([`decode_body`]).
    pub fn pixels(&self) -> Vec<u8> {
        decode_body(
            &self.body,
            self.width() as u16,
            self.height() as u16,
            self.planes(),
            self.compressed(),
        )
    }
}

/// Reads the file the way `crest_import_read_picture` does and returns the `BMHD` and
/// the `BODY`, or `None` when the original returns 0 (not a `FORM ILBM`/`FORM PBM `, a
/// read error, or a picture larger than 320 x 200).
///
/// `BMHD` chunks are read with a fixed length of 20 bytes, `CMAP` and `CAMG` with their
/// chunk length and without the pad byte of an odd length; other chunks are skipped
/// with `(size + 1) & !1`. A missing `BMHD` leaves the header the previous import left
/// in `DS:1DE2`; `last_bmhd` is that buffer (all zero before the first import).
///
/// DEVIATION: the original's `file_read` only fails on a DOS error, so at the end of a
/// file without a `BODY` it reads nothing forever (the chunk header keeps its last
/// value and the loop never ends) and a short `BODY` leaves the rest of the buffer
/// undefined. The port stops with `None` at the end of the file and fills a short
/// `BODY` with zeros.
///
/// Origin: TEAMED.EXE 1000:0268 (crest_import_read_picture)
pub fn read_picture(data: &[u8], last_bmhd: &[u8; BMHD_LEN]) -> Option<IffPicture> {
    let mut pos = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        if pos >= data.len() && n > 0 {
            return None;
        }
        let end = (pos + n).min(data.len());
        let s = &data[pos..end];
        pos = end;
        Some(s)
    };
    let head = take(8)?;
    if head.len() < 8 || &head[0..4] != b"FORM" {
        return None;
    }
    let kind = take(4)?;
    if kind != b"ILBM" && kind != b"PBM " {
        return None;
    }
    let mut bmhd = *last_bmhd;
    loop {
        let head = take(8)?;
        if head.len() < 8 {
            return None;
        }
        let id: [u8; 4] = head[0..4].try_into().unwrap();
        // bytes_reverse4 (1000:0068): the big-endian chunk length.
        let size = u32::from_be_bytes(head[4..8].try_into().unwrap()) as usize;
        match &id {
            b"BODY" => {
                let mut body = take(size)?.to_vec();
                body.resize(size, 0);
                let pic = IffPicture { bmhd, body };
                return (pic.width() <= MAX_WIDTH && pic.height() <= MAX_HEIGHT).then_some(pic);
            }
            b"BMHD" => {
                let b = take(BMHD_LEN)?;
                bmhd[..b.len()].copy_from_slice(b);
            }
            // The palette (into a stack buffer) and the Amiga view mode are read and
            // never used.
            b"CMAP" | b"CAMG" => {
                take(size)?;
            }
            _ => {
                take((size + 1) & !1)?;
            }
        }
    }
}

/// Decodes a `BODY` into `width * height` chunky pixels.
///
/// With 8 planes the rows are chunky (`width` bytes, one "plane" per row); otherwise
/// each row holds `planes` bit planes of `width >> 3` bytes, plane `p` setting bit `p`
/// of the pixel. Compressed rows are ByteRun1: `n >= 0` copies `n + 1` bytes, `n < 0`
/// repeats the next byte `1 - n` times (also for `n = -128`, which ByteRun1 defines as
/// a no-op).
///
/// DEVIATION: a run that overshoots the row makes the original's byte counter wrap and
/// decode on through memory; the port ends the row there. Bytes past the end of the
/// body read as 0, and a height of 0 (65536 rows in the original) decodes nothing.
///
/// Origin: TEAMED.EXE 16b7:382b (ilbm_decode_body)
pub fn decode_body(body: &[u8], width: u16, height: u16, planes: u8, compressed: bool) -> Vec<u8> {
    let at = |i: usize| body.get(i).copied().unwrap_or(0);
    let chunky = planes == 8;
    let row_bytes = usize::from(if chunky { width } else { width >> 3 });
    let row_planes = if chunky { 1 } else { usize::from(planes) };
    let mut buf: Vec<u8> = Vec::with_capacity(row_bytes * row_planes * usize::from(height));
    let mut si = 0usize;
    for _ in 0..height {
        for _ in 0..row_planes {
            if compressed {
                let mut left = row_bytes as i32;
                while left > 0 {
                    let n = at(si) as i8;
                    si += 1;
                    if n >= 0 {
                        let count = n as usize + 1;
                        for _ in 0..count {
                            buf.push(at(si));
                            si += 1;
                        }
                        left -= count as i32;
                    } else {
                        let count = usize::from((n as u8).wrapping_neg()) + 1;
                        let v = at(si);
                        si += 1;
                        buf.extend(std::iter::repeat_n(v, count));
                        left -= count as i32;
                    }
                }
            } else {
                for _ in 0..row_bytes {
                    buf.push(at(si));
                    si += 1;
                }
            }
        }
    }
    let n = usize::from(width) * usize::from(height);
    if chunky {
        buf.resize(buf.len().max(n), 0);
        buf.truncate(n);
        return buf;
    }
    let mut out = vec![0u8; n];
    let bit = |i: usize| buf.get(i).copied().unwrap_or(0);
    let mut si = 0usize;
    for row in 0..usize::from(height) {
        let di = row * usize::from(width);
        let mut dh: u8 = 1;
        for _ in 0..planes {
            let mut dl: u8 = 0x80;
            for x in 0..usize::from(width) {
                if bit(si) & dl != 0 {
                    out[di + x] |= dh;
                }
                if dl == 1 {
                    dl = 0x80;
                    si += 1;
                } else {
                    dl >>= 1;
                }
            }
            dh = dh.wrapping_shl(1);
        }
    }
    out
}

/// Builds a minimal `FORM` file (test and fixture support): `BMHD` with the given size,
/// plane count and compression, then the `BODY`.
pub fn build(
    kind: &[u8; 4],
    width: u16,
    height: u16,
    planes: u8,
    compression: u8,
    body: &[u8],
) -> Vec<u8> {
    build_with_cmap(kind, width, height, planes, compression, None, body)
}

/// [`build`] with an optional `CMAP` chunk between `BMHD` and `BODY`.
pub fn build_with_cmap(
    kind: &[u8; 4],
    width: u16,
    height: u16,
    planes: u8,
    compression: u8,
    cmap: Option<&[u8]>,
    body: &[u8],
) -> Vec<u8> {
    let mut bmhd = [0u8; BMHD_LEN];
    bmhd[0..2].copy_from_slice(&width.to_be_bytes());
    bmhd[2..4].copy_from_slice(&height.to_be_bytes());
    bmhd[8] = planes;
    bmhd[10] = compression;
    bmhd[14] = 1;
    bmhd[15] = 1;
    bmhd[16..18].copy_from_slice(&width.to_be_bytes());
    bmhd[18..20].copy_from_slice(&height.to_be_bytes());
    let mut chunks = Vec::new();
    chunks.extend_from_slice(kind);
    let mut list: Vec<(&[u8; 4], &[u8])> = vec![(b"BMHD", &bmhd[..])];
    if let Some(c) = cmap {
        list.push((b"CMAP", c));
    }
    list.push((b"BODY", body));
    for (id, data) in list {
        chunks.extend_from_slice(id);
        chunks.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunks.extend_from_slice(data);
        if data.len() % 2 == 1 {
            chunks.push(0);
        }
    }
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&(chunks.len() as u32).to_be_bytes());
    out.extend(chunks);
    out
}

/// ByteRun1-compresses one row (test and fixture support; literal runs only where no
/// repeat of at least 3 bytes starts).
pub fn byte_run1(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && row[i + run] == row[i] && run < 128 {
            run += 1;
        }
        if run >= 3 {
            out.push((1 - run as i16) as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        let start = i;
        while i < row.len() && i - start < 128 {
            if i + 2 < row.len() && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planar(pixels: &[u8], w: usize, h: usize, planes: u8) -> Vec<Vec<u8>> {
        let mut rows = Vec::new();
        for y in 0..h {
            for p in 0..planes {
                let mut row = vec![0u8; w / 8];
                for x in 0..w {
                    if pixels[y * w + x] >> p & 1 != 0 {
                        row[x / 8] |= 0x80 >> (x % 8);
                    }
                }
                rows.push(row);
            }
        }
        rows
    }

    #[test]
    fn ilbm_round_trip_compressed_and_raw() {
        let (w, h) = (32usize, 5usize);
        let pixels: Vec<u8> = (0..w * h).map(|i| ((i * 7 + i / w) % 32) as u8).collect();
        let rows = planar(&pixels, w, h, 5);
        let raw: Vec<u8> = rows.concat();
        let packed: Vec<u8> = rows.iter().flat_map(|r| byte_run1(r)).collect();
        for (body, comp) in [(raw, 0u8), (packed, 1)] {
            let file = build(b"ILBM", w as u16, h as u16, 5, comp, &body);
            let pic = read_picture(&file, &[0; BMHD_LEN]).unwrap();
            assert_eq!(pic.width(), 32);
            assert_eq!(pic.pixels(), pixels);
        }
    }

    #[test]
    fn eight_planes_are_chunky() {
        let pixels: Vec<u8> = (0..24u8).collect();
        let body: Vec<u8> = pixels.chunks(6).flat_map(byte_run1).collect();
        let file = build(b"PBM ", 6, 4, 8, 1, &body);
        assert_eq!(
            read_picture(&file, &[0; BMHD_LEN]).unwrap().pixels(),
            pixels
        );
    }

    #[test]
    fn run_of_minus_128_repeats_129_times() {
        let out = decode_body(&[0x80, 7], 129, 1, 8, true);
        assert_eq!(out, vec![7; 129]);
    }

    #[test]
    fn rejects_wrong_form_and_large_pictures() {
        let ok = build(b"ILBM", 320, 200, 1, 0, &[]);
        assert!(read_picture(&ok, &[0; BMHD_LEN]).is_some());
        let wide = build(b"ILBM", 321, 10, 1, 0, &[]);
        assert!(read_picture(&wide, &[0; BMHD_LEN]).is_none());
        let tall = build(b"ILBM", 16, 201, 1, 0, &[]);
        assert!(read_picture(&tall, &[0; BMHD_LEN]).is_none());
        let mut other = ok.clone();
        other[8..12].copy_from_slice(b"ANIM");
        assert!(read_picture(&other, &[0; BMHD_LEN]).is_none());
        assert!(read_picture(b"RIFF", &[0; BMHD_LEN]).is_none());
        // No BODY: the port stops at the end of the file.
        let mut no_body = b"FORM\0\0\0\x04ILBM".to_vec();
        no_body.extend_from_slice(b"CMAP\0\0\0\x03abc");
        assert!(read_picture(&no_body, &[0; BMHD_LEN]).is_none());
    }

    #[test]
    fn skips_unknown_chunks_padded_and_keeps_old_header_without_bmhd() {
        let mut f = b"FORM\0\0\0\0PBM ".to_vec();
        f.extend_from_slice(b"ANNO\0\0\0\x03xyz\0");
        f.extend_from_slice(b"BODY\0\0\0\x04");
        f.extend_from_slice(&[1, 2, 3, 4]);
        let mut last = [0u8; BMHD_LEN];
        last[1] = 2;
        last[3] = 2;
        last[8] = 8;
        let pic = read_picture(&f, &last).unwrap();
        assert_eq!(pic.pixels(), vec![1, 2, 3, 4]);
    }
}
