//! Parser für die Original-Datenformate von Bundesliga Manager Professional.
//!
//! Jedes Modul dokumentiert die Ghidra-Adressen in `BMMAIN.EXE`, aus denen das Format
//! rekonstruiert wurde.

use std::path::PathBuf;

/// Verzeichnis mit den Original-Spieldaten aus der Umgebungsvariable `BMP_DATA`.
/// Tests, die Originaldaten brauchen, werden übersprungen, wenn sie fehlt.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("BMP_DATA").map(PathBuf::from)
}
