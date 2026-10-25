//! Contract negotiation `contract_negotiation` (segment 3277): the contracts
//! tab of the squad screen (office "Mannschaft" -> second sub icon) and the
//! contract offer after buying a player on the transfer market.
//! Spec: `docs/re/logic/transfer.md` §4.10.
//!
//! The decision itself is [`bmp_game::transfer::contract_decision`]; this
//! module ports the list, the input panel and the messages around it.

use bmp_game::squad::{LineupRules, squad_count_listed};
use bmp_game::state::GameState;
use bmp_game::transfer::{
    ContractDialog, ContractHit, ContractOffer, contract_decision, contract_salary_suggestion,
};

use super::scorers::screen_player_info;
use super::squad_screen::{
    Screen, board_handle_input, board_open, board_reset, group, squad_list_draw,
    squad_list_hover_info,
};
use super::widgets::{TextButtons, dialog_okay, ui_input_number_typed};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, currency};
use crate::gfx::{ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// Column mask of the contracts list: ART, NAME, SP, ST, TO, STATUS, TD,
/// V.DAUER, GEHALT.
const CONTRACT_COLUMNS: u32 = 0x0003_d146;
/// `squad_list_draw` mode of the contracts list (offer colours).
const CONTRACT_LIST_MODE: u8 = 3;
/// `squad_list_hover_info` results of the four screen icons.
const ICON_LIST: i16 = 0x65;
const ICON_BOARD: i16 = 0x66;
const ICON_EXIT: i16 = 0x68;

/// The squad screen pieces the browse mode works with (`arg2` = its icon
/// rects, `arg3`/`bmp` = its pictures).
pub(super) struct Browse<'a> {
    pub sc: &'a mut Screen,
    pub btn: &'a [[i16; 4]; 4],
    pub rules: LineupRules,
}

fn num(host: &Host, v: i32) -> Vec<u8> {
    bmp_game::rng::format_long_thousands(v, host.number_format())
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: i16) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, color);
}

/// The monthly salary as shown (`v * DS:2252 / DS:224e`, unsigned).
fn shown_salary(v: i32) -> i32 {
    ((v.wrapping_mul(GAME_OPTION_B) as u32) / GAME_OPTION_A as u32) as i32
}

/// Draws the contracts list of the current squad (rows `first..=last`).
fn draw_list(
    host: &mut Host,
    session: &mut Session,
    state: &GameState,
    first: i8,
    last: i8,
    suspended_allowed: bool,
) {
    squad_list_draw(
        host,
        state,
        &mut session.rng,
        6,
        0x12,
        first,
        last,
        0xec,
        0xb0,
        CONTRACT_COLUMNS,
        CONTRACT_LIST_MODE,
        suspended_allowed,
    );
}

/// `"GEHALT PRO MONAT (IN DM)"` of the negotiation panel.
fn salary_label(mod2026: bool) -> Vec<u8> {
    // MOD2026: the mod's salaries are in euros.
    [b"GEHALT PRO MONAT (IN", currency(mod2026), b")"].concat()
}

