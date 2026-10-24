//! The advertising contracts screen behind the office main icon "Info"
//! (segment 362f): six board sponsors, the shirt sponsor, the income table
//! and the offers to accept. Spec: `docs/re/logic/stats.md` §4.5.
//!
//! Data of the current manager `m`:
//! * `far_0000[2m]` shirt contract months left (0 = free), `[2m + 1]` its logo;
//! * `far_4b9a[2(6m + i)]` months left of board `i`, `[.. + 1]` its logo;
//! * `manager_contracts[m]`: `[0]` shirt, `[1..=6]` boards, `[7]` TV,
//!   `[8]` advertising expenses (all per month);
//! * the offers of `advertising_offers_generate` (§3.6): amounts in
//!   `far_002e` and lengths in years in `far_2ec2`, kind 0 = shirt,
//!   kind 1 = boards, ten offers each, offer `i` shows logo `i`.

use bmp_game::rng::{NumberFormat, format_long_thousands, ldiv, lmul};
use bmp_game::state::{GameState, OFFERS_PER_KIND};

use super::widgets::ui_dialog_box;
use super::{GAME_OPTION_A, GAME_OPTION_B, Session};
use crate::gfx::{
    BitmapId, ui_draw_frame, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow,
    ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, LEFT_UP, RIGHT_DOWN, Run};

/// `LBL[15..18]`, `UI[221..222]`, `LBL[319]`, `DS:54be`.
const CONTRACT: &[u8] = b"VERTRAG: ";
const MONTHS: &[u8] = b" MONAT";
const YEARS: &[u8] = b" JAHR";
const PLURAL: &[u8] = b"E";
const CANCELLABLE: &[u8] = b"K}NDBAR";
const NO_INTEREST: &[u8] = b"KEIN INTERESSE...";
const CURRENCY: &[u8] = b" DM";
const MINUS: &[u8] = b"-";
/// `LBL[137..138]`.
const STILL_BOUND: [&[u8]; 2] = [b"Sie stehen noch", b"unter Vertrag"];

/// `BP-4` before a sponsor was clicked.
const KIND_NONE: u8 = 100;
/// Tab number of the shirt sponsor (the picture of the player).
const SHIRT_TAB: i16 = 10;
/// Offer kinds (`BP-4`).
const KIND_SHIRT: u8 = 0;
const KIND_BOARD: u8 = 1;

/// The two pictures of the screen: `PIC\31.VGA` (buttons, `FAR:2e76`) and
/// `PIC\32.VGA` (sponsor logos, `FAR:4b5a`).
struct Pics {
    buttons: BitmapId,
    logos: BitmapId,
}

