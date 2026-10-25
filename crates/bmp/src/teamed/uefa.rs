//! Package "uefa": the strength list of the 136 foreign clubs (spec:
//! `docs/re/teamed/uefa.md`).
//!
//! "UEFA" on the bar scrolls the screen to row 87 (`vga_show_page(3)`) and lists the
//! foreign clubs 64..199 in two columns of 34, two pages of 68, with their three strength
//! bytes (`MANA.DAT` block 2). A click on a row edits the club's name and then its
//! three strengths with `text_input`; a strength that lowers the average of all foreign
//! strengths below 79 is refused with "EINGABE NICHT ANGENOMMEN, / UEFA-CUP SONST ZU
//! LEICHT.".

use bmp_formats::mana::{self, ManaDat};
use bmp_game::rng::ldiv;

use crate::gfx::{self, Gfx};
use crate::host::{Host, LEFT_DOWN, Run};

use super::dialog::text_input;
use super::{Editor, GRID_ORIGIN_Y, clear_grid, format_number, screen_set_visible, set_font};

/// First foreign club.
pub const FIRST_FOREIGN_CLUB: usize = mana::GERMAN_CLUB_COUNT;
/// Clubs per page (two columns of [`ROWS`]).
pub const CLUBS_PER_PAGE: i16 = 0x44;
/// Rows per column.
pub const ROWS: i16 = 0x22;
/// Lowest accepted average of the foreign strengths (`12a5:2ce0`).
pub const MIN_AVERAGE_STRENGTH: i32 = 0x4f;

/// The column headers "A M S" twice (`DS:0416..042C`), drawn at `(x, 0x10)` in colour 7
/// and at `(x + 1, 0x10)` in colour 1.
const HEADERS: [(i16, &[u8]); 6] = [
    (0x61, b"A"),
    (0x74, b"M"),
    (0x89, b"S"),
    (0x101, b"A"),
    (0x114, b"M"),
    (0x129, b"S"),
];

/// "EINGABE NICHT ANGENOMMEN," (`DS:0448`).
const REFUSED_1: &[u8] = b"EINGABE NICHT ANGENOMMEN,";
/// "UEFA-CUP SONST ZU LEICHT." (`DS:042E`).
const REFUSED_2: &[u8] = b"UEFA-CUP SONST ZU LEICHT.";
/// MOD2026: [`REFUSED_2`] with the Conference League, which replaces the UEFA cup; the full
/// name does not fit the message box.
const MOD2026_REFUSED_2: &[u8] = b"CONFERENCE L. SONST ZU LEICHT.";

/// The second line of the refusal of a too low strength.
fn refused_2(mod2026: bool) -> &'static [u8] {
    // MOD2026: a mod game names the Conference League.
    if mod2026 {
        MOD2026_REFUSED_2
    } else {
        REFUSED_2
    }
}

/// Whether the foreign clubs are strong enough: the average of the 408 strength bytes
/// of clubs 64..199 (signed, MSC long division) is at least 79. A negative sum also
/// passes (the original tests the high word of the quotient for non-zero first).
///
/// Origin: TEAMED.EXE 12a5:2ce0 (uefa_strength_ok)
pub fn strength_ok(m: &ManaDat) -> bool {
    let sum: i32 = m.strength[FIRST_FOREIGN_CLUB..]
        .iter()
        .flatten()
        .map(|&b| i32::from(b as i8))
        .sum();
    let average = ldiv(sum, 0x198);
    (average >> 16) != 0 || (average as u16) >= MIN_AVERAGE_STRENGTH as u16
}

/// Club of list row `row` in column `col` on `page`.
pub fn club_at(page: u8, col: i16, row: i16) -> i16 {
    (col + i16::from(page) * 2) * ROWS + row + FIRST_FOREIGN_CLUB as i16
}

/// A message box in the crest view at `(1, 2)-(0x77, 0x28)` ("Fehler:" and two lines),
/// shown for 120 ticks; the area is saved before and restored after. In the scrolled
/// UEFA screen its top ten rows lie above the visible part.
///
/// Origin: TEAMED.EXE 1000:1bd2 (dialog_message_uefa)
pub fn message(host: &mut Host, ed: &mut Editor, line1: &[u8], line2: &[u8]) -> Run<()> {
    let backup = host.blank_bitmap(0x78, 0x32, 0);
    host.g
        .blit_bitmap(ed.grid.bitmap, 0, 0, backup, 0, 0, 0x78, 0x32);
    let g = &mut host.g;
    let rp = &mut ed.grid;
    gfx::ui_fill_rect(g, rp, 1, 2, 0x77, 0x28, 0x10);
    gfx::ui_draw_frame(g, rp, 1, 2, 0x77, 0x28, 0x13);
    set_font(rp, 2);
    gfx::ui_draw_text_centered_shadow(g, rp, b"Fehler:", 1, 0x77, 0xc, 0x13, 0xff);
    set_font(rp, 3);
    gfx::ui_draw_text_centered_shadow(g, rp, line1, 1, 0x77, 0x18, 0x13, 0xff);
    gfx::ui_draw_text_centered_shadow(g, rp, line2, 1, 0x77, 0x20, 0x13, 0xff);
    host.delay_ticks(0x78)?;
    host.g
        .blit_bitmap(backup, 0, 0, ed.grid.bitmap, 0, 0, 0x78, 0x32);
    // DEVIATION: the original never frees the backup picture (a leak per message).
    host.g.free(backup);
    Ok(())
}

