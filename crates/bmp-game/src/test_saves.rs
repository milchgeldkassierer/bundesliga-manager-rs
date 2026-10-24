//! The `.MAN` saves under `BMP_DATA` for tests against real game states.

use crate::state::GameState;
use bmp_formats::savegame::SaveGame;
use std::path::PathBuf;

/// Every `.MAN` save in `BMP_DATA` and `BMP_DATA/BMP`, parsed; empty when
/// `BMP_DATA` is unset.
///
/// # Panics
/// When `BMP_DATA` is set but holds no save, or a save does not parse.
pub(crate) fn save_states() -> Vec<(PathBuf, GameState)> {
    let Some(dir) = bmp_formats::data_dir() else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for d in [dir.clone(), dir.join("BMP")] {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("man")) {
                let data = std::fs::read(&p).unwrap();
                let save = SaveGame::parse(&data).unwrap();
                v.push((p, GameState::from_savegame(&save).unwrap()));
            }
        }
    }
    assert!(!v.is_empty(), "BMP_DATA has no .MAN save");
    v
}
