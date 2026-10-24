//! The transfer market `screen_transfer_market` (office "Mannschaft" -> third
//! sub icon): the own squad on the left, the twelve market places on the
//! right, the offer box, the balance and the KAUFEN/LEIHEN switch.
//! Spec: `docs/re/logic/transfer.md` §4.9, §4.11.
//!
//! The decisions (offers for own players, listing, bids, purchases) are the
//! functions of [`bmp_game::transfer`]; this module ports the screen around
//! them.

use bmp_game::squad::{InsertRejected, LineupRules, auto_lineup, squad_count_listed};
use bmp_game::state::GameState;
use bmp_game::transfer::{
    ListRejected, MarketBid, answer_listed_player_offer, answer_squad_player_offer,
    human_bid_allowed, list_squad_player, listed_player_offer, mark_bid_rejected, market_bid,
    market_fee_offer_accepted, offer_compensation, purchase_cancel, purchase_complete,
    purchase_insert, squad_player_offer,
};

use super::negotiation::contract_negotiation_single;
use super::scorers::screen_player_info;
use super::squad_screen::{
    ListHover, player_append_foot_text_of, squad_list_draw, squad_list_hover_info,
};
use super::widgets::{
    TextButtons, dialog_two_choice, ui_dialog_box, ui_input_number, ui_wait_right_click_button,
};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{Gfx, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// The squad group of the transfer market.
const MARKET: usize = 4;
/// Column mask of the own list: ART, NAME, STÄRKEN, ST, STATUS, TD.
const OWN_COLUMNS: u32 = 0x0000_90c6;
/// Column mask of the market list: ART, NAME, STÄRKEN, WERT.
const MARKET_COLUMNS: u32 = 0x0002_0086;
/// `FAR:2ea2` value that also selects a market row (with event 3).
const SELECT_EVENT: u16 = 0xe8;
/// `FAR:513c` values of the one-year games (no loans).
const ONE_YEAR_GAMES: [u16; 2] = [0x7ac, 0x7c9];
/// No highlighted market row.
const NO_ROW: i16 = 0x7f;

fn num(host: &Host, v: i32) -> Vec<u8> {
    bmp_game::rng::format_long_thousands(v, host.number_format())
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: i16) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, color);
}

/// The XOR bar of market row `r`.
fn xor_market_row(host: &mut Host, r: i16) {
    Gfx::rastport_set_draw_mode(&mut host.rp, 2);
    fill(host, 0xa3, 6 * r + 0x1d, 0x13a, 6 * r + 0x21, 0x1e);
    Gfx::rastport_set_draw_mode(&mut host.rp, 0);
}

/// `fmt(v * DS:2252 / DS:224e)` with the empty currency prefix.
fn money(host: &Host, v: i32) -> Vec<u8> {
    num(host, v.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A)
}

/// The BEHALTEN / VERKAUFEN question for an offer of `fee`: shows the offer
/// and the compensation for the `len` remaining contract seasons, reduces
/// `fee` by it and returns the clicked icon (1 = keep, 2 = sell).
///
/// Origin: BMMAIN.EXE 3277:1b5f (transfer_offer_received)
fn transfer_offer_received(host: &mut Host, fee: &mut i32, len: i32) -> Run<usize> {
    host.number_no_separators = false;
    text_at(host, b"Angebot", 0xa5, 0x8e, 1);
    text_at(host, b"Abl|se", 0xa5, 0x98, 1);
    let mut s = b": ".to_vec();
    s.extend(money(host, *fee));
    s.extend_from_slice(b" DM.");
    text_at(host, &s, 0xd9, 0x8e, 1);
    let cut = offer_compensation(*fee, len);
    *fee = fee.wrapping_sub(cut);
    let mut s = b": ".to_vec();
    s.extend(money(host, cut));
    s.extend_from_slice(b" DM.");
    text_at(host, &s, 0xd9, 0x98, 1);
    ui_draw_line(&mut host.g, &mut host.rp, 0xa3, 0x9a, 0x13a, 0x9a, 0xb);
    let mut icons = TextButtons::default();
    icons.register(host, 0xaf, 0x6e, b"BEHALTEN", 1);
    icons.register(host, 0xf5, 0x6e, b"VERKAUFEN", 0x12);
    loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let h = icons.hit(mx, my);
        if host.button == LEFT_DOWN && h != 0 {
            return Ok(h);
        }
    }
}

