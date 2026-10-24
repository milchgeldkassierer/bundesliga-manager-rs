//! The message viewer `screen_messages` 4091:05c4 (`docs/re/logic/news.md` §6.4): the office
//! mail (the inbox of the current manager, three messages per page, scrolled one message at a
//! time) and the single message shown at once by `message_post`.
//!
//! The inbox is [`GameState::messages`] (`FAR:5524`), its length [`GameState::message_counts`]
//! (`DS:0618`); messages are added and removed by [`bmp_game::news::message_post`] and
//! [`bmp_game::news::message_delete`]. The viewer itself never changes the inbox.

use bmp_game::state::GameState;

use crate::gfx::{
    BitmapId, ui_draw_frame, ui_draw_line, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, LEFT_UP, Run};

/// `DS:2694`: the text of an empty inbox.
const NO_MESSAGE: &[u8] = b"Keine Nachricht vorhanden.";
/// `y` of the first line of the first panel.
const FIRST_LINE_Y: i16 = 0x7a;
/// Messages per page (one per panel).
const PAGE_ROWS: u8 = 3;
/// The index value "exit" of the paging loop.
const EXIT: i16 = 0xff;

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

#[allow(clippy::too_many_arguments)]
fn blit(host: &mut Host, src: BitmapId, sx: i16, sy: i16, dx: i16, dy: i16, w: i16, h: i16) {
    let screen = host.screen();
    host.g.blit_bitmap(src, sx, sy, screen, dx, dy, w, h);
}

/// Waits for a click on the message strip and returns `(y - 0x6c) / 0x29`: 0 up, 1 exit,
/// 2 down (3 for the strip's last pixel row, which no button handles); automatic play exits.
///
/// Origin: BMMAIN.EXE 4091:0aaa (ui_messages_wait_button)
fn ui_messages_wait_button(host: &mut Host, auto: bool) -> Run<i16> {
    if auto {
        return Ok(1);
    }
    loop {
        let ev = host.input_poll_mouse()?;
        if ev != LEFT_DOWN {
            continue;
        }
        let (x, y) = host.mouse_xy();
        if y > 0x6b && y < 0xe8 && x > 0xf5 && x < 0x106 {
            return Ok((y - 0x6c) / 0x29);
        }
    }
}

/// The lines of a message up to its NUL: the `^`-terminated parts (text after the last `^`
/// is not drawn).
fn message_lines(text: &[u8]) -> Vec<&[u8]> {
    let text = text.split(|&c| c == 0).next().unwrap_or(&[]);
    let mut parts: Vec<&[u8]> = text.split(|&c| c == b'^').collect();
    parts.pop();
    parts
}

/// What a click on the strip does to the paging loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Click {
    /// Nothing to scroll (or no button): wait for the next click.
    Again,
    /// Leave the viewer.
    Exit,
    /// Clear the panels and continue the loop with the index `i` (incremented before the
    /// next message is drawn).
    Page(i16),
}

/// The paging rules of `screen_messages`: `i` is the index of the last message drawn, `row`
/// the messages on the page, `count` the inbox length. "Up" needs `i > 2` and goes back
/// `row + 1` (not below -1), "down" needs a message after `i` and goes back 2: a full page
/// scrolls by one message either way.
///
/// Origin: BMMAIN.EXE 4091:05c4 (screen_messages, 4091:0986..09cf)
fn messages_click(sel: i16, i: i16, row: u8, count: u8) -> Click {
    match sel {
        0 if i > 2 => Click::Page((i - (i16::from(row) + 1)).max(-1)),
        1 => Click::Exit,
        2 if i16::from(count) > i + 1 => Click::Page(i - 2),
        _ => Click::Again,
    }
}