#[allow(clippy::too_many_arguments)]
fn blit(host: &mut Host, src: BitmapId, sx: i16, sy: i16, dx: i16, dy: i16, w: i16, h: i16) {
    let screen = host.screen();
    host.g.blit_bitmap(src, sx, sy, screen, dx, dy, w, h);
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn manager(state: &GameState) -> usize {
    state.current_manager as usize
}

/// Index of offer `i` of `kind` for manager `m` into `far_2ec2` (and, times
/// 4, into `far_002e`).
fn offer_index(m: usize, kind: u8, i: i8) -> usize {
    (2 * m + usize::from(kind)) * OFFERS_PER_KIND + i as usize
}

/// The offer amount `O(kind, i)` (0 = no interest).
pub(crate) fn offer_amount(state: &GameState, kind: u8, i: i8) -> i32 {
    let o = 4 * offer_index(manager(state), kind, i);
    i32::from_le_bytes(state.far_002e[o..o + 4].try_into().expect("4 bytes"))
}

fn set_offer_amount(state: &mut GameState, kind: u8, i: i8, v: i32) {
    let o = 4 * offer_index(manager(state), kind, i);
    state.far_002e[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// The offer length `Y(kind, i)` in years.
fn offer_years(state: &GameState, kind: u8, i: i8) -> u8 {
    state.far_2ec2[offer_index(manager(state), kind, i)]
}

/// Countdown byte and logo byte of the shirt contract (`slot == 10`) or of
/// board `slot`.
fn contract_bytes(state: &GameState, kind: u8, slot: i16) -> (usize, &[u8]) {
    let m = manager(state);
    if kind == KIND_SHIRT {
        (2 * m, &state.far_0000[..])
    } else {
        (2 * (6 * m + slot as usize), &state.far_4b9a[..])
    }
}

/// How an accept click on an offer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accept {
    /// The offer became the contract.
    Signed,
    /// The slot still runs a contract ("Sie stehen noch unter Vertrag").
    StillBound,
}

/// Accepts offer `offer` of `kind` for the shirt (`kind` 0) or board `slot`:
/// the logo and the length in months (`years * 12`, a byte) go to the
/// countdown bytes, the amount to `manager_contracts`. Board offers are used
/// up (`O(1, offer) = 0`); shirt offers stay.
///
/// Origin: BMMAIN.EXE 362f:2e98..362f:3009 (screen_manager_contract)
pub(crate) fn accept_offer(state: &mut GameState, kind: u8, slot: i16, offer: i8) -> Accept {
    let m = manager(state);
    let months = offer_years(state, kind, offer).wrapping_mul(12);
    let amount = offer_amount(state, kind, offer);
    if kind == KIND_SHIRT {
        if state.far_0000[2 * m] != 0 {
            return Accept::StillBound;
        }
        state.far_0000[2 * m + 1] = offer as u8;
        state.far_0000[2 * m] = months;
        state.manager_contracts[m][0] = amount;
    } else {
        let b = 2 * (6 * m + slot as usize);
        if state.far_4b9a[b] != 0 {
            return Accept::StillBound;
        }
        state.far_4b9a[b + 1] = offer as u8;
        state.far_4b9a[b] = months;
        state.manager_contracts[m][1 + slot as usize] = amount;
        set_offer_amount(state, kind, offer, 0);
    }
    Accept::Signed
}

/// One click on the advertising expenses field: left of x 0x107 lowers them
/// by 2500, else raises them by 2500; above 50000 they wrap to 2500, below
/// 2500 to 50000.
///
/// Origin: BMMAIN.EXE 362f:3152..362f:31cf (screen_manager_contract)
pub(crate) fn expenses_step(expenses: i32, x: i16) -> i32 {
    let delta = if x < 0x107 { -5000 } else { 0 } + 2500;
    let v = expenses.wrapping_add(delta);
    if v > 50000 {
        2500
    } else if v < 2500 {
        50000
    } else {
        v
    }
}

/// Sum of the six board contracts and the monthly total
/// `shirt + boards + TV - expenses` (32-bit, wrapping).
///
/// Origin: BMMAIN.EXE 362f:2ab2..362f:2bdd (contract_draw_board)
pub(crate) fn contract_sums(c: &[i32; 9]) -> (i32, i32) {
    let boards = c[1..7].iter().fold(0i32, |a, &v| a.wrapping_add(v));
    let total = c[0]
        .wrapping_add(c[7].wrapping_add(boards))
        .wrapping_sub(c[8]);
    (boards, total)
}

/// Head line of [`contract_info_line`]: `"VERTRAG: <n> MONAT[E]"` or, for
/// offers (`years`), `"VERTRAG: <n>JAHR[E]"` (the original skips the first
/// character of `" JAHR"` while `DS:224d == 0`), or `"K}NDBAR"` for `n == 0`.
/// `fmt` is the number format in effect (the pad width `DS:079c`).
///
/// Origin: BMMAIN.EXE 362f:25a2..362f:269a (contract_info_line)
pub(crate) fn contract_head_text(n: u8, years: bool, fmt: NumberFormat) -> Vec<u8> {
    if n == 0 {
        return CANCELLABLE.to_vec();
    }
    let mut t = CONTRACT.to_vec();
    t.extend(format_long_thousands(i32::from(n), fmt));
    if years {
        // `UI[221] + 1 - DS:224d` (`DS:2252 != 3`, the shipped data).
        t.extend_from_slice(&YEARS[1..]);
    } else {
        t.extend_from_slice(MONTHS);
    }
    if n > 1 {
        t.extend_from_slice(PLURAL);
    }
    t
}

/// Amount text of [`contract_info_line`]: the magnitude scaled by
/// `DS:2252 / DS:224e`, padded to 10 characters with thousands separators,
/// plus `" DM"`; the flag tells whether a `"-"` goes in front.
///
/// Origin: BMMAIN.EXE 362f:26cd..362f:27b0 (contract_info_line)
pub(crate) fn contract_amount_text(amount: i32) -> (bool, Vec<u8>) {
    let negative = amount < 0;
    let v = if negative {
        amount.wrapping_neg()
    } else {
        amount
    };
    let fmt = NumberFormat {
        pad_width: 10,
        no_separators: false,
        comma_separator: GAME_OPTION_A == 4,
    };
    // DS:224d == 0: no currency prefix (LBL[324]).
    let mut t = format_long_thousands(ldiv(lmul(GAME_OPTION_B, v), GAME_OPTION_A), fmt);
    t.extend_from_slice(CURRENCY);
    (negative, t)
}

/// Draws a contract or offer: with `left` the amount only, after clearing
/// `(x, y)-(0x134, y + 10)`, left-aligned at `x`; else the head line (see
/// [`contract_head_text`], `years` picks the unit) centred in 0xd9..0x139
/// and the amount centred 12 rows lower.
///
/// Origin: BMMAIN.EXE 362f:2551 (contract_info_line)
fn contract_info_line(
    host: &mut Host,
    n: u8,
    amount: i32,
    x: i16,
    y: i16,
    left: bool,
    years: bool,
) {
    let mut y = y;
    if left {
        fill(host, x, y, 0x134, y + 10, 0);
    } else {
        let head = contract_head_text(n, years, host.number_format());
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            &head,
            0xd9,
            0x139,
            y + 7,
            1,
            0xff,
        );
        y += 12;
    }
    let (negative, text) = contract_amount_text(amount);
    if negative {
        ui_draw_text_at(&mut host.g, &mut host.rp, MINUS, x, y + 7, 1);
    }
    // DS:079c = 1, DS:07b2 = 1 after the number.
    host.number_pad = 1;
    host.number_no_separators = true;
    // The original skips leading '%' characters (there are none: the pad is '^').
    let start = text.iter().take_while(|&&c| c == b'%').count();
    if left {
        ui_draw_text_at(&mut host.g, &mut host.rp, &text, x, y + 7, 1);
    } else {
        let t = &text[start..];
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, t, 0xd9, 0x139, y + 7, 1, 0xff);
    }
}

