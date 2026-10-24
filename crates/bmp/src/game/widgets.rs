//! Small UI helpers of segment 4091 used by several screens.

use bmp_game::rng::{format_long_thousands, spinner_round_bound};

use super::GAME_OPTION_A;
use crate::gfx::{Gfx, ui_draw_frame, ui_draw_line, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_DOWN, LEFT_UP, RIGHT_DOWN, Run};
/// Most text buttons that can be registered (`DS:07b3` stops counting at 10; the
/// 11th registration overwrites slot 10).
const ICON_SLOTS: usize = 11;

/// Registered 57x12 text buttons: `DS:07b3` count, `FAR:533c` x and `FAR:90ae` y.
#[derive(Debug, Clone, Default)]
pub struct TextButtons {
    count: usize,
    xy: [(i16, i16); ICON_SLOTS],
}

impl TextButtons {
    /// Registers and draws a 57x12 bevelled text button at `(x, y)` with `text`
    /// centred in `color` (font 2). The four corner pixels are kept as they were.
    ///
    /// Origin: BMMAIN.EXE 4091:123d (ui_register_icon_slot)
    pub fn register(&mut self, host: &mut Host, x: i16, y: i16, text: &[u8], color: u8) {
        let mut corners = [[0u8; 2]; 2];
        for (a, row) in corners.iter_mut().enumerate() {
            for (b, px) in row.iter_mut().enumerate() {
                *px = host
                    .g
                    .rastport_get_pixel(&host.rp, x + a as i16 * 0x38, y + b as i16 * 0xb)
                    as u8;
            }
        }
        self.xy[self.count] = (x, y);
        if self.count < 10 {
            self.count += 1;
        }
        let (g, rp) = (&mut host.g, &mut host.rp);
        ui_fill_rect(g, rp, x, y, x + 0x38, y + 0xb, 0);
        let (yb, yb2) = (y as u8, (y as u8).wrapping_add(0xb));
        ui_draw_line(g, rp, x, yb, x + 0x38, yb, 2);
        ui_draw_line(g, rp, x + 0x38, yb, x + 0x38, yb2, 7);
        ui_draw_line(g, rp, x, yb, x, yb2, 1);
        ui_draw_line(g, rp, x, yb2, x + 0x38, yb2, 6);
        for (a, row) in corners.iter().enumerate() {
            for (b, &px) in row.iter().enumerate() {
                Gfx::rastport_set_color(rp, px);
                g.rastport_put_pixel(rp, x + a as i16 * 0x38, y + b as i16 * 0xb);
            }
        }
        host.set_font(2);
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            text,
            x + 1,
            x + 0x39,
            y + 8,
            color,
            0xff,
        );
    }

    /// Number of registered buttons (`DS:07b3`).
    pub fn count(&self) -> usize {
        self.count
    }

    /// Forgets the last button (`DEC DS:07b3`), so the next registration
    /// redraws it in place.
    pub fn drop_last(&mut self) {
        self.count = self.count.saturating_sub(1);
    }

    /// 1-based index of the button under the pointer, or 0.
    ///
    /// Origin: BMMAIN.EXE 4091:11cc (ui_hit_icon_slot)
    pub fn hit(&self, mx: i16, my: i16) -> usize {
        self.xy[..self.count]
            .iter()
            .position(|&(x, y)| x < mx && x + 0x39 > mx && y < my && y + 0xc > my)
            .map_or(0, |i| i + 1)
    }
}

/// Shows the pressed frame on the 50x41 icon button at `(x, y)` until the left
/// button is released (or auto play is on), then the normal frame again.
///
/// Origin: BMMAIN.EXE 4091:1722 (ui_wait_right_click_button)
pub fn ui_wait_right_click_button(host: &mut Host, x: i16, y: i16, auto_play: bool) -> Run<()> {
    host.ui.draw_button(
        &mut host.g,
        &mut host.rp,
        x,
        y,
        0,
        0xa022u16 as i16,
        false,
        8,
    );
    loop {
        host.input_poll_mouse()?;
        if host.button == LEFT_UP || auto_play {
            break;
        }
    }
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, y, 0, 0x6022, false, 8);
    Ok(())
}

/// `DS:4f8c` (`LBL[329]`): messages starting with this prefix open at full size
/// and stay half as long.
const QUICK_PREFIX: &[u8] = b"Auto-";