/// XOR highlight of list row `row` in column `col` (the grid rastport is in mode 2).
fn toggle_highlight(g: &mut Gfx, ed: &mut Editor, col: i16, row: i16) {
    let x = 0xa0 * col;
    let y = 7 * row + 0x11;
    gfx::ui_fill_rect(g, &mut ed.grid, x + 5, y, x + 0x96, y + 5, 1);
}

/// Draws strength byte `part` of `club` at `(x, y)` in colour 2.
fn draw_strength(g: &mut Gfx, ed: &mut Editor, club: usize, part: usize, x: i16, y: i16) {
    let text = format_number(i32::from(ed.mana.strength[club][part] as i8));
    gfx::ui_draw_text_at(g, &mut ed.grid, &text, x, y, 2);
}

/// Clears the list, draws the bar, the headers and the clubs of the current page.
///
/// Origin: TEAMED.EXE 12a5:2359 (uefa_screen, page drawing)
fn draw_list(host: &mut Host, ed: &mut Editor) {
    let g = &mut host.g;
    screen_set_visible(g, false);
    Gfx::rastport_set_draw_mode(&mut ed.grid, 0);
    clear_grid(g, &mut ed.grid);
    g.blit_bitmap(ed.bar, 0, 0, ed.grid.bitmap, 0x138, 0xc, 8, 0x9b);
    set_font(&mut ed.grid, 3);
    for (x, text) in HEADERS {
        gfx::ui_draw_text_at(g, &mut ed.grid, text, x, 0x10, 7);
        gfx::ui_draw_text_at(g, &mut ed.grid, text, x + 1, 0x10, 1);
    }
    let (mut x, mut y) = (5i16, 0x16i16);
    let first = i16::from(ed.uefa_page) * CLUBS_PER_PAGE;
    for i in first..first + CLUBS_PER_PAGE {
        let club = (i + FIRST_FOREIGN_CLUB as i16) as usize;
        let name = mana::c_str(&ed.mana.clubs[club].name).to_vec();
        gfx::ui_draw_text_at(g, &mut ed.grid, &name, x, y, 1);
        draw_strength(g, ed, club, 0, x + 0x5a, y);
        draw_strength(g, ed, club, 1, x + 0x6e, y);
        draw_strength(g, ed, club, 2, x + 0x82, y);
        y += 7;
        if first - i == -(ROWS - 1) {
            y = 0x16;
            x = 0xa5;
        }
    }
    screen_set_visible(g, true);
}

/// "UEFA" on the bar: the list of the 136 foreign clubs with their three strengths.
/// A click on the bar's upper part ("LIGA") returns to the crest grid, on its lower
/// part ("UEFA") turns the page; a click on a row edits that club.
///
/// Origin: TEAMED.EXE 12a5:233d (uefa_screen)
pub fn run(host: &mut Host, ed: &mut Editor) -> Run<()> {
    // The club under the pointer (-1 none) and its column and row.
    let mut current: i16 = -1;
    let (mut cur_col, mut cur_row) = (0i16, 0i16);
    // The input buffer ([BP-0x50]); its stale bytes count in text_input's width check.
    let mut buf = [0u8; 0x32];
    host.g.vga_show_page_teamed(3);
    'page: loop {
        draw_list(host, ed);
        loop {
            Gfx::rastport_set_draw_mode(&mut ed.grid, 2);
            loop {
                let ev = host.input_poll_mouse()?;
                let (x, y) = host.mouse_xy();
                let y = y.wrapping_sub(GRID_ORIGIN_Y);
                if ev == LEFT_DOWN && x > 0x136 && y < 0xa8 {
                    if y < 0x5a {
                        // "LIGA": back to the crest grid.
                        Gfx::rastport_set_draw_mode(&mut ed.grid, 0);
                        host.g.vga_show_page_teamed(1);
                        return Ok(());
                    }
                    // "UEFA": the other page. The highlight variable is kept although
                    // the redraw removed the highlight (as in the original).
                    ed.uefa_page ^= 1;
                    continue 'page;
                }
                if !(6..=0x112).contains(&x) || !(0x12..=0xfd).contains(&y) {
                    if current != -1 {
                        toggle_highlight(&mut host.g, ed, cur_col, cur_row);
                        current = -1;
                    }
                } else {
                    let col = i16::from(x > 0xa5);
                    let row = (y - 0x11) / 7;
                    let club = club_at(ed.uefa_page, col, row);
                    if club != current {
                        if current != -1 {
                            toggle_highlight(&mut host.g, ed, cur_col, cur_row);
                        }
                        toggle_highlight(&mut host.g, ed, col, row);
                        (cur_col, cur_row, current) = (col, row, club);
                    }
                }
                if ev == LEFT_DOWN && current != -1 {
                    break;
                }
            }
            toggle_highlight(&mut host.g, ed, cur_col, cur_row);
            Gfx::rastport_set_draw_mode(&mut ed.grid, 0);
            edit_club(host, ed, &mut buf, current as usize, cur_col, cur_row)?;
            current = -1;
        }
    }
}

