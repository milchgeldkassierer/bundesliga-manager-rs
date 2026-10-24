//! Transfer logic of `BMMAIN.EXE` segment 3277: player values and salaries,
//! the club-run part of the transfer market, the clubs' and players' answers
//! to bids and contract offers, and the state changes of a sale or purchase.
//!
//! `squad(g, i)` is the record `i` of group `g = FAR:304a`
//! ([`GameState::current_manager`]); group 4 is the transfer market, of
//! which records 0..12 are used. Several routines switch `FAR:304a` to 4 and
//! restore it from `FAR:5358` ([`GameState::saved_manager`]), as the
//! original does.
//!
//! Dialogs, input and drawing (`screen_transfer_market`,
//! `transfer_offer_received`, `contract_negotiation`, the training bars)
//! belong to the screens; their decisions are the functions here, in the
//! original's order of random draws.
//!
//! Full documentation with addresses: `docs/re/logic/transfer.md`.

use crate::league::{LEAGUE_ROUND_COUNT, league_team_ids};
use crate::news::message_delete;
use crate::rng::{Rng, clamp_int, ldiv, lmul, lshl, uldiv};
use crate::squad::{InsertRejected, group, squad_insert_player, squad_remove_player};
use crate::state::{
    CLUB_LISTED, GameState, LOAN_PRICE_MODE, NO_OWNER, PLAYER_COUNT, SQUAD_SCAN, SquadPlayer,
    TRANSFER_LIST, TRANSFER_LIST_SCAN,
};

/// [`TRANSFER_LIST`] as a value of `FAR:304a`.
const MARKET: i8 = TRANSFER_LIST as i8;
/// [`TRANSFER_LIST_SCAN`] as the record count of the insert and remove helpers.
const MARKET_SLOTS: u8 = TRANSFER_LIST_SCAN as u8;
/// [`SQUAD_SCAN`] as the record count of the insert and remove helpers.
const SQUAD_SLOTS: u8 = SQUAD_SCAN as u8;
/// Status bit 6: an offer for this squad player is pending.
const OFFER_FOR_SQUAD_PLAYER: u8 = 0x40;
/// Status bit 7: an offer for this listed player is pending; in
/// [`player_value_calc`] it selects the 130 % / 95..100 % factor.
const OFFER_FOR_LISTED_PLAYER: u8 = 0x80;
/// Bit 7 of the rejection mask (`goals[0]` of a market record): any bid of
/// this player was rejected.
const REJECTED_ANY: u8 = 0x80;

/// DS:543e weights of the value terms; row 0 is the fee formula, row 1 the
/// salary formula.
const VALUE_WEIGHTS: [[i8; 6]; 2] = [[58, 4, 4, 5, 4, 25], [70, 8, 7, 5, 8, 2]];
/// `day_counter` (DS:07dc) above this is the late season: salary demands
/// grow with age and an empty contract offer counts as a refusal.
const LATE_SEASON_DAY: i32 = 0x141;

/// Value of a player record: the asking fee or offer (fee formula, even
/// `mode`) or the monthly salary (salary formula, odd `mode`) of
/// `squad(FAR:304a, slot)`.
///
/// Mode bit 1 is a loan (a third of the fee, then 75..95 %), bit 2 the
/// random spread of an asking price; both only apply to the fee formula.
/// Modes in use: 0 offer, 4 asking price, 1 and 3 salary of a new record,
/// 5 salary demand. Random draws, all in the fee formula: 95..100 % when
/// [`OFFER_FOR_LISTED_PLAYER`] is set and `sale_club <= 0x3f`, then the
/// spread for bit 2, then the loan factor for bit 1.
///
/// # Panics
/// When `FAR:304a` is not a group index, or when the spread of a fee whose
/// tenth and twentieth differ by -1 in their low words makes `random_range`
/// divide by zero (R6003 in the original).
///
/// Origin: BMMAIN.EXE 3277:25de (player_value_calc)
pub fn player_value_calc(state: &GameState, rng: &mut Rng, slot: u8, mode: u8) -> i32 {
    let p = record(state, slot);
    player_value_of(rng, p, state.players[p.player_id as usize].age, mode)
}

/// [`player_value_calc`] of record `p` whose player is `age` years old.
pub fn player_value_of(rng: &mut Rng, p: &SquadPlayer, age: u8, mode: u8) -> i32 {
    if mode & 1 != 0 {
        return record_salary(p, age);
    }
    let mut v = lmul(value_points(p, age, &VALUE_WEIGHTS[0]), 10000);
    if v > 110_000 {
        v = v.wrapping_sub(100_000);
    }
    if mode & 2 != 0 {
        v /= 3;
    }
    if p.status & OFFER_FOR_LISTED_PLAYER != 0 {
        let percent = if p.sale_club > 0x3f {
            130
        } else {
            i32::from(rng.random_range(0x5f, 0x64))
        };
        v = lmul(v, percent) / 100;
    }
    v = lmul(v, 5) / 3;
    if mode & 4 != 0 {
        let a = v / 15;
        let r = rng.random_range((v / 20) as i16, (v / 10) as i16);
        v = v.wrapping_add(i32::from(r)).wrapping_sub(a);
    }
    if mode & 2 != 0 {
        v = lmul(v, i32::from(rng.random_range(0x4b, 0x5f))) / 100;
    }
    v / 1000 * 1000
}

/// The salary formula of [`player_value_calc`] (odd modes, no random draws).
fn salary_formula(state: &GameState, slot: u8) -> i32 {
    let p = record(state, slot);
    record_salary(p, state.players[p.player_id as usize].age)
}

/// [`salary_formula`] of record `p` whose player is `age` years old.
fn record_salary(p: &SquadPlayer, age: u8) -> i32 {
    let points = value_points(p, age, &VALUE_WEIGHTS[1]);
    let mut v = lmul(lmul(points, points), points) / 1000;
    let mut s = i32::from(p.skills[0]) + i32::from(p.skills[1]);
    if s > 100 {
        s = ((s - 100) >> 1) + 100;
        if s > 135 {
            v = lmul(v, s - 35) / 100;
        }
    }
    v = v.max(2);
    lmul(lmul(v, 85), 5) / 3 / 100 * 100
}

/// The weighted sum of skills, statistics, value terms, tendency, cards and
/// age that both formulas of [`player_value_calc`] start from.
fn value_points(p: &SquadPlayer, age: u8, w: &[i8; 6]) -> i32 {
    let w = w.map(i32::from);
    let h = (u16::from(p.skills[1]) + u16::from(p.skills[0])) as i16 / 2;
    let mut v = i32::from(h) * w[0] / 100;

    let goals = (i32::from(p.goals[0]) + i32::from(p.goals[1])) * 30 + i32::from(p.goals[2]) * 40;
    let mut r1 = (goals / 100) as i16;
    let [t50, t20, t30] = p.value_terms.map(u32::from);
    let mut r2 = ((20 * t20 + 30 * t30 + 50 * t50) as i32 / 100) as i16;
    clamp_int(&mut r1, 0, 20);
    clamp_int(&mut r2, 0, 200);
    v += w[1] * i32::from(r1) * 5 / 100;
    v += i32::from(r2) * 100 / 200 * w[2] / 100;
    v += i32::from((w[3] as i16).wrapping_mul(i16::from(p.tendency)) / 100);

    let mut r3 = (6 * u16::from(p.cards[0]) + u16::from(p.cards[1])) as i16;
    clamp_int(&mut r3, 0, 20);
    v += w[4] * i32::from(r3) * 5 / 100;
    let mut r4 = 0x2d - i16::from(age);
    clamp_int(&mut r4, 0, 27);
    v + i32::from(r4) * 100 / 27 * w[5] / 100
}

fn record(state: &GameState, slot: u8) -> &SquadPlayer {
    &state.squads[group(state)][slot as usize]
}

fn record_mut(state: &mut GameState, slot: u8) -> &mut SquadPlayer {
    let g = group(state);
    &mut state.squads[g][slot as usize]
}

fn market(state: &GameState, slot: u8) -> &SquadPlayer {
    &state.squads[TRANSFER_LIST][slot as usize]
}

fn market_mut(state: &mut GameState, slot: u8) -> &mut SquadPlayer {
    &mut state.squads[TRANSFER_LIST][slot as usize]
}