/// Two-line message box in the top right corner (0xa4..0x140, 0x0c..0x2c):
/// it grows from the centre, shows the lines for 60 ticks (30 for an
/// "Auto-..." message), then the saved background is restored the same way.
/// No click is needed. Always returns `true` (a message was shown).
///
/// Origin: BMMAIN.EXE 176b:0e57 (ui_dialog_box)
pub fn ui_dialog_box(host: &mut Host, line1: &[u8], line2: &[u8]) -> Run<bool> {
    let quick = line1.len() >= QUICK_PREFIX.len() && line1[..QUICK_PREFIX.len()] == *QUICK_PREFIX;
    let (start, full) = (
        (0xb3i16, 0x1bi16, 7i16, 3i16),
        (0xa4i16, 0x0ci16, 0x9di16, 0x21i16),
    );
    let saved = host.blank_bitmap(0x9e, 0x22, 0);
    let screen = host.screen();
    host.g
        .blit_bitmap(screen, 0xa4, 0x0c, saved, 0, 0, 0x9d, 0x22);
    host.set_font(1);
    Gfx::rastport_set_draw_mode(&mut host.rp, 0);
    Gfx::rastport_set_color(&mut host.rp, 0x10);
    let (mut x, mut y, mut w, mut h) = if quick { full } else { start };
    loop {
        host.g.vga_wait_vretrace();
        host.g
            .rastport_fill_rect(&host.rp, x + 1, y + 1, w + x - 1, h + y - 1);
        x -= 1;
        w += 10;
        y -= 1;
        h += 2;
        if h >= 0x23 {
            break;
        }
    }
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_frame(g, rp, x + 3, y + 3, w + x - 0xc, h + y - 3, 0);
    ui_draw_frame(g, rp, x + 2, y + 2, w + x - 0xb, h + y - 2, 0x13);
    ui_draw_text_centered_shadow(g, rp, line1, 0xb9, 0x12f, 0x1a, 0x1d, 0);
    ui_draw_text_centered_shadow(g, rp, line2, 0xb9, 0x12f, 0x27, 0x1d, 0);
    host.delay_ticks((2 - i32::from(quick)) * 0x1e)?;
    host.set_font(2);
    let (mut x, mut y, mut w, mut h) = if quick { full } else { start };
    loop {
        host.g.vga_wait_vretrace();
        let screen = host.screen();
        host.g
            .blit_bitmap(saved, x - 0xa4, y - 0xc, screen, x, y, w, h);
        x -= 1;
        w += 10;
        y -= 1;
        h += 2;
        if h >= 0x23 {
            break;
        }
    }
    host.g.free(saved);
    Ok(true)
}

/// Draws the spinner value centred in the frame (`"1.20"` style decimals with
/// the English option).
///
/// Origin: BMMAIN.EXE 4091:0b1b (ui_spinner_draw_value)
fn ui_spinner_draw_value(host: &mut Host, v: i32, v0: i32, x: i16, y: i16) {
    let mut buf = format_long_thousands(v, host.number_format());
    if GAME_OPTION_A > 1 && v0 != 0 {
        buf.resize(buf.len().max(5), 0);
        buf[2] = buf[1];
        buf[1] = b'.';
        buf[3] = b'0';
        buf[4] = 0;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        buf.truncate(n);
    }
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &buf, x, x + 0x46, y + 0x18, 9, 0);
}

/// A value spinner (`PIC\21.VGA`) at `(x, y)`: a click into the upper half
/// raises the value by `step`, into the lower half lowers it, within
/// `lo..=hi`; a right click ends. Returns the new value, or -1 when it was not
/// changed.
///
/// Origin: BMMAIN.EXE 4091:0ba5 (ui_value_spinner)
pub fn ui_value_spinner(
    host: &mut Host,
    v: i32,
    x: i16,
    y: i16,
    lo: i32,
    hi: i32,
    step: i32,
) -> Run<i32> {
    let v0 = v;
    let mut v = v;
    let lo = spinner_round_bound(lo, GAME_OPTION_A, v0);
    let hi = spinner_round_bound(hi, GAME_OPTION_A, v0);
    let pic = host
        .load_picture(0x15)
        .expect("PIC\\21.VGA (fatal_error_exit in the original)");
    let screen = host.screen();
    host.g.blit_bitmap(pic, 0, 0, screen, x, y, 0x46, 0x27);
    host.set_font(1);
    host.number_no_separators = false;
    ui_spinner_draw_value(host, v, v0, x, y - 2);
    loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let pix = host.g.rastport_get_pixel(&host.rp, mx, my) as u8;
        if host.button == LEFT_DOWN
            && x < mx
            && mx < x + 0x46
            && y < my
            && my < y + 0x27
            && pix == 4
        {
            if my > y + 0x12 {
                v = v.wrapping_sub(step);
            } else {
                v = v.wrapping_add(step);
            }
            if v < lo {
                v = lo;
            } else if v > hi {
                v = hi;
            } else {
                ui_fill_rect(
                    &mut host.g,
                    &mut host.rp,
                    x + 2,
                    y + 0x0f,
                    x + 0x43,
                    y + 0x17,
                    0,
                );
                ui_spinner_draw_value(host, v, v0, x, y - 2);
            }
        }
        if host.button == RIGHT_DOWN {
            break;
        }
    }
    host.g.free(pic);
    Ok(if v == v0 { -1 } else { v })
}

