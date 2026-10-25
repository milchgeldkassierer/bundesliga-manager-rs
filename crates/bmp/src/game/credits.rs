//! The loans screen (segment 212a): the bank picture, the lender buttons, the
//! bank's interest table, the list of the loans taken from the chosen lender
//! and the account summary. Spec: `docs/re/logic/finance.md` §2.5-§2.9.
//!
//! The loan bookkeeping is [`bmp_game::finance::credit_take`] and
//! [`bmp_game::finance::credit_sum`].

use bmp_game::finance::{BANK, CreditField, credit_sum, credit_take};
use bmp_game::rng::{format_long_thousands, ldiv, lmul};
use bmp_game::state::{Credit, GameState};

use super::stadium::{StatusText, status_line_show};
use super::start_menu::Pictures;
use super::widgets::{dialog_okay, ui_dialog_box, ui_input_number, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, currency};
use crate::gfx::{Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_DOWN, Run};

/// `DS:2538..2558`.
const DUE_ON: &[u8] = b"F[LLIG AM ";
const INTEREST_PER_MONTH: &[u8] = b"ZINSEN PRO MONAT: ";
const TABLE_HEADER: &[u8] = b"Dauer  Zinssatz";
const MONTHS: &[u8] = b" Mon.";
const DEBTS_HERE: &[u8] = b"SCHULDEN HIER: ";
const INTEREST_HERE: &[u8] = b"ZINSEN HIER: ";
const BALANCE: &[u8] = b"Kontostand:";
const TOTAL_DEBTS: &[u8] = b"Gesamtschulden:";
const INTEREST_MONTH: &[u8] = b"Zinsen/Monat:";
/// `DS:290c..2914`: the loan prompts on the status line.
const ASK_AMOUNT: &[u8] = b"Wie hoch soll der Kredit sein ?";
const ASK_MONTHS: &[u8] = b"Laufzeit des Kredites (Mon.) ?";
const ASK_RATE: &[u8] = b"Prozentsatz des Kredites ?";

/// Most money a manager may owe the bank (`4 Mio. DM`) or one fellow manager.
const BANK_LIMIT: i32 = 4_000_000;
const MANAGER_LIMIT: i32 = 1_000_000;

/// `display(x)` of finance.md §0.2.
fn display(x: i32) -> i32 {
    ldiv(lmul(x, GAME_OPTION_B), GAME_OPTION_A)
}

/// `internal(v)`: a typed amount in DM.
fn internal(v: i32) -> i32 {
    ldiv(lmul(v, GAME_OPTION_A), GAME_OPTION_B)
}

fn num(host: &Host, v: i32) -> Vec<u8> {
    format_long_thousands(v, host.number_format())
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: u8) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, i16::from(color));
}

