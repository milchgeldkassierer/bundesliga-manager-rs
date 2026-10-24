//! Package "crest": the crest editor of the selected club and the IFF crest import
//! (spec: `docs/re/teamed/crest.md`, to be written by the package).

use crate::host::{Host, Run};

use super::Editor;

/// Clicks in the crest editor (the lower half while a club is selected): painting in
/// the enlarged crest, the colour palette, "Iff-Bild laden", "Undo", "Ur-Wappen".
///
/// TODO(#158) package crest: port 12a5:1684..1fad of the main loop (rastport_get_pixel /
/// put_pixel on the 4x4 enlarged crest, the colour picker) and the import dialog
/// `1579:1215` with its browser `1579:05b7` and the IFF reader `1000:0268`/`04f1`.
///
/// Origin: TEAMED.EXE 12a5:1684 (teamed_main_loop, crest editor)
pub fn click(host: &mut Host, ed: &mut Editor, x: i16, y: i16) -> Run<()> {
    let _ = (host, ed, x, y);
    Ok(())
}

/// Draws the crest editor for the selected club in the lower half of the screen
/// (enlarged crest, palette, buttons, preview). Called when a club is selected.
///
/// TODO(#158) package crest: port 1000:1d73 (crest_editor_draw).
///
/// Origin: TEAMED.EXE 1000:1d73 (crest_editor_draw)
#[allow(dead_code)] // TODO(#158): called by package club when a club is selected.
pub fn draw_editor(host: &mut Host, ed: &mut Editor) -> Run<()> {
    let _ = (host, ed);
    Ok(())
}

/// "AENDERUNG SICHERN" with a changed crest: writes `PIC\<n>.VGA`.
///
/// TODO(#158) package crest: port 1000:019c (crest_write_vga).
///
/// Origin: TEAMED.EXE 1000:019c (crest_write_vga)
#[allow(dead_code)] // TODO(#158): called by package club from its buttons.
pub fn save_crest(host: &mut Host, ed: &mut Editor) -> Run<()> {
    let _ = (host, ed);
    Ok(())
}