/// Runs `f` with `FAR:304a` switched to the market and restores it from
/// `FAR:5358` afterwards.
fn at_market<T>(state: &mut GameState, f: impl FnOnce(&mut GameState) -> T) -> T {
    state.current_manager = MARKET;
    let r = f(state);
    state.current_manager = state.saved_manager;
    r
}

/// The human manager `FAR:304a` as an index.
fn manager(state: &GameState) -> usize {
    state
        .current_index()
        .filter(|&m| m < TRANSFER_LIST)
        .expect("FAR:304a is not a human manager")
}

/// Removes every club-listed player from the transfer market and lists a
/// random number of free players for random clubs, with fresh skills and an
/// asking price. Players listed by human managers stay.
///
/// The original's quirks are kept: removing a record moves the next one into
/// the slot the scan has just passed, and the club search for a player of a
/// human's club reuses the refill counter, so the refill loop can run more or
/// fewer times than drawn.
///
/// # Panics
/// When the market is not packed (a free record after a taken one), as
/// inserting then finds no room; the original writes the fee out of bounds.
///
/// Origin: BMMAIN.EXE 3277:1e38 (transfer_market_refresh)
pub fn transfer_market_refresh(state: &mut GameState, rng: &mut Rng) {
    at_market(state, |state| {
        let mut kept = 0i16;
        for i in 0..MARKET_SLOTS {
            let pid = market(state, i).player_id;
            if squad_remove_player(state, i, false, MARKET_SLOTS) {
                state.players[pid as usize].owner_manager = NO_OWNER;
            } else {
                kept += 1;
            }
        }
        let mut n = MARKET_SLOTS as i16 - kept;
        if n != 0 {
            n -= rng.random_range(0, n);
            let mut i = 0i16;
            while n > i {
                refill_market_slot(state, rng, &mut i);
                i += 1;
            }
        }
    });
}

/// One pass of the refill loop of [`transfer_market_refresh`]; `i` is the
/// loop counter that the tier search overwrites.
fn refill_market_slot(state: &mut GameState, rng: &mut Rng, i: &mut i16) {
    let s = rng.random_range(0, MARKET_SLOTS as i16 - 1) as u8;
    let old = market(state, s).player_id;
    if old != 0 {
        if state.players[old as usize].owner_manager != CLUB_LISTED {
            return;
        }
        squad_remove_player(state, s, true, MARKET_SLOTS);
        state.players[old as usize].owner_manager = NO_OWNER;
    }

    let mut pid;
    let mut tries = 0;
    loop {
        pid = rng.random_range(1, PLAYER_COUNT as i16 - 1) as usize;
        tries += 1;
        if state.players[pid].owner_manager == NO_OWNER || tries >= 0x5dc {
            break;
        }
    }
    state.players[pid].skills[2] = rng.random_range(0x2d, 0x37) as u8;
    let mut team = i16::from(state.players[pid].team);
    if state.manager_index_of_team(team as u8, 0xff) != 0 {
        let tier = (0..3)
            .find(|&t| league_team_ids(t).contains(&(team as u8)))
            .unwrap_or(2);
        *i = tier as i16;
        // DS:2272 holds the same ranges.
        let ids = league_team_ids(tier);
        let (lo, hi) = (i16::from(ids.start), i16::from(ids.end) - 1);
        loop {
            team = rng.random_range(lo, hi);
            if state.manager_index_of_team(team as u8, 0xff) == 0 {
                break;
            }
        }
    }
    let club = &state.clubs[team as usize];
    let (strength, base_strength) = (club.strength[1], club.base_strength[1]);
    let mut v = rng.random_range(0, 14) + i16::from(strength) - 10;
    clamp_int(&mut v, 10, 99);
    state.players[pid].skills[0] = v as u8;
    let mut v = rng.random_range(0, 14) + i16::from(base_strength) - 10;
    if rng.random_range(0, 4) == 0 {
        v += rng.random_range(0x0c, 0x14);
    }
    clamp_int(&mut v, 10, 99);
    state.players[pid].skills[1] = v as u8;

    let slot = squad_insert_player(state, rng, pid as u8, 0, 0, MARKET_SLOTS)
        .expect("a market record was freed for the new player");
    state.players[pid].owner_manager = CLUB_LISTED;
    state.players[pid].team = team as u8;
    market_mut(state, slot).market_value = player_value_calc(state, rng, slot, 4);
}

/// Whether the club that lists market record `slot` accepts a bid of
/// `offer` (for a loan when `loan` is set, against a third of the asking
/// price). Bids below 75..85 % are refused and bids above 120..130 %
/// accepted outright (one draw each, in this order); in between the bid in
/// percent must beat a draw of 80..120.
///
/// # Panics
/// When both the asking price and the bid are 0 (R6003 in the original).
///
/// Origin: BMMAIN.EXE 3277:2171 (market_fee_offer_accepted)
pub fn market_fee_offer_accepted(
    state: &GameState,
    rng: &mut Rng,
    slot: u8,
    offer: i32,
    loan: bool,
) -> bool {
    let mut v = market(state, slot).market_value;
    if loan {
        v /= 3;
    }
    if lmul(v, i32::from(rng.random_range(0x4b, 0x55))) / 100 > offer {
        return false;
    }
    if lmul(v, i32::from(rng.random_range(0x78, 0x82))) / 100 < offer {
        return true;
    }
    ldiv(lmul(offer, 100), v) > i32::from(rng.random_range(0x50, 0x78))
}

/// Percentage of the season still to play in the league of manager
/// `FAR:304a`.
///
/// # Panics
/// When the manager's league is not 0..=2 (the original divides by the
/// match-day count 0 of league 3, R6003).
fn season_left_percent(state: &GameState) -> i32 {
    let league = state.managers[manager(state)].league as usize;
    let round = state.competition_rounds[league] as i8;
    ldiv(
        i32::from(round) * -100,
        i32::from(LEAGUE_ROUND_COUNT[league]),
    ) + 100
}

/// `(len - 1) * 100` plus the rest of the season, the contract length in
/// percent of a season that both salary demands scale with.
fn contract_percent(state: &GameState, len: i32) -> i32 {
    len.wrapping_sub(1)
        .wrapping_mul(100)
        .wrapping_add(season_left_percent(state))
}

/// Whether the player of `squad(FAR:304a, slot)` accepts a contract of
/// `len` seasons at `salary` per month (0 keeps the current value).
///
/// With the current length he takes any raise and refuses any cut; for a
/// shorter contract he takes a raise above 20 % and refuses one up to 5 %
/// (unsigned compares). Otherwise the salary must exceed his demand: the
/// salary formula scaled by the contract length (one draw) and, in the late
/// season, by `132 - age .. 152 - age` percent (a second draw); a demand of
/// 100 000 or more is never met.
///
/// # Panics
/// When `FAR:304a` is not a human manager or the league is not 0..=2.
///
/// Origin: BMMAIN.EXE 3277:2270 (contract_demand_accepted)
pub fn contract_demand_accepted(
    state: &GameState,
    rng: &mut Rng,
    slot: u8,
    mut len: i32,
    mut salary: i32,
) -> bool {
    let p = record(state, slot);
    let cur_len = i32::from(p.contract_years);
    let cur_salary = p.market_value as u32;
    if len == 0 {
        len = cur_len;
    }
    if salary == 0 {
        salary = p.market_value;
    }
    let age = state.players[p.player_id as usize].age as i8;
    if cur_len == len && cur_salary < salary as u32 {
        return true;
    }
    if cur_len == len && cur_salary > salary as u32 {
        return false;
    }
    if cur_len > len && uldiv(cur_salary.wrapping_mul(120), 100) < salary as u32 {
        return true;
    }
    if cur_len > len && uldiv(cur_salary.wrapping_mul(105), 100) >= salary as u32 {
        return false;
    }
    let len = contract_percent(state, len);
    let mut v = salary_formula(state, slot);
    let k = (lshl(len / 6 + 110, 3) / 10) as i16;
    v = lmul(
        v,
        i32::from(rng.random_range(k.wrapping_sub(6), k.wrapping_add(4))),
    ) / 100;
    if state.day_counter > LATE_SEASON_DAY {
        let age = i16::from(age);
        v = lmul(v, i32::from(rng.random_range(0x84 - age, 0x98 - age))) / 100;
    }
    v < 100_000 && salary > v
}

