//! The editor's dialogs that more than one feature package uses.

use crate::gfx;
use crate::gfx::Gfx;
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

/// Width and height of the message box area at the top left of the screen.
const MESSAGE_W: i16 = 0x78;
const MESSAGE_H: i16 = 0x32;

/// A message box at the top left of the screen: "Fehler:" (font 2) over two centred
/// lines (font 3), shown for 90 ticks (about five seconds) without waiting for input;
/// the area is saved in a blank picture and restored afterwards. Leaves the screen
/// rastport's font at 3.
///
/// Origin: TEAMED.EXE 1000:1a31 (dialog_message)
pub fn message(host: &mut Host, ed: &mut Editor, line1: &[u8], line2: &[u8]) -> Run<()> {
    let backup = host.blank_bitmap(MESSAGE_W, MESSAGE_H, 0);
    let g = &mut host.g;
    // The backup comes from the screen bitmap (DS:59FE) at (0, 0).
    g.blit_bitmap(ed.screen.bitmap, 0, 0, backup, 0, 0, MESSAGE_W, MESSAGE_H);
    let rp = &mut ed.screen;
    gfx::ui_fill_rect(g, rp, 1, 2, 0x77, 0x28, 0x10);
    gfx::ui_draw_frame(g, rp, 1, 2, 0x77, 0x28, 0x13);
    set_font(rp, 2);
    gfx::ui_draw_text_centered_shadow(g, rp, b"Fehler:", 1, 0x77, 0xc, 0x13, 0xff);
    set_font(rp, 3);
    gfx::ui_draw_text_centered_shadow(g, rp, line1, 1, 0x77, 0x18, 0x13, 0xff);
    gfx::ui_draw_text_centered_shadow(g, rp, line2, 1, 0x77, 0x20, 0x13, 0xff);
    host.delay_ticks(0x5a)?;
    host.g
        .blit_bitmap(backup, 0, 0, ed.screen.bitmap, 0, 0, MESSAGE_W, MESSAGE_H);
    // The original never frees this picture (a leak of one 120x50 bitmap per message).
    host.g.free(backup);
    Ok(())
}

/// Limits above this value of [`text_input`]'s `limit` count characters
/// (`limit - 1000`) instead of pixels.
pub const LIMIT_CHARS: u16 = 1000;