fn centered(host: &mut Host, text: &[u8], y: i16) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, 0x97, 0x103, y, 1, 0);
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn line(host: &mut Host, x0: i16, y0: u8, x1: i16, y1: u8, color: u8) {
    ui_draw_line(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn button(host: &mut Host, x: i16, y: i16, w: i16, h: u16, color: u8) -> [i16; 4] {
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, y, w, h as i16, false, color)
        .1
}

fn current(state: &GameState) -> usize {
    (state.current_manager as usize).min(3)
}

/// The three text lines of a loan: date taken, amount and rate; due date;
/// interest per payment.
///
/// Origin: BMMAIN.EXE 212a:119b (credit_list_draw, line texts)
pub fn credit_lines(c: &Credit, fmt: bmp_game::rng::NumberFormat, mod2026: bool) -> [Vec<u8>; 3] {
    let f = |v: i32, years: bool| {
        let mut fy = fmt;
        fy.no_separators = years;
        format_long_thousands(v, fy)
    };
    let mut l1 = f(i32::from(c.day_taken), false);
    l1.push(b'.');
    l1.extend_from_slice(&f(i32::from(c.month_taken) + 1, false));
    l1.push(b'.');
    l1.extend_from_slice(&f(i32::from(c.year_taken), true));
    l1.extend_from_slice(b": ");
    l1.extend_from_slice(&f(display(c.principal), false));
    // MOD2026: the mod's amounts are in euros.
    l1.extend_from_slice(currency(mod2026));
    l1.extend_from_slice(b" (");
    l1.extend_from_slice(&f(i32::from(c.rate), false));
    l1.extend_from_slice(b"%)");
    let mut l2 = DUE_ON.to_vec();
    l2.extend_from_slice(&f(i32::from(c.payment_day), false));
    l2.push(b'.');
    l2.extend_from_slice(&f(i32::from(c.due_month) + 1, false));
    l2.push(b'.');
    l2.extend_from_slice(&f(i32::from(c.due_year), true));
    let mut l3 = INTEREST_PER_MONTH.to_vec();
    l3.extend_from_slice(&f(display(c.interest_payment), false));
    l3.extend_from_slice(currency(mod2026));
    [l1, l2, l3]
}

/// `head`, the shown `amount` and the currency: the money texts of the loans screen.
fn money_text(
    head: &[u8],
    amount: i32,
    fmt: bmp_game::rng::NumberFormat,
    mod2026: bool,
) -> Vec<u8> {
    // MOD2026: the mod's amounts are in euros.
    [
        head,
        &format_long_thousands(display(amount), fmt),
        currency(mod2026),
    ]
    .concat()
}

/// The limit line of the refusal dialog: `"maximal 1 Mio. DM"` from a manager,
/// `"maxmimal 4 Mio. DM"` (sic) from the bank.
fn limit_line(bank: bool, mod2026: bool) -> Vec<u8> {
    let head: &[u8] = if bank {
        b"maxmimal 4 Mio."
    } else {
        b"maximal 1 Mio."
    };
    [head, currency(mod2026)].concat()
}

/// The last line of the usury dialog, `"von 1.000.000 DM belegt."`.
fn usury_fine_line(mod2026: bool) -> Vec<u8> {
    [b"von 1.000.000", currency(mod2026), b" belegt."].concat()
}

/// Lists the used loan slots of `lender` (`None` only clears the separator
/// lines, -1 in the original) in the lower left panel.
///
/// Origin: BMMAIN.EXE 212a:119b (credit_list_draw)
fn credit_list_draw(host: &mut Host, state: &GameState, lender: Option<usize>) {
    host.number_no_separators = false;
    host.set_font(2);
    line(host, 7, 0x84, 0x82, 0x84, 6);
    line(host, 7, 0x9f, 0x82, 0x9f, 6);
    // Lender -1 (`None`) skips the list and clears all three slots.
    let mut n: i16 = 0;
    if let Some(lender) = lender {
        for c in &state.managers[current(state)].credits[lender] {
            if c.principal == 0 {
                continue;
            }
            fill(host, 9, 0x6a + 0x1c * n, 0x81, 0x81 + 0x1c * n, 7);
            let mut y = (n + 4) * 0x1c;
            n += 1;
            if n == 2 {
                y -= 1;
            }
            let lines = credit_lines(c, host.number_format(), host.mod2026);
            for (k, l) in lines.iter().enumerate() {
                text_at(host, l, 9, y + 8 * k as i16, 1);
            }
        }
    }
    for k in n..3 {
        fill(host, 9, 0x6a + 0x1c * k, 0x81, 0x81 + 0x1c * k, 7);
    }
    host.set_font(1);
}

/// Balance, total debts and monthly interest in the lower right panel.
///
/// Origin: BMMAIN.EXE 212a:23cb (credit_summary_draw)
fn credit_summary_draw(host: &mut Host, state: &GameState) {
    let m = &state.managers[current(state)];
    fill(host, 0x97, 0x69, 0x102, 0xb9, 7);
    line(host, 0x97, 0x84, 0x102, 0x84, 6);
    line(host, 0x97, 0x9f, 0x102, 0x9f, 6);
    host.number_no_separators = false;
    let amount = |host: &Host, v: i32| money_text(b"", v, host.number_format(), host.mod2026);
    centered(host, BALANCE, 0x72);
    let t = amount(host, m.balance);
    centered(host, &t, 0x7e);
    centered(host, TOTAL_DEBTS, 0x8d);
    let t = amount(host, credit_sum(m, CreditField::Principal, None));
    centered(host, &t, 0x99);
    centered(host, INTEREST_MONTH, 0xa8);
    let t = amount(host, credit_sum(m, CreditField::Interest, None));
    centered(host, &t, 0xb4);
}

/// Restores the status-line strip from the office background.
fn restore_status_strip(host: &mut Host) {
    let screen = host.screen();
    if let Some(work) = host.ui.work {
        host.g
            .blit_bitmap_or_restore_page2(work, 0, 0xd9, screen, 0, 0xd9, 0x10b, 0xa);
    }
}

/// Why a loan request is refused, checked in this order (later checks win).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanRefusal {
    /// "So viel Geld besitzt <name> gar nicht..."
    LenderTooPoor,
    /// "Sie können nur maximal 1 Mio. DM Kredit von einem Mitspieler aufnehmen."
    ManagerLimit,
    /// "Sie können nur maxmimal 4 Mio. DM Kredit von der Bank aufnehmen."
    BankLimit,
}

