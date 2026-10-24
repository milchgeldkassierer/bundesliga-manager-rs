//! "PROGRAMM-ENDE": the save question and writing `MANA.DAT`.

use bmp_game::strength::recompute_strengths;

use crate::host::{Host, Run};

use super::Editor;
use super::dialog::{self, Choice};

/// Outcome of [`teamed_save_mana`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveResult {
    /// "MACH MAL !": the file was written; the program ends.
    Saved,
    /// "OH NEIN !": nothing written; the program ends.
    NotSaved,
    /// Click outside the question (9): back to the editor.
    Cancelled,
}

/// Asks "Daten / unwiederbringlich / abspeichern ?" and on "MACH MAL !" recomputes the
/// strengths of the German clubs and writes all blocks of `MANA.DAT`.
///
/// Origin: TEAMED.EXE 1579:0030 (teamed_save_mana)
pub fn teamed_save_mana(host: &mut Host, ed: &mut Editor) -> Run<SaveResult> {
    let choice = dialog::two_choice(
        host,
        ed,
        [b"Daten", b"unwiederbringlich", b"abspeichern ?"],
        b"MACH MAL !",
        b"OH NEIN !",
    )?;
    match choice {
        Choice::Right => return Ok(SaveResult::NotSaved),
        Choice::Outside => return Ok(SaveResult::Cancelled),
        Choice::Left => {}
    }
    recompute_strengths(&mut ed.mana, &mut ed.rng);
    let path = host.data.join("MANA.DAT");
    // The original ignores write errors (file_open/file_write results are not checked
    // after the open succeeded); the port reports them on stderr and carries on.
    if let Err(e) = std::fs::write(&path, ed.mana.to_bytes()) {
        eprintln!("{}: {e}", path.display());
    }
    Ok(SaveResult::Saved)
}
