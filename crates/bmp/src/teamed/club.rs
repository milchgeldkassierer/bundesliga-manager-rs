//! Package "club": selecting a club and editing its data on the main screen (spec:
//! `docs/re/teamed/club.md`, to be written by the package).

use crate::host::{Host, Run};

use super::Editor;

/// Click on a crest with no club selected: select it.
///
/// TODO(#158) package club: port 12a5:11b7..12db (the crest goes into both boxes beside
/// the buttons, `DS:0336` = club, copies of its data) and open the crest editor with
/// [`super::crest::draw_editor`] (package crest).
///
/// Origin: TEAMED.EXE 12a5:11b7 (teamed_main_loop, select club)
pub fn select(host: &mut Host, ed: &mut Editor) -> Run<()> {
    let _ = (host, ed);
    Ok(())
}

/// Clicks on the information panel while a club is selected.
///
/// TODO(#158) package club: port 12a5:056b..07a1 (name, league swap with "MIT WEM
/// TAUSCHEN ?", rank, points and goals with `1000:207d`, `1000:26f1`).
///
/// Origin: TEAMED.EXE 12a5:056b (teamed_main_loop, panel editing)
pub fn edit_panel(host: &mut Host, ed: &mut Editor, x: i16, y: i16, ev: u16) -> Run<()> {
    let _ = (host, ed, x, y, ev);
    Ok(())
}

/// "NICHT UEBERNEHMEN" / "AENDERUNG SICHERN" while a club is selected.
///
/// TODO(#158) package club: port 12a5:1361..1544 (copies back, crest file via
/// `1000:019c` from package crest).
///
/// Origin: TEAMED.EXE 12a5:1361 (teamed_main_loop, club buttons)
pub fn buttons(host: &mut Host, ed: &mut Editor, y: i16) -> Run<()> {
    let _ = (host, ed, y);
    Ok(())
}

/// Click on a player row of the selected club.
///
/// TODO(#158) package club: port 12a5:202e..22ab and the player swap of
/// 12a5:0aba..0d6e (`1000:012b`).
///
/// Origin: TEAMED.EXE 12a5:202e (teamed_main_loop, player names)
pub fn edit_player(host: &mut Host, ed: &mut Editor, x: i16, y: i16) -> Run<()> {
    let _ = (host, ed, x, y);
    Ok(())
}