/// The salary that `contract_negotiation` fills in when a contract length
/// of `len` seasons is typed: the highest demand [`contract_demand_accepted`]
/// can draw for that length, but at least the current salary (unsigned
/// compare). No random draws.
///
/// # Panics
/// When `FAR:304a` is not a human manager or the league is not 0..=2.
///
/// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation, salary suggestion)
pub fn contract_salary_suggestion(state: &GameState, slot: u8, len: i32) -> i32 {
    let p = record(state, slot);
    let len = contract_percent(state, len);
    let mut v = salary_formula(state, slot);
    let k = lshl(len / 6 + 0x7a, 3) / 10;
    v = lmul(v, k.wrapping_add(4)) / 100;
    if state.day_counter > LATE_SEASON_DAY {
        let age = state.players[p.player_id as usize].age;
        v = lmul(v, i32::from(0x98 - i16::from(age))) / 100;
    }
    if p.market_value as u32 > v as u32 {
        v = p.market_value;
    }
    v
}

/// The terms typed into `contract_negotiation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractOffer {
    /// Contract length in seasons, 0 = not entered.
    pub len: i32,
    /// Monthly salary, 0 = not entered.
    pub salary: i32,
    /// Length the player insists on (negotiation state minus 100), 0 = none.
    pub forced: i16,
}

/// The result of [`contract_decision`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractHit {
    /// Icon 1, "NEUER VERTRAG".
    NewContract = 1,
    /// Icon 2, "ABBRUCH", or a refused offer.
    Cancel = 2,
    /// Neither length nor salary was entered early in the season; the
    /// player's cool-down is left alone.
    NothingEntered = 3,
}

/// The message box `contract_negotiation` shows after the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractDialog {
    /// "Wer wird sich denn SO lange verpflichten ?": more than 4 seasons.
    TooLong,
    /// "So dumm ist ... leider nicht...": a longer contract for less money.
    NotFooled,
    /// "... ist nicht an Ihrem Angebot interessiert."
    NotInterested,
}

/// Decides a contract offer of `contract_negotiation` for
/// `squad(FAR:304a, slot)` and applies it: on acceptance the record gets the
/// new length and salary, and unless nothing was entered the player gets a
/// cool-down of 10..18 (also after an acceptance).
///
/// `new_contract` is the clicked button ("NEUER VERTRAG" or "ABBRUCH").
/// `single` is the mode used after a purchase; it applies the "nothing
/// entered" rule only in the late season, where it cancels. (The original
/// also writes the terms for "nothing entered" in single mode, which that
/// makes unreachable.) Random draws: [`contract_demand_accepted`] when
/// asked, then the cool-down.
///
/// The browse mode's refusals before the input (loaned, retiring, cool-down)
/// are screen checks and not part of this decision.
///
/// TODO(re): the spec assigns `forced` only on the browse path; which value
/// the single mode passes is open.
///
/// # Panics
/// When `FAR:304a` is not a human manager or the league is not 0..=2.
///
/// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation, decision at 3277:3281)
pub fn contract_decision(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
    single: bool,
    offer: ContractOffer,
    new_contract: bool,
) -> (ContractHit, Option<ContractDialog>) {
    let ContractOffer {
        mut len,
        mut salary,
        forced,
    } = offer;
    let mut hit = if new_contract {
        ContractHit::NewContract
    } else {
        ContractHit::Cancel
    };
    let mut dialog = None;
    if len > 4 {
        dialog = Some(ContractDialog::TooLong);
        hit = ContractHit::Cancel;
    }
    let p = record(state, slot);
    let (cur_len, cur_salary) = (i32::from(p.contract_years), p.market_value);
    let late = state.day_counter > LATE_SEASON_DAY;
    if (!single || late) && len == 0 && (salary == 0 || cur_len == 0) && forced == 0 {
        hit = if late {
            ContractHit::Cancel
        } else {
            ContractHit::NothingEntered
        };
    }
    if len == 0 {
        len = if forced != 0 {
            i32::from(forced)
        } else {
            cur_len
        };
    }
    if salary == 0 {
        salary = cur_salary;
    }
    if hit == ContractHit::NewContract && len > cur_len && cur_salary > salary {
        dialog = Some(ContractDialog::NotFooled);
        hit = ContractHit::Cancel;
    }
    if forced != 0 && (cur_salary > salary || i32::from(forced) != len) {
        hit = ContractHit::Cancel;
    }
    if hit == ContractHit::NewContract {
        if forced == 0 && !contract_demand_accepted(state, rng, slot, len, salary) {
            dialog = Some(ContractDialog::NotInterested);
            hit = ContractHit::Cancel;
        } else {
            let p = record_mut(state, slot);
            p.contract_years = len as u8;
            p.market_value = salary;
        }
    }
    if hit != ContractHit::NothingEntered {
        let cool_down = rng.random_range(0x0a, 0x12) as u8;
        if record_mut(state, slot).negotiation > 0x63 {
            message_remove(state, group(state), slot);
        }
        record_mut(state, slot).negotiation = cool_down;
    }
    (hit, dialog)
}

/// `rec.msg = message_remove(rec.msg)` on record `slot` of `squad`: the
/// pending message leaves the inbox and the record's pointer is cleared.
fn message_remove(state: &mut GameState, squad: usize, slot: u8) {
    let ptr = std::mem::take(&mut state.squads[squad][usize::from(slot)].message_ptr);
    message_delete(state, ptr);
}

/// The compensation `transfer_offer_received` deducts from an accepted
/// offer for a player with `len` contract seasons left ("Abl|se"):
/// `offer / 7 * len`. The seller receives `offer` minus this amount.
///
/// Origin: BMMAIN.EXE 3277:1b5f (transfer_offer_received)
pub fn offer_compensation(offer: i32, len: i32) -> i32 {
    lmul(offer / 7, len)
}

/// Rounds `value * draw` percent down to a multiple of 100 the way the offer
/// dialogs do.
fn offer_amount(value: i32, draw: i16) -> i32 {
    lmul(value, i32::from(draw)) / 10000 * 100
}

/// The gross amount of the pending offer for `squad(FAR:304a, slot)`: 85..150 %
/// of the player's value (mode 0), rounded down to 100.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:0989)
pub fn squad_player_offer(state: &GameState, rng: &mut Rng, slot: u8) -> i32 {
    let value = player_value_calc(state, rng, slot, 0);
    offer_amount(value, rng.random_range(0x55, 0x96))
}

/// The gross amount of the pending offer for the player a human manager
/// listed in market record `slot`: 67..110 % of the value (mode 0), rounded
/// down to 100.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn listed_player_offer(state: &mut GameState, rng: &mut Rng, slot: u8) -> i32 {
    let value = at_market(state, |s| player_value_calc(s, rng, slot, 0));
    offer_amount(value, rng.random_range(0x43, 0x6e))
}

/// Closes the pending offer for `squad(FAR:304a, slot)`; on a sale the
/// manager receives `net` (the offer minus [`offer_compensation`]) and the
/// player leaves for the free pool.
///
/// The original reads the target club and the skills for
/// [`club_strength_after_sale`] after removing the record, so they come from
/// the player who moved into the slot, whose target club is cleared.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:0989)
pub fn answer_squad_player_offer(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
    net: i32,
    sell: bool,
) {
    let m = manager(state);
    let p = record_mut(state, slot);
    let pid = p.player_id as usize;
    p.status ^= OFFER_FOR_SQUAD_PLAYER;
    message_remove(state, group(state), slot);
    if !sell {
        return;
    }
    state.managers[m].balance = state.managers[m].balance.wrapping_add(net);
    squad_remove_player(state, slot, true, SQUAD_SLOTS);
    let next = record_mut(state, slot);
    let to = std::mem::take(&mut next.sale_club);
    let avg = next.average_skill();
    state.players[pid].team = to;
    // At this call site the stale index is provably 0 (spec §5).
    club_strength_after_sale(state, rng, to, avg);
    state.players[pid].owner_manager = NO_OWNER;
}