/// The checks of a loan of `amount` from `lender` before the duration and the
/// rate are asked for.
///
/// Origin: BMMAIN.EXE 212a:167f (screen_credits, 212a:1f8a..20b0)
pub fn loan_refusal(state: &GameState, lender: usize, amount: i32) -> Option<LoanRefusal> {
    let m = &state.managers[current(state)];
    if lender != BANK {
        let mut r = None;
        if state.managers[lender].balance < amount {
            r = Some(LoanRefusal::LenderTooPoor);
        }
        if credit_sum(m, CreditField::Principal, Some(lender)).wrapping_add(amount) > MANAGER_LIMIT
        {
            r = Some(LoanRefusal::ManagerLimit);
        }
        r
    } else if credit_sum(m, CreditField::Principal, Some(BANK)).wrapping_add(amount) > BANK_LIMIT {
        Some(LoanRefusal::BankLimit)
    } else {
        None
    }
}

/// Asks for the amount (and, from a fellow manager, the duration and rate)
/// of a loan from `lender`, takes it and moves the money. `row` is the
/// hovered row of the interest table.
///
/// Origin: BMMAIN.EXE 212a:167f (screen_credits, 212a:1eaf..2388)
fn take_loan(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    lender: usize,
    row: usize,
) -> Run<()> {
    status_line_show(host, session, StatusText::Prompt(ASK_AMOUNT));
    let amount = internal(ui_input_number(host, 0xba, 0xe1, 7, 0xb, 0xff)?);
    restore_status_strip(host);
    let mut refused = false;
    let lender_name = state.managers[lender.min(3)].name().to_vec();
    // MOD2026: the mod's limits and fine are in euros.
    let mod2026 = host.mod2026;
    match loan_refusal(state, lender, amount) {
        Some(LoanRefusal::LenderTooPoor) => {
            refused = if GAME_OPTION_A == 1 {
                dialog_okay(
                    host,
                    b"So viel Geld",
                    b"besitzt",
                    &lender_name,
                    b"gar nicht...",
                )?
            } else {
                dialog_okay(
                    host,
                    &lender_name,
                    b"So viel Geld",
                    b"besitzt",
                    b"gar nicht...",
                )?
            };
        }
        Some(LoanRefusal::ManagerLimit) => {
            refused = dialog_okay(
                host,
                b"Sie k|nnen nur",
                &limit_line(false, mod2026),
                b"Kredit von einem",
                b"Mitspieler aufnehmen.",
            )?;
        }
        Some(LoanRefusal::BankLimit) => {
            refused = dialog_okay(
                host,
                b"Sie k|nnen nur",
                &limit_line(true, mod2026),
                b"Kredit von der",
                b"Bank aufnehmen.",
            )?;
        }
        None => {}
    }
    host.set_font(1);
    if !refused && amount != 0 {
        let (months, rate) = if lender != BANK {
            status_line_show(host, session, StatusText::Prompt(ASK_MONTHS));
            let months = ui_input_number(host, 0xd2, 0xe1, 2, 0xb, 0)?;
            restore_status_strip(host);
            if months > 24 {
                refused = ui_dialog_box(host, b"Maximal", b"24 Monate.")?;
            }
            let mut rate = 0;
            if !refused && months != 0 {
                status_line_show(host, session, StatusText::Prompt(ASK_RATE));
                rate = ui_input_number(host, 0xaf, 0xe1, 2, 0xb, 0)?;
                restore_status_strip(host);
                if rate > 13 {
                    // No fine is actually charged.
                    dialog_okay(
                        host,
                        b"Wegen Wucher werden Sie",
                        b"mit einer",
                        b"Disziplinarstrafe von",
                        &usury_fine_line(mod2026),
                    )?;
                    refused = dialog_okay(
                        host,
                        b"Okay...",
                        b"War nur ein Scherz...",
                        b"Das war aber trotzdem",
                        b"kein fairer Zug...",
                    )?;
                }
                if rate < 3 {
                    refused = dialog_okay(
                        host,
                        b"Mindest-Prozentsatz:",
                        b"l}cherliche",
                        b"minimale",
                        b"3 Prozent.",
                    )?;
                }
            }
            (months, rate)
        } else {
            ((row as i32 + 1) * 3, i32::from(state.interest_rates[row]))
        };
        if !refused && months != 0 && rate != 0 {
            if credit_take(state, amount, rate as i8, lender, months as i8) {
                credit_list_draw(host, state, Some(lender));
                let cur = current(state);
                state.managers[cur].balance = state.managers[cur].balance.wrapping_add(amount);
                if lender != BANK {
                    let l = &mut state.managers[lender];
                    l.balance = l.balance.wrapping_sub(amount);
                }
                credit_summary_draw(host, state);
            } else {
                ui_dialog_box(host, b"Schon 3 Kredite", b"aufgenommen.")?;
            }
        }
    }
    status_line_show(host, session, StatusText::None);
    Ok(())
}