/// The negotiation panel for record `slot` and its input loop. Returns the
/// typed terms and whether "NEUER VERTRAG" was clicked, plus the panel's
/// button slot.
fn negotiate_input(
    host: &mut Host,
    state: &GameState,
    slot: u8,
    forced: i16,
) -> Run<(ContractOffer, bool, u8)> {
    let g = group(state);
    let p = state.squads[g][slot as usize].clone();
    let pl = &state.players[p.player_id as usize];
    let mut s = pl.name().to_vec();
    s.extend_from_slice(b", ");
    s.extend(num(host, i32::from(pl.age)));
    s.extend_from_slice(b" JAHRE ALT, ");
    s.extend(num(host, i32::from(p.appearances[0])));
    s.extend_from_slice(b" LIGASPIELE");
    let (btn_id, _) = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 4, 0xc7, 0xee, 0x1f, true, 8);
    // The saved background comes from vga_load_picture, which resets the
    // field width DS:079c to 1.
    host.number_pad = 1;
    ui_draw_line(&mut host.g, &mut host.rp, 8, 0xd1, 0xb4, 0xd1, 1);
    text_at(host, &s, 8, 0xcf, 1);
    let shown_len = |p_len: u8| i32::from(if forced != 0 { forced as u8 } else { p_len });
    text_at(host, b"VERTRAGSDAUER (IN SAISONS)", 8, 0xd9, 2);
    text_at(host, b":", 0x82, 0xd9, 2);
    let t = num(host, shown_len(p.contract_years));
    text_at(host, &t, 0x87, 0xd9, 2);
    text_at(host, &salary_label(host.mod2026), 8, 0xe1, 2);
    text_at(host, b":", 0x82, 0xe1, 2);
    let t = num(host, shown_salary(p.market_value));
    text_at(host, &t, 0x87, 0xe1, 2);
    let mut buttons = TextButtons::default();
    buttons.register(host, 0xb8, 0xca, b"NEUER VERTRAG", 0x12);
    buttons.register(host, 0xb8, 0xd8, b"ABBRUCH", 1);
    let mut offer = ContractOffer {
        len: 0,
        salary: 0,
        forced,
    };
    let hit = loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        if host.button == LEFT_DOWN {
            let h = buttons.hit(mx, my);
            if h != 0 {
                break h;
            }
        }
        if !(host.button == LEFT_DOWN && mx > 8 && mx < 0xb4 && my > 0xd2 && my < 0xe2) {
            continue;
        }
        let f = i16::from(my > 0xd9);
        fill(host, 0x87, 0xd3 + 8 * f, 0xb4, 0xd9 + 8 * f, 8);
        if f == 0 {
            if p.loan != 0 {
                continue;
            }
            let (len, typed) = ui_input_number_typed(host, 0x87, 0xd9, 1, 2, 8)?;
            offer.len = len;
            if !typed {
                let t = num(host, shown_len(p.contract_years));
                text_at(host, &t, 0x87, 0xd9, 2);
                continue;
            }
            fill(host, 0x87, 0xdb, 0xb4, 0xe1, 8);
            let v = contract_salary_suggestion(state, slot, len);
            let t = num(host, v.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A);
            text_at(host, &t, 0x87, 0xe1, 2);
            offer.salary = v;
        } else {
            let (v, typed) = ui_input_number_typed(host, 0x87, 0xe1, 6, 2, 8)?;
            offer.salary = v.wrapping_mul(GAME_OPTION_A) / GAME_OPTION_B;
            if !typed {
                let t = num(host, shown_salary(p.market_value));
                text_at(host, &t, 0x87, 0xe1, 2);
            }
        }
    };
    Ok((offer, hit == 1, btn_id))
}

/// Shows the message box of a refused or impossible offer.
fn show_dialog(host: &mut Host, name: &[u8], d: ContractDialog) -> Run<()> {
    match d {
        ContractDialog::TooLong => dialog_okay(
            host,
            b"Wer wird sich denn SO",
            b"lange verpflichten ?",
            b"(Mal abgesehen",
            b"von Bundi's)",
        )?,
        ContractDialog::NotFooled => {
            dialog_okay(host, b"So dumm", b"ist", name, b"leider nicht...")?
        }
        ContractDialog::NotInterested => dialog_okay(
            host,
            name,
            b"ist nicht an",
            b"Ihrem Angebot",
            b"interessiert.",
        )?,
    };
    Ok(())
}

/// Negotiates with record `slot` of the current squad and applies the result.
/// Returns whether a new contract was signed.
fn negotiate(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    slot: u8,
    forced: i16,
    single: bool,
    suspended_allowed: bool,
) -> Run<bool> {
    let (offer, new_contract, btn_id) = negotiate_input(host, state, slot, forced)?;
    let g = group(state);
    let name = state.players[state.squads[g][slot as usize].player_id as usize]
        .name()
        .to_vec();
    if offer.len > 4 {
        show_dialog(host, &name, ContractDialog::TooLong)?;
    }
    let (hit, dialog) =
        contract_decision(state, &mut session.rng, slot, single, offer, new_contract);
    if let Some(d) = dialog
        && d != ContractDialog::TooLong
    {
        show_dialog(host, &name, d)?;
    }
    let signed = hit == ContractHit::NewContract;
    if signed && !single {
        fill(host, 8, 0xc8, 0xf3, 0xe5, 8);
        host.set_font(1);
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            b"Ihr Angebot wurde angenommen !",
            6,
            0xf4,
            0xda,
            1,
            8,
        );
        host.delay_ticks(60)?;
        let s = i16::from(slot);
        fill(host, 4, (s + 5) * 6, 0xef, s * 6 + 0x23, 8);
    }
    if !single {
        draw_list(
            host,
            session,
            state,
            slot as i8,
            slot as i8,
            suspended_allowed,
        );
    }
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, btn_id, 0);
    Ok(signed)
}

/// The contract offer for record `slot` of the current squad after a
/// purchase (single mode): no list, no refusals before the input. Returns
/// whether the player signed.
///
/// TODO(re): the original passes an unset `forced` here (transfer.md §4.10);
/// the port uses 0.
///
/// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation, slot != -1)
pub(super) fn contract_negotiation_single(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    slot: u8,
) -> Run<bool> {
    // The original loads PIC\6.VGA when no picture is passed and frees it at
    // the end; single mode never draws it.
    if slot == 0x7f {
        return Ok(false);
    }
    negotiate(host, session, state, slot, 0, true, false)
}

