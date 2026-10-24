//! "Iff-Bild laden": the crest import of the crest editor (spec:
//! `docs/re/teamed/crest.md`, "IFF import").
//!
//! The screen scrolls down to show the lower part of the 329-line screen, a file
//! browser picks an IFF picture (`FORM ILBM` / `FORM PBM `, at most 320 x 200), the
//! picture is drawn centred into the view and a 39x39 frame that follows the pointer
//! chooses the 38x38 part that becomes the working crest (left button; the right button
//! cancels). The browser is a close copy of BMMAIN's file selector
//! ([`crate::game::load_game::file_selector`]) and shares its directory model
//! ([`SaveSession`], [`list_directory`]).

use std::path::PathBuf;

use bmp_formats::iff::{self, BMHD_LEN};

use crate::game::load_game::{SaveSession, find_case_insensitive, list_directory};
use crate::game::text_to_upper_de;
use crate::game::widgets::TextButtons;
use crate::gfx::{self, Bitmap, Gfx, RastPort};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

use super::{
    CREST_SIZE, Editor, GRID_ORIGIN_Y, crest, dialog, format_number, screen_set_visible, set_font,
};

/// Entries per page of the file list (two columns of 18).
const PAGE: usize = 0x24;
/// Drive buttons (`DS:04CC`, 8 bytes each); a label is shown for every drive DOS
/// reports (`_dos_setdrive`), DOSBox reports 26.
const DRIVES: [&[u8]; 8] = [b"A:", b"B:", b"C:", b"D:", b"E:", b"F:", b"G:", b"H:"];
const LOGICAL_DRIVES: usize = 26;
/// The message for a picture the reader refuses (`DS:03E0`, `DS:03C8`).
const WRONG_FORMAT: [&[u8]; 2] = [b"FALSCHES GRAFIKFORMAT.", b"X<=320,Y<=200,256FARBEN"];
/// Farthest top left corner of the crop frame (view coordinates).
const CROP_MAX_X: i16 = 0x118;
const CROP_MAX_Y: i16 = 0xd6;

/// The import's state kept between imports.
#[derive(Debug, Clone)]
pub struct ImportState {
    /// The browser's drive and directory (`DS:050C`, `DS:20DC`): the next browser starts
    /// where the last one ended.
    pub dir: SaveSession,
    /// `DS:1DE2`: the `BMHD` of the last picture read (a file without one reuses it).
    pub bmhd: [u8; BMHD_LEN],
    /// The main loop's buffer `[BP-54]` that receives the browser's result: the file
    /// name line edits it in place, so its stale bytes survive between imports.
    pub name_buf: [u8; NAME_BUF_LEN],
}

/// Size of the result buffer `[BP-54]..[BP-04]` of the main loop.
pub const NAME_BUF_LEN: usize = 0x50;

impl Default for ImportState {
    fn default() -> Self {
        Self {
            dir: SaveSession::default(),
            bmhd: [0; BMHD_LEN],
            name_buf: [0; NAME_BUF_LEN],
        }
    }
}