/// The message viewer. With `single`, it shows only that message; without, the inbox of
/// the current manager (`FAR:304a`) from the newest message on, or "Keine Nachricht
/// vorhanden." when it is empty. Every `^`-terminated line of a message is drawn centred
/// (the date line in colour 0x19 and 2 pixels apart, the others in 0x18), text after the
/// last `^` is not drawn, and every message starts its own panel. The strip of `PIC\2.VGA`
/// scrolls ([`messages_click`]) and exits; with `single`, "down" exits as well when the inbox
/// holds more than one message. The background is restored from VRAM page 2.
///
/// `count` is `DS:0618[FAR:304a]` (also used in the `single` case) and `inbox` the messages
/// `FAR:5524[FAR:304a * 20..]`; a missing entry below `count` draws nothing.
///
/// Origin: BMMAIN.EXE 4091:05c4 (screen_messages)
pub(super) fn screen_messages(
    host: &mut Host,
    inbox: &[bmp_formats::savegame::StringEntry],
    count: u8,
    single: Option<&[u8]>,
    auto: bool,
) -> Run<()> {
    let pic = host.load_picture(2).expect("PIC\\2.VGA");
    fill(host, 0x3b, 0x72, 0xf5, 0xea, 4);
    ui_draw_frame(&mut host.g, &mut host.rp, 0x39, 0x70, 0x106, 0xea, 7);
    for k in 0..3i16 {
        let d = 41 * k;
        let (g, rp) = (&mut host.g, &mut host.rp);
        let l = |v: i16| v as u8;
        ui_draw_line(g, rp, 0x3a, l(0x71 + d), 0xf2, l(0x71 + d), 1);
        ui_draw_line(g, rp, 0x3a, l(0x71 + d), 0x3a, l(0x97 + d), 1);
        ui_draw_line(g, rp, 0x3b, l(0x98 + d), 0xf3, l(0x98 + d), 7);
        ui_draw_line(g, rp, 0xf3, l(0x72 + d), 0xf3, l(0x98 + d), 7);
    }
    blit(host, pic, 0, 0, 0xf5, 0x71, 0x12, 0x7a);
    host.set_font(1);
    host.screen_fade(1)?;
    let mut i: i16 = 0;
    let mut y = FIRST_LINE_Y;
    let mut row: u8 = 0;
    loop {
        let text = match single {
            Some(t) => Some(t),
            None => usize::try_from(i)
                .ok()
                .and_then(|j| inbox.get(j))
                .map(|e| e.bytes.as_slice()),
        };
        if let Some(text) = text {
            let mut first = true;
            for line in message_lines(text) {
                let color = if first { 0x19 } else { 0x18 };
                ui_draw_text_centered_shadow(
                    &mut host.g,
                    &mut host.rp,
                    line,
                    0x3a,
                    0xf5,
                    y,
                    color,
                    0,
                );
                y += 8 + if first { 2 } else { 0 };
                first = false;
            }
        }
        row += 1;
        y += 7;
        if count == 0 && single.is_none() {
            ui_draw_text_centered_shadow(
                &mut host.g,
                &mut host.rp,
                NO_MESSAGE,
                0x3a,
                0xf5,
                0xb1,
                1,
                0,
            );
            i = -1;
        }
        if row == PAGE_ROWS || i16::from(count) - i - 1 == 0 || single.is_some() {
            let page = loop {
                let sel = ui_messages_wait_button(host, auto)?;
                let dy = sel * 0x28 + i16::from(sel == 2);
                let h = 0x28 + i16::from(sel == 2);
                blit(host, pic, 0x13, dy, 0xf5, 0x71 + dy, 0x12, h);
                while !auto && host.input_poll_mouse()? != LEFT_UP {}
                blit(host, pic, 0, dy, 0xf5, 0x71 + dy, 0x12, h);
                match messages_click(sel, i, row, count) {
                    Click::Again => {}
                    Click::Exit => break EXIT,
                    Click::Page(next) => break next,
                }
            };
            i = page;
            if i != EXIT {
                for k in 0..3i16 {
                    fill(host, 0x3b, 0x72 + 41 * k, 0xf0, 0x97 + 41 * k, 4);
                }
            }
            y = FIRST_LINE_Y;
            row = 0;
        }
        i += 1;
        if i16::from(count) <= i || single.is_some() {
            break;
        }
    }
    if let Some(work) = host.ui.work {
        let screen = host.screen();
        host.g
            .blit_bitmap_or_restore_page2(work, 0x39, 0x6e, screen, 0x39, 0x6e, 0xce, 0x89);
    }
    host.g.free(pic);
    Ok(())
}

/// The office mail of the current manager (`screen_messages(NULL)`).
///
/// Origin: BMMAIN.EXE 4091:05c4 (screen_messages)
pub(super) fn screen_messages_inbox(host: &mut Host, state: &GameState, auto: bool) -> Run<()> {
    let m = (state.current_manager as usize).min(3);
    let inbox = state.messages.get(m).map_or(&[][..], Vec::as_slice);
    screen_messages(host, inbox, state.message_counts[m], None, auto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_text_keeps_the_lines_before_the_last_separator() {
        let parts = message_lines(b"1. Juli 1968^Kuntz h{ngt^die Schuhe an den Nagel.^rest");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], b"die Schuhe an den Nagel.");
        assert!(message_lines(b"no separator").is_empty());
        assert_eq!(message_lines(b"a^b^\0c^"), [b"a", b"b"]);
    }

    #[test]
    fn up_needs_a_message_before_the_page_and_scrolls_by_one() {
        // Messages 0..2 shown: nothing above.
        assert_eq!(messages_click(0, 2, 3, 5), Click::Again);
        // Messages 1..3 shown: i 3 - 4 = -1, so the next page starts at 0.
        assert_eq!(messages_click(0, 3, 3, 5), Click::Page(-1));
        // Messages 2..4 shown: next page starts at 1.
        assert_eq!(messages_click(0, 4, 3, 5), Click::Page(0));
        // A short page never goes below -1.
        assert_eq!(messages_click(0, 3, 1, 4), Click::Page(1));
    }

    #[test]
    fn down_needs_a_message_after_the_page_and_scrolls_by_one() {
        // Messages 0..2 of 5 shown: i 2 - 2 = 0, the next page starts at 1.
        assert_eq!(messages_click(2, 2, 3, 5), Click::Page(0));
        // The last message is shown: nothing below.
        assert_eq!(messages_click(2, 4, 3, 5), Click::Again);
        assert_eq!(messages_click(2, 2, 3, 3), Click::Again);
        // Empty inbox (i = -1).
        assert_eq!(messages_click(2, -1, 1, 0), Click::Again);
        assert_eq!(messages_click(0, -1, 1, 0), Click::Again);
    }

    #[test]
    fn exit_and_the_last_pixel_row() {
        assert_eq!(messages_click(1, 2, 3, 5), Click::Exit);
        assert_eq!(messages_click(3, 2, 3, 5), Click::Again);
    }

    #[test]
    fn a_single_message_exits_with_down_only_when_the_inbox_has_more() {
        // The single case runs with i = 0: down pages (and the loop then leaves).
        assert_eq!(messages_click(2, 0, 1, 2), Click::Page(-2));
        assert_eq!(messages_click(2, 0, 1, 1), Click::Again);
    }
}
