//! Validates the crest pictures `PIC/<n>.VGA` that `bmp mod2026 build` writes (PRD R5, R6):
//! each committed `docs/mod2026/crests/<n>.png` decodes from the built file with its exact size
//! and palette indices, the other pictures keep the original bytes, and two builds write the
//! same files. Skipped without `BMP_DATA`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use bmp_formats::vga;
use bmp_game::mod2026;

fn crests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mod2026/crests")
}

/// Every `<n>.png` in `docs/mod2026/crests`, read from the checkout (not the embedded copy).
fn committed_crests() -> BTreeMap<u8, Vec<u8>> {
    fs::read_dir(crests_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .map(|path| {
            let n = path.file_stem().unwrap().to_str().unwrap().parse().unwrap();
            (n, fs::read(&path).unwrap())
        })
        .collect()
}

/// Builds the mod data from `data` into a fresh temp directory and returns its `PIC/*.VGA`
/// files as (file name, bytes).
fn built_pictures(data: &Path, name: &str) -> BTreeMap<String, Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("bmp-mod2026-vga-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    mod2026::build(data, &dir).unwrap();
    let pictures = pictures(&dir.join("PIC"));
    fs::remove_dir_all(&dir).unwrap();
    pictures
}

fn pictures(pic: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(pic)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_name().to_str().unwrap().ends_with(".VGA"))
        .map(|entry| {
            let name = entry.file_name().into_string().unwrap();
            (name, fs::read(entry.path()).unwrap())
        })
        .collect()
}

#[test]
fn every_committed_crest_is_embedded() {
    let embedded: BTreeMap<u8, Vec<u8>> = mod2026::CRESTS
        .iter()
        .map(|&(n, png)| (n, png.to_vec()))
        .collect();
    assert_eq!(embedded, committed_crests());
}

#[test]
fn built_crests_decode_to_the_png_indices() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    let built = built_pictures(&data, "decode");
    let crests = committed_crests();
    assert_eq!(crests.len(), 64);
    for (n, png) in &crests {
        let name = format!("{n}.VGA");
        let image = vga::parse(&built[&name]).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
        let (width, height) = (reader.info().width, reader.info().height);
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut pixels).unwrap();
        assert_eq!(
            (u32::from(image.width), u32::from(image.height)),
            (width, height),
            "{name} size"
        );
        assert_eq!(image.pixels, pixels, "{name} pixels");
    }
    // Pictures without a crest PNG keep the copied original bytes, except the icon sheet
    // 3.VGA with the 3. LIGA captions (checked in `mod2026`'s unit tests).
    for (name, bytes) in pictures(&data.join("PIC")) {
        let n: u8 = name.trim_end_matches(".VGA").parse().unwrap();
        if !crests.contains_key(&n) && n != vga::ICON_SHEET {
            assert_eq!(built[&name], bytes, "{name} must stay the original");
        }
    }
}

#[test]
fn two_builds_write_identical_pictures() {
    let Some(data) = bmp_formats::data_dir() else {
        return;
    };
    let a = built_pictures(&data, "a");
    assert!(a.len() >= 64);
    assert_eq!(a, built_pictures(&data, "b"));
}
