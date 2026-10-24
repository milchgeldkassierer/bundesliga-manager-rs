//! Validates the Mod 2026 crest pictures `docs/mod2026/crests/<crest>.png` (PRD R5): one per
//! `crest` of `clubs.json`, each listed in `SOURCES.md`, and in the size and the colour indices
//! of the original crest pictures `PIC/80.VGA`..`PIC/143.VGA`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bmp_formats::{pal, vga};
use serde_json::Value;

const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
const SOURCES_MD: &str = include_str!("../../../docs/mod2026/crests/SOURCES.md");

/// The original crest pictures.
const ORIGINAL_CRESTS: std::ops::RangeInclusive<u8> = 80..=143;

/// The crest PNGs carry the first 32 entries of the main palette, which cover every index the
/// original crests use.
const CREST_PALETTE_ENTRIES: usize = 32;

fn crests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mod2026/crests")
}

/// `crest` -> club `name` of every club in `clubs.json`.
fn club_crests() -> BTreeMap<u8, String> {
    let clubs: Vec<Value> = serde_json::from_str(CLUBS_JSON).expect("clubs.json must parse");
    clubs
        .iter()
        .map(|club| {
            let crest = club["crest"].as_u64().expect("`crest` must be an integer");
            let name = club["name"].as_str().expect("`name` must be a string");
            (u8::try_from(crest).unwrap(), name.to_owned())
        })
        .collect()
}

/// The table rows of `SOURCES.md`: crest, club, logo URL, fetch date.
fn source_rows() -> Vec<[&'static str; 4]> {
    SOURCES_MD
        .lines()
        .filter(|line| line.starts_with("| ") && !line.starts_with("| Crest |"))
        .map(|line| {
            let cells: Vec<&str> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim())
                .collect();
            cells
                .try_into()
                .unwrap_or_else(|_| panic!("SOURCES.md row needs 4 cells: {line}"))
        })
        .collect()
}

fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, &b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        })
}

#[test]
fn one_png_per_club_crest() {
    let expected: BTreeSet<String> = club_crests()
        .keys()
        .map(|crest| format!("{crest}.png"))
        .collect();
    let found: BTreeSet<String> = std::fs::read_dir(crests_dir())
        .expect("docs/mod2026/crests")
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".png"))
        .collect();
    assert_eq!(expected.len(), 64);
    assert_eq!(found, expected);
}

#[test]
fn sources_list_every_crest_once() {
    let clubs = club_crests();
    let rows = source_rows();
    assert_eq!(rows.len(), clubs.len());
    let mut listed = BTreeSet::new();
    for [crest, club, url, fetched] in rows {
        let crest: u8 = crest.parse().unwrap_or_else(|_| panic!("crest `{crest}`"));
        assert!(listed.insert(crest), "crest {crest} listed twice");
        assert_eq!(
            Some(club),
            clubs.get(&crest).map(String::as_str),
            "crest {crest}"
        );
        assert!(url.starts_with("https://"), "crest {crest}: URL `{url}`");
        assert!(is_iso_date(fetched), "crest {crest}: date `{fetched}`");
    }
}

#[test]
fn crests_have_the_original_size_and_colour_indices() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    let original = |crest: u8| {
        let path = data.join(vga::file_name(crest));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        vga::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
    };
    let size = {
        let first = original(*ORIGINAL_CRESTS.start());
        (u32::from(first.width), u32::from(first.height))
    };
    let allowed: BTreeSet<u8> = ORIGINAL_CRESTS
        .flat_map(|crest| original(crest).pixels)
        .collect();
    let pal_file = pal::PalFile::load(&data).expect("PIC/1.PAL");
    let palette = pal_file.palettes[pal::PALETTE_MAIN].to_rgb8()[..CREST_PALETTE_ENTRIES].concat();

    for crest in club_crests().keys() {
        let path = crests_dir().join(format!("{crest}.png"));
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{crest}.png: {e}"));
        let mut reader = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .unwrap_or_else(|e| panic!("{crest}.png: {e}"));
        let info = reader.info();
        assert_eq!((info.width, info.height), size, "{crest}.png size");
        assert_eq!(
            (info.color_type, info.bit_depth),
            (png::ColorType::Indexed, png::BitDepth::Eight),
            "{crest}.png must be 8-bit indexed"
        );
        assert_eq!(
            info.palette.as_deref(),
            Some(palette.as_slice()),
            "{crest}.png palette must be entries 0..{CREST_PALETTE_ENTRIES} of 1.PAL"
        );
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader
            .next_frame(&mut pixels)
            .unwrap_or_else(|e| panic!("{crest}.png: {e}"));
        let stray: BTreeSet<u8> = pixels
            .iter()
            .copied()
            .filter(|index| !allowed.contains(index))
            .collect();
        assert!(
            stray.is_empty(),
            "{crest}.png uses indices {stray:?} that no original crest uses"
        );
    }
}