/// Outline of tab `tab` in `color`: the shirt (10) gets a frame around the
/// player picture, a board the lines below and right of its logo.
///
/// Origin: BMMAIN.EXE 362f:23e4 (contract_draw_tab)
fn contract_draw_tab(host: &mut Host, tab: i16, color: u8) {
    let (g, rp) = (&mut host.g, &mut host.rp);
    if tab == SHIRT_TAB {
        ui_draw_frame(g, rp, 5, 0x3b, 0x6e, 0xe5, color);
    } else {
        let x = tab * 0x34 + 0x36;
        ui_draw_line(g, rp, tab * 0x34 + 5, 0x32, x, 0x32, color);
        ui_draw_line(g, rp, x, 0x13, x, 0x32, color);
    }
}

/// 38x30 logo `n` at `(x, y)`.
///
/// Origin: BMMAIN.EXE 362f:248a (contract_blit_sponsor_large)
fn contract_blit_sponsor_large(host: &mut Host, logos: BitmapId, n: u8, x: i16, y: i16) {
    let (n, sy) = if n > 4 { (n - 5, 0x21) } else { (n, 0) };
    blit(host, logos, i16::from(n) * 0x27, sy + 1, x, y, 0x26, 0x1e);
}

/// 26x16 logo `n` on the shirt of the player at (0x33, 0x6e).
///
/// Origin: BMMAIN.EXE 362f:24ed (contract_blit_sponsor_small)
fn contract_blit_sponsor_small(host: &mut Host, logos: BitmapId, n: u8) {
    let (n, sy) = if n > 4 { (n - 5, 0x55) } else { (n, 0x43) };
    blit(host, logos, i16::from(n) * 0x1c, sy, 0x33, 0x6e, 0x1a, 0x10);
}

/// Accept and decline buttons (greyed when the offer is 0) and the offer
/// `O(kind, i)` or "KEIN INTERESSE...".
///
/// Origin: BMMAIN.EXE 362f:281f (advertising_offer_panel)
fn advertising_offer_panel(host: &mut Host, pics: &Pics, state: &GameState, kind: u8, i: i8) {
    fill(host, 0xd9, 0x3c, 0x139, 0x5a, 0);
    let amount = offer_amount(state, kind, i);
    let sx = if amount == 0 { 0x60 } else { 0 };
    blit(host, pics.buttons, sx, 0x2f, 0xd9, 0x5b, 0x30, 0x10);
    blit(host, pics.buttons, sx, 0x3f, 0x109, 0x5b, 0x30, 0x10);
    if amount == 0 {
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            NO_INTEREST,
            0xd9,
            0x139,
            0x4e,
            1,
            0xff,
        );
    } else {
        let years = offer_years(state, kind, i);
        contract_info_line(host, years, amount, 0xd9, 0x3e, false, true);
    }
}