/// The import (`Iff-Bild laden`): scrolled view, browser, picture, crop frame.
///
/// Origin: TEAMED.EXE 12a5:1b16 (teamed_main_loop, crest editor: Iff-Bild laden)
pub fn import(host: &mut Host, ed: &mut Editor) -> Run<()> {
    host.g.vga_show_page_teamed(3);
    // The browser draws through the screen rastport and bitmap globals (DS:1E44,
    // DS:59FE), which point at the crest view meanwhile.
    let name = browser(host, ed, b"")?;
    if name.is_empty() {
        host.g.vga_show_page_teamed(1);
        return Ok(());
    }
    screen_set_visible(&mut host.g, false);
    if !load_picture(host, ed, &name) {
        host.g.vga_show_page_teamed(1);
        screen_set_visible(&mut host.g, true);
        return dialog::message(host, ed, WRONG_FORMAT[0], WRONG_FORMAT[1]);
    }
    let g = &mut host.g;
    gfx::ui_fill_rect(g, &mut ed.grid, 0, 0xd6, 0x27, 0xfe, 0xb);
    Gfx::rastport_set_draw_mode(&mut ed.grid, 2);
    screen_set_visible(g, true);
    // 1000:00fb: wait until no mouse event is pending.
    host.input_wait_release()?;
    let (mut old_x, mut old_y) = (-100i16, 0i16);
    let view = ed.grid.bitmap;
    let ev = loop {
        let ev = host.input_poll_mouse()?;
        let (mut x, my) = host.mouse_xy();
        let mut y = my.wrapping_sub(GRID_ORIGIN_Y);
        if x > CROP_MAX_X || y > CROP_MAX_Y {
            x = x.min(CROP_MAX_X);
            y = y.min(CROP_MAX_Y);
            host.g.mouse_set_position(x, GRID_ORIGIN_Y + y);
        }
        if (x, y) != (old_x, old_y) {
            let g = &mut host.g;
            gfx::ui_draw_frame(g, &mut ed.grid, old_x, old_y, old_x + 0x27, old_y + 0x27, 1);
            gfx::ui_draw_frame(g, &mut ed.grid, x, y, x + 0x27, y + 0x27, 1);
            g.blit_bitmap(view, x + 1, y + 1, view, 1, 0xd7, CREST_SIZE, CREST_SIZE);
            (old_x, old_y) = (x, y);
        }
        if ev == RIGHT_DOWN || ev == LEFT_DOWN {
            break ev;
        }
    };
    let g = &mut host.g;
    gfx::ui_draw_frame(g, &mut ed.grid, old_x, old_y, old_x + 0x27, old_y + 0x27, 1);
    Gfx::rastport_set_draw_mode(&mut ed.grid, 0);
    let [work, undo] = ed.scratch;
    if ev == LEFT_DOWN {
        g.blit_bitmap(
            view,
            old_x + 1,
            old_y + 1,
            work,
            0,
            0,
            CREST_SIZE,
            CREST_SIZE,
        );
        g.blit_bitmap(work, 0, 0, undo, 0, 0, CREST_SIZE, CREST_SIZE);
        // 12a5:1f13: probably meant for the box at (0x73, 0x2F) of the screen, but the
        // coordinates are the source position: a 38x38 picture has nothing there, so
        // the clipped blit draws nothing.
        g.blit_bitmap(work, 0x73, 0x2f, view, 0, 0, CREST_SIZE, CREST_SIZE);
    }
    crest::draw_editor(host, ed, work, true)?;
    host.g.vga_show_page_teamed(1);
    ed.crest_changed = true;
    Ok(())
}

/// Reads the picture `path` (a DOS path from the browser) and draws it centred into
/// the view: `((320 - w) / 2, (200 - h) / 2 + 0xD)`. False when the reader refuses the
/// file or it cannot be opened.
///
/// Origin: TEAMED.EXE 1000:049e (crest_import_load), 1000:04f1 (crest_import_convert)
fn load_picture(host: &mut Host, ed: &mut Editor, path: &[u8]) -> bool {
    let Some(bytes) = resolve(&host.data, path).and_then(|p| std::fs::read(p).ok()) else {
        return false;
    };
    let Some(pic) = iff::read_picture(&bytes, &ed.import.bmhd) else {
        return false;
    };
    ed.import.bmhd = pic.bmhd;
    let (w, h) = (pic.width(), pic.height());
    if w <= 0 || h <= 0 {
        // DEVIATION: an empty or "negative" size passes the original's signed size
        // check and makes its decoder run through memory; the port refuses the file.
        return false;
    }
    let mut bm = Bitmap::new_mem(w, h);
    bm.storage = crate::gfx::Storage::Mem(pic.pixels());
    let id = host.g.alloc(bm);
    host.g.blit_bitmap(
        id,
        0,
        0,
        ed.grid.bitmap,
        (0x140 - w) / 2,
        (0xc8 - h) / 2 + 0xd,
        w,
        h,
    );
    host.g.free(id);
    true
}

/// The host file of a browser result `C:\DIR\NAME` (DOS names ignore case); `None` for
/// other drives (the port only knows the game drive C:).
fn resolve(data: &std::path::Path, path: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(path).to_string();
    let rest = text
        .strip_prefix("C:\\")
        .or_else(|| text.strip_prefix("c:\\"))?;
    let mut p = data.to_path_buf();
    for part in rest.split('\\').filter(|s| !s.is_empty()) {
        let next = p.join(part);
        p = find_case_insensitive(&next).unwrap_or(next);
    }
    p.is_file().then_some(p)
}

/// One xor bar of the list highlight (`col` 0/1, `row` 0..17).
fn xor_row(g: &mut Gfx, rp: &mut RastPort, row: i16, col: i16) {
    gfx::ui_fill_rect(
        g,
        rp,
        col * 0x78 + 0xa,
        row * 7 + 0x25,
        col * 0x78 + 0x7d,
        row * 7 + 0x29,
        1,
    );
}