/// The loans screen: choose a lender (fellow managers or the bank), see the
/// loans taken from it, and take a new one by clicking a row of the interest
/// table.
///
/// Origin: BMMAIN.EXE 212a:167f (screen_credits)
pub fn screen_credits(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    state: &mut GameState,
) -> Run<()> {
    host.number_no_separators = false;
    let pic7 = host.picture(7)?;
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    for a in 0..2i16 {
        for b in 0..2i16 {
            button(host, 6 + 0x90 * a, 0xf + 0x59 * b, 0x8b - 0x1e * a, 0x52, 7);
        }
    }
    let screen = host.screen();
    host.g.blit_bitmap(pic7, 0, 0, screen, 7, 0x10, 0x8a, 0x51);
    host.g
        .blit_bitmap(pic7, 0x8b, 0, screen, 0x83, 0x69, 0xe, 0x51);
    let cur = current(state);
    let managers = usize::from(state.manager_count).min(4);
    let mut rects: Vec<[i16; 4]> = Vec::new();
    let mut lender_of: Vec<usize> = Vec::new();
    for s in 0..managers {
        if s == cur {
            continue;
        }
        let n = rects.len() as i16;
        rects.push(button(host, 0x10c, 0x2d * n + 0xf, 0, 0, 7));
        host.g.blit_bitmap(
            pics.portraits,
            s as i16 * 0x29 + 5,
            8,
            screen,
            0x115,
            0x2d * n + 0x18,
            0x20,
            0x17,
        );
        lender_of.push(s);
    }
    let mut sel_btn = rects.len() as i16;
    rects.push(button(host, 0x10c, 0x2d * sel_btn + 0xf, 0, 0x8000, 7));
    host.g.blit_bitmap(
        pic7,
        0x9b,
        0,
        screen,
        0x115,
        0x2d * sel_btn + 0x18,
        0x20,
        0x17,
    );
    lender_of.push(BANK);
    rects.push(button(host, 0x10c, 0xc1, 0, 0x22, 7));
    let exit = managers as i16 + 1;
    let mut lender = BANK;
    credit_list_draw(host, state, Some(lender));
    host.set_font(1);
    text_at(host, TABLE_HEADER, 0xa0, 0x18, 1);
    host.number_pad = 2;
    for r in 0..6i16 {
        let y = 0x25 + 11 * r;
        let mut t = num(host, i32::from(3 * (r + 1)));
        t.extend_from_slice(MONTHS);
        text_at(host, &t, 0xa0 - 3 * i16::from(r > 2), y, 2);
        let mut t = num(host, i32::from(state.interest_rates[r as usize]));
        t.push(b'%');
        text_at(host, &t, 0xdf, y, 2);
        if r < 5 {
            let ly = (0x27 + 11 * r) as u8;
            line(host, 0x97, ly, 0x102, ly, 6);
        }
    }
    host.number_pad = 1;
    credit_summary_draw(host, state);
    host.screen_fade(1)?;
    let mut hover: i16 = -1;
    let mut toggle = false;
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let mut si = Ui::hit_test_buttons(&rects, exit, mx, my, Some(ev as i16));
        let click = host.button == LEFT_DOWN;
        if click && si != 0 && si < exit && lender_of[si as usize - 1] != lender {
            host.ui.draw_button(
                &mut host.g,
                &mut host.rp,
                0x10c,
                0x2d * sel_btn + 0xf,
                0,
                0x2000,
                false,
                8,
            );
            sel_btn = si - 1;
            host.ui.draw_button(
                &mut host.g,
                &mut host.rp,
                0x10c,
                0x2d * sel_btn + 0xf,
                0,
                0xa000u16 as i16,
                false,
                8,
            );
            lender = lender_of[si as usize - 1];
            credit_list_draw(host, state, Some(lender));
            si = 0;
        }
        if click && 0x83 < mx && mx < 0x92 {
            let a = 0x9f < my && my < 0xbb;
            let b = 0x69 < my && my < 0x84;
            if a == b {
                toggle = !toggle;
                if toggle {
                    credit_list_draw(host, state, None);
                    host.set_font(2);
                    let m = &state.managers[cur];
                    for (label, field, y) in [
                        (DEBTS_HERE, CreditField::Principal, 0x8f),
                        (INTEREST_HERE, CreditField::Interest, 0x98),
                    ] {
                        let t = money_text(
                            label,
                            credit_sum(m, field, Some(lender)),
                            host.number_format(),
                            host.mod2026,
                        );
                        text_at(host, &t, 9, y, 1);
                    }
                    host.set_font(1);
                } else {
                    credit_list_draw(host, state, Some(lender));
                }
            }
        }
        if 0xa0 < mx && mx < 0x103 && 0x1e < my && my < 0x5d {
            let row = (my - 0x1f) / 11;
            if row != hover {
                if lender == BANK {
                    let rate = i32::from(state.interest_rates[row as usize]);
                    status_line_show(host, session, StatusText::BankCredit(rate));
                } else if hover == -1 {
                    let name = state.managers[lender].name().to_vec();
                    status_line_show(host, session, StatusText::ManagerCredit(&name));
                }
                hover = row;
            }
        } else if hover != -1 {
            status_line_show(host, session, StatusText::None);
            hover = -1;
        }
        if click && hover != -1 {
            take_loan(host, session, state, lender, hover as usize)?;
            si = 0;
        }
        if si == exit || session.auto_play {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    host.g.free(pic7);
    host.number_no_separators = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::assert_mod_texts_narrower;
    use bmp_game::rng::NumberFormat;

    fn fmt() -> NumberFormat {
        NumberFormat {
            no_separators: false,
            comma_separator: false,
            pad_width: 1,
        }
    }

    #[test]
    fn loan_lines() {
        let c = Credit {
            principal: 100_000,
            interest_payment: 7000,
            day_taken: 7,
            payment_day: 7,
            month_taken: 6,
            due_month: 9,
            rate: 7,
            unknown_0d: 0,
            year_taken: 2019,
            due_year: 2019,
        };
        let [l1, l2, l3] = credit_lines(&c, fmt(), false);
        assert_eq!(l1, b"7.7.2019: 100.000 DM (7%)");
        assert_eq!(l2, b"F[LLIG AM 7.10.2019");
        assert_eq!(l3, b"ZINSEN PRO MONAT: 7.000 DM");
        let [m1, m2, m3] = credit_lines(&c, fmt(), true);
        assert_eq!(m1, b"7.7.2019: 100.000 \xd5 (7%)");
        assert_eq!(m2, l2);
        assert_eq!(m3, b"ZINSEN PRO MONAT: 7.000 \xd5");
        assert_mod_texts_narrower(&[(m1, l1), (m3, l3)]);
    }

    /// MOD2026: the limit and fine lines of the loan dialogs and the amounts of the account
    /// summary.
    #[test]
    fn dialog_and_summary_amounts_in_euros() {
        let texts = |mod2026| {
            vec![
                limit_line(false, mod2026),
                limit_line(true, mod2026),
                usury_fine_line(mod2026),
                money_text(b"", -2_345_678, fmt(), mod2026),
                money_text(DEBTS_HERE, 4_000_000, fmt(), mod2026),
                money_text(INTEREST_HERE, 280_000, fmt(), mod2026),
            ]
        };
        let (modded, original) = (texts(true), texts(false));
        assert_eq!(original[0], b"maximal 1 Mio. DM");
        assert_eq!(modded[1], b"maxmimal 4 Mio. \xd5");
        assert_eq!(modded[2], b"von 1.000.000 \xd5 belegt.");
        assert_eq!(original[2], b"von 1.000.000 DM belegt.");
        assert_eq!(modded[4], b"SCHULDEN HIER: 4.000.000 \xd5");
        assert_eq!(original[5], b"ZINSEN HIER: 280.000 DM");
        assert_mod_texts_narrower(&modded.into_iter().zip(original).collect::<Vec<_>>());
    }

    #[test]
    fn loan_limits() {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.managers[1].balance = 50_000;
        assert_eq!(loan_refusal(&s, BANK, 4_000_000), None);
        assert_eq!(
            loan_refusal(&s, BANK, 4_000_001),
            Some(LoanRefusal::BankLimit)
        );
        s.managers[0].credits[BANK][0].principal = 3_000_000;
        assert_eq!(
            loan_refusal(&s, BANK, 1_000_001),
            Some(LoanRefusal::BankLimit)
        );
        assert_eq!(loan_refusal(&s, 1, 40_000), None);
        assert_eq!(
            loan_refusal(&s, 1, 60_000),
            Some(LoanRefusal::LenderTooPoor)
        );
        s.managers[1].balance = 5_000_000;
        assert_eq!(
            loan_refusal(&s, 1, 1_000_001),
            Some(LoanRefusal::ManagerLimit)
        );
    }
}