/// The sponsor's offer after a purchase from a club: with a chance of 1 in 7
/// a subsidy of 20..65 % of the fee (rounded to 10 000) is offered.
///
/// Origin: BMMAIN.EXE 1000:272d (sponsor_subsidy_offer)
fn sponsor_subsidy_offer(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    base: i32,
) -> Run<()> {
    if session.rng.random_range(0, 6) != 0 {
        return Ok(());
    }
    let amt =
        (base / 100).wrapping_mul(i32::from(session.rng.random_range(20, 65))) / 10000 * 10000;
    host.number_no_separators = false;
    let mut text = money(host, amt);
    text.extend_from_slice(b" DM AN ?");
    let m = state.current_manager as usize;
    let who = state.managers[m].name().to_vec();
    if dialog_two_choice(
        host,
        session.auto_play,
        &who,
        b"NEHMEN SIE EINEN SPONSOR-",
        b"ZUSCHU~ VON",
        &text,
        b"ABER IMMER!",
        b"LIEBER NICHT.",
    )? == 1
    {
        state.managers[m].balance = state.managers[m].balance.wrapping_add(amt);
    }
    Ok(())
}

/// Screen state that the original keeps in locals.
struct Market {
    hover: i16,
    loan: bool,
    redraw_own: bool,
    redraw_market: bool,
    /// `[bp-6]`: the last offer; the original never initialises it.
    offer: i32,
    refused_msg: bool,
    rules: LineupRules,
}

/// The ART/NAME/... list of the own squad, rows `first..=last`.
fn draw_own_list(host: &mut Host, session: &mut Session, state: &GameState, first: i8, last: i8) {
    squad_list_draw(
        host,
        state,
        &mut session.rng,
        4,
        0x18,
        first,
        last,
        0x99,
        0xb6,
        OWN_COLUMNS,
        4,
        false,
    );
}

/// A click on row `slot` of the own list: player info (right button), the
/// pending offer for the player, or putting him on the market.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:0989)
fn own_player_click(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    mk: &mut Market,
    slot: u8,
    click: u16,
) -> Run<()> {
    if click == RIGHT_DOWN {
        return screen_player_info(host, session, state, slot);
    }
    let m = state.current_manager as usize;
    let p = state.squads[m][slot as usize].clone();
    if p.negotiation & 0x80 != 0 {
        ui_dialog_box(host, b"Dieser Spieler h|rt", b"am Saisonende auf.")?;
        return Ok(());
    }
    if p.status & 0x40 != 0 {
        host.set_font(1);
        let mut offer = squad_player_offer(state, &mut session.rng, slot);
        let h = transfer_offer_received(host, &mut offer, i32::from(p.contract_years))?;
        mk.offer = offer;
        answer_squad_player_offer(state, &mut session.rng, slot, offer, h != 1);
        mk.redraw_market = true;
        if h == 1 {
            draw_own_list(host, session, state, slot as i8, slot as i8);
        } else {
            mk.redraw_own = true;
        }
        return Ok(());
    }
    match list_squad_player(state, &mut session.rng, slot) {
        Ok(_) => {
            mk.redraw_market = true;
            mk.redraw_own = true;
        }
        Err(ListRejected::ThreeListed) => {
            ui_dialog_box(host, b"Schon 3 Spieler auf", b"dem Transfermarkt")?;
        }
        Err(ListRejected::OnLoan) => {
            ui_dialog_box(host, b"Dieser Spieler ist", b"nur ausgeliehen")?;
        }
        Err(ListRejected::Insert(e)) => insert_rejected(host, e)?,
    }
    Ok(())
}