/// Closes the pending offer for the player manager `FAR:304a` listed in
/// market record `slot`; on a sale the manager receives `net` and the player
/// leaves the market for the free pool.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn answer_listed_player_offer(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
    net: i32,
    sell: bool,
) {
    let m = manager(state);
    let rec = market_mut(state, slot);
    let pid = rec.player_id as usize;
    rec.status ^= OFFER_FOR_LISTED_PLAYER;
    message_remove(state, TRANSFER_LIST, slot);
    if !sell {
        return;
    }
    let rec = market_mut(state, slot);
    let to = std::mem::take(&mut rec.sale_club);
    let avg = rec.average_skill();
    state.players[pid].team = to;
    // TODO(re): the original indexes the club strengths with a stale stack
    // byte here (spec §7 question 2); 0, as at the other call site, keeps
    // the random draws and may differ in the deltas.
    club_strength_after_sale(state, rng, to, avg);
    state.managers[m].balance = state.managers[m].balance.wrapping_add(net);
    at_market(state, |s| squad_remove_player(s, slot, true, MARKET_SLOTS));
    state.players[pid].owner_manager = NO_OWNER;
}

/// Why [`list_squad_player`] did not put a player on the market.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListRejected {
    /// "Schon 3 Spieler auf dem Transfermarkt".
    ThreeListed,
    /// "Dieser Spieler ist nur ausgeliehen".
    OnLoan,
    /// The market is full (the insert helper's own dialog).
    Insert(InsertRejected),
}

/// Moves `squad(FAR:304a, slot)` onto the transfer market and returns its
/// market record. A manager may list at most 3 players and no loaned-in
/// player. The whole record is copied, so its goal counts double as the
/// rejection mask; the insert draws 5 random numbers that the copy then
/// overwrites.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:0989)
pub fn list_squad_player(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
) -> Result<u8, ListRejected> {
    let m = manager(state);
    let listed = (0..state.squad_player_count(TRANSFER_LIST))
        .filter(|&i| state.players[market(state, i).player_id as usize].owner_manager == m as u8)
        .count();
    if listed >= 3 {
        return Err(ListRejected::ThreeListed);
    }
    let pid = record(state, slot).player_id;
    if state.players[pid as usize].owner_manager != m as u8 {
        return Err(ListRejected::OnLoan);
    }
    let k = at_market(state, |s| {
        squad_insert_player(s, rng, pid, 1, 0, MARKET_SLOTS)
    })
    .map_err(ListRejected::Insert)?;
    let p = &mut state.squads[m][slot as usize];
    p.lineup_position = 0;
    *market_mut(state, k) = p.clone();
    squad_remove_player(state, slot, true, SQUAD_SLOTS);
    Ok(k)
}

/// What a click on market record `slot` leads to for manager `FAR:304a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketBid {
    /// "Angebot wurde bereits abgelehnt".
    AlreadyRejected,
    /// The manager's own listed player has an offer
    /// ([`listed_player_offer`], [`answer_listed_player_offer`]).
    OfferForOwnPlayer,
    /// The manager takes his own listed player back without paying.
    Reclaim,
    /// The manager types a bid.
    EnterBid,
    /// The rejection bit of the manager is set without bit 7 for a
    /// club-listed player: the original bids the screen's last offer value
    /// without asking.
    RepeatLastOffer,
}

/// Classifies a click on market record `slot` the way `screen_transfer_market`
/// branches.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn market_bid(state: &GameState, slot: u8) -> MarketBid {
    let m = manager(state);
    let rec = market(state, slot);
    let own = state.players[rec.player_id as usize].owner_manager;
    let mask = rec.goals[0];
    let mine = mask & (1 << m) != 0;
    if mask & REJECTED_ANY != 0 && mine {
        MarketBid::AlreadyRejected
    } else if own == m as u8 && rec.status & OFFER_FOR_LISTED_PLAYER != 0 {
        MarketBid::OfferForOwnPlayer
    } else if own == m as u8 {
        MarketBid::Reclaim
    } else if mine && own == CLUB_LISTED {
        // TODO(re): the bid is the screen's uninitialised `offer` on its
        // first use (spec §7 question 9); a port can start it at 0.
        MarketBid::RepeatLastOffer
    } else {
        MarketBid::EnterBid
    }
}

/// Whether a bid of `offer` for the player another human manager listed in
/// market record `slot` lies within 60..140 % of his value (a third of it
/// for a loan); otherwise the seller only sees "DIESER BETRAG LIEGT AUSSERHALB
/// DES ERLAUBTEN !". The value may draw one random number.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn human_bid_allowed(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
    offer: i32,
    loan: bool,
) -> bool {
    let mut value = at_market(state, |s| player_value_calc(s, rng, slot, 0));
    if loan {
        value /= 3;
    }
    !(lmul(value, 60) / 100 > offer || lmul(value, 140) / 100 < offer)
}

/// Marks market record `slot` as rejected for manager `FAR:304a`.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn mark_bid_rejected(state: &mut GameState, slot: u8) {
    let m = manager(state);
    market_mut(state, slot).goals[0] |= (1 << m) | REJECTED_ANY;
}

/// A player of market record `market_slot` inserted into the buyer's squad
/// by [`purchase_insert`], waiting for the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Purchase {
    /// Record of the player in the buyer's squad.
    pub squad_slot: u8,
    /// Market record of the player.
    pub market_slot: u8,
    /// The player's club before the purchase.
    pub club: u8,
    /// Salary the insert computed for the new record.
    pub salary: i32,
}

/// Inserts the player of market record `slot` into the squad of manager
/// `FAR:304a` after an accepted bid; `reclaim` allows a 25th player.
/// A purchase from another human manager (`from_human`) first gives the
/// player the skills of his market record.
///
/// Unless the bid is a loan or a reclaim, `contract_negotiation` follows and
/// decides between [`purchase_complete`] and [`purchase_cancel`].
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn purchase_insert(
    state: &mut GameState,
    rng: &mut Rng,
    slot: u8,
    loan: bool,
    reclaim: bool,
    from_human: bool,
) -> Result<Purchase, InsertRejected> {
    let rec = market(state, slot);
    let pid = rec.player_id;
    if from_human {
        state.players[pid as usize].skills = rec.skills;
    }
    let club = state.players[pid as usize].team;
    let loan_marker = if loan { LOAN_PRICE_MODE } else { 0 };
    let limit = SQUAD_SLOTS + u8::from(reclaim);
    let squad_slot = squad_insert_player(state, rng, pid, 1, loan_marker, limit)?;
    Ok(Purchase {
        squad_slot,
        market_slot: slot,
        club,
        salary: record(state, squad_slot).market_value,
    })
}

/// Undoes [`purchase_insert`] when no contract was signed ("Der Transfer
/// findet nicht statt"); the caller then marks the bid rejected unless the
/// seller is a human manager.
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn purchase_cancel(state: &mut GameState, purchase: Purchase) {
    let pid = record(state, purchase.squad_slot).player_id;
    squad_remove_player(state, purchase.squad_slot, true, SQUAD_SLOTS);
    state.players[pid as usize].team = purchase.club;
}

/// Completes a purchase for `offer`: the new squad record takes over the
/// market record's state (all of it for a loan or a reclaim, the status and
/// counters from another human manager), the market record is removed and
/// the money moves. A loaned player stays owned by his club and joins for
/// one season at the salary of [`purchase_insert`]. A purchase from a club
/// (not a reclaim) is followed by the sponsor's offer (`sponsor_offer_dialog`
/// 1000:272d with `offer`).
///
/// Origin: BMMAIN.EXE 3277:04a5 (screen_transfer_market, 3277:1033)
pub fn purchase_complete(
    state: &mut GameState,
    purchase: Purchase,
    offer: i32,
    loan: bool,
    reclaim: bool,
    from_human: bool,
) {
    let m = manager(state);
    let listed = market(state, purchase.market_slot).clone();
    let pid = listed.player_id as usize;
    let s = record_mut(state, purchase.squad_slot);
    if from_human {
        s.status = listed.status;
        s.injury = listed.injury;
        s.suspended = listed.suspended;
        s.cards = listed.cards;
    }
    if reclaim || loan {
        *s = listed;
        if loan {
            s.contract_years = 1;
            s.loan = purchase.club.wrapping_add(0x80);
            s.market_value = purchase.salary;
            s.goals = [0; 3];
        }
    }
    at_market(state, |s| {
        squad_remove_player(s, purchase.market_slot, true, MARKET_SLOTS)
    });
    if from_human {
        let seller = state.players[pid].owner_manager as usize;
        state.managers[seller].balance = state.managers[seller].balance.wrapping_add(offer);
        state.players[pid].team = state.managers[m].team;
    }
    if !loan || reclaim {
        state.players[pid].owner_manager = m as u8;
    }
    if !reclaim {
        state.managers[m].balance = state.managers[m].balance.wrapping_sub(offer);
    }
}

