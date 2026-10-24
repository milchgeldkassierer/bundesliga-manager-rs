//! Writes a test IFF picture for the crest-import scenario (`tests/visual/teamed_crest.*`).
//!
//! Usage: `iff_fixture <out> <kind>` with kind
//! * `ilbm`: 96x64, 5 bit planes, ByteRun1 (the import's usual case);
//! * `pbm`: 48x40, 8 planes (chunky), uncompressed;
//! * `wide`: 336x16, 5 planes: wider than 320, refused ("FALSCHES GRAFIKFORMAT.").
//!
//! Every file has a 256-colour `CMAP`. The editor ignores the palette, but its bytes
//! 0x2F8..0x2FB (colours 253 and 254) end up in the stack slot the converter
//! (`1000:04f1`) uses as its uninitialised bitmap descriptor pointer: they are set to
//! `0050:0010`, a free spot of the DOS data area, so the original draws the picture
//! (see `docs/re/teamed/crest.md`).
use bmp_formats::iff;

fn pattern(w: usize, h: usize) -> Vec<u8> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let ring = ((x as i32 - 30).pow(2) + (y as i32 - 28).pow(2)) / 90;
            ((x / 5 + y / 7 + ring as usize * 3) % 31 + 1) as u8
        })
        .collect()
}

fn planar_rows(px: &[u8], w: usize, h: usize, planes: u8) -> Vec<Vec<u8>> {
    let mut rows = Vec::new();
    for y in 0..h {
        for p in 0..planes {
            let mut row = vec![0u8; w / 8];
            for x in 0..w {
                if px[y * w + x] >> p & 1 != 0 {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
            rows.push(row);
        }
    }
    rows
}

/// The 256-colour palette: a grey ramp with the descriptor pointer in colours 253/254.
fn cmap() -> Vec<u8> {
    let mut c: Vec<u8> = (0..768).map(|i| (i / 3) as u8).collect();
    c[0x2f8..0x2fc].copy_from_slice(&[0x10, 0x00, 0x50, 0x00]);
    c
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (out, kind) = (&args[1], args[2].as_str());
    let bytes = match kind {
        "ilbm" | "wide" => {
            let (w, h) = if kind == "ilbm" { (96, 64) } else { (336, 16) };
            let rows = planar_rows(&pattern(w, h), w, h, 5);
            let body: Vec<u8> = rows.iter().flat_map(|r| iff::byte_run1(r)).collect();
            iff::build_with_cmap(b"ILBM", w as u16, h as u16, 5, 1, Some(&cmap()), &body)
        }
        "pbm" => {
            let px: Vec<u8> = pattern(48, 40).iter().map(|v| 31 - v).collect();
            iff::build_with_cmap(b"PBM ", 48, 40, 8, 0, Some(&cmap()), &px)
        }
        _ => panic!("unknown kind {kind}"),
    };
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(out, bytes).unwrap();
}