/// The contracts tab of the squad screen: the contracts list, hover texts,
/// player info on a right click, the tactics board (view only) and the
/// negotiation for a clicked player. Returns `false` when the screen's exit
/// icon was clicked, `true` when the list icon was.
///
/// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation, slot == -1)
pub(super) fn contract_negotiation_browse(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    b: Browse<'_>,
) -> Run<bool> {
    let Browse { sc, btn, rules } = b;
    let g = group(state);
    let cnt = (squad_count_listed(state, g) as i8).wrapping_sub(1);
    host.number_no_separators = true;
    fill(host, 3, 8, 0xef, 0xb8, 8);
    draw_list(host, session, state, 0, 0x18, sc.list.suspended_allowed);
    board_reset(sc);
    host.screen_fade(1)?;
    let mut board_open_id: Option<u8> = None;
    let mut saved_icon = None;
    let mut slot: i16;
    loop {
        host.set_font(2);
        let click;
        loop {
            let x1 = 0xef - 0x4b * i16::from(board_open_id.is_some());
            slot = squad_list_hover_info(
                host,
                state,
                &mut sc.list,
                6,
                0x1e,
                0,
                cnt,
                x1,
                0xb8,
                CONTRACT_COLUMNS,
                btn,
            )?;
            let c = host.button;
            host.button = 0;
            if board_open_id.is_some() {
                board_handle_input(host, state, rules, sc, None, 0xa8, true, 1)?;
            }
            if slot != 0x7f {
                click = c;
                break;
            }
        }
        if slot == ICON_BOARD && board_open_id.is_none() {
            let tmp = host.blank_bitmap(0x32, 0x29, 0);
            let screen = host.screen();
            host.g
                .blit_bitmap(screen, 0x10c, 0x29, tmp, 0, 0, 0x32, 0x29);
            host.g
                .blit_bitmap(screen, 0x10c, 0x83, screen, 0x10c, 0x29, 0x32, 0x29);
            let (id, _) = board_open(host, state, sc, false);
            board_open_id = Some(id);
            saved_icon = Some(tmp);
        }
        if slot == ICON_LIST
            && let Some(id) = board_open_id.take()
        {
            host.ui.redraw_buttons(&mut host.g, &mut host.rp, id, 0);
            if let Some(tmp) = saved_icon.take() {
                let screen = host.screen();
                host.g
                    .blit_bitmap(tmp, 0, 0, screen, 0x10c, 0x29, 0x32, 0x29);
                host.g.free(tmp);
            }
            slot = 100;
        }
        if slot < 0x63 {
            let i = slot as u8;
            let p = state.squads[g][slot as usize].clone();
            let pid = p.player_id as usize;
            if click == RIGHT_DOWN {
                screen_player_info(host, session, state, i)?;
            } else {
                let name = state.players[pid].name().to_vec();
                let mut forced = i16::from(p.negotiation);
                let mut go = true;
                if p.loan != 0 {
                    let club = state.clubs[usize::from(p.loan & 0x7f)].name().to_vec();
                    dialog_okay(host, &name, b"ist leider von", &club, b"nur ausgeliehen.")?;
                    go = false;
                } else if forced & 0x80 != 0 {
                    dialog_okay(
                        host,
                        &name,
                        b"wird sich mit",
                        b"Ablauf des Vertrages",
                        b"zur Ruhe setzen.",
                    )?;
                    go = false;
                } else if forced > 99 {
                    forced -= 100;
                } else if forced != 0 {
                    dialog_okay(
                        host,
                        &name,
                        b"ist im Moment",
                        b"nicht verhandlungs-",
                        b"bereit.",
                    )?;
                    host.delay_ticks(30)?;
                    // A pointer in the top left corner (0, 0) bypasses the refusal.
                    if host.mouse_xy() != (0, 0) {
                        go = false;
                    }
                    forced = 0;
                }
                if go {
                    negotiate(
                        host,
                        session,
                        state,
                        i,
                        forced,
                        false,
                        sc.list.suspended_allowed,
                    )?;
                }
            }
        }
        if !((slot != ICON_LIST && slot != ICON_EXIT) || board_open_id.is_some()) {
            break;
        }
    }
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x10c, 0x83, 0, 0x2000, false, 8);
    host.ui.draw_button(
        &mut host.g,
        &mut host.rp,
        0x10c,
        0x29,
        0,
        0xa000u16 as i16,
        false,
        8,
    );
    Ok(slot != ICON_EXIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::assert_mod_texts_narrower;

    /// MOD2026: the salary label names euros.
    #[test]
    fn salary_label_in_euros() {
        assert_eq!(salary_label(false), b"GEHALT PRO MONAT (IN DM)");
        assert_eq!(salary_label(true), b"GEHALT PRO MONAT (IN \xd5)");
        assert_mod_texts_narrower(&[(salary_label(true), salary_label(false))]);
    }
}