/// Line input at `(x, y)` (the baseline) of a rastport into `buf`.
///
/// Draws each accepted character in `fg` (0 means 0x20) on `bg` with a 3x5 cursor
/// block after it; returns on Enter with the text NUL-terminated in `buf` (an empty
/// text means "cancelled" to every caller). Letters are upper-cased. With `text` every
/// key but Enter is accepted (control characters included); without it only the
/// digits, and the return value is their decimal value (wrapping 32-bit, as the long
/// arithmetic of the original); with `text` the original returns an undefined value,
/// the port 0. A character is refused when `limit > 1000` and the text already has
/// `limit - 1000` characters, or otherwise when the width of the text plus the new
/// character would exceed `limit` pixels. That width is measured over one byte more
/// than the text (`text_width(buf, len + 1)`): the stale byte left in the reused
/// buffer by an earlier, longer input counts, so `buf` keeps its old contents.
///
/// DEVIATION: Backspace on an empty text falls through to the "add a character"
/// path (`1000:0b30..0b38`) and appends the control character 8 in text mode; the port
/// ignores it, like BMMAIN's input (`game::start_menu::text_input`).
///
/// Origin: TEAMED.EXE 1000:0a7d (text_input; Ghidra splits it into 0a7d, 0ae9, 0c0e)
#[allow(clippy::too_many_arguments)]
pub fn text_input(
    host: &mut Host,
    rp: &mut gfx::RastPort,
    buf: &mut [u8],
    mut x: i16,
    y: i16,
    limit: u16,
    fg: u8,
    bg: u8,
    text: bool,
) -> Run<i32> {
    let fg = if fg == 0 { 0x20 } else { fg };
    let (lo, hi) = if text { (0x20, 0x7a) } else { (b'0', b'9') };
    Gfx::rastport_set_bg_color(rp, bg);
    buf[0] = 0;
    let mut len = 0usize;
    let mut key = 0u8;
    let (height, descent) = rp
        .font
        .and_then(|f| host.g.fonts[usize::from(f)].as_ref())
        .map_or((0, 0), |f| (f.height as i16, f.descent as i16));
    loop {
        // 1000:0ccc: the cursor block.
        Gfx::rastport_set_color(rp, fg);
        host.g.rastport_fill_rect(rp, x + 2, y - 4, x + 4, y);
        if key == b'\r' {
            break;
        }
        key = host.getch()?;
        Gfx::rastport_set_color(rp, bg);
        host.g.rastport_fill_rect(rp, x + 2, y - 4, x + 4, y);
        if key == 8 {
            if len != 0 {
                len -= 1;
                let c = buf[len];
                buf[len] = 0;
                let w = host.g.rastport_text_width(rp, &[c]);
                x -= w;
                host.g.rastport_fill_rect(
                    rp,
                    x,
                    y + descent - height + 1,
                    x + host.g.rastport_text_width(rp, &[c]),
                    y + descent,
                );
            }
            continue;
        }
        let up = key & 0xdf;
        let c = if up.is_ascii_uppercase() { up } else { key };
        let fits = if limit > LIMIT_CHARS {
            usize::from(limit - LIMIT_CHARS) > len
        } else {
            let w = host.g.rastport_text_width(rp, &[c]) as u16;
            let end = (len + 1).min(buf.len());
            let tw = host.g.rastport_text_width(rp, &buf[..end]) as u16;
            w.wrapping_add(tw) <= limit
        };
        let accept = if text {
            key != b'\r' && fits
        } else {
            // The range check compares the key sign-extended (CBW).
            let k = i16::from(key as i8);
            i16::from(lo) <= k && k <= i16::from(hi) && fits
        };
        if accept && len < buf.len() - 1 {
            Gfx::rastport_move_to(rp, x, y);
            Gfx::rastport_set_color(rp, fg);
            host.g.rastport_draw_text(rp, &[c]);
            buf[len] = c;
            len += 1;
            x += host.g.rastport_text_width(rp, &[c]);
        }
    }
    buf[len] = 0;
    Gfx::rastport_set_color(rp, bg);
    host.g.rastport_fill_rect(rp, x + 2, y - 4, x + 4, y);
    if text {
        return Ok(0);
    }
    Ok(buf[..len].iter().fold(0i32, |v, &d| {
        v.wrapping_mul(10)
            .wrapping_add(i32::from(d) - i32::from(b'0'))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{Bitmap, RastPort};
    use crate::host::parse_script;

    fn run_input(keys: &str, limit: u16, text: bool) -> Option<(Vec<u8>, i32)> {
        let data = bmp_formats::data_dir()?;
        let script = parse_script(&format!("wait 100\n{keys}key enter\nwait 1000\n")).unwrap();
        let mut host = Host::with_script(&data, script, &std::env::temp_dir(), false).unwrap();
        let bm = host.g.alloc(Bitmap::new_mem(320, 50));
        let mut rp = RastPort::new(bm);
        set_font(&mut rp, 3);
        let mut buf = [0u8; 0x52];
        let v = text_input(&mut host, &mut rp, &mut buf, 0x35, 7, limit, 0xb, 0xc, text).unwrap();
        Some((bmp_formats::mana::c_str(&buf).to_vec(), v))
    }

    #[test]
    fn digit_input_returns_the_value() {
        let Some((text, v)) = run_input("type 4a2\n", 0x4d, false) else {
            return;
        };
        assert_eq!((text.as_slice(), v), (&b"42"[..], 42));
    }

    #[test]
    fn text_input_upper_cases_and_limits_characters() {
        let Some((text, _)) =
            run_input("type ab:c\nkey backspace\ntype dz\n", LIMIT_CHARS + 4, true)
        else {
            return;
        };
        assert_eq!(text, b"AB:D");
    }
}
