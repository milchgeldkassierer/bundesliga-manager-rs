//! Package "crest": the crest editor of the selected club (spec:
//! `docs/re/teamed/crest.md`).
//!
//! While a club is selected the crest grid gives way to the editor (`1000:1d73`): the
//! working copy of the crest enlarged 4x4 at the left, the 32 colour fields of `PIC\61`
//! with the buttons "Iff-Bild laden", "Undo" and "Ur-Wappen", and a 38x38 preview.
//! The right button picks the colour under the pointer in the enlarged crest, the left
//! button paints with it while held, or picks a colour field. "AENDERUNG SICHERN"
//! writes a changed crest to `PIC\<n>.VGA` ([`save_crest`]). The picture import lives
//! in [`super::crest_import`].

use std::path::Path;

use bmp_formats::vga;

use crate::gfx::{self, BitmapId, Gfx};
use crate::host::{Host, LEFT_DOWN, LEFT_UP, RIGHT_DOWN, Run};

use super::{CREST_SIZE, Editor, crest_import, screen_set_visible, set_font};

/// The enlarged crest: `(1, 0xD)` of the view, 4x4 pixels per crest pixel, framed by
/// `(0, 0xC)-(0x99, 0xA5)`. Clicks count inside the frame (`1 < x < 0x99`,
/// `0xC < y < 0xA5`).
pub const ZOOM_X0: i16 = 1;
pub const ZOOM_Y0: i16 = 0xd;
/// Top left of the preview (the crest at its real size) in the view.
pub const PREVIEW_X: i16 = 0x9f;
pub const PREVIEW_Y: i16 = 0x7f;
/// Colour fields: 8 columns of 0x14, 4 rows of 0x10 from `(0x9E, 0xD)` (`PIC\61`).
pub const FIELD_X0: i16 = 0x9e;
/// Colour of the frame around the chosen colour field.
const FIELD_FRAME: u8 = 0x1d;

/// Rows of the right half (`(y - 0xD) / 0x10`): 0..3 the colour fields, then the
/// three buttons.
const ROW_IMPORT: i16 = 4;
const ROW_UNDO: i16 = 5;
const ROW_ORIGINAL: i16 = 6;

/// The frame of colour field `c` (`(c % 8) * 0x14 + 0x9F`, `(c / 8) * 0x10 + 0xE`, 0x11
/// x 0xD), C division (colours above 31 land below the fields).
pub fn field_frame(c: i16) -> (i16, i16, i16, i16) {
    let (col, row) = (c % 8, c / 8);
    (
        col * 0x14 + 0x9f,
        row * 0x10 + 0xe,
        col * 0x14 + 0xb0,
        row * 0x10 + 0x1b,
    )
}

/// Inside the enlarged crest (`1 < x < 0x99`, `0xC < y < 0xA5`, view coordinates).
pub fn in_zoom(x: i16, y: i16) -> bool {
    x > 1 && x < 0x99 && y > 0xc && y < 0xa5
}

/// The crest pixel under view position `(x, y)` of the enlarged crest
/// (`(x - 1) >> 2`, `(y - 0xD) >> 2`).
pub fn zoom_cell(x: i16, y: i16) -> (i16, i16) {
    ((x - ZOOM_X0) >> 2, (y - ZOOM_Y0) >> 2)
}