/// The file browser in the crest view: two columns of directories (colour 3) and
/// files (colour 1), the directory and file name lines, the page number, eight drive
/// buttons. Left click: enter a directory, pick a file (a second click on the picked
/// file returns it), edit the directory or file name line; right click: next page (or
/// return the name on the file line); a left click outside the box cancels. Returns
/// the full path (`C:\DIR\NAME`) or an empty name when cancelled.
///
/// `mode` is the caller's string: `"k"` colours a `PIC` directory 0x12, a non-empty
/// string other than `"k"` colours `.PIC` files 0x11; the editor passes `""`.
///
/// Origin: TEAMED.EXE 1579:05b7 (crest_import_browser)
pub fn browser(host: &mut Host, ed: &mut Editor, mode: &[u8]) -> Run<Vec<u8>> {
    let k_mode = mode == b"k";
    let empty_mode = mode.is_empty();
    let data = host.data.clone();
    let rp = &mut ed.grid;
    set_font(rp, 3);
    let backup = host.blank_bitmap(0xf2, 0xb5, 0);
    let g = &mut host.g;
    g.blit_bitmap(rp.bitmap, 6, 0x21, backup, 0, 0, 0xf2, 0xb5);
    gfx::ui_fill_rect(g, rp, 6, 0x21, 0xf6, 0xd5, 8);
    gfx::ui_draw_frame(g, rp, 6, 0x21, 0xf7, 0xd5, 1);
    let mut y = 0xa3u8;
    while y < 0xb8 {
        gfx::ui_draw_line(g, rp, 7, y, 0xf6, y, 0xb);
        y += 10;
    }
    let mut buttons = TextButtons::default();
    for row in 0..2i16 {
        for col in 0..4i16 {
            let i = (row * 4 + col) as usize;
            let label: &[u8] = if i < LOGICAL_DRIVES { DRIVES[i] } else { b"" };
            let (x, y) = (col * 0x3b + 0xa, 0xd * row + 0xbb);
            buttons.register_on(g, rp, x, y, label, 0x1a, (x, x + 0x3f));
        }
    }
    let state = &mut ed.import.dir;
    let mut result: Vec<u8> = Vec::new();
    // strcpy(result, DS:36CE): that string is never written, so the line starts empty.
    ed.import.name_buf[0] = 0;
    let mut input = [0u8; 0x52];
    let picked = 'list: loop {
        let mut first = 0usize;
        Gfx::rastport_set_draw_mode(rp, 0);
        gfx::ui_fill_rect(&mut host.g, rp, 7, 0x22, 0xf6, 0xa1, 8);
        let (dirs, files) = if state.on_floppy() {
            (Vec::new(), Vec::new())
        } else {
            list_directory(&state.path(&data))
        };
        let mut text = b"VERZEICHNIS: ".to_vec();
        // DEVIATION: on a floppy drive `getcwd` fails and the original shows what its
        // buffer held before; the port shows the C: directory (as in BMMAIN's selector).
        text.extend(text_to_upper_de(&state.cwd_text()));
        text.truncate(0x2c);
        let g = &mut host.g;
        gfx::ui_fill_rect(g, rp, 0xa, 0xa5, 0xf0, 0xaa, 8);
        gfx::ui_draw_text_at(g, rp, &text, 0xa, 0xaa, 1);
        gfx::ui_fill_rect(g, rp, 0xa, 0xaf, 0xf0, 0xb4, 8);
        gfx::ui_draw_text_at(g, rp, b"FILENAME      :", 0xa, 0xb4, 1);
        if !result.is_empty() {
            gfx::ui_draw_text_at(g, rp, &result, 0x3f, 0xb4, 1);
        }
        'page: loop {
            let (mut hl_row, mut hl_col) = (-1i16, -1i16);
            let g = &mut host.g;
            gfx::ui_fill_rect(g, rp, 0xd2, 0xaf, 0xf0, 0xb4, 8);
            let mut text = b"SEITE: ".to_vec();
            text.extend(format_number((first / PAGE + 1) as i32));
            gfx::ui_draw_text_at(g, rp, &text, 0xd2, 0xb4, 2);
            let (mut x, mut y) = (0xai16, 0x29i16);
            let advance = |x: &mut i16, y: &mut i16| {
                *y += 7;
                if *y > 0xa0 {
                    *y = 0x29;
                    *x = 0x82;
                }
            };
            for d in dirs.iter().take(first + PAGE).skip(first) {
                let color = if k_mode && d == b"PIC" { 0x12 } else { 3 };
                gfx::ui_draw_text_at(g, rp, d, x, y, color);
                advance(&mut x, &mut y);
            }
            let mut i = first.saturating_sub(dirs.len());
            while i < files.len() && dirs.len() + i < first + PAGE {
                let f = &files[i];
                let pic = f.len() >= 4 && &f[f.len() - 4..] == b".PIC";
                let color = if !k_mode && pic && !empty_mode {
                    0x11
                } else {
                    1
                };
                gfx::ui_draw_text_at(g, rp, f, x, y, color);
                advance(&mut x, &mut y);
                i += 1;
            }
            screen_set_visible(g, true);
            loop {
                let ev = host.input_poll_mouse()?;
                let (mut mx, my) = host.mouse_xy();
                let mut v = my.wrapping_sub(GRID_ORIGIN_Y);
                if ev == LEFT_DOWN && !((0xa..=0xf6).contains(&mx) && (0x21..=0xd5).contains(&v)) {
                    break 'list Vec::new();
                }
                let g = &mut host.g;
                Gfx::rastport_set_draw_mode(rp, 2);
                if mx > 0xa && mx < 0xf6 && v > 0x24 && v < 0xa0 {
                    // The original reuses both variables for column and row; the
                    // checks below see them.
                    mx = i16::from(mx > 0x82);
                    v = (v - 0x25) / 7;
                    if v != hl_row || hl_col != mx {
                        if hl_row != -1 {
                            xor_row(g, rp, hl_row, hl_col);
                        }
                        hl_col = mx;
                        hl_row = v;
                        xor_row(g, rp, v, mx);
                    }
                } else if hl_row != -1 {
                    xor_row(g, rp, hl_row, hl_col);
                    hl_row = -1;
                }
                Gfx::rastport_set_draw_mode(rp, 0);
                if v < 0xa5 && ev == RIGHT_DOWN && dirs.len() + files.len() > PAGE {
                    first += PAGE;
                    if dirs.len() + files.len() < first {
                        first = 0;
                    }
                    Gfx::rastport_set_draw_mode(rp, 0);
                    gfx::ui_fill_rect(g, rp, 7, 0x22, 0xf6, 0xa1, 8);
                    continue 'page;
                }
                if (ev == LEFT_DOWN || ev == RIGHT_DOWN)
                    && mx > 0xa
                    && mx < 0xf6
                    && v > 0xa3
                    && v < 0xb7
                {
                    let field = i16::from(v > 0xad);
                    if ev == LEFT_DOWN || field == 0 {
                        let x1 = if field == 0 { 0xf0 } else { 0xcd };
                        gfx::ui_fill_rect(g, rp, 0x3f, field * 10 + 0xa5, x1, field * 10 + 0xaa, 8);
                    }
                    if field == 0 {
                        dialog::text_input(host, rp, &mut input, 0x3f, 0xaa, 0xaf, 1, 8, true)?;
                        state.change_dir(&data, bmp_formats::mana::c_str(&input));
                        continue 'list;
                    }
                    if ev == LEFT_DOWN {
                        let buf = &mut ed.import.name_buf;
                        dialog::text_input(host, rp, buf, 0x3f, 0xb4, 0x8c, 1, 8, true)?;
                        result = bmp_formats::mana::c_str(buf).to_vec();
                    }
                    if !result.is_empty() {
                        break 'list result;
                    }
                }
                if host.button == LEFT_DOWN && hl_row != -1 {
                    let idx = (hl_col * 0x12) as usize + first + hl_row as usize;
                    if idx < dirs.len() {
                        state.change_dir(&data, &dirs[idx]);
                        continue 'list;
                    }
                    if let Some(file) = files.get(idx - dirs.len()) {
                        if *file == result {
                            break 'list result;
                        }
                        result = file.clone();
                        strcpy(&mut ed.import.name_buf, &result);
                        let g = &mut host.g;
                        gfx::ui_fill_rect(g, rp, 0x3f, 0xaf, 0xf0, 0xb4, 8);
                        gfx::ui_draw_text_at(g, rp, &result, 0x3f, 0xb4, 1);
                    }
                }
                let (px, py) = host.mouse_xy();
                let slot = buttons.hit(px, py.wrapping_sub(GRID_ORIGIN_Y));
                if host.button == LEFT_DOWN && slot != 0 && slot <= LOGICAL_DRIVES {
                    state.change_drive(slot);
                    continue 'list;
                }
            }
        }
    };
    let g = &mut host.g;
    g.blit_bitmap(backup, 0, 0, rp.bitmap, 6, 0x21, 0xf2, 0xb5);
    g.free(backup);
    if picked.is_empty() {
        return Ok(picked);
    }
    // getcwd + "\" (unless the root) + the name.
    let mut path = state.cwd_text();
    if path.len() > 3 {
        path.push(b'\\');
    }
    path.extend_from_slice(&picked);
    strcpy(&mut ed.import.name_buf, &path);
    Ok(path)
}

/// `strcpy` into a fixed buffer: the bytes and a NUL, the rest keeps its old contents.
fn strcpy(buf: &mut [u8], s: &[u8]) {
    let n = s.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&s[..n]);
    buf[n] = 0;
}