/// The shirt logo, the six boards (empty ones as brown fields) and the
/// income table.
///
/// Origin: BMMAIN.EXE 362f:29a8 (contract_draw_board)
fn contract_draw_board(host: &mut Host, pics: &Pics, state: &GameState) {
    let m = manager(state);
    if state.far_0000[2 * m] != 0 {
        contract_blit_sponsor_small(host, pics.logos, state.far_0000[2 * m + 1]);
    }
    for i in 0..6i16 {
        let b = 2 * (6 * m + i as usize);
        let x = i * 0x34 + 8;
        if state.far_4b9a[b] == 0 {
            fill(host, x, 0x13, i * 0x34 + 0x2e, 0x31, 9);
        } else {
            contract_blit_sponsor_large(host, pics.logos, state.far_4b9a[b + 1], x, 0x13);
        }
    }
    let c = state.manager_contracts[m];
    let (boards, total) = contract_sums(&c);
    contract_info_line(host, state.far_0000[2 * m], c[0], 0xf0, 0x80, true, false);
    contract_info_line(host, 0, boards, 0xf0, 0x8e, true, false);
    contract_info_line(host, 0, c[7], 0xf0, 0x9c, true, false);
    contract_info_line(host, 0, c[8], 0xf0, 0xb9, true, false);
    contract_info_line(host, 0, total, 0xf0, 0xd2, true, false);
}

/// Polls until the left button is released (`FAR:2ea2 == 2`).
fn wait_left_up(host: &mut Host) -> Run<()> {
    while host.button != LEFT_UP {
        host.input_poll_mouse()?;
    }
    Ok(())
}