/// Free training slots of manager `FAR:304a`: 20 minus the four A slots
/// (`kind == 0`) or 10 minus the four B slots, in byte arithmetic.
///
/// Origin: BMMAIN.EXE 3277:01a9 (training_free_slots)
pub fn training_free_slots(state: &GameState, kind: i8) -> i8 {
    let mgr = &state.managers[manager(state)];
    let (total, slots) = if kind == 0 {
        (20u8, &mgr.training_141[..4])
    } else {
        (10, &mgr.training_146[..])
    };
    slots.iter().fold(total, |left, &s| left.wrapping_sub(s)) as i8
}

/// Strengthens a random part of `club`'s first and base strength after it
/// bought a player of average skill `avg` (3 random draws). The original
/// sums the strengths with an uninitialised index instead of the loop
/// counter; it is 0 here, as it provably is after a squad player's sale.
///
/// # Panics
/// When `club` is not a club index.
///
/// Origin: BMMAIN.EXE 24a4:23da (club_strength_after_sale)
pub fn club_strength_after_sale(state: &mut GameState, rng: &mut Rng, club: u8, avg: i16) {
    let c = &mut state.clubs[club as usize];
    let sum = 3 * (i16::from(c.strength[0]) + i16::from(c.base_strength[0]));
    let mut s = (avg - sum / 6 + 10) / 3;
    let k = rng.random_range(0, 2) as usize;
    clamp_int(&mut s, 1, 10);
    c.strength[k] = c.strength[k].wrapping_add(rng.random_range(s - 1, s + 1) as u8);
    c.base_strength[k] = c.base_strength[k].wrapping_add(rng.random_range(s - 1, s + 1) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Layout;
    use crate::test_saves::save_states;

    /// Manager 0 of club 5 in league 0 at match day 17 of 34, early season.
    /// Every player is free and belongs to club 30.
    fn state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 5;
        s.competition_rounds = [17, 1, 1];
        s.day_counter = 100;
        for p in &mut s.players {
            p.owner_manager = NO_OWNER;
            p.team = 30;
        }
        s
    }

    /// Puts player `pid` (age 25) into record `i` of group `g`: skills
    /// 60/40, tendency 50, salary formula 7600, fee formula 650 000.
    fn put(s: &mut GameState, g: usize, i: usize, pid: u8) -> &mut SquadPlayer {
        s.players[pid as usize].age = 25;
        let r = &mut s.squads[g][i];
        *r = SquadPlayer::zeroed();
        r.player_id = pid;
        r.skills = [60, 40, 0];
        r.tendency = 50;
        r
    }

    fn value(s: &GameState, slot: u8, mode: u8) -> i32 {
        let mut rng = Rng::new(1);
        let v = player_value_calc(s, &mut rng, slot, mode);
        assert_eq!(rng, Rng::new(1), "mode {mode} drew");
        v
    }

    #[test]
    fn value_of_a_plain_record() {
        let mut s = state();
        put(&mut s, 0, 0, 7);
        assert_eq!(value(&s, 0, 0), 650_000);
        // Modes 1, 3 and 5 are the same salary formula.
        for mode in [1, 3, 5] {
            assert_eq!(value(&s, 0, mode), 7600);
        }
    }

    #[test]
    fn value_clamps_and_wraps() {
        let mut s = state();
        s.players[7].age = 50;
        let r = &mut s.squads[0][0];
        r.player_id = 7;
        r.skills = [99, 99, 0];
        r.goals = [255; 3];
        r.tendency = 65;
        // 6 * 43 is not truncated to a byte: 258, clamped to 20.
        r.cards = [43, 0, 0];
        // (50 + 20) * 0xffff / 100 = 45874 wraps negative and clamps to 0.
        r.value_terms = [0xffff, 0xffff, 0];
        assert_eq!(value(&s, 0, 0), 966_000);
        // Skill sum 198 counts as 149 and scales the salary by 114 %.
        assert_eq!(value(&s, 0, 1), 109_900);
        // 50 * 0xffff / 100 = 32767 clamps to 200.
        s.squads[0][0].value_terms = [0xffff, 0, 0];
        assert_eq!(value(&s, 0, 0), 1_033_000);
    }

    #[test]
    fn fee_above_110000_loses_100000() {
        let mut s = state();
        let r = put(&mut s, 0, 0, 7);
        r.skills = [0; 3];
        r.tendency = 0;
        // Age 33: 12 * 100 / 27 * 25 / 100 = 11 points.
        s.players[7].age = 33;
        assert_eq!(value(&s, 0, 0), 183_000);
        s.players[7].age = 32;
        assert_eq!(value(&s, 0, 0), 33_000);
        // Salary floor: 11 points cube to 1, raised to 2.
        assert_eq!(value(&s, 0, 1), 200);
    }

    #[test]
    fn fee_random_factors_in_order() {
        let mut s = state();
        put(&mut s, 0, 0, 7).status = OFFER_FOR_LISTED_PLAYER;
        let mut rng = Rng::new(42);
        let mut expected = rng;
        let v = 390_000 * i32::from(expected.random_range(95, 100)) / 100 * 5 / 3;
        assert_ne!(i32::from((v / 10) as i16), v / 10);
        let a = v / 15;
        // v / 20 and v / 10 are cut to their low words.
        let r = expected.random_range((v / 20) as i16, (v / 10) as i16);
        let v = v + i32::from(r) - a;
        assert_eq!(player_value_calc(&s, &mut rng, 0, 4), v / 1000 * 1000);
        assert_eq!(rng, expected);

        // A target club above 0x3f fixes the factor at 130 %; a loan is a third.
        s.squads[0][0].sale_club = 0x40;
        assert_eq!(value(&s, 0, 0), 845_000);
        let mut rng = Rng::new(42);
        let mut expected = rng;
        let v = 130_000 * 130 / 100 * 5 / 3;
        let v = v * i32::from(expected.random_range(75, 95)) / 100;
        assert_eq!(player_value_calc(&s, &mut rng, 0, 2), v / 1000 * 1000);
        assert_eq!(rng, expected);
    }

    #[test]
    fn club_answers_bids() {
        let mut s = state();
        put(&mut s, 4, 0, 7).market_value = 300_000;
        let mut rng = Rng::new(5);
        let mut expected = rng;
        // Below 75 % of the price: refused after one draw.
        assert!(!market_fee_offer_accepted(&s, &mut rng, 0, 224_999, false));
        expected.rand();
        assert_eq!(rng, expected);
        // Above 130 %: accepted after two draws.
        assert!(market_fee_offer_accepted(&s, &mut rng, 0, 390_001, false));
        expected.rand();
        expected.rand();
        assert_eq!(rng, expected);
        // A loan bid is weighed against a third of the price.
        assert!(market_fee_offer_accepted(&s, &mut rng, 0, 130_001, true));
        for seed in 0..20 {
            let mut rng = Rng::new(seed);
            let mut expected = rng;
            let low = 300_000 * i32::from(expected.random_range(75, 85)) / 100;
            let high = 300_000 * i32::from(expected.random_range(120, 130)) / 100;
            assert!(low <= 300_000 && high >= 300_000);
            let accepted = 100 > i32::from(expected.random_range(80, 120));
            assert_eq!(
                market_fee_offer_accepted(&s, &mut rng, 0, 300_000, false),
                accepted
            );
            assert_eq!(rng, expected);
        }
    }

    /// Manager 0's record 0 on a 2-season contract at 10 000.
    fn contract_state() -> GameState {
        let mut s = state();
        let r = put(&mut s, 0, 0, 7);
        r.contract_years = 2;
        r.market_value = 10_000;
        s
    }

    #[test]
    fn contract_terms_without_demand() {
        let s = contract_state();
        let mut rng = Rng::new(3);
        assert!(contract_demand_accepted(&s, &mut rng, 0, 2, 10_001));
        assert!(!contract_demand_accepted(&s, &mut rng, 0, 0, 9_999));
        // Unsigned compare: a negative salary is a huge raise.
        assert!(contract_demand_accepted(&s, &mut rng, 0, 2, -1));
        // Shorter contract: above 120 % yes, up to 105 % no.
        assert!(contract_demand_accepted(&s, &mut rng, 0, 1, 12_001));
        assert!(!contract_demand_accepted(&s, &mut rng, 0, 1, 10_500));
        assert_eq!(rng, Rng::new(3));
    }

    #[test]
    fn contract_demand_draws() {
        let mut s = contract_state();
        // Half the season left: 3 seasons are 250 %, k = 1208 / 10 = 120,
        // demand 7600 * 114..124 % = 8664..9424.
        for seed in 0..10 {
            let mut rng = Rng::new(seed);
            assert!(contract_demand_accepted(&s, &mut rng, 0, 3, 9_425));
            assert!(!contract_demand_accepted(&s, &mut rng, 0, 3, 8_664));
            let mut expected = Rng::new(seed);
            expected.rand();
            expected.rand();
            assert_eq!(rng, expected);
        }
        let mut rng = Rng::new(9);
        let mut expected = rng;
        let v = 7600 * i32::from(expected.random_range(114, 124)) / 100;
        assert!(
            !contract_demand_accepted(&s, &mut rng, 0, 3, v),
            "the salary must exceed the demand"
        );
        assert_eq!(rng, expected);
        // Late season: a second draw of 132 - age .. 152 - age percent.
        s.day_counter = 322;
        let mut rng = Rng::new(9);
        let mut expected = rng;
        let v = 7600 * i32::from(expected.random_range(114, 124)) / 100;
        let v = v * i32::from(expected.random_range(107, 127)) / 100;
        assert!(contract_demand_accepted(&s, &mut rng, 0, 3, v + 1));
        assert_eq!(rng, expected);
    }

    #[test]
    fn contract_demand_of_100_000_is_never_met() {
        let mut s = state();
        let r = put(&mut s, 0, 0, 7);
        r.skills = [99, 99, 0];
        r.goals = [255; 3];
        r.tendency = 65;
        r.value_terms = [400; 3];
        s.players[7].age = 18;
        let formula = salary_formula(&s, 0);
        // Formula 113 600; one season with half of it left is 50 %, k = 944 /
        // 10 = 94, so the demand is 88..98 %: 99 968 (met) up to 111 328.
        let mut outcomes = [false; 2];
        for seed in 0..40 {
            let mut rng = Rng::new(seed);
            let mut expected = rng;
            let demand = formula * i32::from(expected.random_range(88, 98)) / 100;
            let accepted = contract_demand_accepted(&s, &mut rng, 0, 1, i32::MAX);
            assert_eq!(accepted, demand < 100_000, "seed {seed}, demand {demand}");
            assert_eq!(rng, expected);
            outcomes[usize::from(accepted)] = true;
        }
        assert_eq!(outcomes, [true; 2]);
    }

    #[test]
    fn salary_suggestion() {
        let mut s = contract_state();
        // k = 1304 / 10 = 130: 7600 * 134 / 100.
        assert_eq!(contract_salary_suggestion(&s, 0, 3), 10_184);
        s.day_counter = 322;
        assert_eq!(contract_salary_suggestion(&s, 0, 3), 12_933);
        s.squads[0][0].market_value = 20_000;
        assert_eq!(contract_salary_suggestion(&s, 0, 3), 20_000);
    }

    fn offer(len: i32, salary: i32, forced: i16) -> ContractOffer {
        ContractOffer {
            len,
            salary,
            forced,
        }
    }

    #[test]
    fn contract_decisions() {
        use ContractDialog::*;
        use ContractHit::*;
        // At 5000 the demand of 8664..9424 is a raise.
        let mut base = contract_state();
        base.squads[0][0].market_value = 5_000;
        let terms = |s: &GameState| {
            let r = &s.squads[0][0];
            (r.contract_years, r.market_value, r.negotiation)
        };
        let cool_down = |seed, draws_before: usize| {
            let mut r = Rng::new(seed);
            for _ in 0..draws_before {
                r.rand();
            }
            r.random_range(10, 18) as u8
        };
        let cases = [
            (false, offer(5, 9_500, 0), true, (Cancel, Some(TooLong)), 0),
            (
                false,
                offer(3, 4_000, 0),
                true,
                (Cancel, Some(NotFooled)),
                0,
            ),
            (false, offer(2, 9_000, 3), true, (Cancel, None), 0),
            (
                false,
                offer(3, 8_600, 0),
                true,
                (Cancel, Some(NotInterested)),
                1,
            ),
            (false, offer(0, 0, 0), true, (NothingEntered, None), 0),
            (true, offer(3, 9_500, 0), false, (Cancel, None), 0),
        ];
        for (single, o, clicked, result, draws) in cases {
            let mut s = base.clone();
            let mut rng = Rng::new(11);
            let got = contract_decision(&mut s, &mut rng, 0, single, o, clicked);
            assert_eq!(got, result, "{o:?}");
            let cool = if result.0 == NothingEntered {
                0
            } else {
                cool_down(11, draws)
            };
            assert_eq!(terms(&s), (2, 5_000, cool), "{o:?}");
        }

        // Accepted: new terms, demand draw, then the cool-down.
        let mut s = base.clone();
        let got = contract_decision(
            &mut s,
            &mut Rng::new(11),
            0,
            false,
            offer(3, 9_500, 0),
            true,
        );
        assert_eq!(got, (NewContract, None));
        assert_eq!(terms(&s), (3, 9_500, cool_down(11, 1)));
        // An insisted-on length is not negotiated; its message goes.
        let mut s = base.clone();
        s.squads[0][0].negotiation = 103;
        s.squads[0][0].message_ptr = 0x1234_5678;
        s.messages = vec![vec![bmp_formats::savegame::StringEntry {
            old_pointer: 0x1234_5678,
            bytes: b"X\0".to_vec(),
        }]];
        s.message_counts[0] = 1;
        let got = contract_decision(&mut s, &mut Rng::new(11), 0, true, offer(0, 0, 3), true);
        assert_eq!(got, (NewContract, None));
        assert_eq!(terms(&s), (3, 5_000, cool_down(11, 0)));
        assert_eq!(s.squads[0][0].message_ptr, 0);
        assert_eq!((s.messages[0].len(), s.message_counts[0]), (0, 0));
        // Late season, nothing entered: cancelled with a cool-down.
        let mut s = base.clone();
        s.day_counter = 322;
        let got = contract_decision(&mut s, &mut Rng::new(11), 0, false, offer(0, 0, 0), true);
        assert_eq!(got, (Cancel, None));
        assert_eq!(terms(&s).2, cool_down(11, 0));
    }

    #[test]
    fn offers_and_compensation() {
        assert_eq!(offer_compensation(70_000, 2), 20_000);
        assert_eq!(offer_compensation(69_999, 2), 19_998);

        let mut s = state();
        put(&mut s, 0, 0, 7);
        let mut rng = Rng::new(8);
        let mut expected = rng;
        let pct = i32::from(expected.random_range(85, 150));
        assert_eq!(
            squad_player_offer(&s, &mut rng, 0),
            650_000 * pct / 10_000 * 100
        );
        assert_eq!(rng, expected);

        put(&mut s, 4, 3, 9);
        s.current_manager = 0;
        let pct = i32::from(expected.random_range(67, 110));
        assert_eq!(
            listed_player_offer(&mut s, &mut rng, 3),
            650_000 * pct / 10_000 * 100
        );
        assert_eq!(rng, expected);
        assert_eq!(s.current_manager, 0);
    }

    fn strength_draws(seed: u16, s: i16) -> (usize, u8, u8) {
        let mut r = Rng::new(seed);
        let k = r.random_range(0, 2) as usize;
        let a = r.random_range(s - 1, s + 1) as u8;
        (k, a, r.random_range(s - 1, s + 1) as u8)
    }

    #[test]
    fn club_strength_after_a_sale() {
        let mut s = state();
        s.clubs[12].strength = [50, 250, 250];
        s.clubs[12].base_strength = [40, 250, 250];
        let mut rng = Rng::new(4);
        // (60 - 270 / 6 + 10) / 3 = 8.
        club_strength_after_sale(&mut s, &mut rng, 12, 60);
        let (k, a, b) = strength_draws(4, 8);
        let mut want: ([u8; 3], [u8; 3]) = ([50, 250, 250], [40, 250, 250]);
        want.0[k] = want.0[k].wrapping_add(a);
        want.1[k] = want.1[k].wrapping_add(b);
        assert_eq!((s.clubs[12].strength, s.clubs[12].base_strength), want);
        // A weak buy still adds 0..2.
        let before = s.clubs[12].clone();
        club_strength_after_sale(&mut s, &mut Rng::new(4), 12, 0);
        let (k, a, _) = strength_draws(4, 1);
        assert_eq!(s.clubs[12].strength[k], before.strength[k].wrapping_add(a));
    }

    #[test]
    fn selling_a_squad_player_uses_the_next_record() {
        let mut s = state();
        s.managers[0].balance = 500;
        put(&mut s, 0, 0, 5).status = OFFER_FOR_SQUAD_PLAYER;
        s.squads[0][0].sale_club = 9;
        let next = put(&mut s, 0, 1, 6);
        next.sale_club = 12;
        next.skills = [30, 60, 90];
        s.players[5].owner_manager = 0;
        s.clubs[12].strength = [45, 0, 0];
        s.clubs[12].base_strength = [45, 0, 0];

        let mut kept = s.clone();
        answer_squad_player_offer(&mut kept, &mut Rng::new(2), 0, 1000, false);
        assert_eq!(kept.squads[0][0].status, 0);
        assert_eq!(kept.players[5].owner_manager, 0);

        let mut rng = Rng::new(2);
        answer_squad_player_offer(&mut s, &mut rng, 0, 1000, true);
        assert_eq!(s.managers[0].balance, 1500);
        assert_eq!(s.squads[0][0].player_id, 6);
        assert_eq!(s.squads[0][0].sale_club, 0);
        assert_eq!(s.players[5].team, 12);
        assert_eq!(s.players[5].owner_manager, NO_OWNER);
        // (60 - 45 + 10) / 3 = 8 from the next player's skills.
        let (k, a, _) = strength_draws(2, 8);
        assert_eq!(s.clubs[12].strength[k], [45, 0, 0][k] + a);
        assert_eq!(s.clubs[9], state().clubs[9]);
    }

    #[test]
    fn listed_player_sale_and_bid_kinds() {
        let mut s = state();
        put(&mut s, 4, 0, 5).status = OFFER_FOR_LISTED_PLAYER;
        s.squads[4][0].sale_club = 12;
        put(&mut s, 4, 1, 6);
        put(&mut s, 4, 2, 7).goals[0] = 0x81;
        put(&mut s, 4, 3, 8).goals[0] = 0x01;
        put(&mut s, 4, 4, 9);
        s.players[5].owner_manager = 0;
        s.players[6].owner_manager = 0;
        for pid in [7, 8] {
            s.players[pid].owner_manager = CLUB_LISTED;
        }
        s.players[9].owner_manager = 1;
        let kinds: Vec<MarketBid> = (0..5).map(|i| market_bid(&s, i)).collect();
        use MarketBid::*;
        assert_eq!(
            kinds,
            [
                OfferForOwnPlayer,
                Reclaim,
                AlreadyRejected,
                RepeatLastOffer,
                EnterBid
            ]
        );
        s.current_manager = 1;
        s.saved_manager = 1;
        assert_eq!(market_bid(&s, 3), EnterBid);
        s.current_manager = 0;
        s.saved_manager = 0;

        let mut rng = Rng::new(6);
        answer_listed_player_offer(&mut s, &mut rng, 0, 2000, true);
        assert_eq!(s.managers[0].balance, 2000);
        assert_eq!(s.squads[4][0].player_id, 6);
        assert_eq!(
            (s.players[5].team, s.players[5].owner_manager),
            (12, NO_OWNER)
        );
        assert_eq!(s.current_manager, 0);
        let mut expected = Rng::new(6);
        for _ in 0..3 {
            expected.rand();
        }
        assert_eq!(rng, expected);

        mark_bid_rejected(&mut s, 2);
        s.current_manager = 1;
        mark_bid_rejected(&mut s, 2);
        assert_eq!(s.squads[4][2].goals[0], 0x80 | 0x02 | 0x01);
    }

    #[test]
    fn listing_a_squad_player() {
        let mut s = state();
        for (i, pid) in [5, 6, 7, 8].into_iter().enumerate() {
            put(&mut s, 0, i, pid).lineup_position = 3;
            s.players[pid as usize].owner_manager = 0;
        }
        s.players[8].owner_manager = 1;
        let mut rng = Rng::new(1);
        assert_eq!(
            list_squad_player(&mut s, &mut rng, 3),
            Err(ListRejected::OnLoan)
        );
        assert_eq!(rng, Rng::new(1));

        let record = s.squads[0][1].clone();
        assert_eq!(list_squad_player(&mut s, &mut rng, 1), Ok(0));
        let mut expected = Rng::new(1);
        for _ in 0..5 {
            expected.rand();
        }
        assert_eq!(rng, expected);
        assert_eq!(
            s.squads[4][0],
            SquadPlayer {
                lineup_position: 0,
                ..record
            }
        );
        assert_eq!(s.squads[0][1].player_id, 7);
        assert_eq!(s.current_manager, 0);

        assert_eq!(list_squad_player(&mut s, &mut rng, 0), Ok(0));
        assert_eq!(list_squad_player(&mut s, &mut rng, 0), Ok(2));
        assert_eq!(
            s.squads[4][..3]
                .iter()
                .map(|r| r.player_id)
                .collect::<Vec<_>>(),
            [5, 6, 7]
        );
        put(&mut s, 0, 0, 20).lineup_position = 0;
        s.players[20].owner_manager = 0;
        assert_eq!(
            list_squad_player(&mut s, &mut rng, 0),
            Err(ListRejected::ThreeListed)
        );
    }

    #[test]
    fn human_bid_range() {
        let mut s = state();
        put(&mut s, 4, 0, 7);
        let mut rng = Rng::new(1);
        for (offer, loan, ok) in [
            (390_000, false, true),
            (389_999, false, false),
            (910_000, false, true),
            (910_001, false, false),
            // A third of 650 000 is 216 666; 60 % of it is 129 999.
            (129_999, true, true),
            (129_998, true, false),
        ] {
            assert_eq!(
                human_bid_allowed(&mut s, &mut rng, 0, offer, loan),
                ok,
                "{offer}"
            );
        }
        assert_eq!(rng, Rng::new(1));
        assert_eq!(s.current_manager, 0);
    }

    /// Market record 0 holds player 7 of club 30, listed by the club.
    fn market_state() -> GameState {
        let mut s = state();
        s.managers[0].balance = 1_000_000;
        let r = put(&mut s, 4, 0, 7);
        r.goals = [1, 2, 3];
        r.cards = [4, 5, 6];
        r.status = 0x80;
        r.skills = [70, 71, 72];
        put(&mut s, 4, 1, 8);
        s.players[7].owner_manager = CLUB_LISTED;
        s.players[8].owner_manager = CLUB_LISTED;
        s.players[7].skills = [10, 11, 12];
        s
    }

    #[test]
    fn buying_from_a_club() {
        let mut s = market_state();
        let mut rng = Rng::new(1);
        let p = purchase_insert(&mut s, &mut rng, 0, false, false, false).unwrap();
        assert_eq!((p.squad_slot, p.club), (0, 30));
        let rec = &s.squads[0][0];
        assert_eq!((rec.player_id, rec.contract_years, rec.loan), (7, 1, 0));
        assert_eq!(rec.skills, [10, 11, 12]);
        assert_eq!(p.salary, rec.market_value);
        assert_eq!(s.players[7].team, 5);

        let mut cancelled = s.clone();
        purchase_cancel(&mut cancelled, p);
        assert_eq!(cancelled.squads[0][0].player_id, 0);
        assert_eq!(cancelled.players[7].team, 30);

        purchase_complete(&mut s, p, 400_000, false, false, false);
        assert_eq!(s.managers[0].balance, 600_000);
        assert_eq!(s.players[7].owner_manager, 0);
        assert_eq!(s.squads[4][0].player_id, 8);
        assert_eq!(s.squads[0][0].goals, [0; 3]);
    }

    #[test]
    fn loan_and_reclaim() {
        let mut s = market_state();
        let p = purchase_insert(&mut s, &mut Rng::new(1), 0, true, false, false).unwrap();
        assert_eq!(s.squads[0][0].loan, LOAN_PRICE_MODE);
        purchase_complete(&mut s, p, 100_000, true, false, false);
        let rec = &s.squads[0][0];
        assert_eq!((rec.contract_years, rec.loan), (1, 30 + 0x80));
        assert_eq!(
            (rec.goals, rec.cards, rec.status),
            ([0; 3], [4, 5, 6], 0x80)
        );
        assert_eq!(rec.market_value, p.salary);
        assert_eq!(rec.skills, [70, 71, 72]);
        assert_eq!(s.players[7].owner_manager, CLUB_LISTED);
        assert_eq!(s.managers[0].balance, 900_000);

        let mut s = market_state();
        s.players[7].owner_manager = 0;
        let p = purchase_insert(&mut s, &mut Rng::new(1), 0, false, true, false).unwrap();
        purchase_complete(&mut s, p, 0, false, true, false);
        assert_eq!(s.squads[0][0], market_state().squads[4][0]);
        assert_eq!(s.managers[0].balance, 1_000_000);
    }

    #[test]
    fn reclaim_may_use_the_25th_place() {
        let mut s = market_state();
        // 23 squad players plus the listed player 7 make 24 of manager 0.
        for i in 0..23 {
            put(&mut s, 0, i, 20 + i as u8);
        }
        s.players[7].owner_manager = 0;
        assert_eq!(
            purchase_insert(&mut s.clone(), &mut Rng::new(1), 1, false, false, false),
            Err(InsertRejected::SquadFull)
        );
        let p = purchase_insert(&mut s, &mut Rng::new(1), 0, false, true, false).unwrap();
        assert_eq!(p.squad_slot, 0);
        assert_eq!(s.squad_player_count(0), 24);
    }

    #[test]
    fn buying_from_another_manager() {
        let mut s = market_state();
        s.manager_count = 2;
        s.managers[1].team = 6;
        s.players[7].owner_manager = 1;
        s.players[7].team = 6;
        let p = purchase_insert(&mut s, &mut Rng::new(1), 0, false, false, true).unwrap();
        assert_eq!(s.squads[0][0].skills, [70, 71, 72]);
        purchase_complete(&mut s, p, 300_000, false, false, true);
        let rec = &s.squads[0][0];
        assert_eq!(
            (rec.status, rec.cards, rec.goals),
            (0x80, [4, 5, 6], [0; 3])
        );
        assert_eq!(s.managers[1].balance, 300_000);
        assert_eq!(s.managers[0].balance, 700_000);
        assert_eq!((s.players[7].owner_manager, s.players[7].team), (0, 5));
    }

    #[test]
    fn free_training_slots_wrap() {
        let mut s = state();
        s.managers[0].training_141 = [5, 5, 5, 6, 99];
        s.managers[0].training_146 = [2, 3, 0, 1];
        assert_eq!(training_free_slots(&s, 0), -1);
        assert_eq!(training_free_slots(&s, 1), 4);
        assert_eq!(training_free_slots(&s, -3), 4);
    }

    #[test]
    fn refresh_slot_pass_skips_the_shifted_record() {
        let mut s = state();
        for (i, pid) in [10, 11, 12].into_iter().enumerate() {
            put(&mut s, 4, i, pid);
        }
        s.players[10].owner_manager = 0;
        s.players[11].owner_manager = CLUB_LISTED;
        s.players[12].owner_manager = CLUB_LISTED;
        // Draw 0 of n = random_range(0, 11) ends the refill before it starts.
        let seed = (0..)
            .find(|&seed| {
                let mut r = Rng::new(seed);
                let n = 11 - r.random_range(0, 11);
                n == 1 && r.random_range(0, 11) == 0
            })
            .unwrap();
        let mut rng = Rng::new(seed);
        transfer_market_refresh(&mut s, &mut rng);
        let ids: Vec<u8> = s.squads[4][..3].iter().map(|r| r.player_id).collect();
        assert_eq!(ids, [10, 12, 0]);
        assert_eq!(s.players[11].owner_manager, NO_OWNER);
        assert_eq!(s.players[12].owner_manager, CLUB_LISTED);
        assert_eq!(s.players[0].owner_manager, NO_OWNER);
        let mut expected = Rng::new(seed);
        expected.rand();
        expected.rand();
        assert_eq!(rng, expected);
    }

    #[test]
    fn refresh_lists_a_player_in_draw_order() {
        let mut s = state();
        s.saved_manager = 2;
        s.managers[0].team = 40;
        // Nine human-listed players leave records 9..=11 free.
        for i in 0..9 {
            put(&mut s, 4, i, 40 + i as u8);
        }
        // Players 1..=150 are taken except 77 of the manager's club 40.
        for p in &mut s.players[1..] {
            p.owner_manager = 0;
        }
        s.players[77].owner_manager = NO_OWNER;
        s.players[77].team = 40;
        s.players[77].age = 25;
        s.clubs[44].strength = [0, 60, 0];
        s.clubs[44].base_strength = [0, 95, 0];
        // Three refills drawn, the first one into a free record.
        let seed = (0..)
            .find(|&seed| {
                let mut r = Rng::new(seed);
                r.random_range(0, 3) == 0 && r.random_range(0, 11) == 11
            })
            .unwrap();
        let mut rng = Rng::new(seed);
        transfer_market_refresh(&mut s, &mut rng);

        let mut e = Rng::new(seed);
        e.rand();
        e.rand();
        while e.random_range(1, 0x96) != 77 {}
        let skill2 = e.random_range(45, 55) as u8;
        // Club 40 is the manager's: a tier-2 club other than 40 is drawn.
        let team = loop {
            let t = e.random_range(38, 57);
            if t != 40 {
                break t as usize;
            }
        };
        let base = (s.clubs[team].strength[1], s.clubs[team].base_strength[1]);
        let mut v = e.random_range(0, 14) + i16::from(base.0) - 10;
        clamp_int(&mut v, 10, 99);
        let skill0 = v as u8;
        let mut v = e.random_range(0, 14) + i16::from(base.1) - 10;
        if e.random_range(0, 4) == 0 {
            v += e.random_range(12, 20);
        }
        clamp_int(&mut v, 10, 99);
        assert_eq!(s.players[77].skills, [skill0, v as u8, skill2]);
        // The insert's five draws and the asking price's spread; the tier
        // search left the refill counter at 2, so the other two refills
        // never happen.
        for _ in 0..6 {
            e.rand();
        }
        assert_eq!(rng, e);
        let rec = &s.squads[4][9];
        assert_eq!((rec.player_id, s.squads[4][10].player_id), (77, 0));
        assert_eq!(
            (s.players[77].owner_manager, s.players[77].team),
            (4, team as u8)
        );
        assert_eq!(rec.market_value % 1000, 0);
        assert_ne!(rec.market_value, value(&s, 9, 1));
        assert_eq!(s.current_manager, 2);
    }

    #[test]
    fn real_saves_refresh_keeps_the_market_consistent() {
        for (path, saved) in save_states() {
            for seed in 1..=8 {
                let mut s = saved.clone();
                transfer_market_refresh(&mut s, &mut Rng::new(seed));
                let ctx = format!("{path:?} seed {seed}");
                assert_eq!(s.current_manager, saved.saved_manager, "{ctx}");
                let ids: Vec<u8> = s.squads[4][..12].iter().map(|r| r.player_id).collect();
                let n = ids.iter().take_while(|&&id| id != 0).count();
                assert!(ids[n..].iter().all(|&id| id == 0), "{ctx} {ids:?}");
                assert!(ids[..n].windows(2).all(|w| w[0] < w[1]), "{ctx} {ids:?}");
                for r in &saved.squads[4][..12] {
                    let owner = saved.players[r.player_id as usize].owner_manager;
                    if r.player_id != 0 && owner < 4 {
                        assert!(
                            ids.contains(&r.player_id),
                            "{ctx}: human-listed player left"
                        );
                    }
                }
                for r in &s.squads[4][..n] {
                    let p = &s.players[r.player_id as usize];
                    assert!(p.owner_manager <= 4, "{ctx}");
                    if p.owner_manager == 4 {
                        assert_eq!(r.market_value % 1000, 0, "{ctx}");
                    }
                }
            }
        }
    }

    #[test]
    fn real_saves_values_of_every_record() {
        for (path, mut s) in save_states() {
            for g in (0..s.manager_count as usize).chain([4]) {
                s.current_manager = g as i8;
                for i in 0..s.squad_player_count(g) {
                    let salary = value(&s, i, 1);
                    assert!(salary >= 200 && salary % 100 == 0, "{path:?} {g} {i}");
                    let fee = player_value_calc(&s, &mut Rng::new(1), i, 4);
                    assert_eq!(fee % 1000, 0, "{path:?} {g} {i}");
                }
            }
        }
    }
}
