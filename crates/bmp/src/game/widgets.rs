//! Small UI helpers of segment 4091 used by several screens.

use crate::gfx::{Gfx, ui_draw_line, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_UP, Run};

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