/// The message of `transfer_list_add` 2ecd:37d8 when it refuses a player.
fn insert_rejected(host: &mut Host, e: InsertRejected) -> Run<()> {
    match e {
        InsertRejected::TransferListFull => {
            ui_dialog_box(host, b"Schon 12 Mann auf", b"dem Transfermarkt")?
        }
        InsertRejected::SquadFull => ui_dialog_box(host, b"Schon 24 Mann", b"im Team")?,
    };
    Ok(())
}

/// A click on market record `idx`: info (right button, own listed players
/// only), the offer for an own listed player, taking him back, or a bid with
/// the purchase that follows.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
fn market_click(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    mk: &mut Market,
    idx: u8,
    click: u16,
) -> Run<()> {
    let m = state.current_manager as usize;
    let rec = state.squads[MARKET][idx as usize].clone();
    let pid = rec.player_id as usize;
    let own = state.players[pid].owner_manager as usize;
    if click == RIGHT_DOWN {
        if own == m {
            state.current_manager = MARKET as i8;
            let r = screen_player_info(host, session, state, idx);
            state.current_manager = state.saved_manager;
            r?;
        }
        return Ok(());
    }
    let mut from_human = false;
    let mut reclaim = false;
    let bid = market_bid(state, idx);
    if bid == MarketBid::AlreadyRejected {
        ui_dialog_box(host, b"Angebot wurde", b"bereits abgelehnt")?;
        return unhighlight(host, mk);
    }
    host.set_font(1);
    match bid {
        MarketBid::OfferForOwnPlayer => {
            let mut offer = listed_player_offer(state, &mut session.rng, idx);
            let h = transfer_offer_received(host, &mut offer, i32::from(rec.contract_years))?;
            answer_listed_player_offer(state, &mut session.rng, idx, offer, h != 1);
            mk.redraw_market = true;
            mk.offer = 0;
        }
        MarketBid::Reclaim => reclaim = true,
        // TODO(re): the original bids its uninitialised `offer` here on the
        // first use (transfer.md §7); the port starts it at 0.
        MarketBid::RepeatLastOffer => {}
        MarketBid::EnterBid => {
            text_at(host, b"Ihr Angebot:", 0xa5, 0x8c, 1);
            ui_draw_line(&mut host.g, &mut host.rp, 0xa3, 0x8e, 0x13a, 0x8e, 1);
            let v = ui_input_number(host, 0xa7, 0x98, 7, 1, 8)?;
            mk.offer = v.wrapping_mul(GAME_OPTION_A) / GAME_OPTION_B;
            host.set_font(2);
        }
        MarketBid::AlreadyRejected => unreachable!(),
    }
    let offer = mk.offer;
    let balance = state.managers[m].balance;
    if !((offer != 0 && balance >= offer) || reclaim) {
        if offer != 0 {
            ui_dialog_box(host, b"Sie haben nicht", b"genug Geld")?;
        }
        return clear_box(host, mk);
    }
    let mut proceed = true;
    if !reclaim && own != MARKET {
        // Listed by another human manager: he is asked.
        proceed = false;
        mk.refused_msg = false;
        let who = state.managers[own].name().to_vec();
        if !human_bid_allowed(state, &mut session.rng, idx, offer, mk.loan) {
            dialog_two_choice(
                host,
                session.auto_play,
                &who,
                b"DIESER BETRAG LIEGT",
                b"AU~ERHALB DES ERLAUBTEN !",
                b"",
                b"NIE",
                b"NEIN",
            )?;
            mk.refused_msg = true;
        } else {
            host.number_no_separators = false;
            let mut l1 = b"NEHMEN SIE DAS ANGEBOT (".to_vec();
            l1.extend(money(host, offer));
            l1.extend_from_slice(b" DM)");
            let mut l2 = b"F]R ".to_vec();
            l2.extend_from_slice(state.players[pid].name());
            l2.extend_from_slice(b" AN ?");
            if dialog_two_choice(
                host,
                session.auto_play,
                &who,
                &l1,
                &l2,
                b"",
                b"NA GUT.",
                b"NEIN !",
            )? == 1
            {
                proceed = true;
                from_human = true;
            }
        }
    }
    let accepted = proceed
        && (from_human
            || reclaim
            || market_fee_offer_accepted(state, &mut session.rng, idx, offer, mk.loan));
    if !accepted {
        ui_dialog_box(host, b"Ihr Angebot", b"wurde abgelehnt")?;
        if !from_human && !mk.refused_msg {
            mark_bid_rejected(state, idx);
        }
        mk.refused_msg = false;
        return clear_box(host, mk);
    }
    let loan = mk.loan;
    let purchase = match purchase_insert(state, &mut session.rng, idx, loan, reclaim, from_human) {
        Ok(p) => p,
        Err(e) => {
            insert_rejected(host, e)?;
            return clear_box(host, mk);
        }
    };
    if !reclaim && !loan && !contract_negotiation_single(host, session, state, purchase.squad_slot)?
    {
        purchase_cancel(state, purchase);
        ui_dialog_box(host, b"Der Transfer", b"findet nicht statt")?;
        if !from_human && !mk.refused_msg {
            mark_bid_rejected(state, idx);
        }
        mk.refused_msg = false;
        return clear_box(host, mk);
    }
    purchase_complete(state, purchase, offer, loan, reclaim, from_human);
    mk.redraw_market = true;
    mk.redraw_own = true;
    if !reclaim && !from_human {
        sponsor_subsidy_offer(host, session, state, offer)?;
    }
    auto_lineup(state, mk.rules);
    clear_box(host, mk)
}

