//! Package "crest": the crest editor of the selected club and the IFF crest import
//! (spec: `docs/re/teamed/crest.md`, to be written by the package).

use bmp_formats::mana;

use crate::gfx::{self, BitmapId, Gfx};
use crate::host::{Host, Run};

use super::{CREST_SIZE, Editor, clear_grid, screen_set_visible, set_font};

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

/// Draws the crest editor in the lower half (the crest view): the frame at the right
/// with `PIC\61.VGA`, the crest `pic` enlarged 4x (5x5 blocks with a 4-pixel pitch)
/// at the left and as a 38x38 preview at (0x9F, 0x7F). With a club selected it also
/// redraws that club's player list from the `MANA.DAT` buffer (`from_copy` false) or
/// from the copy. The screen is off while drawing.
///
/// Origin: TEAMED.EXE 1000:1d73 (crest_editor_draw)
pub fn draw_editor(host: &mut Host, ed: &mut Editor, pic: BitmapId, from_copy: bool) -> Run<()> {
    let g = &mut host.g;
    screen_set_visible(g, false);
    clear_grid(g, &mut ed.grid);
    set_font(&mut ed.grid, 2);
    gfx::ui_fill_rect(g, &mut ed.grid, 0x9e, 0xc, 0x13f, 0xa6, 6);
    gfx::ui_draw_frame(g, &mut ed.grid, 0x9d, 0xc, 0x13f, 0xa6, 0xb);
    if ed.selected != 0xff {
        let m = if from_copy { &ed.copy } else { &ed.mana };
        let players = &m.clubs[usize::from(ed.selected)].players;
        let rp = &mut ed.screen;
        for i in 0..10i16 {
            let (y0, y1) = ((i + 2) * 7, i * 7 + 0x12);
            gfx::ui_fill_rect(g, rp, 0xa3, y0, 0xea, y1, 0xd);
            gfx::ui_fill_rect(g, rp, 0xf6, y0, 0x13d, y1, 0xd);
            let left = mana::c_str(&players[i as usize]).to_vec();
            gfx::ui_draw_text_at(g, rp, &left, 0xa3, y1, 0xb);
            let right = mana::c_str(&players[i as usize + 10]).to_vec();
            gfx::ui_draw_text_at(g, rp, &right, 0xf6, y1, 0xb);
        }
    }
    g.blit_bitmap(
        ed.crest_editor_pic,
        0,
        0,
        ed.grid.bitmap,
        0x9d,
        0xc,
        0xa8,
        0x9b,
    );
    for sx in 0..CREST_SIZE {
        let x0 = sx * 4 + 1;
        for sy in 0..CREST_SIZE {
            let color = g.get_pixel(sx, sy, pic);
            Gfx::rastport_set_color(&mut ed.grid, color as u8);
            let y0 = sy * 4 + 0xd;
            g.rastport_fill_rect(&ed.grid, x0, y0, x0 + 4, y0 + 4);
        }
    }
    gfx::ui_draw_frame(g, &mut ed.grid, 0, 0xc, 0x99, 0xa5, 0xb);
    gfx::ui_fill_rect(g, &mut ed.grid, 0x9e, 0x7e, 0xc5, 0xa5, 0);
    g.blit_bitmap(
        pic,
        0,
        0,
        ed.grid.bitmap,
        0x9f,
        0x7f,
        CREST_SIZE,
        CREST_SIZE,
    );
    screen_set_visible(g, true);
    Ok(())
}

/// "AENDERUNG SICHERN" with a changed crest: writes `PIC\<n>.VGA`.
///
/// TODO(#158) package crest: port 1000:019c (crest_write_vga).
///
/// Origin: TEAMED.EXE 1000:019c (crest_write_vga)
pub fn save_crest(host: &mut Host, ed: &mut Editor) -> Run<()> {
    let _ = (host, ed);
    Ok(())
}
