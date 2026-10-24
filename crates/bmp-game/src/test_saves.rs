//! The `.MAN` saves under `BMP_DATA` for tests against real game states.

use crate::state::GameState;
use bmp_formats::savegame::SaveGame;
use std::path::PathBuf;

/// Every save of [`bmp_formats::save_files`], parsed; empty when `BMP_DATA`
/// is unset.
///
/// # Panics
/// When `BMP_DATA` is set but holds no save, or a save does not parse.
pub(crate) fn save_states() -> Vec<(PathBuf, GameState)> {
    bmp_formats::save_files()
        .into_iter()
        .map(|p| {
            let data = std::fs::read(&p).unwrap();
            let save = SaveGame::parse(&data).unwrap();
            let state = GameState::from_savegame(&save).unwrap();
            (p, state)
        })
        .collect()
}