/// `DS:2698`: the label of the [`dialog_okay`] button.
const OKAY: &[u8] = b"OKAY";

/// Draws the bevelled dialog panel with a saved background and returns its
/// button slot. The background bitmap comes from `vga_load_picture`, which
/// resets the field width `DS:079c` to 1.
fn dialog_panel(host: &mut Host, x: i16, w: i16) -> u8 {
    host.number_pad = 1;
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, 0x6e, w, 0x41, true, 0x10)
        .0
}

/// Four-line message box with an "OKAY" button: waits for a left click on
/// the button, then restores the background. Always returns `true`.
///
/// TODO(#15): the original also leaves at once while automatic play
/// (`DS:05d4`) is on; the callers do not pass that flag yet.
///
/// Origin: BMMAIN.EXE 4091:0e3a (dialog_okay)
pub fn dialog_okay(host: &mut Host, l1: &[u8], l2: &[u8], l3: &[u8], l4: &[u8]) -> Run<bool> {
    let slot = dialog_panel(host, 0x3c, 0xb9);
    host.set_font(1);
    for (k, line) in [l1, l2, l3, l4].into_iter().enumerate() {
        let y = 0x7b + 10 * k as i16;
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, line, 0x3c, 0xf5, y, 0x0a, 0xff);
    }
    let mut buttons = TextButtons::default();
    buttons.register(host, 0xb2, 0xa0, OKAY, 0x1a);
    let saved = host.mouse_xy();
    host.g.mouse_set_position(0xbf, 0xa5);
    host.screen_fade(1)?;
    loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        if host.button == LEFT_DOWN && buttons.hit(mx, my) != 0 {
            break;
        }
    }
    host.g.mouse_set_position(saved.0, saved.1);
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, slot, 0);
    Ok(true)
}

/// Question box: `title` (the addressed manager's name, or `"An Alle:"`,
/// `DS:269c`, for all managers) over
/// three lines and the buttons `left` and `right`. Returns 1 for `left`, 0
/// for `right`, and -1 when automatic play (`auto_play`, `DS:05d4`) ends it
/// without a choice. The pointer starts on `right` and is put back
/// afterwards.
///
/// Origin: BMMAIN.EXE 4091:146c (dialog_two_choice)
#[allow(clippy::too_many_arguments)]
pub fn dialog_two_choice(
    host: &mut Host,
    auto_play: bool,
    title: &[u8],
    l1: &[u8],
    l2: &[u8],
    l3: &[u8],
    left: &[u8],
    right: &[u8],
) -> Run<i8> {
    let slot = dialog_panel(host, 0x4b, 0x9b);
    host.set_font(1);
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_text_centered_shadow(g, rp, title, 0x4f, 0xe6, 0x77, 0x0b, 0xff);
    ui_draw_line(g, rp, 0x4d, 0x79, 0xe4, 0x79, 0x0b);
    host.set_font(2);
    for (k, line) in [l1, l2, l3].into_iter().enumerate() {
        let y = 0x83 + 8 * k as i16;
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, line, 0x4f, 0xe6, y, 0x0a, 0xff);
    }
    ui_draw_line(&mut host.g, &mut host.rp, 0x4d, 0x9b, 0xe4, 0x9b, 0x0b);
    // Drawn as text buttons, but unregistered again (`DS:07b3 -= 2`): the
    // hit test below is by coordinates.
    let mut buttons = TextButtons::default();
    buttons.register(host, 0x59, 0xa0, left, 0x11);
    buttons.register(host, 0x9f, 0xa0, right, 1);
    let saved = host.mouse_xy();
    host.g.mouse_set_position(0xb9, 0xa5);
    let mut res: i8 = 0;
    loop {
        host.input_poll_mouse()?;
        if host.button == LEFT_DOWN {
            let (mx, my) = host.mouse_xy();
            for k in 0..2i16 {
                if k * 0x46 + 0x59 < mx && mx < k * 0x46 + 0x92 && 0x9f < my && my < 0xac {
                    res = 2 - k as i8;
                }
            }
        }
        if res != 0 || auto_play {
            break;
        }
    }
    host.g.mouse_set_position(saved.0, saved.1);
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, slot, 0);
    Ok(res - 1)
}