/// Sponsor boards, shirt sponsor and TV income with the offers to accept.
///
/// Click a board logo (or the player for the shirt) to select it and see its
/// contract, then the big logo field to browse the offers with the two arrow
/// buttons; the left button below the offer accepts it, the right one
/// cancels. The expenses field steps the advertising expenses. A right click
/// anywhere leaves the screen.
///
/// Origin: BMMAIN.EXE 362f:2be8 (screen_manager_contract)
pub fn screen_manager_contract(
    host: &mut Host,
    _session: &mut Session,
    state: &mut GameState,
) -> Run<()> {
    // The pictures are part of the game data; vga_load_picture exits the
    // program when one is missing.
    let pics = Pics {
        buttons: host.picture(0x1f)?,
        logos: host.picture(0x20)?,
    };
    host.screen_fade(0)?;
    host.g.mouse_pointer_hide(0);
    host.screen_picture(0x2e)?;
    host.g.mouse_pointer_show();
    host.set_font(2);
    contract_draw_board(host, &pics, state);
    // palette_load_rgb4(DS:0798 = 1): the second palette of PIC\1.PAL.
    host.g.palette_load_slot(1, 0x100);
    host.screen_fade(1)?;

    // BP-0x22: an offer is shown and can be accepted.
    let mut offer_mode = false;
    // BP-4: kind of the selected tab, BP-2: its number, BP-0x26: the tab
    // drawn highlighted, BP-6: the offer shown (uninitialised in the
    // original; read only once `offer_mode` has set it).
    let mut kind = KIND_NONE;
    let mut slot: i16 = 0;
    let mut prev_tab: i8 = -1;
    let mut offer: i8 = 0;
    loop {
        host.input_poll_mouse()?;
        if host.button == RIGHT_DOWN {
            break;
        }
        if host.button != LEFT_DOWN {
            continue;
        }
        let (x, y) = host.mouse_xy();

        // Accept / decline buttons below the offer.
        if offer_mode
            && x > 0xd9
            && x < 0x139
            && y > 0x5a
            && y < 0x6d
            && offer_amount(state, kind, offer) != 0
        {
            let dx = if x > 0x108 { 0x10a } else { 0xd9 };
            let sy = if x < 0x108 { 0x3f } else { 0x2f };
            blit(host, pics.buttons, 0x30, sy, dx, 0x5b, 0x30, 0x10);
            wait_left_up(host)?;
            blit(host, pics.buttons, 0, 0x2f, dx, 0x5b, 0x30, 0x10);
            if x < 0x109 {
                match accept_offer(state, kind, slot, offer) {
                    Accept::Signed => offer_mode = false,
                    Accept::StillBound => {
                        let [a, b] = STILL_BOUND;
                        ui_dialog_box(host, a, b)?;
                    }
                }
                contract_draw_board(host, &pics, state);
            }
            if !offer_mode || x > 0x108 {
                offer_mode = false;
                blit(host, pics.buttons, 0x3c, 0, 0xb5, 0x3c, 0x1e, 0x17);
                blit(host, pics.buttons, 0x3c, 0x17, 0xb5, 0x54, 0x1e, 0x17);
                blit(host, pics.buttons, 0x60, 0x2f, 0xd9, 0x5b, 0x30, 0x10);
                blit(host, pics.buttons, 0x60, 0x3f, 0x109, 0x5b, 0x30, 0x10);
            }
        }

        // Advertising expenses.
        if x > 0xd8 && x < 0x135 && y > 0xb9 && y < 0xc5 {
            let m = manager(state);
            state.manager_contracts[m][8] = expenses_step(state.manager_contracts[m][8], x);
            contract_draw_board(host, &pics, state);
        }

        // Select the shirt (player picture) or a board.
        let shirt = x > 3 && x < 100 && y > 0x50 && y < 0xd2 && prev_tab != SHIRT_TAB as i8;
        let board = x > 3 && x < 0x13c && y > 0x12 && y < 0x33;
        if shirt || board {
            slot = (x - 4) / 0x34;
            kind = if y < 0x50 { KIND_BOARD } else { KIND_SHIRT };
            if kind == KIND_SHIRT {
                slot = SHIRT_TAB;
            }
            if i16::from(prev_tab) == slot {
                continue;
            }
            fill(host, 0xd9, 0x3c, 0x139, 0x5a, 0);
            offer_mode = false;
            blit(host, pics.buttons, 0x60, 0x2f, 0xd9, 0x5b, 0x30, 0x10);
            blit(host, pics.buttons, 0x60, 0x3f, 0x109, 0x5b, 0x30, 0x10);
            fill(host, 0x7f, 0x43, 0xa4, 0x60, 9);
            contract_draw_tab(host, slot, 0x1d);
            if prev_tab != -1 {
                contract_draw_tab(host, i16::from(prev_tab), 0x1c);
            }
            prev_tab = slot as i8;
            let m = manager(state);
            let (b, bytes) = contract_bytes(state, kind, slot);
            let months = bytes[b];
            let amount = if kind == KIND_SHIRT {
                state.manager_contracts[m][0]
            } else {
                state.manager_contracts[m][1 + slot as usize]
            };
            contract_info_line(host, months, amount, 0xd9, 0x3e, false, false);
        }

        // The big logo field opens the offers, starting at the current logo.
        if kind != KIND_NONE && x > 0x7e && x < 0xa0 && y > 0x43 && y < 0x6c && !offer_mode {
            offer_mode = true;
            let (b, bytes) = contract_bytes(state, kind, slot);
            offer = bytes[b + 1] as i8;
            blit(host, pics.buttons, 0, 0, 0xb5, 0x3c, 0x1e, 0x17);
            blit(host, pics.buttons, 0, 0x17, 0xb5, 0x54, 0x1e, 0x17);
            contract_blit_sponsor_large(host, pics.logos, offer as u8, 0x7f, 0x43);
            advertising_offer_panel(host, &pics, state, kind, offer);
        }

        // Arrow buttons: the upper one shows the next offer, the lower one
        // the previous.
        if offer_mode && x > 0xb4 && x < 0xd3 && y > 0x3b && y < 0x6c {
            let down = i16::from(y > 0x52);
            blit(
                host,
                pics.buttons,
                0x1e,
                down * 0x17,
                0xb5,
                down * 0x18 + 0x3c,
                0x1e,
                0x17,
            );
            wait_left_up(host)?;
            blit(
                host,
                pics.buttons,
                0,
                down * 0x17,
                0xb5,
                down * 0x18 + 0x3c,
                0x1e,
                0x17,
            );
            if y < 0x53 && offer < 9 {
                offer += 1;
            }
            if y > 0x52 && offer > 0 {
                offer -= 1;
            }
            contract_blit_sponsor_large(host, pics.logos, offer as u8, 0x7f, 0x43);
            advertising_offer_panel(host, &pics, state, kind, offer);
        }
    }
    host.g.free(pics.logos);
    host.g.free(pics.buttons);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> NumberFormat {
        NumberFormat {
            pad_width: 1,
            no_separators: true,
            ..Default::default()
        }
    }

    #[test]
    fn head_line_units_and_plural() {
        assert_eq!(contract_head_text(0, false, fmt()), b"K}NDBAR");
        assert_eq!(contract_head_text(1, false, fmt()), b"VERTRAG: 1 MONAT");
        assert_eq!(contract_head_text(24, false, fmt()), b"VERTRAG: 24 MONATE");
        assert_eq!(contract_head_text(1, true, fmt()), b"VERTRAG: 1JAHR");
        assert_eq!(contract_head_text(3, true, fmt()), b"VERTRAG: 3JAHRE");
    }

    #[test]
    fn amount_is_padded_with_separators() {
        assert_eq!(
            contract_amount_text(201_399),
            (false, b"^^^201.399 DM".to_vec())
        );
        assert_eq!(
            contract_amount_text(-2500),
            (true, b"^^^^^2.500 DM".to_vec())
        );
        assert_eq!(
            contract_amount_text(1_234_567),
            (false, b"^1.234.567 DM".to_vec())
        );
    }

    #[test]
    fn sums_subtract_the_expenses() {
        let c = [
            201_399, 50_000, 40_000, 60_000, 50_000, 40_000, 52_599, 62_000, 50_000,
        ];
        assert_eq!(contract_sums(&c), (292_599, 505_998));
    }

    #[test]
    fn expenses_step_and_wrap() {
        assert_eq!(expenses_step(50_000, 0x106), 47_500);
        assert_eq!(expenses_step(47_500, 0x107), 50_000);
        assert_eq!(expenses_step(50_000, 0x107), 2500);
        assert_eq!(expenses_step(2500, 0x100), 50_000);
        assert_eq!(expenses_step(5000, 0x100), 2500);
    }

    fn state() -> Box<GameState> {
        let mut s = Box::new(GameState::zeroed());
        s.current_manager = 1;
        s
    }

    #[test]
    fn accepting_a_board_offer_signs_and_uses_it_up() {
        let mut s = state();
        // Offer 7 of kind 1 for manager 1: index (2 + 1) * 10 + 7 = 37.
        s.far_002e[4 * 37..4 * 37 + 4].copy_from_slice(&45_000i32.to_le_bytes());
        s.far_2ec2[37] = 2;
        assert_eq!(accept_offer(&mut s, KIND_BOARD, 3, 7), Accept::Signed);
        assert_eq!(&s.far_4b9a[2 * 9..2 * 9 + 2], &[24, 7]);
        assert_eq!(s.manager_contracts[1][4], 45_000);
        assert_eq!(offer_amount(&s, KIND_BOARD, 7), 0);
        // The slot is taken now.
        s.far_002e[4 * 37..4 * 37 + 4].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(accept_offer(&mut s, KIND_BOARD, 3, 7), Accept::StillBound);
        assert_eq!(s.manager_contracts[1][4], 45_000);
    }

    #[test]
    fn accepting_a_shirt_offer_keeps_the_offer() {
        let mut s = state();
        // Offer 2 of kind 0 for manager 1: index 20 + 2 = 22.
        s.far_002e[4 * 22..4 * 22 + 4].copy_from_slice(&180_000i32.to_le_bytes());
        s.far_2ec2[22] = 3;
        assert_eq!(
            accept_offer(&mut s, KIND_SHIRT, SHIRT_TAB, 2),
            Accept::Signed
        );
        assert_eq!(&s.far_0000[2..4], &[36, 2]);
        assert_eq!(s.manager_contracts[1][0], 180_000);
        assert_eq!(offer_amount(&s, KIND_SHIRT, 2), 180_000);
        assert_eq!(
            accept_offer(&mut s, KIND_SHIRT, SHIRT_TAB, 2),
            Accept::StillBound
        );
    }
}
