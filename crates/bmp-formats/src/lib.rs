//! Parsers for the original data formats of Bundesliga Manager Professional.
//!
//! Every module documents the Ghidra addresses in `BMMAIN.EXE` its format was
//! reconstructed from.

pub mod cp;
pub mod exepack;
pub mod font;
pub mod mana;
pub mod pal;
pub mod savegame;
pub mod tore;
pub mod vga;

use std::path::PathBuf;

/// Directory holding the original game data, taken from the `BMP_DATA` environment
/// variable. Tests that need original data are skipped when it is unset.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("BMP_DATA").map(PathBuf::from)
}
