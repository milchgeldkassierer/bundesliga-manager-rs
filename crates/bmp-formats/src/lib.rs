//! Parsers for the original data formats of Bundesliga Manager Professional.
//!
//! Every module documents the Ghidra addresses in `BMMAIN.EXE` its format was
//! reconstructed from.

pub mod cmf;
pub mod cp;
pub mod exepack;
pub mod font;
pub mod iff;
pub mod mana;
pub mod pal;
pub mod savegame;
pub mod tore;
pub mod vga;
pub mod voc;

use std::path::PathBuf;

/// Directory holding the original game data, taken from the `BMP_DATA` environment
/// variable. Tests that need original data are skipped when it is unset.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("BMP_DATA").map(PathBuf::from)
}

/// Test support: every `.MAN` save in `BMP_DATA` and `BMP_DATA/BMP`, sorted;
/// empty when `BMP_DATA` is unset, so the real-save tests that loop over it skip.
///
/// # Panics
/// When `BMP_DATA` is set but holds no `.MAN` save, so a misconfigured data
/// directory fails the real-save tests instead of passing them with zero saves.
pub fn save_files() -> Vec<PathBuf> {
    let Some(dir) = data_dir() else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for d in [dir.clone(), dir.join("BMP")] {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("man")) {
                v.push(p);
            }
        }
    }
    assert!(
        !v.is_empty(),
        "BMP_DATA ({}) has no .MAN save",
        dir.display()
    );
    v.sort();
    v
}