/// Waits until the mouse buttons are released, then for a left click (or
/// automatic play).
///
/// TODO(re): the original also requires `FAR:3046 == 1` (one pending mouse
/// event), which the port does not model.
///
/// Origin: BMMAIN.EXE 176b:0d0a (input_wait_click)
pub fn input_wait_click(host: &mut Host, auto_play: bool) -> Run<()> {
    host.input_wait_release()?;
    if auto_play {
        return Ok(());
    }
    while host.input_poll_mouse()? != LEFT_DOWN {}
    Ok(())
}

/// Erases the 3x5 cursor block or a typed character: filled with `bg`, or for
/// `bg == 0xff` restored from the office background in VRAM page 2.
fn input_erase(host: &mut Host, x: i16, y: i16, w: i16, h: i16, bg: u8) {
    if bg == 0xff {
        let screen = host.screen();
        if let Some(work) = host.ui.work {
            host.g
                .blit_bitmap_or_restore_page2(work, x, y, screen, x, y, w, h);
        }
    } else {
        ui_fill_rect(&mut host.g, &mut host.rp, x, y, x + w - 1, y + h - 1, bg);
    }
}

/// Typed number entry at `(x, y)` (baseline) with at most `max_chars` digits:
/// a block cursor, digits only, Backspace deletes, Esc cancels (0), Enter
/// accepts. `bg == 0xff` restores the background from VRAM page 2 instead of
/// filling it.
///
/// TODO(re): the erase rectangles follow a damaged decompile of 176b:07d7 and
/// may be off by a pixel.
///
/// Origin: BMMAIN.EXE 176b:07d7 (ui_scroll_text_box, numeric mode)
pub fn ui_input_number(
    host: &mut Host,
    x: i16,
    y: i16,
    max_chars: usize,
    color: u8,
    bg: u8,
) -> Run<i32> {
    Ok(ui_input_number_typed(host, x, y, max_chars, color, bg)?.0)
}

/// [`ui_input_number`] that also tells whether anything was typed (the
/// caller's buffer is not empty); Esc counts as nothing typed.
///
/// Origin: BMMAIN.EXE 176b:07d7 (ui_scroll_text_box, numeric mode)
pub fn ui_input_number_typed(
    host: &mut Host,
    mut x: i16,
    y: i16,
    max_chars: usize,
    color: u8,
    bg: u8,
) -> Run<(i32, bool)> {
    let mut text: Vec<u8> = Vec::new();
    let height = host.rp.font.map_or(8, |f| {
        host.g.fonts[f as usize]
            .as_ref()
            .map_or(8, |f| f.height as i16)
    });
    loop {
        ui_fill_rect(&mut host.g, &mut host.rp, x + 2, y - 4, x + 4, y, color);
        let key = host.getch()?;
        if key == b'\r' {
            ui_fill_rect(&mut host.g, &mut host.rp, x + 2, y - 4, x + 4, y, bg);
            return Ok((number_value(&text), !text.is_empty()));
        }
        input_erase(host, x + 2, y - 4, 3, 5, bg);
        match key {
            0x1b => return Ok((0, false)),
            8 => {
                if let Some(c) = text.pop() {
                    let w = host.g.rastport_text_width(&host.rp, &[c]);
                    x -= w;
                    input_erase(host, x, y - height + 1, w + 1, height + 1, bg);
                }
            }
            b'0'..=b'9' if text.len() < max_chars => {
                Gfx::rastport_move_to(&mut host.rp, x, y);
                Gfx::rastport_set_color(&mut host.rp, color);
                host.g.rastport_draw_text(&host.rp, &[key]);
                text.push(key);
                x += host.g.rastport_text_width(&host.rp, &[key]);
            }
            _ => {}
        }
    }
}

/// The value of the typed digits (`v = v * 10 + d`, 32-bit wrap).
pub fn number_value(digits: &[u8]) -> i32 {
    digits.iter().fold(0i32, |v, &d| {
        v.wrapping_mul(10)
            .wrapping_add(i32::from(d) - i32::from(b'0'))
    })
}
