//! "PROGRAMM-ENDE": the save question and writing `MANA.DAT`.

use bmp_formats::mana::ManaDat;

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

/// The strength bytes of clubs 0..63 from the points, goals and rank of the previous
/// season (and `random_range`), see `docs/re/mana.md` "Block 2".
///
/// TODO(#158) package uefa: port 1579:006d..04b5 (seven `random_range` calls from
/// 1579:01d5 on; `rand()` is never seeded, so `Editor::rng` starts in the MSC start
/// state). The skeleton leaves the strengths unchanged.
///
/// Origin: TEAMED.EXE 1579:0030 (teamed_save_mana, strength part)
pub fn recompute_strengths(m: &mut ManaDat, rng: &mut bmp_game::rng::Rng) {
    let _ = (m, rng);
}