/// Draws the crest editor in the lower half: clears the grid, the panel background
/// with the player list of the selected club (from the copies when `use_copy`), the
/// background picture `PIC\61` at `(0x9D, 0xC)`, `crest` enlarged 4x4 with its frame,
/// and the preview box with `crest` at `(0x9F, 0x7F)`. The screen is off meanwhile.
/// The selection draws the club's crest picture with the `MANA.DAT` player names
/// (`use_copy` false); the editor itself draws the working copy with the copies.
///
/// Origin: TEAMED.EXE 1000:1d73 (crest_editor_draw)
pub fn draw_editor(host: &mut Host, ed: &mut Editor, crest: BitmapId, use_copy: bool) -> Run<()> {
    let g = &mut host.g;
    screen_set_visible(g, false);
    super::clear_grid(g, &mut ed.grid);
    set_font(&mut ed.grid, 2);
    gfx::ui_fill_rect(g, &mut ed.grid, 0x9e, 0xc, 0x13f, 0xa6, 6);
    gfx::ui_draw_frame(g, &mut ed.grid, 0x9d, 0xc, 0x13f, 0xa6, 0xb);
    if ed.selected != 0xff {
        let m = if use_copy { &ed.copy } else { &ed.mana };
        let players = &m.clubs[usize::from(ed.selected)].players;
        let rp = &mut ed.screen;
        for i in 0..10i16 {
            let (y0, y1) = ((i + 2) * 7, i * 7 + 0x12);
            gfx::ui_fill_rect(g, rp, 0xa3, y0, 0xea, y1, 0xd);
            gfx::ui_fill_rect(g, rp, 0xf6, y0, 0x13d, y1, 0xd);
            let left = bmp_formats::mana::c_str(&players[i as usize]).to_vec();
            gfx::ui_draw_text_at(g, rp, &left, 0xa3, y1, 0xb);
            let right = bmp_formats::mana::c_str(&players[i as usize + 10]).to_vec();
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
    let crest_rp = gfx::RastPort::new(crest);
    for cx in 0..CREST_SIZE {
        let x0 = cx * 4 + ZOOM_X0;
        for cy in 0..CREST_SIZE {
            let c = g.rastport_get_pixel(&crest_rp, cx, cy);
            Gfx::rastport_set_color(&mut ed.grid, c as u8);
            let y0 = cy * 4 + ZOOM_Y0;
            g.rastport_fill_rect(&ed.grid, x0, y0, x0 + 4, y0 + 4);
        }
    }
    gfx::ui_draw_frame(g, &mut ed.grid, 0, 0xc, 0x99, 0xa5, 0xb);
    gfx::ui_fill_rect(g, &mut ed.grid, 0x9e, 0x7e, 0xc5, 0xa5, 0);
    g.blit_bitmap(
        crest,
        0,
        0,
        ed.grid.bitmap,
        PREVIEW_X,
        PREVIEW_Y,
        CREST_SIZE,
        CREST_SIZE,
    );
    screen_set_visible(g, true);
    Ok(())
}

/// Frames colour field `c` in `color` (0 erases the frame of the chosen colour).
fn frame_field(g: &mut Gfx, ed: &mut Editor, c: i16, color: u8) {
    let (x0, y0, x1, y1) = field_frame(c);
    gfx::ui_draw_frame(g, &mut ed.grid, x0, y0, x1, y1, color);
}

/// Chooses the colour field in `row` (0..3) and column `col`: erases the frame of the
/// previous colour (if any), frames the field at `(row, col)` and takes colour
/// `row * 8 + col`.
///
/// The new frame is placed by row and column, not by the colour: column 8 (`x` 318 or
/// 319) frames `(0x13F, row * 0x10 + 0xE)` at the right edge (one visible column) while
/// the colour is `row * 8 + 8`, whose own field frame (erased by the next choice) lies
/// one row lower.
///
/// Origin: TEAMED.EXE 12a5:1a49 (teamed_main_loop, crest editor: colour field)
fn choose_color(g: &mut Gfx, ed: &mut Editor, row: i16, col: i16) {
    if ed.crest_color != -1 {
        frame_field(g, ed, ed.crest_color, 0);
    }
    let (x0, y0) = (col * 0x14 + 0x9f, row * 0x10 + 0xe);
    gfx::ui_draw_frame(g, &mut ed.grid, x0, y0, x0 + 0x11, y0 + 0xd, FIELD_FRAME);
    ed.crest_color = row * 8 + col;
}

/// The crest-editor part of the main loop for event `ev` at `(x, y)` (view
/// coordinates). Returns true when the original jumps to the end of the loop
/// iteration (`12a5:22AE`), so the player-name check that follows does not run.
///
/// * right button in the enlarged crest: picks the colour under the pointer;
/// * left button in the enlarged crest: saves the working copy for "Undo" and paints
///   one crest pixel (4x4 in the enlarged crest, 1 pixel in the preview) at every
///   pointer position until the button is released, then marks the crest changed and
///   goes on with the release position;
/// * left button right of `x = 0x9E` (`0xC < y < 0x9C`), row `(y - 0xD) / 0x10`: 0..3
///   a colour field, 4 "Iff-Bild laden", 5 "Undo", 6 "Ur-Wappen".
///
/// Origin: TEAMED.EXE 12a5:1671 (teamed_main_loop, crest editor, up to 12a5:2028)
pub fn click(host: &mut Host, ed: &mut Editor, ev: u16, mut x: i16, mut y: i16) -> Run<bool> {
    if ed.selected == 0xff {
        return Ok(false);
    }
    if ev == RIGHT_DOWN && in_zoom(x, y) {
        // 12a5:16d0: the colour under the pointer (rastport_get_pixel of the view).
        let c = host.g.rastport_get_pixel(&ed.grid, x, y) as i16;
        let old = ed.crest_color;
        ed.crest_color = c;
        if old != -1 {
            frame_field(&mut host.g, ed, old, 0);
        }
        frame_field(&mut host.g, ed, c, FIELD_FRAME);
        return Ok(true);
    }
    let [work, undo] = ed.scratch;
    if ev == LEFT_DOWN && in_zoom(x, y) {
        // 12a5:1821: Undo keeps the crest from before this stroke.
        host.g
            .blit_bitmap(work, 0, 0, undo, 0, 0, CREST_SIZE, CREST_SIZE);
        let mut work_rp = gfx::RastPort::new(work);
        loop {
            if in_zoom(x, y) {
                let (px, py) = zoom_cell(x, y);
                let c = ed.crest_color as u8;
                Gfx::rastport_set_color(&mut ed.grid, c);
                Gfx::rastport_set_color(&mut work_rp, c);
                host.g.rastport_fill_rect(
                    &ed.grid,
                    px * 4 + 1,
                    py * 4 + 0xd,
                    (px + 1) * 4,
                    (py + 4) * 4,
                );
                host.g
                    .rastport_put_pixel(&ed.grid, px + PREVIEW_X, py + PREVIEW_Y);
                host.g.rastport_put_pixel(&work_rp, px, py);
            }
            host.input_poll_mouse()?;
            let (mx, my) = host.mouse_xy();
            (x, y) = (mx, my.wrapping_sub(super::GRID_ORIGIN_Y));
            if host.button == LEFT_UP {
                break;
            }
        }
        ed.crest_changed = true;
    }
    if !(ev == LEFT_DOWN && x > 0x9e && y > 0xc && y < 0x9c) {
        return Ok(false);
    }
    let row = (y - 0xd) / 0x10;
    match row {
        0..=3 => {
            // 12a5:1a49: a colour field.
            let col = (x - FIELD_X0) / 0x14;
            choose_color(&mut host.g, ed, row, col);
        }
        ROW_IMPORT => crest_import::import(host, ed)?,
        ROW_UNDO => {
            // 12a5:1f7d
            host.g
                .blit_bitmap(undo, 0, 0, work, 0, 0, CREST_SIZE, CREST_SIZE);
            draw_editor(host, ed, work, true)?;
        }
        ROW_ORIGINAL => {
            // 12a5:1fb8: the crest picture as loaded (or as last saved).
            let crest = ed.crests[usize::from(ed.selected)];
            host.g
                .blit_bitmap(crest, 0, 0, work, 0, 0, CREST_SIZE, CREST_SIZE);
            draw_editor(host, ed, work, true)?;
            ed.crest_changed = false;
        }
        _ => {}
    }
    Ok(true)
}

/// "AENDERUNG SICHERN" with a changed crest (`[BP-DA]`): writes the working copy to
/// `PIC\<crest number>.VGA` and copies it into the crest picture of the club. Does
/// nothing when the crest was not changed.
///
/// Origin: TEAMED.EXE 12a5:1496 (teamed_main_loop, AENDERUNG SICHERN: crest)
pub fn save_crest(host: &mut Host, ed: &mut Editor) -> Run<()> {
    if !ed.crest_changed {
        return Ok(());
    }
    let sel = usize::from(ed.selected);
    let n = ed.mana.crest[sel];
    let work = ed.scratch[0];
    crest_write_vga(&host.g, work, &host.data, n);
    host.g
        .blit_bitmap(work, 0, 0, ed.crests[sel], 0, 0, CREST_SIZE, CREST_SIZE);
    Ok(())
}

/// The `.VGA` file of a 38x38 crest: width, height, packed length and the game's RLE
/// stream (`16b7:3195`, the library's encoder, = [`vga::encode_rle`]).
///
/// The encoder can read one byte past the 0x5A4 pixels (see
/// [`vga::encode_rle_with_past_end`]); in the editor that byte is the first one after
/// the picture's pixel block in the far heap, which never matches as far as observed.
///
/// Origin: TEAMED.EXE 1000:019c (crest_write_vga)
pub fn crest_file_bytes(pixels: &[u8]) -> Vec<u8> {
    let rle = vga::encode_rle(pixels);
    let mut out = Vec::with_capacity(6 + rle.len());
    out.extend_from_slice(&(CREST_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&(CREST_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&(rle.len() as u16).to_le_bytes());
    out.extend(rle);
    out
}

/// Writes `pic\<n>.vga` (the existing file of any spelling, else `PIC/<n>.VGA`). The
/// original ignores write errors; the port reports them on stderr.
///
/// Origin: TEAMED.EXE 1000:019c (crest_write_vga)
fn crest_write_vga(g: &Gfx, bm: BitmapId, data: &Path, n: u8) {
    let pixels: Vec<u8> = (0..CREST_SIZE)
        .flat_map(|y| (0..CREST_SIZE).map(move |x| (x, y)))
        .map(|(x, y)| g.get_pixel(x, y, bm) as u8)
        .collect();
    let dir = crate::game::load_game::find_case_insensitive(&data.join("PIC"))
        .unwrap_or_else(|| data.join("PIC"));
    let path = dir.join(format!("{n}.VGA"));
    let path = crate::game::load_game::find_case_insensitive(&path).unwrap_or(path);
    if let Err(e) = std::fs::write(&path, crest_file_bytes(&pixels)) {
        eprintln!("{}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_field_frames() {
        assert_eq!(field_frame(0), (0x9f, 0xe, 0xb0, 0x1b));
        assert_eq!(field_frame(9), (0xb3, 0x1e, 0xc4, 0x2b));
        assert_eq!(
            field_frame(31),
            (0x9f + 7 * 0x14, 0xe + 3 * 0x10, 0xb0 + 7 * 0x14, 0x4b)
        );
    }

    #[test]
    fn zoom_cells() {
        assert!(!in_zoom(1, 0x20));
        assert!(in_zoom(2, 0xd));
        assert!(!in_zoom(0x99, 0x20));
        assert!(!in_zoom(0x20, 0xa5));
        assert_eq!(zoom_cell(2, 0xd), (0, 0));
        assert_eq!(zoom_cell(5, 0x10), (1, 0));
        assert_eq!(zoom_cell(0x98, 0xa4), (37, 37));
    }

    #[test]
    fn crest_file_round_trips() {
        let pixels: Vec<u8> = (0..0x5a4).map(|i| (i / 40 % 7) as u8).collect();
        let bytes = crest_file_bytes(&pixels);
        let img = vga::parse(&bytes).unwrap();
        assert_eq!((img.width, img.height), (38, 38));
        assert_eq!(img.pixels, pixels);
    }
}
