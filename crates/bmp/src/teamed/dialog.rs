//! The editor's dialogs that more than one feature package uses.

use crate::gfx;
use crate::host::{Host, LEFT_DOWN, Run};

use super::{Editor, GRID_ORIGIN_Y, set_font};

/// Answer of [`two_choice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The right button (returns 0).
    Right,
    /// The left button (returns 1).
    Left,
    /// A click outside the box (returns 9).
    Outside,
}

/// A box in the crest view (`(0x4E, 0x33)-(0xE8, 0x7F)`) with three centred lines and
/// two buttons; waits for a click on a button or outside the box and restores the
/// area. The original returns 1 (left), 0 (right) or 9 (outside).
///
/// Origin: TEAMED.EXE 1000:215b (dialog_two_choice)
pub fn two_choice(
    host: &mut Host,
    ed: &mut Editor,
    lines: [&[u8]; 3],
    left: &[u8],
    right: &[u8],
) -> Run<Choice> {
    let backup = host.blank_bitmap(0x9b, 0x4d, 0);
    let g = &mut host.g;
    g.blit_bitmap(ed.grid.bitmap, 0x4e, 0x33, backup, 0, 0, 0x9b, 0x4d);
    let rp = &mut ed.grid;
    gfx::ui_fill_rect(g, rp, 0x4e, 0x33, 0xe8, 0x7f, 7);
    gfx::ui_draw_frame(g, rp, 0x4e, 0x33, 0xe8, 0x7f, 2);
    set_font(rp, 2);
    for (line, y) in lines.iter().zip([0x40, 0x4c, 0x58]) {
        gfx::ui_draw_text_centered_shadow(g, rp, line, 0x4e, 0xe8, y, 0x1d, 0xff);
    }
    set_font(rp, 3);
    gfx::ui_draw_frame(g, rp, 0x52, 0x6e, 0x91, 0x7b, 2);
    gfx::ui_draw_frame(g, rp, 0xa5, 0x6e, 0xe4, 0x7b, 2);
    gfx::ui_draw_text_centered_shadow(g, rp, left, 0x52, 0x91, 0x77, 0x1d, 0);
    gfx::ui_draw_text_centered_shadow(g, rp, right, 0xa5, 0xe4, 0x77, 0x1d, 0);
    // 12a5:2d39(2): the page switch of an earlier version, now empty.
    let choice = loop {
        let ev = host.input_poll_mouse()?;
        let (x, y) = host.mouse_xy();
        let y = y.wrapping_sub(GRID_ORIGIN_Y);
        let mut c = 0u8;
        if ev == LEFT_DOWN {
            if x > 0x4e && x < 0x91 && y > 0x6d && y < 0x7c {
                c = 2;
            }
            if x > 0xa5 && x < 0xe4 && y > 0x6d && y < 0x7c {
                c = 1;
            }
            if x < 0x4e || y < 0x33 || x > 0xe8 || y > 0x7f {
                c = 10;
            }
        }
        if c != 0 {
            break c;
        }
    };
    host.g
        .blit_bitmap(backup, 0, 0, ed.grid.bitmap, 0x4e, 0x33, 0x9b, 0x4d);
    host.g.free(backup);
    Ok(match choice {
        2 => Choice::Left,
        1 => Choice::Right,
        _ => Choice::Outside,
    })
}

/// A message box with two lines (e.g. "DIESER TEAMNAME" / "EXISTIERT BEREITS.").
///
/// TODO(#158) package club: port the body; the skeleton shows nothing.
///
/// Origin: TEAMED.EXE 1000:1a31 (dialog_message)
#[allow(dead_code)] // TODO(#158): called by the packages club and crest.
pub fn message(host: &mut Host, ed: &mut Editor, line1: &[u8], line2: &[u8]) -> Run<()> {
    let _ = (host, ed, line1, line2);
    Ok(())
}

/// Line input at `(x, y)` of a rastport: characters `' '..='z'` (or digits only when
/// `text` is false), Backspace, Enter; returns the entered bytes (empty = cancelled).
///
/// TODO(#158) package club: port the body (1000:0a7d..0d8c; Ghidra splits it into
/// 1000:0a7d, 0ae9 and 0c0e). It is also used by the UEFA list (package uefa) and the
/// crest import (package crest). The skeleton returns an empty input.
///
/// Origin: TEAMED.EXE 1000:0a7d (text_input)
#[allow(clippy::too_many_arguments, dead_code)] // TODO(#158): called by all packages.
pub fn text_input(
    host: &mut Host,
    rp: &mut gfx::RastPort,
    x: i16,
    y: i16,
    max_len: u16,
    width: i16,
    color: u8,
    text: bool,
) -> Run<Vec<u8>> {
    let _ = (host, rp, x, y, max_len, width, color, text);
    Ok(Vec::new())
}