/// Clears the offer box and takes the highlight off the market row.
fn clear_box(host: &mut Host, mk: &mut Market) -> Run<()> {
    fill(host, 0xa3, 0x84, 0x13a, 0x9b, 8);
    unhighlight(host, mk)
}

fn unhighlight(host: &mut Host, mk: &mut Market) -> Run<()> {
    if !mk.redraw_own && mk.hover != NO_ROW {
        xor_market_row(host, mk.hover);
    }
    mk.hover = NO_ROW;
    Ok(())
}

/// The transfer market screen.
///
/// TODO(#24): `manager_ai_weekly(m, 0)` 1f9d:0002 is not called on exit.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market)
pub fn screen_transfer_market(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    rules: LineupRules,
) -> Run<()> {
    let m = state.current_manager as usize;
    let mut mk = Market {
        hover: NO_ROW,
        loan: false,
        redraw_own: true,
        redraw_market: true,
        offer: 0,
        refused_msg: false,
        rules,
    };
    let mut first_fade = true;
    // copy_protection_penalty("Transfermarkt"): the protection counts as passed.
    let (g, rp) = (&mut host.g, &mut host.rp);
    host.ui.draw_button(g, rp, 0, 0x0d, 0x9d, 0xb5, false, 8);
    let r1 = host
        .ui
        .draw_button(g, rp, 0xa1, 0x0d, 0x9b, 0x70, false, 8)
        .1;
    let r2 = host.ui.draw_button(g, rp, 0x10c, 0xc1, 0, 0x22, false, 8).1;
    host.ui.draw_button(g, rp, 0xa1, 0x83, 0x9b, 0x31, false, 8);
    let btns = [r1, r2];
    host.set_font(1);
    text_at(host, b"Kontostand:", 0xa5, 0xa4, 0xb);
    ui_draw_line(&mut host.g, &mut host.rp, 0xa3, 0xa6, 0x13a, 0xa6, 0xb);
    let mut lh = ListHover::new(rules.suspended_allowed);
    let mut own_cnt: i8 = 0;
    loop {
        host.input_wait_release()?;
        if mk.redraw_own || mk.redraw_market {
            fill(host, 0xa3, 0x84, 0x13a, 0x9b, 8);
            let mut icons = TextButtons::default();
            let loan = u8::from(mk.loan);
            icons.register(host, 0xaf, 0x6e, b"LEIHEN", 0x11 * loan + 1);
            icons.register(
                host,
                0xf5,
                0x6e,
                b"KAUFEN",
                0xefu8.wrapping_mul(loan).wrapping_add(0x12),
            );
            host.set_font(2);
            host.number_no_separators = false;
            let mut s = money(host, state.managers[m].balance);
            host.number_no_separators = true;
            s.extend_from_slice(b" DM");
            fill(host, 0xa5, 0xa9, 0x118, 0xb2, 8);
            host.set_font(1);
            text_at(host, &s, 0xa5, 0xb0, 1);
        }
        if mk.redraw_own {
            host.set_font(2);
            draw_own_list(host, session, state, 0, 0x18);
            own_cnt = (squad_count_listed(state, m) as i8).wrapping_sub(1);
            auto_lineup(state, rules);
        }
        if mk.redraw_market {
            state.current_manager = MARKET as i8;
            squad_list_draw(
                host,
                state,
                &mut session.rng,
                0xa3,
                0x17,
                0,
                0x0c,
                0x13b,
                0x6b,
                MARKET_COLUMNS,
                u8::from(mk.loan) + 1,
                rules.suspended_allowed,
            );
            state.current_manager = state.saved_manager;
            mk.redraw_market = false;
            if first_fade {
                host.screen_fade(1)?;
                first_fade = false;
            }
        }
        mk.redraw_own = false;
        // Hover loop (3277:08c8).
        let mut si: i16;
        loop {
            si = squad_list_hover_info(
                host,
                state,
                &mut lh,
                2,
                0x24,
                0,
                own_cnt,
                0x9b,
                0xbe,
                OWN_COLUMNS,
                &btns,
            )?;
            if si != 0x7f {
                break;
            }
            let (mx, my) = host.mouse_xy();
            let r = my.wrapping_sub(0x1d) / 6;
            let c = mx.wrapping_sub(0xa1);
            let listed = i16::from(squad_count_listed(state, MARKET));
            if c >= 0 && r > 0 && c <= 0x98 && r <= listed {
                if mk.hover != r {
                    if mk.hover != NO_ROW {
                        xor_market_row(host, mk.hover);
                    }
                    xor_market_row(host, r);
                    mk.hover = r;
                    let pid = state.squads[MARKET][(r - 1) as usize].player_id as usize;
                    let pl = &state.players[pid];
                    let mut s = b"VON ".to_vec();
                    if pl.owner_manager == MARKET as u8 {
                        s.extend_from_slice(state.clubs[pl.team as usize].name());
                    } else {
                        s.extend(text_to_upper_de(
                            state.managers[pl.owner_manager as usize].name(),
                        ));
                    }
                    s.extend_from_slice(b", ");
                    s.extend(num(host, i32::from(pl.age)));
                    s.extend_from_slice(b" J.");
                    player_append_foot_text_of(&mut s, state, MARKET, (r - 1) as usize);
                    fill(host, 2, 0xb7, 0x9b, 0xbf, 8);
                    ui_draw_text_centered_shadow(
                        &mut host.g,
                        &mut host.rp,
                        &s,
                        2,
                        0x9b,
                        0xbe,
                        1,
                        0xff,
                    );
                }
                if host.button == SELECT_EVENT || host.button == RIGHT_DOWN {
                    si = r + 999;
                    break;
                }
            } else if mk.hover != NO_ROW {
                fill(host, 2, 0xb7, 0x9b, 0xbf, 8);
                xor_market_row(host, mk.hover);
                mk.hover = NO_ROW;
            }
        }
        let click = host.button;
        if si == 0x65 {
            let (_, my) = host.mouse_xy();
            let r = my.wrapping_sub(0x1d) / 6;
            if r >= 0 && r <= i16::from(squad_count_listed(state, MARKET)) && click != 0 {
                si = r + 999;
            }
            if my > 0x6d && !ONE_YEAR_GAMES.contains(&state.scenario) {
                mk.loan = !mk.loan;
                mk.redraw_market = true;
            }
        }
        if si < 0x19 {
            own_player_click(host, session, state, &mut mk, si as u8, click)?;
        } else if si >= 1000 {
            market_click(host, session, state, &mut mk, (si - 1000) as u8, click)?;
        }
        if si == 0x66 {
            ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
            // TODO(#24): manager_ai_weekly(m, 0) 1f9d:0002.
            return Ok(());
        }
    }
}
