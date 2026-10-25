//! Validates the 2x crests `docs/mod2026/crests/hires/<crest>.png` (PRD R5b): one per crest,
//! embedded as `HIRES_CRESTS`, exactly twice the crest's width and height, with the crest's
//! palette and only colour indices the crests use, and closer to its own crest than to any other.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bmp_game::mod2026::{CRESTS, HIRES_CRESTS};

fn crests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mod2026/crests")
}

/// The picture numbers of the `<n>.png` files directly in `dir`.
fn png_numbers(dir: &Path) -> BTreeSet<u8> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".png").map(|n| n.parse().unwrap())
        })
        .collect()
}

/// Size, palette and pixels of an 8-bit indexed PNG.
struct Indexed {
    size: (u32, u32),
    palette: Vec<u8>,
    pixels: Vec<u8>,
}

fn read(path: &Path) -> Indexed {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let info = reader.info();
    assert_eq!(
        (info.color_type, info.bit_depth),
        (png::ColorType::Indexed, png::BitDepth::Eight),
        "{} must be 8-bit indexed",
        path.display()
    );
    let size = (info.width, info.height);
    let palette = info.palette.as_deref().unwrap_or_default().to_vec();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut pixels)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    pixels.truncate(frame.buffer_size());
    Indexed {
        size,
        palette,
        pixels,
    }
}

#[test]
fn every_committed_hires_crest_is_embedded() {
    let embedded: BTreeSet<u8> = HIRES_CRESTS.iter().map(|&(n, _)| n).collect();
    assert_eq!(embedded, png_numbers(&crests_dir().join("hires")));
    for &(n, png) in HIRES_CRESTS {
        let path = crests_dir().join("hires").join(format!("{n}.png"));
        assert_eq!(png, std::fs::read(&path).unwrap(), "{}", path.display());
    }
}

#[test]
fn hires_crests_are_twice_the_crests_in_the_crest_colours() {
    let numbers = png_numbers(&crests_dir().join("hires"));
    assert_eq!(numbers.len(), 64);
    assert_eq!(numbers, CRESTS.iter().map(|&(n, _)| n).collect());
    let crests: Vec<Indexed> = numbers
        .iter()
        .map(|n| read(&crests_dir().join(format!("{n}.png"))))
        .collect();
    let crest_indices: BTreeSet<u8> = crests.iter().flat_map(|c| c.pixels.clone()).collect();
    for (n, crest) in numbers.iter().zip(&crests) {
        let hires = read(&crests_dir().join("hires").join(format!("{n}.png")));
        assert_eq!(
            hires.size,
            (2 * crest.size.0, 2 * crest.size.1),
            "hires/{n}.png size"
        );
        assert_eq!(hires.palette, crest.palette, "hires/{n}.png palette");
        let stray: BTreeSet<u8> = hires
            .pixels
            .iter()
            .copied()
            .filter(|index| !crest_indices.contains(index))
            .collect();
        assert!(
            stray.is_empty(),
            "hires/{n}.png uses indices {stray:?} that no crest uses"
        );
    }
}

/// How many pixels of `hires` differ from `crest` drawn at twice its size.
fn differing_pixels(hires: &Indexed, crest: &Indexed) -> usize {
    let (w, h) = crest.size;
    (0..2 * h)
        .flat_map(|y| (0..2 * w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            hires.pixels[(y * 2 * w + x) as usize] != crest.pixels[(y / 2 * w + x / 2) as usize]
        })
        .count()
}

/// The 2x versions are redrawn from larger sources, so no pixel threshold separates a right
/// from a wrong one; each must still look more like its own crest than like any other crest.
#[test]
fn every_hires_crest_is_closest_to_its_own_crest() {
    let crests: Vec<(u8, Indexed)> = png_numbers(&crests_dir())
        .into_iter()
        .map(|n| (n, read(&crests_dir().join(format!("{n}.png")))))
        .collect();
    for (n, crest) in &crests {
        let hires = read(&crests_dir().join("hires").join(format!("{n}.png")));
        let own = differing_pixels(&hires, crest);
        for (m, other) in crests
            .iter()
            .filter(|(m, c)| m != n && c.size == crest.size)
        {
            let theirs = differing_pixels(&hires, other);
            assert!(
                own < theirs,
                "hires/{n}.png differs from crest {n} in {own} pixels but from crest {m} in only {theirs}"
            );
        }
    }
}