/// Edits the name and the three strengths of `club` in its list row.
///
/// Origin: TEAMED.EXE 12a5:28c4 (uefa_screen, click on a row)
fn edit_club(
    host: &mut Host,
    ed: &mut Editor,
    buf: &mut [u8],
    club: usize,
    col: i16,
    row: i16,
) -> Run<()> {
    let x = 0xa0 * col + 5;
    let y = 7 * row + 0x11;
    // The name: up to 77 pixels, copied with strncpy(.., 23) (zero-padded).
    gfx::ui_fill_rect(&mut host.g, &mut ed.grid, x, y, x + 0x59, y + 5, 0);
    text_input(host, &mut ed.grid, buf, x, y + 5, 0x4d, 0xb, 0, true)?;
    if buf[0] != 0 {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let name = &mut ed.mana.clubs[club].name;
        for (i, b) in name.iter_mut().enumerate() {
            *b = if i < len { buf[i] } else { 0 };
        }
    }
    gfx::ui_fill_rect(&mut host.g, &mut ed.grid, x, y, x + 0x6d, y + 5, 0);
    let name = mana::c_str(&ed.mana.clubs[club].name).to_vec();
    gfx::ui_draw_text_at(&mut host.g, &mut ed.grid, &name, x, y + 5, 1);
    // The strengths: two digits each. A refused value is replaced by the club's first
    // strength as it was before this edit (the original keeps only that one byte; for
    // the second and third value this restores the wrong number).
    let mut old = 0u8;
    for (part, (dx, clear_to)) in [(0x5a, 0x81), (0x6e, 0x91), (0x82, 0x91)]
        .into_iter()
        .enumerate()
    {
        let value = text_input(host, &mut ed.grid, buf, x + dx, y + 5, 0x3ea, 0xb, 0, false)?;
        if part == 0 {
            old = ed.mana.strength[club][0];
        }
        if buf[0] != 0 {
            ed.mana.strength[club][part] = value as u8;
            if !strength_ok(&ed.mana) {
                ed.mana.strength[club][part] = old;
                message(host, ed, REFUSED_1, refused_2(host.mod2026))?;
            }
        }
        gfx::ui_fill_rect(&mut host.g, &mut ed.grid, x + dx, y, x + clear_to, y + 5, 0);
        draw_strength(&mut host.g, ed, club, part, x + dx, y + 5);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mana_with_strength(v: u8) -> ManaDat {
        let mut m = ManaDat::parse(&vec![0u8; mana::FILE_SIZE]).unwrap();
        for s in m.strength.iter_mut() {
            *s = [v; 3];
        }
        m
    }

    #[test]
    fn average_threshold_is_79() {
        assert!(strength_ok(&mana_with_strength(79)));
        assert!(!strength_ok(&mana_with_strength(78)));
        let mut m = mana_with_strength(79);
        m.strength[100][0] = 78;
        // 32231 / 408 = 78.99..: truncated to 78.
        assert!(!strength_ok(&m));
        // German clubs do not count.
        let mut m = mana_with_strength(79);
        m.strength[0] = [0; 3];
        assert!(strength_ok(&m));
    }

    #[test]
    fn negative_sums_pass() {
        // Bytes >= 0x80 are negative; a negative average has a non-zero high word.
        assert!(strength_ok(&mana_with_strength(0xf0)));
        assert!(!strength_ok(&mana_with_strength(0)));
    }

    /// MOD2026: the refusal names the Conference League in a mod game and fits the message
    /// box (font 3, centred on 1..0x77 in its frame; skipped without `BMP_DATA`).
    #[test]
    fn the_refusal_names_the_conference_league_in_the_mod() {
        use crate::game::{MICRO4, centered_fits, euro_gfx};
        assert_eq!(refused_2(false), b"UEFA-CUP SONST ZU LEICHT.");
        assert_eq!(refused_2(true), b"CONFERENCE L. SONST ZU LEICHT.");
        let Some(g) = euro_gfx() else {
            eprintln!("skipped: no BMP_DATA");
            return;
        };
        assert!(centered_fits(
            &g,
            MICRO4,
            refused_2(true),
            (1, 0x77),
            2..=0x76
        ));
        let full = b"CONFERENCE LEAGUE SONST ZU LEICHT.";
        assert!(!centered_fits(&g, MICRO4, full, (1, 0x77), 2..=0x76));
    }

    #[test]
    fn rows_map_to_clubs() {
        assert_eq!(club_at(0, 0, 0), 64);
        assert_eq!(club_at(0, 0, 33), 97);
        assert_eq!(club_at(0, 1, 0), 98);
        assert_eq!(club_at(1, 0, 0), 132);
        assert_eq!(club_at(1, 1, 33), 199);
    }
}
