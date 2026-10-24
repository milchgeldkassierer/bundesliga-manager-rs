//! The season loop of `BMMAIN.EXE` (segment 2a58) and the season logic of segment 1cb5.
//!
//! [`season_main_loop`] runs the whole game from the first office turn on: per half-week slot
//! ([`season_slot_start`]) the finances, the office turns ([`office_turn_start`]), the slot's
//! match days, the days up to the next slot with their special dates
//! ([`special_date_events`]), and after the season's last day the season-end office turns, the
//! all-time table ([`season_end_eternal_points`]), promotion and relegation, the summer break
//! and the next season's draws. Its screens and dialogs go through [`SeasonUi`]; the match
//! engine's hooks of the season area ([`match_gate_receipts_and_aftermath`],
//! [`match_ticker_events`]) are wired to it inside the loop. [`calendar_compute_date`]
//! (segment 1310) turns the day counter into a date.
//!
//! Also here: the rest of segment 1cb5: the weekly events ([`random_weekly_events`]), the
//! season end of every manager ([`season_end_messages`]) with the spread of the persons over
//! the tiers ([`season_rebalance_player_clubs`]), the tier ranking by strength and the random
//! club pick for free pool players that the new game needs, and the weekly ban countdown
//! ([`player_suspension_countdown`]).
//!
//! Full documentation with addresses: `docs/re/logic/season.md` §1, §3 and §4.

use bmp_formats::savegame::HighScoreTable;

use crate::ai::{league_strength_drift, match_attendance_estimate, squad_recalc_team_strength};
use crate::cups::{self, NOT_IN_CUP};
use crate::finance::{DAYS_IN_MONTH, FinanceEvent, finance_daily_processing, message_queue_add};
use crate::highscore::{highscore_build_entry, highscore_game_end_insert};
use crate::matchsim::{
    MATCH_DAY_MANAGER, MatchHooks, TickerEvent, has_rescheduled_match_today,
    league_postpone_matches_weather, match_pick_scorer, matchday_play, matchday_play_rescheduled,
};
use crate::newgame::manager_compute_wage_budget;
use crate::news::{
    self, EVENT_CHANCE, EVENT_GOAL, EVENT_RED_CARD, EVENT_YELLOW_CARD, MessageArg, NO_FREE_SLOT,
    NewsEnv, Newspaper, OPPONENT, message_delete, message_post, team_slot_free_index,
};
use crate::rng::{
    NumberFormat, Rng, clamp_int, format_long_thousands, ldiv, lmul, long_abs_diff, lrem, uldiv,
};
use crate::runtime::{LiveMatch, NO_BOOKING, Runtime, SEASON_START_DAY, TIER_FIRST};
use crate::squad::{
    auto_lineup, count_players_of_owner, find_player_by_id, player_init_random_attrs,
    squad_count_listed, squad_count_starters, squad_insert_player, squad_remove_player,
};
use crate::state::{
    GameState, Layout, MANAGER_SLOTS, NO_OWNER, NewsRecord, RESCHEDULED_GAMES_DAY, SQUAD_SCAN,
    TRANSFER_LIST, TRANSFER_LIST_SCAN, strcpy,
};
use crate::stats::{
    FORM_PLACEHOLDER, advertising_offers_generate, pick_team_for_player, player_injure,
    player_pool_season_shuffle, player_value_percent, top_scorer_plays_for_current_manager,
};
use crate::transfer::{player_value_calc, transfer_market_refresh};

/// DS:53d0 / DS:53d4: day of month and 1-based month of the special dates: Christmas and the
/// birthdays of the two programmers, Jens Onnen and Werner Krahe.
const SPECIAL_DATES: [(i8, i8); 3] = [(24, 12), (12, 11), (19, 4)];
/// The last day of the season (`DS:224c + 0x142`): the loop ends the season after it.
const SEASON_LAST_DAY: i32 = SEASON_START_DAY as i32 + 0x142;
/// The office loop of the season end (OFFICE_A) records a manager's season once the day counter
/// is past this day; the normal office loop (OFFICE_B) uses [`SEASON_LAST_DAY`].
const SEASON_END_OFFICE_DAY: i32 = 0x140;
/// `DS:224d * 7 + 0x82` and `0xcf - DS:224d * 14`: the days strictly between them are the winter
/// break (DS:224d is 0 in the shipped data).
const WINTER_BREAK: (i32, i32) = (0x82, 0xcf);
/// [`crate::state::Manager::aggression`] of a manager who cannot field a team.
const CANNOT_FIELD_A_TEAM: u8 = 100;
/// DS:2256: the fixed attendance of the DFB-Pokal final.
const FIXED_ATTENDANCE: i32 = 76_000;
/// Gate money of each club in the DFB-Pokal final.
const FIXED_GATE_MONEY: i32 = 0x81e20;
/// DS:5390: ticket price on neutral ground, by the home club's tier.
const NEUTRAL_TICKET_PRICE: [i16; 4] = [16, 14, 10, 8];

/// Converts a day number into day of month, month and year: `n == 0` takes the
/// season day counter, otherwise `n` is a fixture index (two per week).
///
/// Origin: BMMAIN.EXE 1310:1190 (calendar_compute_date)
pub fn calendar_compute_date(state: &mut GameState, n: u16) {
    let mut day: i32;
    state.month = 0;
    if n == 0 {
        day = state.day_counter;
    } else {
        let n = if n == 0x78 { 0 } else { n };
        day = i32::from((n >> 1).wrapping_mul(7));
        if n & 1 != 0 {
            day += 4;
        } else {
            day += i32::from(SEASON_START_DAY);
        }
    }
    state.year = i32::from(state.start_year);
    day += 0xd2;
    while day > 0x16d {
        day -= 0x16d;
        state.year += 1;
    }
    loop {
        let len = i32::from(DAYS_IN_MONTH[state.month as usize]);
        if len >= day {
            break;
        }
        day -= len;
        state.month += 1;
        if state.month > 11 {
            state.month = 0;
        }
    }
    state.day_of_month = day;
}

/// The number of active human managers as the loops of the original count them (FAR:304a is
/// a signed byte).
fn manager_loop_count(state: &GameState) -> i8 {
    state.active_managers().len() as i8
}

/// The top of a pass of the season loop, one pass per half-week slot (`season_week`): the
/// date, every manager's finances of the day, the weekly strength drift and, on a slot with a
/// competition, the weekly squad update. `finance_events` gets each manager's
/// [`FinanceEvent`]s of the day. Returns the slot's competition bits
/// ([`GameState::week_competitions`], the "skip" value 9 read as 0).
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, loop top 2a58:3194)
pub fn season_slot_start(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    finance_events: &mut dyn FnMut(&GameState, u8, &[FinanceEvent]),
) -> u8 {
    calendar_compute_date(state, 0);
    state.loop_month = state.month as u8;
    let mut wf = state.week_competitions[usize::from(state.season_week)];
    if wf == 9 {
        // TODO(re): season.md §7 Q1, which module writes the value 9.
        wf = 0;
    }
    for m in 0..manager_loop_count(state) {
        state.current_manager = m;
        let events = finance_daily_processing(state, rng, fmt);
        finance_events(state, m as u8, &events);
    }
    league_strength_drift(state, rt, rng, 1);
    if wf != 0 {
        weekly_squad_update(state, rt, rng);
    }
    wf
}

/// Every manager's lineup flags, automatic lineup and club strength of the week.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, after the loop top and at 2a58:36f9)
fn weekly_squad_update(state: &mut GameState, rt: &Runtime, rng: &mut Rng) {
    for m in 0..manager_loop_count(state) {
        state.current_manager = m;
        let mu = m as usize;
        state.lineup_state[mu][0] = state.lineup_state[mu][1];
        auto_lineup(state, rt.lineup_rules);
        squad_recalc_team_strength(state, rt, rng, m as u8, false);
    }
}

/// The start of a normal office turn of the current manager: it becomes the saved manager,
/// and with chance `1 / (managers + 3)` the transfer market changes.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, OFFICE_B 2a58:3b26)
pub fn office_turn_start(state: &mut GameState, rng: &mut Rng) {
    state.saved_manager = state.current_manager;
    if rng.random_range(0, i16::from(state.manager_count) + 3) == 0 {
        transfer_market_refresh(state, rng);
    }
}

/// Counts down the ban of record `slot` of squad `group` on the weekly day (bit 1 of the
/// status) and lifts it at 0: the status loses both ban bits and the current manager's lineup
/// is set again.
///
/// Origin: BMMAIN.EXE 1cb5:2b88 (player_suspension_countdown)
pub fn player_suspension_countdown(state: &mut GameState, rt: &Runtime, group: usize, slot: usize) {
    let weekly = lrem(state.day_counter - i32::from(SEASON_START_DAY), 7) == 0;
    let s = &mut state.squads[group][slot];
    if s.status & 2 == 2 && s.suspended != 0 && weekly {
        s.suspended -= 1;
    }
    if s.status & 3 != 0 && s.suspended == 0 {
        s.status &= 0xfc;
        auto_lineup(state, rt.lineup_rules);
    }
}

/// DS:0292: the share (in percent) each of the four training areas gives to the three skills.
const TRAINING_MIX: [[i8; 3]; 4] = [[80, 5, 15], [30, 40, 30], [20, 25, 55], [5, 65, 30]];
/// [`crate::state::SquadPlayer::status`] bit of a club's offer for a listed player.
const OFFER_FOR_LISTED: u8 = 0x80;
/// [`crate::state::SquadPlayer::status`] bit of a club's offer for a squad player.
const OFFER_FOR_SQUAD_PLAYER: u8 = 0x40;
/// [`crate::state::SquadPlayer::negotiation`] bit: the player leaves when his contract ends.
const LEAVES: u8 = 0x80;
/// Row of [`GameState::team_slots`] the weekly event messages go to.
const EVENT_MESSAGE_ROW: u8 = 2;

/// Whether `day` lies in the winter break (`DS:224d` is 0).
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, the tests at 1cb5:2069, 1cb5:2134 and
/// 1cb5:24a9)
fn in_winter_break(day: i32) -> bool {
    day > WINTER_BREAK.0 && day < WINTER_BREAK.1
}

/// The weekly events of the current manager (FAR:304a), once per half-week after the days
/// were advanced, up to day 321: manager 0 keeps the transfer list and the message slots
/// up; other clubs bid for the players the manager listed; hooligans damage the stadium and
/// the stadium's comfort may be graded down; during the season the manager's players get
/// injured in training, want to leave, get offers from abroad or from other clubs; and
/// training develops every player's fitness, skills and form. Messages go to the inbox.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events)
pub fn random_weekly_events(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
) {
    if state.day_counter > SEASON_LAST_DAY - 1 {
        return;
    }
    let m = state.current_manager as u8;
    if m == 0 {
        transfer_list_upkeep(state, rng);
    }
    listed_player_bids(state, rt, rng, fmt, m);
    hooligan_damage(state, rng, fmt, m);
    comfort_downgrade(state, rng, fmt, m);
    if state.day_counter < SEASON_LAST_DAY {
        squad_week(state, rt, rng, fmt, m);
        squad_player_bids(state, rng, fmt, m);
    }
    training_week(state, rt, rng, m);
    auto_lineup(state, rt.lineup_rules);
}

/// Manager 0's part: the listed players lose fitness and their offers may lapse; the message
/// slots count down and delete their message at 0.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:1417..1508)
fn transfer_list_upkeep(state: &mut GameState, rng: &mut Rng) {
    let mut i = 0;
    while i < usize::from(squad_count_listed(state, TRANSFER_LIST)) {
        let s = &mut state.squads[TRANSFER_LIST][i];
        if s.exhaustion > 0x38 {
            s.exhaustion = s.exhaustion.wrapping_sub(rng.random_range(3, 9) as u8);
        }
        drop_lapsed_offer(state, rng, TRANSFER_LIST, i);
        i += 1;
    }
    for row in 0..3 {
        for slot in 0..10 {
            let count = &mut state.team_slots[row][slot];
            if *count != 0 {
                *count -= 1;
                if *count == 0 {
                    message_delete(state, state.message_pointers[row][slot]);
                }
            }
        }
    }
}

/// With chance 1/4 a club's offer for record `i` of `group` lapses and its message goes.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:145d..14a4 and 1cb5:1a41..1a88)
fn drop_lapsed_offer(state: &mut GameState, rng: &mut Rng, group: usize, i: usize) {
    let s = &state.squads[group][i];
    if rng.random_range(0, 3) == 0 && s.status > 0x1f && s.message_ptr != 0 {
        let msg = s.message_ptr;
        state.squads[group][i].status &= 0x1f;
        message_delete(state, msg);
        state.squads[group][i].message_ptr = 0;
    }
}

/// The bids of other clubs for the players manager `m` put on the transfer list: a refused
/// bid of `m` alone may be forgotten, the bans count down, and a club bids with chance
/// `value / 180` (value in 10 000 DM) times 2/3.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:1508..16ab)
fn listed_player_bids(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    m: u8,
) {
    let bit = 1u8.wrapping_shl(u32::from(m));
    for i in 0..usize::from(squad_count_listed(state, TRANSFER_LIST)) {
        state.saved_manager = m as i8;
        let s = &mut state.squads[TRANSFER_LIST][i];
        if u16::from(s.goals[0] & 0x7f) == 1u16 << m && rng.random_range(0, 2) == 0 {
            s.goals[0] ^= bit;
        }
        let pid = usize::from(s.player_id);
        if state.players[pid].owner_manager != m {
            continue;
        }
        player_suspension_countdown(state, rt, TRANSFER_LIST, i);
        if state.squads[TRANSFER_LIST][i].negotiation & LEAVES != 0 {
            continue;
        }
        state.current_manager = TRANSFER_LIST as i8;
        let q = ldiv(player_value_calc(state, rng, i as u8, 0), 10_000) as i16;
        if rng.random_range(0, 180) < q
            && rng.random_range(0, 2) != 0
            && state.squads[TRANSFER_LIST][i].message_ptr == 0
        {
            state.squads[TRANSFER_LIST][i].status |= OFFER_FOR_LISTED;
            let club = pick_team_for_player(state, rng, q as u8, false);
            state.squads[TRANSFER_LIST][i].sale_club = club;
            let msg = post_offer(state, rng, fmt, club, pid);
            state.squads[TRANSFER_LIST][i].message_ptr = msg;
        }
        state.current_manager = state.saved_manager;
    }
}

/// "<club>^ist an <player>^interessiert." Returns the message's identity.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, the posts at 1cb5:167d and 1cb5:1f11)
fn post_offer(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    club: u8,
    pid: usize,
) -> u32 {
    let club = state.clubs[usize::from(club)].name().to_vec();
    let player = state.players[pid].name().to_vec();
    let args = [MessageArg::Text(&club), MessageArg::Text(&player)];
    message_post(state, rng, fmt, false, 1, &args).pointer
}

/// Hooligans of the last home game (flag set at kick-off) damage the stadium: 2..=5 times
/// (four times the seats plus half the standing places) DM, in thousands; a stadium in a
/// good condition may lose a grade.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:16ab..18f4)
fn hooligan_damage(state: &mut GameState, rng: &mut Rng, fmt: &mut NumberFormat, m: u8) {
    let r = &mut state.managers[usize::from(m)];
    if r.value_13e & 1 == 0 {
        return;
    }
    let k = i32::from(rng.random_range(2, 5));
    let places = (r.stadium[1][0] << 2).wrapping_add(r.stadium[0][0] >> 1);
    let damage = lmul(ldiv(lmul(k, places), 1000), 1000);
    r.balance = r.balance.wrapping_sub(damage);
    r.value_13e ^= 1;
    let mut l3 = b"von ".to_vec();
    // DS:224d is 0 (no currency prefix) and DS:2252 == DS:224e.
    l3.extend(format_long_thousands(damage, *fmt));
    l3.extend_from_slice(b" DM an.");
    if r.stadium[5][0] > 1 {
        let down = if r.balance > 2_000_000 {
            rng.random_range(0, 1) == 0 || rng.random_range(0, 2) == 0
        } else {
            rng.random_range(0, 2) == 0
        };
        if down {
            let r = &mut state.managers[usize::from(m)];
            r.stadium[5][0] = r.stadium[5][0].wrapping_sub(1);
        }
    }
    let lines: [&[u8]; 3] = [
        b"Randalierer im Stadion",
        b"richteten einen Sachschaden",
        &l3,
    ];
    message_queue_add(state, rng, fmt, lines, EVENT_MESSAGE_ROW, 0);
}

/// A Bundesliga stadium's comfort is graded down with chance `1 / (443 - 52 * comfort)`.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:18f4..19e8)
fn comfort_downgrade(state: &mut GameState, rng: &mut Rng, fmt: &mut NumberFormat, m: u8) {
    let r = &state.managers[usize::from(m)];
    let hi = 0x1bai16.wrapping_add((r.stadium[6][0] as i16).wrapping_mul(-0x34));
    if rng.random_range(0, hi) == 0 && r.stadium[6][0] > 1 && r.league == 0 {
        let r = &mut state.managers[usize::from(m)];
        r.stadium[6][0] = r.stadium[6][0].wrapping_sub(1);
        let lines: [&[u8]; 3] = [
            b"In der neuen Komfortbewertung",
            b"wird ihr Stadion um eine Note",
            b"schlechter beurteilt.",
        ];
        message_queue_add(state, rng, fmt, lines, EVENT_MESSAGE_ROW, 0);
    }
}

/// Per player of manager `m` during the season: a training injury, the wish to leave when the
/// contract ends, the bans, lapsed offers, the negotiation cool-down, and offers from abroad
/// to extend a one-year contract.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:19e8..1e88)
fn squad_week(state: &mut GameState, rt: &Runtime, rng: &mut Rng, fmt: &mut NumberFormat, m: u8) {
    let g = usize::from(m);
    let level = i16::from(state.level as i8);
    let match_slot = state.week_competitions[usize::from(state.season_week)] != 0;
    for i in 0..usize::from(squad_count_listed(state, g)) {
        let s = &state.squads[g][i];
        let mut risk = (if match_slot { 0 } else { 25 } - i16::from(s.exhaustion) + 160) * 2;
        if risk < 40 {
            risk = 40;
        }
        if s.suspended == 0 && s.status == 0 && rng.random_range(0, level * 30 + risk + 80) == 0 {
            let injury = player_injure(state, rng, g, i);
            let name = state.players[usize::from(state.squads[g][i].player_id)]
                .name()
                .to_vec();
            let lines: [&[u8]; 3] = [b"Verletzung im Training:", &name, &injury];
            message_queue_add(state, rng, fmt, lines, EVENT_MESSAGE_ROW, 0);
            auto_lineup(state, rt.lineup_rules);
        }
        let unrest = player_value_percent(state, &state.squads[g][i]);
        let s = &state.squads[g][i];
        if i32::from(rng.random_range(0x20, 0x2d)) < unrest
            && s.negotiation & LEAVES == 0
            && s.negotiation < 100
        {
            state.squads[g][i].negotiation |= LEAVES;
            state.saved_manager = m as i8;
            let slot = team_slot_free_index(state, m);
            let name = state.players[usize::from(state.squads[g][i].player_id)]
                .name()
                .to_vec();
            let msg = message_post(state, rng, fmt, false, 4, &[MessageArg::Text(&name)]);
            if slot == NO_FREE_SLOT {
                message_delete(state, msg.pointer);
            } else {
                state.message_pointers[g][usize::from(slot)] = msg.pointer;
                state.team_slots[g][usize::from(slot)] = 3;
            }
        }
        if state.squads[g][i].suspended != 0 {
            state.squads[g][i].lineup_position = 0;
            auto_lineup(state, rt.lineup_rules);
        }
        drop_lapsed_offer(state, rng, g, i);
        let s = &state.squads[g][i];
        if s.negotiation > 99 && s.negotiation & LEAVES == 0 {
            if rng.random_range(0, 5) == 0 && s.message_ptr != 0 {
                let msg = s.message_ptr;
                state.squads[g][i].negotiation = rng.random_range(9, 17) as u8;
                message_delete(state, msg);
                state.squads[g][i].message_ptr = 0;
            }
        } else if s.negotiation != 0 && s.negotiation & LEAVES == 0 {
            state.squads[g][i].negotiation -= 1;
        }
        if state.squads[g][i].loan != 0 {
            continue;
        }
        offer_from_abroad(state, rng, fmt, g, i);
    }
}

/// A club from abroad offers a player on a one-year contract to extend it by 2..=4 years,
/// the likelier the closer he is to 25, the better his skills and the fewer his value terms.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:1cee..1e88)
fn offer_from_abroad(
    state: &mut GameState,
    rng: &mut Rng,
    fmt: &mut NumberFormat,
    g: usize,
    i: usize,
) {
    let s = &state.squads[g][i];
    let avg = (i16::from(s.skills[0]) + i16::from(s.skills[1]) + i16::from(s.skills[2])) / 3;
    let age = state.players[usize::from(s.player_id)].age;
    let mut a = long_abs_diff(0, i32::from(25 - i16::from(age))) as i16;
    if a > 8 {
        a = 8;
    }
    let a = (ldiv(-100 * i32::from(a), 8) as i16).wrapping_add(100);
    let terms = s.value_terms[0]
        .wrapping_add(s.value_terms[1])
        .wrapping_add(s.value_terms[2]);
    let mut b = (terms / 36) as i16;
    if b > 5 {
        b = 5;
    }
    let mut c = ldiv(40 * i32::from(a) + 60 * i32::from(avg), 100) as i16;
    c = c.wrapping_add(ldiv(i32::from(c.wrapping_mul(b)) * 10, -100) as i16);
    if rng.random_range(0, c) == 0
        && rng.random_range(0, 3) == 0
        && s.message_ptr == 0
        && s.negotiation & LEAVES == 0
        && s.contract_years == 1
    {
        let r = rng.random_range(0, 100);
        let years: i32 = match r {
            91.. => 4,
            71.. => 3,
            _ => 2,
        };
        let contract = s.contract_years;
        state.squads[g][i].negotiation = years as u8 + 100;
        let name = state.players[usize::from(state.squads[g][i].player_id)]
            .name()
            .to_vec();
        let args = [
            MessageArg::Text(&name),
            MessageArg::Number(i32::from(contract)),
            MessageArg::Number(years),
        ];
        state.squads[g][i].message_ptr = message_post(state, rng, fmt, false, 0, &args).pointer;
    }
}

/// A rare bid of another club for a player of manager `m`: chance `value / 200` (value in
/// 10 000 DM) times 1/451; for a player worth more than 900 000 DM, with chance 1/13 a club
/// from abroad.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:1e88..1fe4)
fn squad_player_bids(state: &mut GameState, rng: &mut Rng, fmt: &mut NumberFormat, m: u8) {
    let g = usize::from(m);
    for i in 0..usize::from(squad_count_listed(state, g)) {
        let s = &state.squads[g][i];
        if s.player_id == 0 || s.loan != 0 || s.negotiation & LEAVES != 0 {
            continue;
        }
        let q = ldiv(player_value_calc(state, rng, i as u8, 0), 10_000) as i16;
        if rng.random_range(0, 200) < q
            && rng.random_range(0, 450) == 0
            && state.squads[g][i].message_ptr == 0
        {
            state.squads[g][i].status |= OFFER_FOR_SQUAD_PLAYER;
            let foreign = q > 90 && rng.random_range(0, 12) == 0;
            let club = pick_team_for_player(state, rng, q as u8, foreign);
            state.squads[g][i].sale_club = club;
            let pid = usize::from(state.squads[g][i].player_id);
            state.squads[g][i].message_ptr = post_offer(state, rng, fmt, club, pid);
        }
    }
}

/// The week's training of manager `m`'s players: the training plan's mix of the four areas
/// moves each skill towards a target drawn around 37 (36 on the easiest level), fitness
/// follows the intensity (at least 6 in the winter break), and the form follows its trend
/// until the trend runs out and is drawn anew.
///
/// Origin: BMMAIN.EXE 1cb5:13bd (random_weekly_events, 1cb5:1fe4..25c8)
fn training_week(state: &mut GameState, rt: &Runtime, rng: &mut Rng, m: u8) {
    let g = usize::from(m);
    let level = state.level as i8;
    let base = 0x25 - i16::from(level == 5);
    let r = &state.managers[g];
    let plan: [i16; 4] = std::array::from_fn(|i| 20 * i16::from(r.training_141[i]));
    let mix: [i16; 3] = std::array::from_fn(|j| {
        let sum = (0..4).fold(0i16, |acc, i| {
            acc.wrapping_add(i16::from(TRAINING_MIX[i][j]).wrapping_mul(plan[i]))
        });
        sum / 400
    });
    let day = state.day_counter;
    let winter = in_winter_break(day);
    for i in 0..usize::from(squad_count_listed(state, g)) {
        player_suspension_countdown(state, rt, g, i);
        let r = &state.managers[g];
        let (intensity, coaches) = (i16::from(r.training_141[4]), r.training_146);
        let s = &mut state.squads[g][i];
        let mut inten = intensity;
        if s.exhaustion > 0x32 {
            s.exhaustion = s.exhaustion.wrapping_sub(rng.random_range(3, 7) as u8);
        }
        if winter && inten < 6 {
            inten = 6;
        }
        s.exhaustion = s
            .exhaustion
            .wrapping_add(((inten - 5) as u8).wrapping_shl(1));
        if s.exhaustion > 0x82 && inten < 6 {
            s.exhaustion = s.exhaustion.wrapping_sub(rng.random_range(1, 3) as u8);
        }
        inten = inten.wrapping_mul(20);
        s.exhaustion = s.exhaustion.clamp(0x3c, 0x96);
        if s.exhaustion > 0x87 {
            let l = i16::from(level);
            inten -= rng.random_range(70 - 13 * l, 5 * (18 - l));
            if inten < 10 {
                inten = 10;
            }
        }
        let band = usize::from(state.players[usize::from(s.player_id)].position_band());
        if band == 0 {
            inten += 30;
        }
        for (j, mix) in mix.into_iter().enumerate() {
            let s = &mut state.squads[g][i];
            let mut t = mix.wrapping_mul(inten) / 100;
            t = t.wrapping_mul(i16::from(s.tendency)) / 50;
            let f = 10 * i16::from(coaches[band]) + 75 - rng.random_range(0, 3);
            t = t.wrapping_mul(f) / 100;
            let mut v = i16::from(s.skills[j]);
            let mut target = i16::from(rng.random_range(0, 4) == 4);
            let mut dir = 0i16;
            let late = winter || day > SEASON_LAST_DAY;
            if level > 2 {
                if late {
                    if rng.random_range(0, 2) == 0 {
                        target += 1;
                    }
                    target = rng.random_range(base - target - 1, base + 2);
                    s.exhaustion = s.exhaustion.wrapping_add(rng.random_range(0, 2) as u8);
                } else {
                    target = rng.random_range(base - target - 2, base + 2);
                }
            } else {
                target = rng.random_range(base - target - 2, base + 2);
            }
            if level < 2 && late {
                s.exhaustion = s.exhaustion.wrapping_add(rng.random_range(0, 2) as u8);
            }
            if t < target - 8 && rng.random_range(0, 1) != 0 {
                dir = -1;
                target = (target - t + 8) / 11;
            } else if t > target + 8 && rng.random_range(0, 1) != 0 {
                dir = 1;
                target = (t - target + 8) / 11;
            }
            if rng.random_range(0, 3) == 0 && dir != 0 {
                if target > 2 {
                    target = rng.random_range(2, 3);
                }
                if target != 0 {
                    v += rng.random_range(1, target) * dir;
                    if j < 2 {
                        clamp_int(&mut v, 1, 99);
                    } else {
                        clamp_int(&mut v, 45, 55);
                    }
                    s.skills[j] = v as u8;
                }
            }
        }
        let s = &mut state.squads[g][i];
        let mut form = i16::from(s.tendency);
        if rng.random_range(0, 3) != 0 {
            let step = i16::from((s.traits & 0x70) >> 4);
            if s.traits & 0x80 != 0 {
                form += step;
            } else {
                form -= step;
            }
        }
        let left = (s.traits & 0x0f).wrapping_sub(1);
        s.traits = (s.traits & 0xf0).wrapping_add(left);
        if left == 0 {
            player_init_random_attrs(rng, s);
        } else {
            clamp_int(&mut form, 30, 68);
            s.tendency = form as u8;
        }
    }
}

/// What a special date brought, for the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialDate {
    /// 0 = Christmas, 1 = Jens Onnen's birthday, 2 = Werner Krahe's birthday.
    pub idx: u8,
    /// Christmas: the bonus every Bundesliga club paid its players (half in the 2. Bundesliga,
    /// a third in the Amateur-Oberliga), if there was one.
    pub christmas_bonus: Option<i32>,
}

/// A special date happens with chance 1/4. At Christmas, with chance 1/2, every manager pays
/// their players a bonus of `random_range(15, 85) * 10 000` DM, divided by the tier + 1. `None`:
/// nothing happens and nothing is shown.
///
/// Origin: BMMAIN.EXE 2a58:2a06 (special_date_events), without the screen
pub fn special_date_events(state: &mut GameState, rng: &mut Rng, idx: u8) -> Option<SpecialDate> {
    if rng.random_range(0, 3) != 0 {
        return None;
    }
    // The birthdays need DS:2252 == DS:224e (the DM version), which the shipped data has.
    let mut christmas_bonus = None;
    if idx == 0 && rng.random_range(0, 1) != 0 {
        let bonus = i32::from(rng.random_range(15, 85)) * 10_000;
        let n = state.active_managers().len();
        for m in &mut state.managers[..n] {
            m.balance = m.balance.wrapping_sub(ldiv(bonus, i32::from(m.league) + 1));
        }
        christmas_bonus = Some(bonus);
    }
    Some(SpecialDate {
        idx,
        christmas_bonus,
    })
}

/// The special date of today's date ([`GameState::day_of_month`], [`GameState::month`]), if any.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, inlined twice in `DAY_ADVANCE`)
fn check_special_dates<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rng: &mut Rng,
    ui: &mut U,
) -> Result<(), U::Stop> {
    for (i, &(day, month)) in SPECIAL_DATES.iter().enumerate() {
        if i32::from(day) == state.day_of_month
            && i32::from(month) - 1 == state.month
            && let Some(date) = special_date_events(state, rng, i as u8)
        {
            ui.special_date(state, date)?;
        }
    }
    Ok(())
}

/// Adds the season to the all-time table ([`crate::state::TeamTable::prestige`]) of the 64
/// German clubs: 69 points down by tier and final rank (none outside the three leagues),
/// 20/30/40/50 for the DFB-Pokal and European cup winners, and 12 for the champion, 3 for ranks
/// 2..5 of the Bundesliga and for the other champions. The caller shows the "SAISONENDE" title first.
///
/// Origin: BMMAIN.EXE 2a58:4597 (season_end_eternal_points), without the title
pub fn season_end_eternal_points(state: &mut GameState, rt: &Runtime) {
    let last_league_club = i16::from(rt.tier_club_ranges[2][1]);
    let [second, third] = rt.tier_bounds.map(i16::from);
    let winners = [
        (state.winner_07ac, 20),
        (state.winner_07af, 30),
        (state.winner_07ae, 40),
        (state.winner_07ad, 50),
    ];
    for t in 0..0x40i16 {
        let table = &mut state.team_tables[t as usize];
        let rank = i16::from(table.table_position);
        let mut v = (i16::from(t < third) + i16::from(t < second)) * 25 - rank + 19;
        if last_league_club < t {
            v = 0;
        }
        table.prestige = table.prestige.wrapping_add(i32::from(v));
        for (winner, bonus) in winners {
            if u16::from(winner).wrapping_sub(t as u16) == 1 {
                table.prestige = table.prestige.wrapping_add(bonus);
            }
        }
        let b = if t < second {
            match rank {
                0 => 12,
                1..5 => 3,
                _ => 0,
            }
        } else if rank == 0 && t <= last_league_club {
            3
        } else {
            0
        };
        table.prestige = table.prestige.wrapping_add(b);
    }
}

/// Manager `m`'s season into their career totals and season log; the attendance records start
/// again. A full log (50 seasons) drops its oldest entry. The entry holds the global rank,
/// the tier, the DFB-Pokal stage (+0x80 for the winner) and the stage of a European cup
/// `(cup << 3) + stage` (+0x80 for the winner); the titles and cup wins count up.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, 2a58:3763..3ab4)
fn season_record_manager<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &Runtime,
    m: u8,
    ui: &mut U,
) -> Result<(), U::Stop> {
    let mu = usize::from(m);
    let team = state.managers[mu].team;
    let t = &state.team_tables[usize::from(team)];
    let r = &mut state.managers[mu];
    for k in 0..2 {
        let [a, w, g1, g2, d, l] = [
            t.points[k],
            t.played[k],
            t.goals_for[k],
            t.goals_against[k],
            t.wins[k],
            t.losses[k],
        ]
        .map(u16::from);
        let lost = w.wrapping_mul(2).wrapping_sub(a);
        let draws = w.wrapping_sub(l).wrapping_sub(d);
        for (field, add) in [a, g1, g2, lost, w, d, l, draws].into_iter().enumerate() {
            r.career_stats[field][k] = r.career_stats[field][k].wrapping_add(add);
        }
    }
    r.records[1] = 0;
    r.values_13a[0] = 0;
    r.records[0] = 0;
    r.records[2] = 99_999;
    let mut e = state.logged_rounds as i16;
    if e > 0x31 {
        r.round_log.copy_within(1..0x32, 0);
        r.values_13a[1] = r.values_13a[1].wrapping_add(1);
        e = 0x31;
    }
    let e = e as usize;
    let tier = usize::from(r.league);
    let pos = r.position_history[rt.tier_round_count[tier] as usize - 1];
    r.round_log[e][0] = pos.wrapping_add(1);
    if pos == 0 && tier == 0 {
        r.titles = r.titles.wrapping_add(1);
        ui.championship_winner(state, m)?;
    }
    let r = &mut state.managers[mu];
    r.round_log[e][0] = r.round_log[e][0].wrapping_add(TIER_FIRST[tier]);
    r.round_log[e][1] = tier as u8;
    let won = |winner: u8| u16::from(team).wrapping_add(1) == u16::from(winner);
    r.round_log[e][2] = r.values_132[0]
        .wrapping_add(1)
        .wrapping_add(if won(state.winner_07ac) { 0x80 } else { 0 });
    if won(state.winner_07ac) {
        r.cup_wins[0] = r.cup_wins[0].wrapping_add(1);
    }
    for (j, winner) in [state.winner_07ad, state.winner_07ae, state.winner_07af]
        .into_iter()
        .enumerate()
    {
        let cup = j + 1;
        let stage = r.values_132[cup];
        if stage != 0 && stage != NOT_IN_CUP {
            r.round_log[e][3] = ((cup as u8) << 3).wrapping_add(stage);
            if won(winner) {
                r.round_log[e][3] = r.round_log[e][3].wrapping_add(0x80);
                r.cup_wins[cup] = r.cup_wins[cup].wrapping_add(1);
            }
        }
    }
    Ok(())
}

/// The fine of a manager whose club could not field a team: 200 000 DM. The dialog "Ihr Spiel
/// wird mit 0:2 gewertet" is the caller's.
///
/// Origin: BMMAIN.EXE 2a58:2051 (match_forfeit_penalty), without the dialog
pub fn match_forfeit_penalty(state: &mut GameState, m: u8) {
    let manager = &mut state.managers[usize::from(m)];
    manager.balance = manager.balance.wrapping_sub(200_000);
}

/// Whether manager `m` forfeits a match because their club cannot field a team; from the 10th
/// of June on (the season's last days) there is no forfeit.
///
/// Origin: BMMAIN.EXE 2a58:20b2 (match_gate_receipts_and_aftermath, the forfeit test)
fn forfeits(state: &GameState, m: usize) -> bool {
    state.managers[m].aggression == CANNOT_FIELD_A_TEAM
        && !(state.month == 5 && state.day_of_month >= 10)
}

/// Gate money, forfeits and the live-match bookkeeping of pairing `home` vs `away` before
/// kick-off (the [`MatchHooks::match_gate_receipts_and_aftermath`] contract): `cup_mask` is
/// the match day's cup mask (0 on a league day), `comp` the league tier or cup row, `slot` the
/// even pairing slot and `league_mask` the league day's tier mask. Returns 0, or 1 / 2 when the
/// home / away human manager forfeits; `forfeit_dialog` shows the forfeit before the fine.
///
/// For each human manager taking part in the day: the home manager books the attendance
/// ([`crate::ai::match_attendance_estimate`]; 76 000 and 532 000 DM to both clubs in the
/// DFB-Pokal final) and the gate money; a cup visitor from a higher tier draws extra
/// spectators up to the stadium capacity. In the DFB-Pokal the away manager gets half the gate,
/// on neutral ground their own estimate at a drawn ticket price. Then, for the human match's
/// first manager: the bans of league matches count down, every player who played gets
/// exhausted by `random_range(2, 4) + 6` (one less for a player whose first skill is higher
/// than the second), the card limits, bookings and the newspaper record start, and a home
/// manager's fans may turn into hooligans.
///
/// Not kept: FAR:3052 (managers per live match).
///
/// Origin: BMMAIN.EXE 2a58:20b2 (match_gate_receipts_and_aftermath)
#[allow(clippy::too_many_arguments)]
pub fn match_gate_receipts_and_aftermath(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    home: u8,
    away: u8,
    cup_mask: u8,
    comp: u8,
    slot: u8,
    league_mask: u8,
    forfeit_dialog: &mut dyn FnMut(&GameState, u8),
) -> u8 {
    let gate_div: i32 = if cup_mask != 0 && comp == 0 { 2 } else { 1 };
    let mut fixed = false;
    let mut att: i32 = 0;
    let mut last: u8 = 0xff;
    for m in 0..state.active_managers().len() {
        let mi = m as u8;
        let league = state.managers[m].league;
        if !(cup_mask != 0 || (league_mask >> league) & 1 != 0 || league_mask == 0) {
            continue;
        }
        if cup_mask == 1 && state.event_state[0] > 4 {
            att = FIXED_ATTENDANCE;
            fixed = true;
        }
        let tier_away = rt.tier_of_club(away);
        let tier_home = rt.tier_of_club(home);
        let team = state.managers[m].team;
        if team != home && team != away {
            last = 0xff;
        }
        if team == home {
            if forfeits(state, m) {
                forfeit_dialog(state, mi);
                match_forfeit_penalty(state, mi);
                return 1;
            }
            rt.manager_plays_now[m] = 1;
            let mode = match cup_mask {
                0 | 1 => 1,
                _ if state.event_state[1] > 4 => 3,
                _ => 2,
            };
            if !fixed {
                att = match_attendance_estimate(state, rt, rng, mi, home, away, mode, false, 0);
            }
            let r = &mut state.managers[m];
            if cup_mask == 0 {
                // At most 19 home games a season; the counter restarts at the season end.
                r.home_attendance[usize::from(r.values_13a[0])] = ldiv(att, 1000) as u8;
                r.records[0] = r.records[0].wrapping_add(att);
                if att >= r.records[1] {
                    r.records[1] = att;
                    r.values_1f4[..2].copy_from_slice(&[i16::from(away), 0]);
                }
                if att <= r.records[2] {
                    r.records[2] = att;
                    r.values_1f4[2..].copy_from_slice(&[i16::from(away), 0]);
                }
                r.values_13a[0] = r.values_13a[0].wrapping_add(1);
            } else if tier_away < league && !fixed {
                let d = league.wrapping_sub(tier_away) as i8;
                let lo = ldiv(att, 8 - 3 * i32::from(d)) as i16;
                att = att.wrapping_add(i32::from(rng.random_range(lo, att as i16)));
                let r = &state.managers[m];
                let cap = r.stadium[0][0].wrapping_add(r.stadium[1][0]);
                if cap < att {
                    att = cap;
                }
            }
            last = mi;
            let r = &mut state.managers[m];
            let gate = if fixed {
                FIXED_GATE_MONEY
            } else {
                ldiv(lmul(i32::from(r.value_10a), att), gate_div)
            };
            r.balance = r.balance.wrapping_add(gate);
        }
        if team == away {
            if forfeits(state, m) {
                forfeit_dialog(state, mi);
                match_forfeit_penalty(state, mi);
                return 2;
            }
            rt.manager_plays_now[m] = 1;
            if gate_div == 2 && !fixed {
                let gate = if att == 0 {
                    let price =
                        rng.random_range(0, 1) + NEUTRAL_TICKET_PRICE[usize::from(tier_home)];
                    att = match_attendance_estimate(
                        state,
                        rt,
                        rng,
                        mi,
                        home,
                        away,
                        1,
                        true,
                        price as u8,
                    );
                    ldiv(lmul(i32::from(price), att), 2)
                } else {
                    // TODO(re): with an uninvolved manager between the two human clubs `last`
                    // is 0xff and the original reads the price of manager record 0xff.
                    let price = state
                        .managers
                        .get(usize::from(last))
                        .map_or(0, |r| r.value_10a);
                    ldiv(lmul(i32::from(price), att), gate_div)
                };
                let r = &mut state.managers[m];
                r.balance = r.balance.wrapping_add(gate);
            }
            if fixed {
                let r = &mut state.managers[m];
                r.balance = r.balance.wrapping_add(FIXED_GATE_MONEY);
            }
            last = mi;
        }

        let a = state.manager_index_of_team(home, 0xfe);
        let b = state.manager_index_of_team(0xfe, away);
        let c = state.manager_index_of_team(home, away) & 0x7f;
        if c != 0 && (a != 0 || b & 0x7f != 0) && last == mi {
            match_aftermath(state, rt, rng, m, cup_mask);
            if c == mi + 1 {
                rt.live_matches.push(LiveMatch { row: comp, slot });
            }
            if a == mi + 1 || b & 0x7f == mi + 1 {
                news_record_start(state, rt, rng, m, home, away, att, gate_div);
            }
        }
    }
    0
}

/// The squad side of a human match at kick-off.
///
/// Origin: BMMAIN.EXE 2a58:20b2 (match_gate_receipts_and_aftermath, 2a58:2530..29ae first part)
fn match_aftermath(state: &mut GameState, rt: &mut Runtime, rng: &mut Rng, m: usize, cup_mask: u8) {
    rt.players_on_pitch[m] = 11;
    let cnt = squad_count_listed(state, m);
    let league_day = cup_mask == 0;
    for i in 0..TRANSFER_LIST_SCAN {
        let s = state.squads[TRANSFER_LIST][i].clone();
        if state.players[usize::from(s.player_id)].owner_manager == m as u8
            && s.status & 1 == 1
            && s.suspended != 0
            && league_day
        {
            state.squads[TRANSFER_LIST][i].suspended -= 1;
        }
    }
    for i in 0..usize::from(cnt) {
        let s = &mut state.squads[m][i];
        if s.status & 1 == 1 && s.suspended != 0 && league_day {
            s.suspended -= 1;
        }
        s.match_rating = 0;
        if s.lineup_position != 0 && s.lineup_position < 12 {
            let p = &state.players[usize::from(s.player_id)];
            let k = u8::from(p.skills[0] > p.skills[1]);
            let tired = (rng.random_range(2, 4) as u8)
                .wrapping_sub(k)
                .wrapping_add(6);
            let s = &mut state.squads[m][i];
            s.exhaustion = s.exhaustion.wrapping_add(tired);
            if league_day {
                let p = &mut state.players[usize::from(s.player_id)];
                p.counters[1] = p.counters[1].wrapping_add(1);
            }
            let comp = usize::from(cup_mask != 0) + usize::from(cup_mask > 1);
            s.appearances[comp] = s.appearances[comp].wrapping_add(1);
            s.value_terms[comp] = s.value_terms[comp].wrapping_add(1);
        }
        let s = &mut state.squads[m][i];
        s.exhaustion = s.exhaustion.clamp(50, 150);
    }
    let (yellow, red) = if cnt < 6 || cup_mask != 0 {
        (0, 0)
    } else {
        (6, 1)
    };
    rt.yellow_cards_left[m] = yellow;
    rt.red_card_possible[m] = red;
    rt.substitutions_left[m] = [1, 2];
    rt.booked_players[m] = [NO_BOOKING; 6];
    rt.live_match_shown = 1;
}

/// Manager `m`'s newspaper record at kick-off; a home manager's fans may turn into hooligans
/// for the next week.
///
/// Origin: BMMAIN.EXE 2a58:20b2 (match_gate_receipts_and_aftermath, 2a58:2530..29ae last part)
#[allow(clippy::too_many_arguments)]
fn news_record_start(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    m: usize,
    home: u8,
    away: u8,
    att: i32,
    gate_div: i32,
) {
    let rec = &mut rt.news_records[m];
    *rec = NewsRecord::zeroed();
    let r = &mut state.managers[m];
    let (me, opp) = if r.team == home {
        rec.spectators = att;
        let capacity = r.stadium[0][0].wrapping_add(r.stadium[1][0]);
        if capacity == r.records[0] {
            rec.sold_out = 1;
        }
        let mut h = (r.stadium[1][0].wrapping_sub(r.stadium[0][0]) as i16) / 1000;
        clamp_int(&mut h, 0, 30);
        h = 30 - h;
        if rng.random_range(0, h * 4 + 15) == 0 {
            r.value_13e |= 1;
        }
        (home, away)
    } else {
        if gate_div == 2 {
            rec.spectators = att.wrapping_add(200_000);
        }
        (away, home)
    };
    rec.club_a = me;
    rec.club_b = opp;
    // The original writes the byte first with the own club's table position, then with the
    // opponent's; past the 64 German clubs it reads beyond the table, and nothing reads the byte.
    rec.opponent_0efa = state
        .team_tables
        .get(usize::from(opp))
        .map_or(0, |t| t.table_position);
}

/// [`TickerEvent::kind`] of an injury.
const EVENT_INJURY: u8 = 4;
/// [`GameState::options`] entry of the goal scenes (DS:0607).
const GOAL_SCENES_OPTION: usize = 9;
/// The original's "nobody" for a scorer or an assistant.
const NOBODY: u8 = 0xfe;

/// The logic of one live ticker event: a chance or a goal counts into the period's chances
/// ([`Runtime::period_chances_used`]), shows the goal scene, picks the scorer and assistant of
/// a human team and books them (goals, match ratings, newspaper record); a card or an injury
/// is applied to the player, a red card bans him for 1..=7 games (one for yellow-red) and
/// recomputes the team strength, and an injured player may be substituted.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events), without the screen
pub fn match_ticker_events<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    e: TickerEvent,
    ui: &mut U,
) {
    let n = state.active_managers().len();
    let side_team = if e.side == 0 { e.home } else { e.away };
    let hm = state.managers[..n]
        .iter()
        .rposition(|r| r.team == side_team)
        .map(|m| m as u8);
    if hm.is_some_and(|m| squad_count_starters(state, usize::from(m)) == 0) {
        return;
    }
    let (att, def) = if e.side == 0 {
        (e.home, e.away)
    } else {
        (e.away, e.home)
    };
    let (m, pl) = (
        usize::from(e.manager_or_kind),
        usize::from(e.player_or_minute),
    );
    match e.kind {
        EVENT_CHANCE | EVENT_GOAL => ticker_chance(state, rt, rng, e, hm, att, def, ui),
        EVENT_RED_CARD => {
            let s = &mut state.squads[m][pl];
            if e.arg10 != 0 {
                s.cards[2] = s.cards[2].wrapping_add(1);
                s.cards[1] = s.cards[1].wrapping_sub(1);
            } else {
                s.cards[0] = s.cards[0].wrapping_add(1);
            }
            s.lineup_position = 0;
            s.status |= 1;
            s.suspended = rng.random_range(1, 7 - 6 * i16::from(e.arg10)) as u8;
            s.match_rating = s.match_rating.wrapping_sub(10);
            news::news_record_match_event(rt, e.manager_or_kind, EVENT_RED_CARD, pl as u8, 0, 0);
            rt.players_on_pitch[m] = rt.players_on_pitch[m].wrapping_sub(1);
            squad_recalc_team_strength(state, rt, rng, e.manager_or_kind, true);
        }
        EVENT_YELLOW_CARD => {
            let s = &mut state.squads[m][pl];
            s.match_rating = s.match_rating.wrapping_sub(10);
            s.cards[1] = s.cards[1].wrapping_add(1);
            news::news_record_match_event(rt, e.manager_or_kind, EVENT_YELLOW_CARD, pl as u8, 0, 0);
        }
        EVENT_INJURY => {
            player_injure(state, rng, m, pl);
            if !ui.auto_play() {
                // The stretcher comes on with chance 1/16 (UI only).
                let stretcher = rng.random_range(0, 15) == 0;
                ui.substitution(state, rt, rng, e.manager_or_kind, e.arg6, stretcher);
            }
        }
        _ => {}
    }
}

/// [`match_ticker_events`] of a chance or a goal.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:11b0..19ed)
#[allow(clippy::too_many_arguments)]
fn ticker_chance<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    e: TickerEvent,
    hm: Option<u8>,
    att: u8,
    def: u8,
    ui: &mut U,
) {
    // The manager of the side in the panel counts the chance on his own side; another human
    // club of the match counts it on the other side.
    for (team, own_side) in [(e.home, 0), (e.away, 1)] {
        let mi = state.manager_index_of_team(team, 0xff);
        if mi == 0 {
            continue;
        }
        let (m, side) = if Some(mi - 1) == hm {
            (mi - 1, own_side)
        } else {
            (mi - 1, 1 - own_side)
        };
        let c = &mut rt.period_chances_used[usize::from(m)][side];
        *c = c.wrapping_add(1);
    }
    let mut scene = 0;
    if state.options[GOAL_SCENES_OPTION] == 0 {
        if !ui.auto_play() && e.arg10 != 0 {
            scene = e.arg10;
        }
    } else {
        scene = ui.goal_scene(state, rng, e.kind == EVENT_GOAL, e.side, e.arg10);
    }
    let n = state.active_managers().len();
    let minute = e.player_or_minute;
    let comp = usize::from(e.manager_or_kind);
    let mut human = false;
    for m in 0..n {
        if state.managers[m].team != att {
            continue;
        }
        let mu = m as u8;
        let sc = match_pick_scorer(state, rng, mu, 0, e.kind).unwrap_or(NOBODY);
        human = true;
        let mut assist = NOBODY;
        if squad_count_starters(state, m) > 1 {
            loop {
                assist = match_pick_scorer(state, rng, mu, 1, 1).unwrap_or(NOBODY);
                if assist != sc {
                    break;
                }
            }
        }
        news::news_record_match_event(rt, mu, e.kind, sc, assist, minute);
        for j in 0..n {
            if state.managers[j].team == def {
                news::news_record_match_event(rt, j as u8, e.kind, OPPONENT, OPPONENT, minute);
            }
        }
        // Without a scorer the original writes into an unrelated record; the early exit of
        // the caller makes that unreachable.
        if let Some(s) = state.squads[m].get_mut(usize::from(sc)) {
            if e.kind == EVENT_GOAL {
                if e.arg10 == 0 {
                    if comp == 0 {
                        let p = &mut state.players[usize::from(s.player_id)];
                        p.counters[0] = p.counters[0].wrapping_add(1);
                    }
                    s.club_goals[comp] = s.club_goals[comp].wrapping_add(1);
                    s.goals[comp] = s.goals[comp].wrapping_add(1);
                    s.match_rating = s.match_rating.wrapping_add(15);
                }
            } else {
                s.match_rating = s.match_rating.wrapping_sub(10);
            }
        }
        if (e.kind == EVENT_GOAL || sc != NOBODY) && assist != NOBODY && scene == 0 && e.arg10 == 0
        {
            let a = &mut state.squads[m][usize::from(assist)];
            a.match_rating = a.match_rating.wrapping_add(10);
        }
    }
    if human {
        return;
    }
    for m in 0..n {
        if state.managers[m].team != def {
            continue;
        }
        news::news_record_match_event(rt, m as u8, e.kind, OPPONENT, OPPONENT, minute);
        for i in 0..SQUAD_SCAN {
            let s = &state.squads[m][i];
            if s.lineup_position == 0 || s.lineup_position >= 12 {
                continue;
            }
            let keeper = state.players[usize::from(s.player_id)].position_value == 0;
            let change: i8 = match (e.kind == EVENT_GOAL, keeper) {
                (true, true) => -8,
                (true, false) => -5,
                (false, true) => 8,
                (false, false) => -3,
            };
            let s = &mut state.squads[m][i];
            s.match_rating = s.match_rating.wrapping_add(change as u8);
        }
    }
}

/// The match-day titles (`matchday_show_title` 1310:01f2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Title {
    /// "Ligaspiel" (1310:0995).
    League,
    /// "DFB-Pokal" (1310:0261).
    DfbCup,
    /// "Europapokal" (1310:0290).
    EuropeanCup,
    /// "Relegationsspiel" (1310:02bf).
    Relegation,
    /// "Nachholspiele" (1310:07a2).
    Rescheduled,
    /// "SAISONENDE" (2a58:4597).
    SeasonEnd,
}

/// The screens and dialogs [`season_main_loop`] shows. The loop-level callbacks may end the
/// loop with [`SeasonUi::Stop`] (the player quits); the callbacks inside a match day cannot, as
/// [`MatchHooks`] has no way to stop the engine: an implementer that wants to stop there
/// answers with the defaults and returns its `Stop` from the next loop-level callback.
pub trait SeasonUi {
    /// Why the loop stopped.
    type Stop;

    /// One office turn of the current manager: `arg` is the slot ([`GameState::season_week`])
    /// or 0xff for a turn of the season end ("SAISONENDE").
    ///
    /// Origin: BMMAIN.EXE 18bc:0b5f (office_menu_dispatcher), after input_wait_release
    fn office_turn(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        arg: u8,
    ) -> Result<(), Self::Stop>;

    /// A title screen before a match day or the season end.
    ///
    /// Origin: BMMAIN.EXE 1310:01f2 (matchday_show_title)
    fn title(&mut self, state: &GameState, title: Title) -> Result<(), Self::Stop>;

    /// Called on every pass through the winter break; the original shows "WINTERPAUSE" once,
    /// marking pixel (1, 1) of the screen, which [`SeasonUi::clear_screen`] clears again.
    fn winter_break(&mut self) -> Result<(), Self::Stop>;

    /// The screen of a special date that happened.
    ///
    /// Origin: BMMAIN.EXE 2a58:2a06 (special_date_events), the screen
    fn special_date(&mut self, state: &GameState, date: SpecialDate) -> Result<(), Self::Stop>;

    /// Manager `m` won the championship.
    ///
    /// Origin: BMMAIN.EXE 27c2:274d (championship_winner_message)
    fn championship_winner(&mut self, state: &GameState, m: u8) -> Result<(), Self::Stop>;

    /// The season end clears the screen (`ui_fill_rect(0, 0, 319, 239, 0)`).
    fn clear_screen(&mut self);

    /// The high-score table of the game's variant, read from `HIGH.XY` (created empty when
    /// missing).
    ///
    /// Origin: BMMAIN.EXE 422b:21c4 (highscore_load_save, read)
    fn highscores(&mut self, state: &GameState) -> HighScoreTable;

    /// At a season end the current manager's high-score entry goes into `HIGH.XY`
    /// (`highscore_screen(2)`, which builds the entry from `rng` in
    /// [`Runtime::highscore_candidate`]).
    ///
    /// Origin: BMMAIN.EXE 422b:2366 (highscore_screen, mode 2)
    fn highscore_insert(&mut self, state: &GameState, rt: &mut Runtime, rng: &mut Rng);

    /// The last season of the game is over: `table` (with the managers' entries) is written
    /// back to `HIGH.XY` and shown, then "ENDE".
    ///
    /// Origin: BMMAIN.EXE 422b:21c4 (highscore_load_save, write), 422b:2366
    /// (highscore_screen, mode 0) and 2a58:000c (game_end_screen)
    fn game_end(&mut self, state: &GameState, table: &HighScoreTable) -> Result<(), Self::Stop>;

    /// The "An Alle … AUSLOSUNG BEIWOHNEN ?" question of cup row `cup`'s draw
    /// ([`cups::CupDrawUi::ask`]).
    fn ask_draw(&mut self, cup: u8) -> bool;

    /// A club drawn into a slot of a shown cup draw ([`cups::CupDrawUi::animate_slot`]).
    fn draw_slot(&mut self, _state: &GameState, _cup: u8, _slot: u8, _team: u8) {}

    /// The end of a shown cup draw ([`cups::CupDrawUi::close`]).
    fn draw_close(&mut self, _state: &mut GameState, _cup: u8) {}

    /// Manager `m`'s club won the cup final of `comp` ([`cups::CupDrawUi::title_winner`]).
    fn cup_winner(&mut self, _state: &GameState, _m: u8, _comp: u8) {}

    /// Manager `m`'s match is lost because their club cannot field a team ("Ihr Spiel wird mit 0:2
    /// gewertet").
    fn forfeit_penalty(&mut self, state: &GameState, m: u8);

    /// The dialogs and messages of manager `m`'s finances of the day: the loans that fell
    /// due ("Ihr Kredit von … wurde heute f{llig.") and the finished constructions shown at
    /// once.
    ///
    /// Origin: BMMAIN.EXE 212a:0a6d (finance_daily_processing), the dialogs
    fn finance_events(&mut self, state: &GameState, m: u8, events: &[FinanceEvent]);

    /// The state of `format_long_thousands` (DS:079c, DS:07b2) that the messages change.
    fn number_format(&mut self) -> &mut NumberFormat;

    /// `DS:05d4`: automatic play ("Spiele automatisch"), which skips the waits and with
    /// them a few random draws and the substitution after an injury.
    fn auto_play(&self) -> bool;

    /// The goal scene of a chance (`goal`) of side `side` (0 home) with the goal-scene option
    /// on, with its replays: `tore_scene_play` 24a4:05ec draws the scene from `rng` and is
    /// called again with its result for a replay (keys `r`/`w`, at most three plays).
    /// `penalty` is the event's penalty flag. Returns the last play's penalty-scene flag.
    ///
    /// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:1290..1300)
    fn goal_scene(
        &mut self,
        state: &GameState,
        rng: &mut Rng,
        goal: bool,
        side: u8,
        penalty: u8,
    ) -> u8;

    /// Manager `m` may change the lineup after an injury in a live match (`cup_mode` is the
    /// match's cup flag); before it the stretcher comes on when `stretcher` (the draw
    /// `random_range(0, 15) == 0` at 2a58:1e2c), else the screen waits. Bringing a player on
    /// draws his exhaustion from `rng`.
    ///
    /// Origin: BMMAIN.EXE 2a58:1f75 (match_substitution_screen)
    fn substitution(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        m: u8,
        cup_mode: u8,
        stretcher: bool,
    );

    /// A screen or dialog of [`season_end_messages`].
    fn season_end(&mut self, state: &GameState, event: SeasonEndEvent<'_>);

    /// The contract talks with the player of record `slot` of the current manager whose
    /// contract ended (mode 1 of the screen). Returns whether he signs again.
    ///
    /// Origin: BMMAIN.EXE 3277:2a8f (contract_negotiation)
    fn contract_negotiation(
        &mut self,
        state: &mut GameState,
        rt: &Runtime,
        rng: &mut Rng,
        slot: u8,
    ) -> bool;

    /// The text measure and number format of the newspapers.
    fn news_env(&self) -> NewsEnv<'_>;

    /// The newspapers of a match day, in manager order.
    fn show_newspapers(&mut self, state: &GameState, papers: Vec<Newspaper>);

    /// Display: [`MatchHooks::period_start`].
    fn period_start(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _league_mask: u8,
        _cup_mask: u8,
        _from: u8,
    ) {
    }

    /// Display: [`MatchHooks::period_minute`].
    fn period_minute(&mut self, _state: &GameState, _rt: &Runtime, _to: u8, _minute: u8) {}

    /// [`MatchHooks::pause_requested`]: the settings key or the pause click.
    fn pause_requested(&mut self, _state: &mut GameState, _rt: &mut Runtime, _minute: u8) -> bool {
        false
    }

    /// Display: [`MatchHooks::period_end`].
    #[allow(clippy::too_many_arguments)]
    fn period_end(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _rng: &mut Rng,
        _league_mask: u8,
        _cup_mask: u8,
        _tier: Option<u8>,
        _to: u8,
    ) {
    }

    /// Display: [`MatchHooks::matchday_results`].
    #[allow(clippy::too_many_arguments)]
    fn matchday_results(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _rng: &mut Rng,
        _league_mask: u8,
        _cup_mask: u8,
        _penalties: bool,
    ) {
    }

    /// Display: a live ticker event is about to be applied ([`match_ticker_events`]): the event
    /// panel, the card and injury whistles and the blinking chance text.
    ///
    /// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, the screen before 2a58:11b0)
    fn ticker_event_start(&mut self, _state: &GameState, _rt: &Runtime, _event: &TickerEvent) {}

    /// Display: a live ticker event was applied: the event texts (scorer and assistant are
    /// the last entries of the newspaper record), the waits and the panel redraw.
    ///
    /// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, the screen after the logic) and
    /// 1310:322c (match_event_redraw)
    fn ticker_event_end(&mut self, _state: &GameState, _rt: &Runtime, _event: &TickerEvent) {}
}

/// The cup draw screens of the season loop.
struct SeasonDraws<'a, U: SeasonUi + ?Sized>(&'a mut U);

impl<U: SeasonUi + ?Sized> cups::CupDrawUi for SeasonDraws<'_, U> {
    fn ask(&mut self, cup: u8) -> bool {
        self.0.ask_draw(cup)
    }

    fn animate_slot(&mut self, state: &GameState, cup: u8, slot: u8, team: u8) {
        self.0.draw_slot(state, cup, slot, team);
    }

    fn close(&mut self, state: &mut GameState, cup: u8) {
        self.0.draw_close(state, cup);
    }

    fn title_winner(&mut self, state: &GameState, m: u8, comp: u8) {
        self.0.cup_winner(state, m, comp);
    }
}

/// The match engine's hooks of the season loop.
struct SeasonHooks<'a, U: SeasonUi + ?Sized>(&'a mut U);

impl<U: SeasonUi + ?Sized> MatchHooks for SeasonHooks<'_, U> {
    fn match_gate_receipts_and_aftermath(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        home: u8,
        away: u8,
        cup_mask: u8,
        comp: u8,
        slot: u8,
        league_mask: u8,
    ) -> u8 {
        match_gate_receipts_and_aftermath(
            state,
            rt,
            rng,
            home,
            away,
            cup_mask,
            comp,
            slot,
            league_mask,
            &mut |s, m| self.0.forfeit_penalty(s, m),
        )
    }

    fn match_ticker_events(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        event: TickerEvent,
    ) {
        self.0.ticker_event_start(state, rt, &event);
        match_ticker_events(state, rt, rng, event, self.0);
        self.0.ticker_event_end(state, rt, &event);
    }

    fn pause_requested(&mut self, state: &mut GameState, rt: &mut Runtime, minute: u8) -> bool {
        self.0.pause_requested(state, rt, minute)
    }

    fn period_start(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        league_mask: u8,
        cup_mask: u8,
        from: u8,
    ) {
        self.0.period_start(state, rt, league_mask, cup_mask, from);
    }

    fn period_minute(&mut self, state: &GameState, rt: &Runtime, to: u8, minute: u8) {
        self.0.period_minute(state, rt, to, minute);
    }

    fn period_end(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        league_mask: u8,
        cup_mask: u8,
        tier: Option<u8>,
        to: u8,
    ) {
        self.0
            .period_end(state, rt, rng, league_mask, cup_mask, tier, to);
    }

    fn matchday_results(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        league_mask: u8,
        cup_mask: u8,
        penalties: bool,
    ) {
        self.0
            .matchday_results(state, rt, rng, league_mask, cup_mask, penalties);
    }

    fn cup_check_ties_extra_time(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        cup_mask: u8,
        count: u8,
        shootout: bool,
    ) -> bool {
        cups::cup_check_ties_extra_time(state, rt, rng, self, cup_mask, count, shootout)
    }

    fn cup_round_complete(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        euro: u8,
    ) {
        cups::cup_round_complete(state, rt, rng, euro, &mut SeasonDraws(&mut *self.0));
    }

    fn match_rate_players_and_newspaper(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        cup_mask: u8,
    ) {
        let papers =
            news::match_rate_players_and_newspaper(state, rt, rng, cup_mask, &self.0.news_env());
        self.0.show_newspapers(state, papers);
    }
}

/// [`season_main_loop`] ended the game after its last season.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameOver;

/// The labels of the original's season loop.
enum At {
    /// `TOP` 2a58:3194: the start of a slot.
    Top,
    /// `OFFICE_A_TEST` 2a58:3abd: the office turns of the season end.
    SeasonEndOffice,
    /// `OFFICE_B_TEST` 2a58:3b26: the normal office turns, then the slot's matches.
    Office,
    /// `DAY_ADVANCE` 2a58:3397: on to the next slot or to the season end.
    DayAdvance,
    /// `SEASON_FINAL` 2a58:3bcf: the season end and the next season's start.
    SeasonFinal,
}

/// The season loop: per half-week slot the finances of each day, the office turns of every
/// manager, the slot's league, cup and rescheduled matches, the special dates and the weekly
/// events; after day 322 the season-end office turns, the season's records and tables,
/// promotion and relegation, the summer break and the next season's draws. `resume`: a game
/// was loaded, and its saved current manager gets the first office turn. Returns at the end of
/// the game's last season, or when `ui` stops it.
///
/// Callees (season.md name → port): `finance_weekly_processing` →
/// [`finance_daily_processing`], `league_init_or_ai` → [`league_strength_drift`],
/// `squad_weekly_update` → [`auto_lineup`], `manager_ai_weekly` →
/// [`squad_recalc_team_strength`], `transfer_random_offer` → [`transfer_market_refresh`],
/// `league_play_round` → [`league_postpone_matches_weather`], the title wrappers →
/// [`matchday_play`], `euro_special_slot` → [`cups::matchday_relegation_match`],
/// `extra_match_pending` / `extra_match_play` → [`has_rescheduled_match_today`] /
/// [`matchday_play_rescheduled`], `top_scorer_weekly_update` (season.md's old, wrong
/// name for 24a4:2cb4) → [`advertising_offers_generate`],
/// `season_reset_tables` → [`cups::league_shuffle_teams`], `competition_reset` →
/// [`cups::euro_cup_qualifiers_build`], `competition_count` → [`cups::cup_clear_match_goals`],
/// `competition_init` → [`cups::cup_draw_first_round`], `office_random_event` →
/// [`manager_compute_wage_budget`]; the bankruptcy, promotion and relegation block is
/// [`cups::season_promotion_relegation`].
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop)
pub fn season_main_loop<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    ui: &mut U,
    resume: bool,
) -> Result<GameOver, U::Stop> {
    let mut resume = resume;
    let saved_mgr = state.current_manager;
    // The office loops compare against `DS:07ab` itself (2a58:3b2f): managers added in the
    // office (start menu, sub icon 5/5) get their turn on the same day.
    let mut wf = 0;
    let mut at = At::Top;
    loop {
        at = match at {
            At::Top => {
                let mut fmt = *ui.number_format();
                wf = season_slot_start(state, rt, rng, &mut fmt, &mut |s, m, events| {
                    ui.finance_events(s, m, events)
                });
                *ui.number_format() = fmt;
                if wf == 0 {
                    At::DayAdvance
                } else {
                    state.current_manager = 0;
                    At::Office
                }
            }
            At::SeasonEndOffice if state.current_manager < manager_loop_count(state) => {
                if resume {
                    state.current_manager = saved_mgr;
                    resume = false;
                }
                let cur = state.current_manager;
                ui.office_turn(state, rt, rng, 0xff)?;
                if state.day_counter > SEASON_END_OFFICE_DAY {
                    state.current_manager = cur;
                    season_record_manager(state, rt, cur as u8, ui)?;
                    state.current_manager = cur + 1;
                    At::SeasonEndOffice
                } else {
                    state.current_manager += 1;
                    At::Office
                }
            }
            At::SeasonEndOffice => At::SeasonFinal,
            At::Office if state.current_manager < manager_loop_count(state) => {
                if resume {
                    state.current_manager = saved_mgr;
                    resume = false;
                }
                office_turn_start(state, rng);
                ui.office_turn(state, rt, rng, state.season_week)?;
                // TODO(re): season.md §7 Q1, the value 9 is not read as 0 here.
                wf = state.week_competitions[usize::from(state.season_week)];
                if state.day_counter > SEASON_LAST_DAY {
                    season_record_manager(state, rt, state.current_manager as u8, ui)?;
                    state.current_manager += 1;
                    At::SeasonEndOffice
                } else {
                    state.current_manager += 1;
                    At::Office
                }
            }
            At::Office => {
                play_slot_matches(state, rt, rng, ui, wf)?;
                At::DayAdvance
            }
            At::DayAdvance => {
                if day_advance(state, rt, rng, ui)? {
                    state.season_week = state.season_week.wrapping_add(1);
                    At::Top
                } else {
                    weekly_squad_update(state, rt, rng);
                    state.current_manager = 0;
                    At::SeasonEndOffice
                }
            }
            At::SeasonFinal => {
                if season_final(state, rt, rng, ui)? {
                    return Ok(GameOver);
                }
                At::Top
            }
        };
    }
}

/// The matches of the slot with competition bits `wf`.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, 2a58:326a..3397)
fn play_slot_matches<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    ui: &mut U,
    wf: u8,
) -> Result<(), U::Stop> {
    for m in 0..manager_loop_count(state) {
        state.current_manager = m;
        let mu = m as usize;
        squad_recalc_team_strength(state, rt, rng, m as u8, true);
        state.lineup_state[mu][1] = state.lineup_state[mu][0];
        state.lineup_state[mu][0] = 1;
        rt.manager_plays_now[mu] = 0;
        rt.lineup_change_count[mu] = 0;
    }
    let slot = state.season_week;
    state.current_manager = MATCH_DAY_MANAGER;
    if wf & 7 != 0 {
        if wf & 1 != 0 {
            league_postpone_matches_weather(state, rt, rng, slot, 0);
        }
        league_postpone_matches_weather(state, rt, rng, slot, 1);
        league_postpone_matches_weather(state, rt, rng, slot, 2);
        ui.title(state, Title::League)?;
        matchday_play(state, rt, rng, &mut SeasonHooks(ui), wf, 0);
    }
    state.current_manager = MATCH_DAY_MANAGER;
    if wf & 8 != 0 {
        ui.title(state, Title::DfbCup)?;
        matchday_play(state, rt, rng, &mut SeasonHooks(ui), 0, 1);
    }
    if wf & 0x70 == 0x10 {
        ui.title(state, Title::Relegation)?;
        cups::matchday_relegation_match(state, rt, rng, &mut SeasonHooks(ui));
    } else if wf & 0x70 != 0 {
        ui.title(state, Title::EuropeanCup)?;
        matchday_play(state, rt, rng, &mut SeasonHooks(ui), 0, 0x0e);
    }
    if wf & RESCHEDULED_GAMES_DAY != 0 && has_rescheduled_match_today(state, slot) {
        ui.title(state, Title::Rescheduled)?;
        matchday_play_rescheduled(state, rt, rng, &mut SeasonHooks(ui), slot);
    }
    Ok(())
}

/// `DAY_ADVANCE`: the round counters, the winter break, the days up to the next match day with
/// their finances and special dates, and the weekly events. Returns whether the season goes on.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, DAY_ADVANCE 2a58:3397..36f9)
fn day_advance<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    ui: &mut U,
) -> Result<bool, U::Stop> {
    let raw = state.week_competitions[usize::from(state.season_week)];
    for (b, round) in state.competition_rounds.iter_mut().enumerate() {
        if (raw >> b) & 1 != 0 {
            *round = round.wrapping_add(1);
        }
    }
    if state.day_counter > WINTER_BREAK.0 && state.day_counter < WINTER_BREAK.1 {
        ui.winter_break()?;
    }
    for m in 0..manager_loop_count(state) {
        state.current_manager = m;
        for s in &mut state.squads[m as usize][..SQUAD_SCAN] {
            if s.suspended != 0 {
                s.lineup_position = 0;
            }
        }
    }
    let start = i32::from(SEASON_START_DAY);
    state.day_counter = state.day_counter.wrapping_add(1);
    // The date is still the one of the loop top.
    check_special_dates(state, rng, ui)?;
    loop {
        calendar_compute_date(state, 0);
        for m in 0..manager_loop_count(state) {
            state.current_manager = m;
            state.saved_manager = m;
            let events = finance_daily_processing(state, rng, ui.number_format());
            ui.finance_events(state, m as u8, &events);
        }
        state.day_counter = state.day_counter.wrapping_add(1);
        check_special_dates(state, rng, ui)?;
        // DS:05da (stop the demo version in December) is 0.
        if lrem(state.day_counter - start, 7) == 0 {
            break;
        }
        // TODO(re): season.md §7 Q2, no DS:224c offset in this test.
        if lrem(state.day_counter - 4, 7) == 0 {
            break;
        }
    }
    for m in 0..manager_loop_count(state) {
        state.current_manager = m;
        random_weekly_events(state, rt, rng, ui.number_format());
        if lrem(state.day_counter - start, 14) == 0 {
            advertising_offers_generate(state, rng);
        }
    }
    Ok(state.day_counter <= SEASON_LAST_DAY)
}

/// The board's verdict on the season (`SEASON_FINAL`): per manager the share of their club's
/// all-time points gain (since `old_points`) against the previous fans value, weighted by the
/// seasons played (up to 5), clamped to 0..=100.
///
/// The result is not reset per manager ([BP-8]): every manager starts from the previous
/// manager's value, and only the last one is returned.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, in SEASON_FINAL 2a58:3bcf..458b)
fn board_verdict(state: &GameState, old_points: &[i32]) -> i16 {
    let div = if state.logged_rounds < 4 {
        state.logged_rounds as u8 + 1
    } else {
        5
    };
    let div = i16::from(div as i8);
    let mut v: i16 = 0;
    for (m, &old) in state.managers.iter().zip(old_points) {
        let gain = state.team_tables[usize::from(m.team)]
            .prestige
            .wrapping_sub(old);
        let q = uldiv(uldiv(lmul(gain, 100) as u32, 108), i32::from(div) as u32) as u16;
        v = m.fans.wrapping_add(q.wrapping_sub(m.fans / div as u16)) as i16;
        clamp_int(&mut v, 0, 100);
    }
    v
}

/// Stores the board's verdict `v` as every manager's fans value, at least 30
/// in the two Bundesligas and 61 in the Bundesliga; a raised floor carries on to the next
/// manager.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, in SEASON_FINAL 2a58:3bcf..458b)
fn board_verdict_store(state: &mut GameState, mut v: i16) {
    let n = state.active_managers().len();
    for m in &mut state.managers[..n] {
        if v < 30 && m.league < 2 {
            v = 30;
        }
        if v < 61 && m.league == 0 {
            v = 61;
        }
        m.fans = v as u16;
    }
}

/// `SEASON_FINAL`: the all-time table, the board's verdict, the cleared tables, promotion and
/// relegation, the game end or the summer break and the next season's start. Returns whether
/// the game is over.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, SEASON_FINAL 2a58:3bcf..458b)
fn season_final<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    ui: &mut U,
) -> Result<bool, U::Stop> {
    let n = state.active_managers().len();
    let mut old_points = [0i32; MANAGER_SLOTS];
    let mut top_scorer = [false; MANAGER_SLOTS];
    for i in 0..n {
        old_points[i] = state.team_tables[usize::from(state.managers[i].team)].prestige;
        rt.goal_message_flags[i] = 0;
        state.current_manager = i as i8;
        top_scorer[i] = top_scorer_plays_for_current_manager(state);
    }
    ui.title(state, Title::SeasonEnd)?;
    season_end_eternal_points(state, rt);
    let v = board_verdict(state, &old_points[..n]);
    for t in &mut state.team_tables {
        for k in 0..2 {
            t.points[k] = 0;
            t.goals_for[k] = 0;
            t.goals_against[k] = 0;
            t.played[k] = 0;
            t.wins[k] = 0;
            t.losses[k] = 0;
            strcpy(&mut t.form[k], FORM_PLACEHOLDER);
        }
    }
    cups::season_promotion_relegation(state, rt, rng);
    league_strength_drift(state, rt, rng, 10);
    board_verdict_store(state, v);
    if i32::from(state.scenario) == state.year {
        // The original also writes 'a' into four fixture permutation strings (2a58:4180); the
        // program ends right after, so nothing reads them.
        let mut table = ui.highscores(state);
        for m in 0..n {
            state.current_manager = m as i8;
            highscore_build_entry(state, rng, m, 1, &mut rt.highscore_candidate);
            highscore_game_end_insert(&mut table, &mut rt.highscore_candidate);
        }
        ui.game_end(state, &table)?;
        return Ok(true);
    }
    for m in 0..n as i8 {
        state.current_manager = m;
        ui.highscore_insert(state, rt, rng);
        advertising_offers_generate(state, rng);
    }
    ui.clear_screen();
    cups::league_shuffle_teams(state, rt, rng);
    cups::euro_cup_qualifiers_build(state, rt);
    season_end_messages(state, rt, rng, &top_scorer[..n], ui);
    state.logged_rounds = state.logged_rounds.wrapping_add(1);
    state.day_counter = state.day_counter.wrapping_add(1);
    loop {
        state.loop_month = state.month as u8;
        calendar_compute_date(state, 0);
        for m in 0..n {
            state.current_manager = m as i8;
            state.saved_manager = m as i8;
            let events = finance_daily_processing(state, rng, ui.number_format());
            ui.finance_events(state, m as u8, &events);
            let mut i = 0;
            while i < squad_count_listed(state, m) {
                player_suspension_countdown(state, rt, m, usize::from(i));
                i += 1;
            }
        }
        state.day_counter = state.day_counter.wrapping_add(1);
        if state.day_of_month == 28 && state.month == 6 {
            break;
        }
    }
    calendar_compute_date(state, 0);
    state.season_week = 0;
    state.day_counter = i32::from(SEASON_START_DAY);
    state.start_year = state.start_year.wrapping_add(1);
    state.league_results = [[[0; 20]; 38]; 3];
    for i in 0..n {
        manager_compute_wage_budget(state, rng, i);
    }
    cups::cup_clear_match_goals(state);
    // UI: office_free_bitmaps(3) 18bc:02ef and office_load_bitmaps(3) 18bc:034d around the draws.
    let ask = &mut SeasonDraws(ui);
    for cup in 0..4 {
        cups::cup_draw_first_round(state, rt, rng, cup, false, ask);
    }
    for cup in 1..4 {
        cups::cup_draw_first_round(state, rt, rng, cup, true, ask);
    }
    Ok(false)
}

/// What the season end shows ([`season_end_messages`]), in the original's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeasonEndEvent<'a> {
    /// Manager `m`'s "Saisonende" report: the final rank in the tier (1-based), the tier and
    /// the event lines; `sponsors_released` adds the dialog "Ihre Werbepartner erlauben …",
    /// `joke` the two dialogs "Ihre Spieler werden {lter und schw{cher..." and "War nur ein
    /// SCHERZ !!!" before the click.
    Report {
        /// The manager.
        m: u8,
        /// Rank in the tier, 1-based.
        rank: i32,
        /// The tier.
        tier: u8,
        /// The lines under "Ereignisse".
        lines: &'a [&'static [u8]],
        /// The advertising partners may cancel their contracts.
        sponsors_released: bool,
        /// The ageing joke.
        joke: bool,
    },
    /// The "Trennungen" screen of manager `m`.
    Separations {
        /// The manager.
        m: u8,
    },
    /// The message of a player who retires ("… h{ngt den Fu~balljob … an den Nagel.").
    Retirement {
        /// The message text.
        message: &'a [u8],
    },
    /// The "Vertragsverhandlungen" screen of manager `m`; `expiring` lists the squad.
    Contracts {
        /// The manager.
        m: u8,
        /// A contract of the squad ends.
        expiring: bool,
    },
    /// Player `pid` signs again ("… bleibt Ihnen auch die n{chste Saison erhalten.").
    Stays {
        /// The player.
        pid: u8,
    },
    /// Player `pid` leaves ("… kehrt Ihrem Verein den R}cken...") for a fee of `fee` DM.
    Leaves {
        /// The player.
        pid: u8,
        /// The fee the club gets.
        fee: i32,
    },
    /// The talks with one player of manager `m` are over (the player may have left the
    /// squad): the contracts list is drawn again.
    ContractDone {
        /// The manager.
        m: u8,
    },
    /// Every manager's report and separations are done (the original waits 70 ticks and
    /// fades out).
    SeparationsDone,
    /// The season-end messages are over (the original fades out).
    Done,
}

/// The report line of a promotion into the 2. Bundesliga and its bonus.
const PROMOTED_TO_SECOND: (&[u8], i32) = (b"Gratulation zum Aufstieg (800.000 DM).", 800_000);
/// The report line of a promotion into the Bundesliga and its bonus.
const PROMOTED_TO_FIRST: (&[u8], i32) = (b"Gratulation zum Aufstieg (1.600.000 DM).", 1_600_000);
/// The report lines of the season-end flags 2, 4 and 8 ([`crate::state::Manager::value_140`]).
const SEASON_END_FLAG_LINES: [(u8, &[u8]); 3] = [
    (2, b"Ihr Abstieg ersch]ttert Millionen Fans."),
    (4, b"DFB verweigert die Lizenz. Sie steigen ab."),
    (8, b"Sie beginnen neu mit 500.000 DM."),
];
/// The report line and the bonus of the league's top scorer.
const TOP_SCORER: (&[u8], i32) = (b"Torsch]tzenk|nig aus Ihrem Team (250.000 DM).", 250_000);
/// The report line of a European cup place.
const EURO_QUALIFIED: &[u8] = b"Sie sind f}r den Europapokal qualifiziert.";
/// The report line of a youth player.
const YOUTH_PLAYER: &[u8] = b"Ein Jugendspieler steigt in den Kader auf.";
/// The report line of a season without events.
const NO_EVENTS: &[u8] = b"Keine besonderen Ereignisse...";

/// The season end of every manager: the "Saisonende" report with the promotion and top-scorer
/// bonuses and a youth player, the "Trennungen" pass (manager 0 ages every person, ends the
/// contracts' last season and returns players to their owners), the retirements and the new
/// persons that replace them; then the season's cards, goals and games are cleared, the
/// tiers of the option matrix follow the managers' tiers, the expiring contracts are
/// negotiated (not in automatic play) and the persons are spread over the tiers again
/// ([`season_rebalance_player_clubs`]). `top_scorer[m]`: manager `m`'s club has the league's
/// top scorer.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages)
pub fn season_end_messages<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    top_scorer: &[bool],
    ui: &mut U,
) {
    let n = manager_loop_count(state);
    let hist = (state.logged_rounds as i16).min(0x31) as usize;
    let mut tier_count = [0i8; 4];
    // [BP-0x7c]: the last youth person, or 6 after the sponsor loop; uninitialised for the
    // first manager when neither ran (season.md §7 Q11).
    let mut stale_person: usize = 0;
    for m in 0..n {
        state.current_manager = m;
        state.saved_manager = m;
        let mu = m as usize;
        let league = state.managers[mu].league;
        tier_count[usize::from(league)] += 1;
        let (rank, tier) = {
            let [pos, tier, ..] = state.managers[mu].round_log[hist];
            let first = TIER_FIRST.get(usize::from(tier)).copied().unwrap_or(0);
            (
                i32::from(u16::from(pos).wrapping_sub(i16::from(first as i8) as u16) as i16),
                tier,
            )
        };
        let mut lines: Vec<&'static [u8]> = Vec::new();
        let sponsors_released = season_end_bonuses(
            state,
            rt,
            m as u8,
            top_scorer[mu],
            &mut lines,
            &mut stale_person,
        );
        if let Some(p) = season_end_youth_player(state, rt, rng, m as u8) {
            lines.push(YOUTH_PLAYER);
            stale_person = p;
        }
        if lines.is_empty() {
            lines.push(NO_EVENTS);
        }
        // DS:2252 == DS:224e (the DM version).
        let joke = rng.random_range(0, 4) == 0;
        ui.season_end(
            state,
            SeasonEndEvent::Report {
                m: m as u8,
                rank,
                tier,
                lines: &lines,
                sponsors_released,
                joke,
            },
        );
        state.saved_manager = m;
        ui.season_end(state, SeasonEndEvent::Separations { m: m as u8 });
        if m == 0 {
            separations(state, rng, ui.auto_play());
        }
        retirements(state, rng, ui, m, n, stale_person);
    }
    for g in 0..n as usize {
        for s in &mut state.squads[g][..SQUAD_SCAN] {
            s.cards = [0; 3];
            s.goals = [0; 3];
            s.appearances = [0; 3];
        }
    }
    let mut any_tier = [0i8; 3];
    for i in 0..3 {
        for (j, any) in any_tier.iter_mut().enumerate() {
            *any = any.wrapping_add(state.options[3 * i + j] as i8);
        }
    }
    for (row, &count) in state.options[..9]
        .as_chunks_mut::<3>()
        .0
        .iter_mut()
        .zip(&tier_count)
    {
        for (o, &any) in row.iter_mut().zip(&any_tier) {
            if *o != 0 && count == 0 {
                *o = 0;
            }
            if *o == 0 && count != 0 && any != 0 {
                *o = 1;
            }
        }
    }
    ui.season_end(state, SeasonEndEvent::SeparationsDone);
    if !ui.auto_play() {
        for m in 0..n {
            contract_talks(state, rt, rng, ui, m);
        }
    }
    ui.season_end(state, SeasonEndEvent::Done);
    state.current_manager = n;
    season_rebalance_player_clubs(state, rt, rng);
}

/// The report's promotion bonus (and the advertising contracts it makes worth a tenth, unless
/// their partners may cancel), the flag lines, the top scorer's bonus and the European cup
/// line of manager `m`. Returns whether the "Werbepartner" dialog shows; `stale_person` is the
/// loop index the original leaves behind (6 after the advertising loop).
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0040..0300)
fn season_end_bonuses(
    state: &mut GameState,
    rt: &Runtime,
    m: u8,
    top_scorer: bool,
    lines: &mut Vec<&'static [u8]>,
    stale_person: &mut usize,
) -> bool {
    let mu = usize::from(m);
    let flags = state.managers[mu].value_140;
    let mut sponsors_released = false;
    if flags & 1 != 0 {
        let (line, bonus) = if state.managers[mu].league != 0 {
            PROMOTED_TO_SECOND
        } else {
            PROMOTED_TO_FIRST
        };
        lines.push(line);
        let r = &mut state.managers[mu];
        r.balance = r.balance.wrapping_add(bonus);
        let contracts = &mut state.manager_contracts[mu];
        if state.far_0000[2 * mu] == 0 {
            contracts[0] = ldiv(contracts[0], 10);
        } else {
            state.far_0000[2 * mu] = 0;
            sponsors_released = true;
        }
        for j in 0..6 {
            let lock = &mut state.far_4b9a[2 * (6 * mu + j)];
            if *lock == 0 {
                contracts[1 + j] = ldiv(contracts[1 + j], 10);
            }
            *lock = 0;
        }
        *stale_person = 6;
    }
    for (bit, line) in SEASON_END_FLAG_LINES {
        if flags & bit != 0 {
            lines.push(line);
        }
    }
    if top_scorer {
        lines.push(TOP_SCORER.0);
        let r = &mut state.managers[mu];
        r.balance = r.balance.wrapping_add(TOP_SCORER.1);
    }
    if cups::team_is_euro_qualified(state, rt, state.managers[mu].team) {
        lines.push(EURO_QUALIFIED);
    }
    sponsors_released
}

/// Manager `m`'s training work of the season ([`crate::state::Manager::fatigue_sum`]) loses a
/// third each season after the first; above 30 units (in twelfths), with chance 1/3 and fewer
/// than 23 players, a free person aged 17..=19 joins the squad as a youth player with skills
/// around half of it (10..=90). Returns the person.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0300..05a8)
fn season_end_youth_player(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    m: u8,
) -> Option<usize> {
    let mu = usize::from(m);
    let r = &mut state.managers[mu];
    let mut yp = ((r.fatigue_sum as u16) / 12) as i16;
    let year = state.year;
    let modern = rt.modern_start;
    if !((year > 1966 && !modern) || (year > 1995 && modern)) {
        return None;
    }
    yp += yp / -3;
    r.fatigue_sum = yp.wrapping_mul(12);
    if !(yp > 30 && rng.random_range(0, 2) == 0 && count_players_of_owner(state, m) < 23) {
        return None;
    }
    yp >>= 1;
    let p = loop {
        let p = rng.random_range(1, 0x96) as usize;
        if state.players[p].owner_manager == NO_OWNER {
            break p;
        }
    };
    clamp_int(&mut yp, 10, 90);
    let person = &mut state.players[p];
    person.attr_20 = rng.random_range(0, 6) as u8;
    person.age = rng.random_range(0x11, 0x13) as u8;
    person.skills[2] = rng.random_range(0x2d, 0x37) as u8;
    person.skills[1] = rng.random_range(yp - 3, yp + 3) as u8;
    person.skills[0] = rng.random_range(yp - 3, yp + 3) as u8;
    // The club byte gets the manager's index; the insertion moves him to the manager's club.
    person.team = m;
    let slot = squad_insert_player(state, rng, p as u8, 0, 0, SQUAD_SCAN as u8)
        .expect("fewer than 23 players leave room in the squad");
    state.players[p].owner_manager = m;
    let value = player_value_calc(state, rng, slot, 1);
    let s = &mut state.squads[mu][usize::from(slot)];
    s.market_value = ldiv(lmul(value, 50), 100);
    s.contract_years = rng.random_range(2, 3) as u8;
    Some(p)
}

/// The "Trennungen" pass of manager 0: every person gets a year older; a squad player's
/// message goes, his transfer flags clear and his contract loses a season (in automatic play
/// he gets a random age of 18..=25 instead), and a player held by a squad other than his
/// owner's returns to the owner (or becomes free).
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0726..0925)
fn separations(state: &mut GameState, rng: &mut Rng, auto_play: bool) {
    let saved = state.current_manager;
    // [BP-0x80]: the record the last returned player got.
    let mut ns: usize = 0;
    for q in 1..0x97usize {
        let found = find_player_by_id(state, q as u8);
        state.players[q].age = state.players[q].age.wrapping_add(1);
        let Some((g, sl)) = found else {
            continue;
        };
        let msg = state.squads[g][sl].message_ptr;
        if msg != 0 {
            message_delete(state, msg);
            state.squads[g][sl].message_ptr = 0;
        }
        let s = &mut state.squads[g][sl];
        s.status &= 0x3f;
        if auto_play {
            state.players[q].age = rng.random_range(0x12, 0x19) as u8;
            state.squads[g][sl].negotiation = 0;
        } else {
            s.contract_years = s.contract_years.wrapping_sub(1);
        }
        let owner = state.players[q].owner_manager;
        if u16::from(owner) == g as u16 {
            continue;
        }
        state.current_manager = owner as i8;
        state.saved_manager = owner as i8;
        if (owner as i8) < 4 {
            match squad_insert_player(state, rng, q as u8, 1, 0, SQUAD_SCAN as u8 + 1) {
                Ok(slot) => {
                    ns = usize::from(slot);
                    let record = state.squads[g][sl].clone();
                    state.squads[usize::from(owner)][ns] = record;
                }
                // TODO(re): the original copies the record to index 0x7f of the owner's
                // group (past the five groups) and later clears record 0x7f of squad 0.
                Err(_) => ns = 0x7f,
            }
            state.players[q].owner_manager = owner;
        } else {
            state.players[q].owner_manager = NO_OWNER;
        }
        state.current_manager = g as i8;
        // The record index, not the group, is compared with the transfer list (season.md §7 Q9).
        let count = if sl == TRANSFER_LIST {
            TRANSFER_LIST_SCAN
        } else {
            SQUAD_SCAN
        };
        squad_remove_player(state, sl as u8, true, count as u8);
        state.saved_manager = saved;
        state.current_manager = saved;
        // Squad 0, whoever the owner was (season.md §7 Q9).
        if let Some(s) = state.squads[usize::from(saved as u8)].get_mut(ns) {
            s.negotiation = 0;
            s.loan = 0;
        }
    }
}

/// The retirements of manager `m`'s turn: a squad player of `m` whose contract ended and who
/// wants to leave retires (his message shows), and a free person older than 32..=34 is
/// replaced; the new person is 18..=25 with skills around a random 30..=92. The new position
/// goes to `stale_person` (season.md §7 Q11). At the last manager every person's league goals
/// and games are cleared.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0925..0b0b)
fn retirements<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rng: &mut Rng,
    ui: &mut U,
    m: i8,
    n: i8,
    stale_person: usize,
) {
    for q in 1..0x97usize {
        if m == n - 1 {
            state.players[q].counters = [0, 0];
        }
        let found = find_player_by_id(state, q as u8);
        if found.is_some_and(|(g, _)| g as i8 != state.current_manager) {
            continue;
        }
        let lim = rng.random_range(0x20, 0x22);
        let regen = match found {
            None => i16::from(state.players[q].age) > lim,
            // TODO(re): season.md §7 Q10, without a record the original tests two bytes of the
            // interrupt vector table; taken as "no".
            Some((g, sl)) => {
                let s = &state.squads[g][sl];
                s.contract_years == 0 && s.negotiation & LEAVES != 0
            }
        };
        if !regen {
            continue;
        }
        if let Some((g, sl)) = found {
            let pid = usize::from(state.squads[g][sl].player_id);
            let name = state.players[pid].name().to_vec();
            let age = i32::from(state.players[pid].age);
            let args = [MessageArg::Text(&name), MessageArg::Number(age)];
            let msg = message_post(state, rng, ui.number_format(), false, 3, &args);
            ui.season_end(state, SeasonEndEvent::Retirement { message: &msg.text });
            message_delete(state, msg.pointer);
            state.squads[g][sl].message_ptr = 0;
            squad_remove_player(state, sl as u8, true, SQUAD_SCAN as u8);
            state.players[q].owner_manager = NO_OWNER;
        }
        state.players[q].age = rng.random_range(0x12, 0x19) as u8;
        let x = rng.random_range(0x1e, 0x5c);
        state.players[q].skills[0] = rng.random_range(x - 5, x + 5) as u8;
        state.players[stale_person].attr_20 = rng.random_range(0, 6) as u8;
        state.players[q].skills[1] = rng.random_range(x - 5, x + 5) as u8;
        state.current_manager = state.saved_manager;
    }
}

/// The "Vertragsverhandlungen" of manager `m`: every player whose contract ended is offered a
/// new one; one who leaves brings half his value in thousands as a fee.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0fb3..12fa)
fn contract_talks<U: SeasonUi + ?Sized>(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    ui: &mut U,
    m: i8,
) {
    state.current_manager = m;
    state.saved_manager = m;
    let g = m as usize;
    let cnt = squad_count_listed(state, g);
    let expiring = state.squads[g][..usize::from(cnt)]
        .iter()
        .any(|s| s.contract_years == 0);
    ui.season_end(
        state,
        SeasonEndEvent::Contracts {
            m: m as u8,
            expiring,
        },
    );
    let mut sl = 0u8;
    for _ in 0..cnt {
        let s = &state.squads[g][usize::from(sl)];
        if s.contract_years != 0 {
            sl += 1;
            continue;
        }
        let pid = s.player_id;
        if ui.contract_negotiation(state, rt, rng, sl) {
            ui.season_end(state, SeasonEndEvent::Stays { pid });
            sl += 1;
        } else {
            let value = player_value_calc(state, rng, sl, 0);
            let fee = lmul(ldiv(value >> 1, 1000), 1000);
            let r = &mut state.managers[g];
            r.balance = r.balance.wrapping_add(fee);
            ui.season_end(state, SeasonEndEvent::Leaves { pid, fee });
            squad_remove_player(state, sl, true, SQUAD_SCAN as u8);
        }
        ui.season_end(state, SeasonEndEvent::ContractDone { m: m as u8 });
    }
}

/// Spreads the persons of the clubs no human runs over the four tiers again: the yearly pool
/// shuffle ([`player_pool_season_shuffle`]) says how many free places each league tier has;
/// the persons already in a tier fill them, and the rest of each tier's places are filled by
/// random persons of the other tiers moved to a random free club of the tier
/// ([`pick_random_free_team_in_tier`]). The shuffle runs again at the end.
///
/// Origin: BMMAIN.EXE 1cb5:2756 (season_rebalance_player_clubs)
pub fn season_rebalance_player_clubs(state: &mut GameState, rt: &Runtime, rng: &mut Rng) {
    // The rankings are computed for pick_random_free_team_in_tier, which never reads them.
    let quota = player_pool_season_shuffle(state, rng);
    let mut want = [quota[0], quota[1], quota[2], 0];
    let mut pool = [[0xffu8; 4]; 0x97];
    let mut have = [0i16; 4];
    for k in 0..4 {
        let [lo, hi] = rt.tier_club_ranges[k];
        for p in 1..0x97usize {
            let person = &state.players[p];
            if person.team < lo || person.team > hi {
                continue;
            }
            if want[k] != 0 {
                want[k] -= 1;
            }
            if person.owner_manager <= 3 {
                continue;
            }
            pool[have[k] as usize][k] = p as u8;
            have[k] += 1;
        }
    }
    let mut avail = have;
    for (k, wanted) in want.iter_mut().enumerate() {
        let mut guard = 0;
        while *wanted > 0 && {
            guard += 1;
            guard - 1 < 1000
        } {
            for j in 0..4 {
                if j == k {
                    continue;
                }
                while avail[j] > 0 && *wanted > 0 {
                    let mut tries = 0;
                    let r = loop {
                        let r = rng.random_range(0, have[j] - 1) as usize;
                        tries += 1;
                        if pool[r][j] != 0xff || tries > 1000 {
                            break r;
                        }
                    };
                    let p = pool[r][j];
                    pool[r][j] = 0xff;
                    let team = pick_random_free_team_in_tier(state, rt, rng, k);
                    // TODO(re): without a person left after 1001 draws the original writes the
                    // club of person 0xff, past the table.
                    if let Some(person) = state.players.get_mut(usize::from(p)) {
                        person.team = team;
                    }
                    avail[j] -= 1;
                    *wanted -= 1;
                }
            }
        }
    }
    player_pool_season_shuffle(state, rng);
}

/// A random club of tier `k` that no human manager runs. Inside the three league tiers the
/// draw favours the first places of the tier: positions 0..3, with chance 1/2 0..6 instead,
/// and with chance 1/6 0..12 instead of either. Beyond the leagues (`k > 2`) any club from the
/// tier's first index up to 255 is drawn once, without the manager check.
///
/// The original also takes a list of the tier's clubs ranked by strength (built by
/// BMMAIN.EXE 1cb5:2a3b) but never reads it.
///
/// Origin: BMMAIN.EXE 1cb5:2987 (pick_random_free_team_in_tier)
pub fn pick_random_free_team_in_tier(
    state: &GameState,
    rt: &Runtime,
    rng: &mut Rng,
    k: usize,
) -> u8 {
    if k > 2 {
        return rng.random_range(i16::from(rt.tier_club_ranges[k][0]), 0xff) as u8;
    }
    loop {
        let mut t = rng.random_range(0, 3) as u8;
        if rng.random_range(0, 1) != 0 {
            t = rng.random_range(0, 6) as u8;
        }
        if rng.random_range(0, 5) == 0 {
            t = rng.random_range(0, 12) as u8;
        }
        let t = t.wrapping_add(TIER_FIRST[k]);
        if state.manager_index_of_team(t, 0xff) == 0 {
            return t;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::savegame::HIGHSCORE_FILE_LEN;

    #[test]
    fn pick_never_returns_a_human_club() {
        let mut s = GameState::zeroed();
        let rt = Runtime::default();
        s.manager_count = 4;
        for m in 0..4 {
            s.managers[m].team = 18 + m as u8;
        }
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let t = pick_random_free_team_in_tier(&s, &rt, &mut rng, 1);
            assert!((22..=30).contains(&t), "{t}");
        }
    }

    #[test]
    fn pick_draws_three_to_five_numbers_per_try() {
        let s = GameState::zeroed();
        let rt = Runtime::default();
        for seed in 0..50 {
            let mut rng = Rng::new(seed);
            let mut probe = rng;
            let t = pick_random_free_team_in_tier(&s, &rt, &mut rng, 2);
            let mut draws = 0;
            while probe != rng {
                probe.rand();
                draws += 1;
            }
            assert!((3..=5).contains(&draws), "seed {seed}: {draws}");
            assert!((38..=50).contains(&t));
        }
    }

    use crate::finance::finance_daily_processing;
    use crate::rng::NumberFormat;
    use crate::test_saves::save_states;
    use bmp_formats::savegame::SaveGame;

    fn advanced(mut rng: Rng, n: usize) -> Rng {
        for _ in 0..n {
            rng.rand();
        }
        rng
    }

    fn draws(mut from: Rng, to: Rng) -> usize {
        let mut n = 0;
        while from != to {
            from.rand();
            n += 1;
            assert!(n < 100_000, "the generator did not reach the end state");
        }
        n
    }

    fn date(day: i32, start_year: u16) -> (i32, i32, i32) {
        let mut s = GameState::zeroed();
        s.day_counter = day;
        s.start_year = start_year;
        calendar_compute_date(&mut s, 0);
        (s.day_of_month, s.month, s.year)
    }

    #[test]
    fn calendar_matches_the_saves() {
        // 07072019.MAN: day 88 of the 1965 season is Wednesday, 25 October 1965.
        assert_eq!(date(88, 1965), (25, 9, 1965));
        // 2021-AUG.MAN: day 207 of 1967 is 21 February 1968.
        assert_eq!(date(207, 1967), (21, 1, 1968));
        // Day 0 is 29 July (0xd2 = 210 days into the year).
        assert_eq!(date(0, 1990), (29, 6, 1990));
    }

    #[test]
    fn fixture_index_dates() {
        let mut s = GameState::zeroed();
        s.start_year = 1965;
        calendar_compute_date(&mut s, 3);
        // Week 1 + 4 days: 7 + 4 = 11 days after day 0.
        assert_eq!((s.day_of_month, s.month), (9, 7));
    }

    /// Records what the loop shows; every office turn is answered with `Ok` until `stop_at`
    /// turns, and with `Err(Done)` from the first turn of a later season on.
    struct Headless {
        start_year: u16,
        turns: u32,
        stop_at: u32,
        /// The current manager and the argument of every office turn.
        office_turns: Vec<(i8, u8)>,
        draws_asked: Vec<u8>,
        titles: Vec<Title>,
        dates: Vec<SpecialDate>,
        champions: Vec<u8>,
        forfeits: Vec<u8>,
        finance: Vec<(u8, FinanceEvent)>,
        fmt: NumberFormat,
        auto_play: bool,
        scenes: Vec<(bool, u8, u8)>,
        scene_result: u8,
        substitutions: Vec<(u8, u8)>,
        season_end: Vec<String>,
        negotiations: Vec<(i8, u8)>,
        keep_contracts: bool,
        table: HighScoreTable,
        highscore_inserts: Vec<i8>,
        winter_passes: u32,
        game_over: bool,
        /// The office turn (1-based) in which a manager is added, as sub icon 5/5 does.
        add_manager_at: u32,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Done;

    impl Headless {
        fn new(state: &GameState) -> Self {
            Headless {
                start_year: state.start_year,
                turns: 0,
                stop_at: u32::MAX,
                office_turns: Vec::new(),
                draws_asked: Vec::new(),
                titles: Vec::new(),
                dates: Vec::new(),
                champions: Vec::new(),
                forfeits: Vec::new(),
                finance: Vec::new(),
                fmt: NumberFormat::default(),
                auto_play: false,
                scenes: Vec::new(),
                scene_result: 0,
                substitutions: Vec::new(),
                season_end: Vec::new(),
                negotiations: Vec::new(),
                keep_contracts: true,
                table: HighScoreTable::parse(&[0; HIGHSCORE_FILE_LEN]).unwrap(),
                highscore_inserts: Vec::new(),
                winter_passes: 0,
                game_over: false,
                add_manager_at: 0,
            }
        }
    }

    fn narrow(s: &[u8]) -> i16 {
        (s.len() * 6) as i16
    }

    impl SeasonUi for Headless {
        type Stop = Done;
        fn office_turn(
            &mut self,
            state: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            arg: u8,
        ) -> Result<(), Done> {
            self.turns += 1;
            self.office_turns.push((state.current_manager, arg));
            if self.turns == self.add_manager_at {
                state.manager_count += 1;
            }
            if state.start_year != self.start_year || self.turns > self.stop_at {
                return Err(Done);
            }
            Ok(())
        }
        fn title(&mut self, _: &GameState, title: Title) -> Result<(), Done> {
            self.titles.push(title);
            Ok(())
        }
        fn winter_break(&mut self) -> Result<(), Done> {
            self.winter_passes += 1;
            Ok(())
        }
        fn special_date(&mut self, _: &GameState, date: SpecialDate) -> Result<(), Done> {
            self.dates.push(date);
            Ok(())
        }
        fn championship_winner(&mut self, _: &GameState, m: u8) -> Result<(), Done> {
            self.champions.push(m);
            Ok(())
        }
        fn clear_screen(&mut self) {}
        fn highscores(&mut self, _: &GameState) -> HighScoreTable {
            self.table.clone()
        }
        fn highscore_insert(&mut self, state: &GameState, _: &mut Runtime, _: &mut Rng) {
            self.highscore_inserts.push(state.current_manager);
        }
        fn game_end(&mut self, _: &GameState, table: &HighScoreTable) -> Result<(), Done> {
            self.table = table.clone();
            self.game_over = true;
            Ok(())
        }
        fn ask_draw(&mut self, cup: u8) -> bool {
            self.draws_asked.push(cup);
            false
        }
        fn forfeit_penalty(&mut self, _: &GameState, m: u8) {
            self.forfeits.push(m);
        }
        fn finance_events(&mut self, _: &GameState, m: u8, events: &[FinanceEvent]) {
            self.finance.extend(events.iter().map(|e| (m, e.clone())));
        }
        fn number_format(&mut self) -> &mut NumberFormat {
            &mut self.fmt
        }
        fn auto_play(&self) -> bool {
            self.auto_play
        }
        fn goal_scene(
            &mut self,
            _: &GameState,
            _: &mut Rng,
            goal: bool,
            side: u8,
            penalty: u8,
        ) -> u8 {
            self.scenes.push((goal, side, penalty));
            self.scene_result
        }
        fn substitution(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            m: u8,
            cup_mode: u8,
            _: bool,
        ) {
            self.substitutions.push((m, cup_mode));
        }
        fn season_end(&mut self, _: &GameState, event: SeasonEndEvent<'_>) {
            self.season_end.push(format!("{event:?}"));
        }
        fn contract_negotiation(
            &mut self,
            state: &mut GameState,
            _: &Runtime,
            _: &mut Rng,
            slot: u8,
        ) -> bool {
            self.negotiations.push((state.current_manager, slot));
            self.keep_contracts
        }
        fn news_env(&self) -> NewsEnv<'_> {
            NewsEnv {
                fmt: NumberFormat::default(),
                text_width: &narrow,
            }
        }
        fn show_newspapers(&mut self, _: &GameState, _: Vec<Newspaper>) {}
    }

    /// Two managers of clubs 3 and 20, without players.
    fn two_managers() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.start_year = 1965;
        s.managers[0].team = 3;
        s.managers[1].team = 20;
        s.managers[1].league = 1;
        s.lineup_state = [[1, 4], [1, 2], [0, 0], [0, 0]];
        s
    }

    /// The inbox texts of manager `m` after the date line.
    fn inbox(s: &GameState, m: usize) -> Vec<Vec<u8>> {
        s.messages
            .get(m)
            .into_iter()
            .flatten()
            .map(|e| {
                let text = e.bytes.strip_suffix(&[0]).unwrap();
                let date = text.iter().position(|&c| c == b'^').unwrap();
                text[date + 1..].to_vec()
            })
            .collect()
    }

    /// [`two_managers`] with manager 1 to move and player 9 "OTTO" (30, skills 50) in their
    /// squad.
    fn weekly_state() -> GameState {
        let mut s = two_managers();
        s.current_manager = 1;
        s.day_counter = 10;
        s.level = 2;
        s.players[9].name[..4].copy_from_slice(b"OTTO");
        s.players[9].age = 30;
        s.players[9].position_value = 40;
        s.players[9].owner_manager = 1;
        let p = &mut s.squads[1][0];
        p.player_id = 9;
        p.skills = [50, 50, 50];
        p.tendency = 50;
        p.traits = 0x15;
        p.exhaustion = 100;
        p.contract_years = 2;
        p.loan = 1;
        s
    }

    #[test]
    fn the_weekly_events_end_after_day_321() {
        let mut s = weekly_state();
        s.day_counter = 0x142;
        s.managers[1].value_13e = 1;
        let (want, mut rng) = (s.clone(), Rng::new(4));
        random_weekly_events(
            &mut s,
            &Runtime::default(),
            &mut rng,
            &mut NumberFormat::default(),
        );
        assert!(s == want);
        assert_eq!(rng, Rng::new(4));
    }

    #[test]
    fn the_weekly_events_run_their_parts_in_order() {
        for (m, day) in [(0, 10), (1, 10), (1, 0x141)] {
            let rt = Runtime::default();
            let mut s = weekly_state();
            s.current_manager = m;
            s.day_counter = day;
            s.squads[0][0] = s.squads[1][0].clone();
            s.squads[0][0].loan = 0;
            s.squads[0][0].contract_years = 1;
            s.squads[TRANSFER_LIST][0] = s.squads[1][0].clone();
            s.squads[TRANSFER_LIST][0].goals[0] = 1 << m;
            s.team_slots[2][4] = 1;
            for r in &mut s.managers[..2] {
                r.value_13e = 1;
                r.stadium[6][0] = 3;
            }
            let mut rng = Rng::new(21);
            let (mut want, mut want_rng) = (s.clone(), rng);
            let (mut fmt, mut want_fmt) = (NumberFormat::default(), NumberFormat::default());
            random_weekly_events(&mut s, &rt, &mut rng, &mut fmt);

            let mu = m as u8;
            if m == 0 {
                transfer_list_upkeep(&mut want, &mut want_rng);
            }
            listed_player_bids(&mut want, &rt, &mut want_rng, &mut want_fmt, mu);
            hooligan_damage(&mut want, &mut want_rng, &mut want_fmt, mu);
            comfort_downgrade(&mut want, &mut want_rng, &mut want_fmt, mu);
            if day < SEASON_LAST_DAY {
                squad_week(&mut want, &rt, &mut want_rng, &mut want_fmt, mu);
                squad_player_bids(&mut want, &mut want_rng, &mut want_fmt, mu);
            }
            training_week(&mut want, &rt, &mut want_rng, mu);
            auto_lineup(&mut want, rt.lineup_rules);
            assert!(s == want, "manager {m}, day {day}");
            assert_eq!((rng, fmt), (want_rng, want_fmt));
            assert_eq!(
                s.team_slots[2][4],
                u8::from(m != 0),
                "only manager 0 counts down"
            );
        }
    }

    #[test]
    fn a_refused_bid_of_one_manager_may_be_forgotten() {
        let seed = (0..u16::MAX)
            .find(|&seed| Rng::new(seed).random_range(0, 2) == 0)
            .unwrap();
        for (mask, kept) in [(0x02, 0x00), (0x82, 0x80), (0x03, 0x03)] {
            let mut s = weekly_state();
            s.players[9].owner_manager = 0;
            s.squads[TRANSFER_LIST][0] = s.squads[1][0].clone();
            s.squads[TRANSFER_LIST][0].goals[0] = mask;
            let rt = Runtime::default();
            listed_player_bids(
                &mut s,
                &rt,
                &mut Rng::new(seed),
                &mut NumberFormat::default(),
                1,
            );
            assert_eq!(s.squads[TRANSFER_LIST][0].goals[0], kept, "mask {mask:#x}");
        }
    }

    /// [`weekly_state`] with every club named by a letter, so the offer text names the club,
    /// and manager 1 as the saved manager (as the listed-player bids leave it), so the
    /// messages go to their inbox.
    fn offer_state() -> GameState {
        let mut s = weekly_state();
        s.saved_manager = 1;
        for (t, club) in s.clubs.iter_mut().enumerate() {
            club.name[0] = b'A' + (t % 26) as u8;
        }
        s
    }

    fn offer_text(s: &GameState, club: u8) -> Vec<u8> {
        let mut text = s.clubs[usize::from(club)].name().to_vec();
        text.extend_from_slice(b"^ist an OTTO^interessiert.^");
        text
    }

    #[test]
    fn a_club_bids_for_a_listed_player() {
        let mut offers = 0;
        for (seed, message_ptr) in (0..600).map(|seed| (seed, 0)).chain([(5, 77), (6, 77)]) {
            let mut s = offer_state();
            s.squads[TRANSFER_LIST][0] = s.squads[1][0].clone();
            s.squads[TRANSFER_LIST][0].message_ptr = message_ptr;
            s.current_manager = TRANSFER_LIST as i8;
            let q = player_value_calc(&s, &mut Rng::new(0), 0, 0) / 10_000;
            assert_eq!(q, 56, "skills 50 at age 30");
            s.current_manager = 1;
            let before = s.clone();

            let mut want = Rng::new(seed);
            // random_range(0, 180) is rand() % 181, random_range(0, 2) is rand() % 3.
            let bid = (want.rand() % 181) < 56 && want.rand() % 3 != 0 && message_ptr == 0;
            let club = bid.then(|| pick_team_for_player(&before, &mut want, 56, false));
            if bid {
                want.rand(); // message_post's date
            }
            let mut rng = Rng::new(seed);
            listed_player_bids(
                &mut s,
                &Runtime::default(),
                &mut rng,
                &mut NumberFormat::default(),
                1,
            );
            assert_eq!(rng, want, "seed {seed}");
            let p = &s.squads[TRANSFER_LIST][0];
            assert_eq!(s.current_manager, 1);
            match club {
                Some(club) => {
                    offers += 1;
                    assert_eq!((p.status, p.sale_club), (OFFER_FOR_LISTED, club));
                    assert_eq!(inbox(&s, 1), [offer_text(&s, club)]);
                    assert_eq!(p.message_ptr, s.messages[1][0].old_pointer);
                }
                None => {
                    assert_eq!((p.status, p.sale_club), (0, 0), "seed {seed}");
                    assert_eq!(p.message_ptr, message_ptr);
                    assert!(inbox(&s, 1).is_empty());
                }
            }
        }
        // 56/181 * 2/3 of 600 is about 124.
        assert!((95..=155).contains(&offers), "{offers}");
    }

    #[test]
    fn an_offer_lapses_with_chance_one_in_four() {
        // random_range(0, 3) is rand() % 4.
        let lapses = (0..u16::MAX)
            .find(|&seed| Rng::new(seed).rand() % 4 == 0 && Rng::new(seed).rand() % 5 != 0)
            .unwrap();
        let stays = (0..u16::MAX)
            .find(|&seed| Rng::new(seed).rand() % 4 != 0 && Rng::new(seed).rand() % 5 == 0)
            .unwrap();
        for (seed, status, lapsed) in [
            (lapses, OFFER_FOR_LISTED | 5, true),
            (lapses, OFFER_FOR_SQUAD_PLAYER, true),
            (lapses, 0x1f, false),
            (stays, OFFER_FOR_LISTED | 5, false),
        ] {
            let mut s = offer_state();
            let msg = message_post(
                &mut s,
                &mut Rng::new(0),
                &mut NumberFormat::default(),
                false,
                1,
                &[MessageArg::Text(b"A"), MessageArg::Text(b"OTTO")],
            )
            .pointer;
            s.squads[1][0].status = status;
            s.squads[1][0].message_ptr = msg;
            let mut rng = Rng::new(seed);
            drop_lapsed_offer(&mut s, &mut rng, 1, 0);
            assert_eq!(rng, advanced(Rng::new(seed), 1));
            let p = &s.squads[1][0];
            if lapsed {
                assert_eq!((p.status, p.message_ptr), (status & 0x1f, 0));
                assert!(inbox(&s, 1).is_empty());
            } else {
                assert_eq!(
                    (p.status, p.message_ptr),
                    (status, msg),
                    "status {status:#x}"
                );
                assert_eq!(inbox(&s, 1).len(), 1);
            }
        }
    }

    #[test]
    fn a_club_rarely_bids_for_a_squad_player() {
        // random_range(0, 200) is rand() % 201, random_range(0, 450) is rand() % 451 and
        // random_range(0, 12) is rand() % 13.
        let bids = |q: i16, abroad: bool| {
            (0..u16::MAX).find(move |&seed| {
                let mut r = Rng::new(seed);
                let bid = r.rand() % 201 < q && {
                    let x = r.rand();
                    x != 0 && x % 451 == 0
                };
                bid && (r.rand() % 13 == 0) == abroad
            })
        };
        // Value 56 (skills 50), 90 (skills 86) and 91 (skills 87): only above 90 may the club
        // come from abroad, and only then is the draw for it made.
        for (skills, q, abroad, foreign) in [
            (50, 56, false, false),
            (86, 90, true, false),
            (87, 91, false, false),
            (87, 91, true, true),
        ] {
            let mut s = offer_state();
            s.squads[1][0].loan = 0;
            s.squads[1][0].skills = [skills; 3];
            assert_eq!(
                player_value_calc(&s, &mut Rng::new(0), 0, 0) / 10_000,
                i32::from(q)
            );
            let seed = bids(q, abroad).unwrap();
            let before = s.clone();
            let mut want = advanced(Rng::new(seed), 2 + usize::from(q > 90));
            let club = pick_team_for_player(&before, &mut want, q as u8, foreign);
            want.rand(); // message_post's date
            let mut rng = Rng::new(seed);
            squad_player_bids(&mut s, &mut rng, &mut NumberFormat::default(), 1);
            assert_eq!(rng, want, "value {q}, abroad {abroad}");
            assert_eq!(club > 0x3f, foreign);
            let p = &s.squads[1][0];
            assert_eq!((p.status, p.sale_club), (OFFER_FOR_SQUAD_PLAYER, club));
            assert_eq!(inbox(&s, 1), [offer_text(&s, club)]);
            assert_eq!(p.message_ptr, s.messages[1][0].old_pointer);
        }
        let mut s = offer_state();
        s.squads[1][0].loan = 0;
        let miss = (0..u16::MAX)
            .find(|&seed| {
                let mut r = Rng::new(seed);
                r.rand() % 201 < 56 && r.rand() % 451 != 0
            })
            .unwrap();
        let mut rng = Rng::new(miss);
        squad_player_bids(&mut s, &mut rng, &mut NumberFormat::default(), 1);
        assert_eq!(rng, advanced(Rng::new(miss), 2));
        assert_eq!(s.squads[1][0].status, 0);
    }

    #[test]
    fn a_club_from_abroad_offers_to_extend_a_one_year_contract() {
        // Age 17 (a = 0), skills 5 and value terms 200 (b = 5): c = 3 - 1 = 2. The draws are
        // random_range(0, 2) (rand() % 3), random_range(0, 3) (rand() % 4) and
        // random_range(0, 100) (rand() % 101), whose 71..=90 give 3 years and 91..=100 give 4.
        let offer = |r: i16| {
            (0..u16::MAX)
                .find(|&seed| {
                    let mut g = Rng::new(seed);
                    g.rand() % 3 == 0 && g.rand() % 4 == 0 && g.rand() % 101 == r
                })
                .unwrap()
        };
        for (r, contract, years) in [
            (70, 1, Some(2)),
            (71, 1, Some(3)),
            (90, 1, Some(3)),
            (91, 1, Some(4)),
            (100, 1, Some(4)),
            (91, 2, None),
        ] {
            let mut s = offer_state();
            s.players[9].age = 17;
            let p = &mut s.squads[1][0];
            p.skills = [5; 3];
            p.value_terms = [100, 100, 0];
            p.contract_years = contract;
            let seed = offer(r);
            let mut rng = Rng::new(seed);
            offer_from_abroad(&mut s, &mut rng, &mut NumberFormat::default(), 1, 0);
            let p = &s.squads[1][0];
            match years {
                Some(years) => {
                    assert_eq!(rng, advanced(Rng::new(seed), 4));
                    assert_eq!(p.negotiation, 100 + years, "draw {r}");
                    let text = format!("OTTO bietet an,^von 1 auf {years}^Jahre zu verl{{ngern.^");
                    assert_eq!(inbox(&s, 1), [text.into_bytes()]);
                    assert_eq!(p.message_ptr, s.messages[1][0].old_pointer);
                }
                None => {
                    assert_eq!(rng, advanced(Rng::new(seed), 2));
                    assert_eq!((p.negotiation, p.message_ptr), (0, 0));
                    assert!(inbox(&s, 1).is_empty());
                }
            }
        }
    }

    #[test]
    fn hooligans_damage_the_stadium() {
        let mut s = weekly_state();
        let r = &mut s.managers[1];
        r.value_13e = 1;
        r.balance = 100_000;
        r.stadium[1][0] = 1000;
        r.stadium[0][0] = 3001;
        r.stadium[5][0] = 1;
        let mut rng = Rng::new(4);
        let mut want = rng;
        let k = i32::from(want.random_range(2, 5));
        want.random_range(0, 3);
        hooligan_damage(&mut s, &mut rng, &mut NumberFormat::default(), 1);
        assert_eq!(rng, want);
        let damage = k * 5500 / 1000 * 1000;
        let r = &s.managers[1];
        assert_eq!(
            (r.balance, r.value_13e, r.stadium[5][0]),
            (100_000 - damage, 0, 1)
        );
        let mut text = b"Randalierer im Stadion^richteten einen Sachschaden^von ".to_vec();
        text.extend(format_long_thousands(damage, NumberFormat::default()));
        text.extend_from_slice(b" DM an. ^");
        assert_eq!(inbox(&s, 1), [text]);
        assert_eq!(s.team_slots[EVENT_MESSAGE_ROW as usize][0], 3);
    }

    #[test]
    fn a_bundesliga_stadium_may_lose_a_comfort_grade() {
        let seed = (0..u16::MAX)
            .find(|&seed| Rng::new(seed).random_range(0, 0x1ba - 2 * 0x34) == 0)
            .unwrap();
        for (league, comfort, lost) in [(0, 2, 1), (1, 2, 0), (0, 1, 0)] {
            let mut s = weekly_state();
            s.managers[1].league = league;
            s.managers[1].stadium[6][0] = comfort;
            comfort_downgrade(&mut s, &mut Rng::new(seed), &mut NumberFormat::default(), 1);
            assert_eq!(
                s.managers[1].stadium[6][0],
                comfort - lost,
                "league {league}"
            );
            assert_eq!(inbox(&s, 1).len(), lost as usize);
        }
    }

    #[test]
    fn an_old_player_announces_that_he_leaves() {
        for m in [1, 3] {
            let mut s = weekly_state();
            s.manager_count = 4;
            s.squads.swap(1, m);
            s.current_manager = m as i8;
            s.saved_manager = 0;
            s.players[9].age = 60;
            s.squads[m][0].status = 1; // banned: no training injury
            squad_week(
                &mut s,
                &Runtime::default(),
                &mut Rng::new(8),
                &mut NumberFormat::default(),
                m as u8,
            );
            assert_eq!(s.squads[m][0].negotiation, LEAVES);
            if m == 1 {
                let text = b"OTTO k}ndigt an,^da~ er seinen Vertrag nicht^mehr verl{ngern wird.^";
                assert_eq!(inbox(&s, 1), [text.to_vec()]);
                assert_eq!(s.team_slots[1][0], 3);
                assert_eq!(s.message_pointers[1][0], s.messages[1][0].old_pointer);
            } else {
                assert!(inbox(&s, 3).is_empty(), "no slot row for manager 3");
            }
        }
    }

    #[test]
    fn training_runs_down_the_form_trend() {
        for (traits, rerolled) in [(0x95, false), (0x01, true)] {
            let mut s = weekly_state();
            s.squads[1][0].traits = traits;
            training_week(&mut s, &Runtime::default(), &mut Rng::new(2), 1);
            let p = &s.squads[1][0];
            if rerolled {
                assert!((0x23..=0x41).contains(&p.tendency));
                assert!((1..=0x0d).contains(&(p.traits & 0x0f)));
            } else {
                assert_eq!(p.traits, 0x94);
                assert!(
                    [50, 51].contains(&p.tendency),
                    "the trend adds 1 or nothing"
                );
            }
            assert!((0x3c..=0x96 + 4).contains(&p.exhaustion));
            assert!(p.skills.iter().all(|&v| (1..=99).contains(&v)));
            assert!((45..=55).contains(&p.skills[2]) || p.skills[2] == 50);
        }
    }

    #[test]
    fn training_draws_in_the_original_order() {
        for (seed, level, day, exhaustion) in [
            (1, 1, 10, 100),
            (2, 3, 10, 0x8c),
            (3, 3, 0x90, 0x8c),
            (4, 1, 0x90, 100),
            (5, 4, 40, 0x90),
            (6, 4, 40, 0x40),
        ] {
            let mut s = weekly_state();
            s.level = level;
            s.day_counter = day;
            s.squads[1][0].exhaustion = exhaustion;
            let r = &mut s.managers[1];
            r.training_141 = [3, 1, 2, 0, 4];
            r.training_146 = [2, 3, 1, 0];
            let mut rng = Rng::new(seed);
            let mut want = rng;
            let mut w = s.squads[1][0].clone();
            // Hand-written replay of 1cb5:1fe4..25c8 for the one player.
            let winter = day > 0x82 && day < 0xcf;
            let mix = [
                80 * 60 + 30 * 20 + 20 * 40,
                5 * 60 + 40 * 20 + 25 * 40,
                15 * 60 + 30 * 20 + 55 * 40,
            ]
            .map(|v: i16| v / 400);
            let mut inten: i16 = 4;
            if w.exhaustion > 0x32 {
                w.exhaustion -= want.random_range(3, 7) as u8;
            }
            if winter {
                inten = 6;
            }
            w.exhaustion = w.exhaustion.wrapping_add(((inten - 5) * 2) as u8);
            if w.exhaustion > 0x82 && inten < 6 {
                w.exhaustion -= want.random_range(1, 3) as u8;
            }
            inten *= 20;
            w.exhaustion = w.exhaustion.clamp(0x3c, 0x96);
            if w.exhaustion > 0x87 {
                let l = i16::from(level as i8);
                inten = (inten - want.random_range(70 - 13 * l, 5 * (18 - l))).max(10);
            }
            let base = 0x25;
            for (j, mix) in mix.into_iter().enumerate() {
                let mut t = mix * inten / 100 * 50 / 50;
                t = t * (10 * 3 + 75 - want.random_range(0, 3)) / 100;
                let mut target = i16::from(want.random_range(0, 4) == 4);
                let late = winter;
                if level > 2 && late {
                    if want.random_range(0, 2) == 0 {
                        target += 1;
                    }
                    target = want.random_range(base - target - 1, base + 2);
                    w.exhaustion += want.random_range(0, 2) as u8;
                } else {
                    target = want.random_range(base - target - 2, base + 2);
                }
                if level < 2 && late {
                    w.exhaustion += want.random_range(0, 2) as u8;
                }
                let mut dir = 0;
                if t < target - 8 && want.random_range(0, 1) != 0 {
                    dir = -1;
                    target = (target - t + 8) / 11;
                } else if t > target + 8 && want.random_range(0, 1) != 0 {
                    dir = 1;
                    target = (t - target + 8) / 11;
                }
                if want.random_range(0, 3) == 0 && dir != 0 {
                    if target > 2 {
                        target = want.random_range(2, 3);
                    }
                    if target != 0 {
                        let mut v = i16::from(w.skills[j]) + want.random_range(1, target) * dir;
                        let (lo, hi) = if j < 2 { (1, 99) } else { (45, 55) };
                        clamp_int(&mut v, lo, hi);
                        w.skills[j] = v as u8;
                    }
                }
            }
            let mut form = 50;
            if want.random_range(0, 3) != 0 {
                form -= 1;
            }
            w.traits = 0x14;
            w.tendency = form;

            training_week(&mut s, &Runtime::default(), &mut rng, 1);
            assert_eq!(rng, want, "seed {seed}");
            assert_eq!(s.squads[1][0], w, "seed {seed}");
        }
    }

    /// Manager 0 of club 3 with eleven starters (players 1..=11, player 1 in goal) at home
    /// against club 5.
    fn ticker_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 3;
        for i in 0..11 {
            let p = &mut s.squads[0][i];
            p.player_id = i as u8 + 1;
            p.lineup_position = i as u8 + 1;
            p.skills = [50, 60, 50];
            p.board_row = (i % 8) as i8;
            p.board_col = 3;
            s.players[i + 1].position_value = if i == 0 { 0 } else { 60 };
        }
        s
    }

    fn ticker(
        kind: u8,
        side: u8,
        player_or_minute: u8,
        manager_or_kind: u8,
        arg10: u8,
    ) -> TickerEvent {
        TickerEvent {
            kind,
            comp: 0,
            slot: 0,
            home: 3,
            away: 5,
            arg6: 1,
            player_or_minute,
            manager_or_kind,
            side,
            arg10,
        }
    }

    /// The scorer and assistant [`match_ticker_events`] draws for manager 0.
    fn scorer_and_assist(s: &GameState, rng: &mut Rng, kind: u8) -> (u8, u8) {
        let sc = match_pick_scorer(s, rng, 0, 0, kind).unwrap();
        let assist = loop {
            let a = match_pick_scorer(s, rng, 0, 1, 1).unwrap();
            if a != sc {
                break a;
            }
        };
        (sc, assist)
    }

    #[test]
    fn a_goal_of_the_human_side_books_scorer_and_assistant() {
        for comp in [0u8, 2] {
            let mut s = ticker_state();
            let mut rt = Runtime::default();
            let mut rng = Rng::new(31);
            let mut want = rng;
            let (sc, assist) = scorer_and_assist(&s, &mut want, EVENT_GOAL);
            let mut ui = Headless::new(&s);
            match_ticker_events(
                &mut s,
                &mut rt,
                &mut rng,
                ticker(EVENT_GOAL, 0, 30, comp, 0),
                &mut ui,
            );
            assert_eq!(rng, want);
            assert_eq!(rt.period_chances_used[0], [1, 0]);
            let c = usize::from(comp);
            let r = &s.squads[0][usize::from(sc)];
            assert_eq!((r.goals[c], r.club_goals[c], r.match_rating), (1, 1, 15));
            let league_goals = s.players[usize::from(r.player_id)].counters[0];
            assert_eq!(
                league_goals,
                u8::from(comp == 0),
                "only league goals count for the list"
            );
            assert_eq!(s.squads[0][usize::from(assist)].match_rating, 10);
            assert_eq!(rt.news_records[0].goals_own, 1);
            assert_eq!(rt.news_records[0].goal_minute[0], 30);
            assert!(ui.scenes.is_empty(), "goal scenes are off");
        }
    }

    #[test]
    fn a_penalty_or_a_penalty_scene_gives_no_assist() {
        for (scenes, scene_result, arg10, assisted) in
            [(0, 0, 1, false), (1, 1, 0, false), (1, 0, 0, true)]
        {
            let mut s = ticker_state();
            s.options[GOAL_SCENES_OPTION] = scenes;
            let mut rng = Rng::new(32);
            let (sc, assist) = scorer_and_assist(&s, &mut rng.clone(), EVENT_GOAL);
            let mut ui = Headless::new(&s);
            ui.scene_result = scene_result;
            let e = ticker(EVENT_GOAL, 0, 0x78, 0, arg10);
            match_ticker_events(&mut s, &mut Runtime::default(), &mut rng, e, &mut ui);
            assert_eq!(
                s.squads[0][usize::from(assist)].match_rating,
                if assisted { 10 } else { 0 }
            );
            let scored = if arg10 == 0 { 1 } else { 0 };
            assert_eq!(
                s.squads[0][usize::from(sc)].goals[0],
                scored,
                "a penalty goal is not counted"
            );
            assert_eq!(ui.scenes.len(), usize::from(scenes));
        }
    }

    #[test]
    fn a_missed_chance_lowers_the_rating() {
        let mut s = ticker_state();
        let mut rng = Rng::new(33);
        let (sc, _) = scorer_and_assist(&s, &mut rng.clone(), EVENT_CHANCE);
        let mut ui = Headless::new(&s);
        match_ticker_events(
            &mut s,
            &mut Runtime::default(),
            &mut rng,
            ticker(EVENT_CHANCE, 0, 12, 0, 0),
            &mut ui,
        );
        let r = &s.squads[0][usize::from(sc)];
        assert_eq!((r.match_rating as i8, r.goals[0]), (-10, 0));
    }

    #[test]
    fn the_defending_human_side_rates_its_keeper_and_field_players() {
        for (kind, keeper, field) in [(EVENT_GOAL, -8, -5), (EVENT_CHANCE, 8, -3)] {
            let mut s = ticker_state();
            s.managers[0].team = 5;
            let mut rt = Runtime::default();
            let mut rng = Rng::new(34);
            let mut ui = Headless::new(&s);
            match_ticker_events(
                &mut s,
                &mut rt,
                &mut rng,
                ticker(kind, 0, 40, 0, 0),
                &mut ui,
            );
            assert_eq!(
                rng,
                Rng::new(34),
                "no scorer is drawn for a club without manager"
            );
            assert_eq!(
                rt.period_chances_used[0],
                [1, 0],
                "counted for the other side"
            );
            assert_eq!(s.squads[0][0].match_rating as i8, keeper);
            assert!(
                s.squads[0][1..11]
                    .iter()
                    .all(|p| p.match_rating as i8 == field)
            );
            assert_eq!(rt.news_records[0].n_events, 1);
        }
    }

    #[test]
    fn a_side_without_players_on_the_pitch_changes_nothing() {
        let mut s = ticker_state();
        for p in &mut s.squads[0] {
            p.lineup_position = 0;
        }
        let (want, mut rt, mut rng) = (s.clone(), Runtime::default(), Rng::new(35));
        let mut ui = Headless::new(&s);
        match_ticker_events(
            &mut s,
            &mut rt,
            &mut rng,
            ticker(EVENT_GOAL, 0, 40, 0, 0),
            &mut ui,
        );
        assert!(s == want);
        assert_eq!(rt.period_chances_used[0], [0, 0]);
    }

    #[test]
    fn a_red_card_bans_the_player() {
        for (arg10, cards) in [(0, [1, 1, 0]), (1, [0, 0, 1])] {
            let mut s = ticker_state();
            s.squads[0][4].cards = [0, 1, 0];
            let rt0 = Runtime::default();
            let mut rt = rt0.clone();
            let mut rng = Rng::new(36);
            let mut want = rng;
            let games = want.random_range(1, 7 - 6 * i16::from(arg10)) as u8;
            let mut ws = s.clone();
            ws.squads[0][4].lineup_position = 0;
            squad_recalc_team_strength(&mut ws, &rt0, &mut want, 0, true);
            let mut ui = Headless::new(&s);
            match_ticker_events(
                &mut s,
                &mut rt,
                &mut rng,
                ticker(EVENT_RED_CARD, 0, 4, 0, arg10),
                &mut ui,
            );
            assert_eq!(rng, want);
            let p = &s.squads[0][4];
            assert_eq!(p.cards, cards);
            assert_eq!((p.lineup_position, p.status, p.suspended), (0, 1, games));
            assert_eq!(p.match_rating as i8, -10);
            assert!((1..=7).contains(&games) && (arg10 == 0 || games == 1));
        }
    }

    #[test]
    fn a_yellow_card_counts() {
        let mut s = ticker_state();
        let mut rng = Rng::new(37);
        let mut ui = Headless::new(&s);
        match_ticker_events(
            &mut s,
            &mut Runtime::default(),
            &mut rng,
            ticker(EVENT_YELLOW_CARD, 0, 2, 0, 0),
            &mut ui,
        );
        assert_eq!(rng, Rng::new(37));
        let p = &s.squads[0][2];
        assert_eq!(
            (p.cards[1], p.match_rating as i8, p.lineup_position),
            (1, -10, 3)
        );
    }

    #[test]
    fn an_injured_player_may_be_substituted_unless_the_game_plays_itself() {
        for auto_play in [false, true] {
            let mut s = ticker_state();
            let mut rng = Rng::new(38);
            let mut want = rng;
            let mut ws = s.clone();
            player_injure(&mut ws, &mut want, 0, 6);
            if !auto_play {
                want.random_range(0, 15);
            }
            let mut ui = Headless::new(&s);
            ui.auto_play = auto_play;
            match_ticker_events(
                &mut s,
                &mut Runtime::default(),
                &mut rng,
                ticker(EVENT_INJURY, 1, 6, 0, 0),
                &mut ui,
            );
            assert_eq!(rng, want);
            assert!(s == ws);
            let want_subs: &[(u8, u8)] = if auto_play { &[] } else { &[(0, 1)] };
            assert_eq!(ui.substitutions, want_subs);
        }
    }

    #[test]
    fn slot_start_runs_the_loop_top_in_order() {
        let rt = Runtime::default();
        let mut s = two_managers();
        s.season_week = 3;
        s.day_counter = 11;
        s.week_competitions[3] = 7;
        let mut rng = Rng::new(17);
        let (mut want, mut want_rng) = (s.clone(), rng);
        assert_eq!(
            season_slot_start(
                &mut s,
                &rt,
                &mut rng,
                &mut NumberFormat::default(),
                &mut |_, _, _| {}
            ),
            7
        );

        calendar_compute_date(&mut want, 0);
        want.loop_month = want.month as u8;
        for m in 0..2 {
            want.current_manager = m;
            finance_daily_processing(&mut want, &mut want_rng, &mut NumberFormat::default());
        }
        league_strength_drift(&mut want, &rt, &mut want_rng, 1);
        for m in 0..2 {
            want.current_manager = m;
            want.lineup_state[m as usize][0] = want.lineup_state[m as usize][1];
            auto_lineup(&mut want, rt.lineup_rules);
            squad_recalc_team_strength(&mut want, &rt, &mut want_rng, m as u8, false);
        }
        assert!(s == want);
        assert_eq!(rng, want_rng);
        assert_eq!((s.loop_month, s.day_of_month), (7, 9));
        assert_eq!(s.lineup_state[0], [4, 4]);
    }

    #[test]
    fn slot_start_hands_every_managers_finance_events_on() {
        let rt = Runtime::default();
        let mut s = two_managers();
        s.day_counter = 0x90;
        s.managers[1].build_days[5] = 1;
        let mut events = Vec::new();
        let mut fmt = NumberFormat::default();
        season_slot_start(&mut s, &rt, &mut Rng::new(5), &mut fmt, &mut |_, m, e| {
            events.extend(e.iter().map(|e| (m, e.clone())))
        });
        let [
            (
                1,
                FinanceEvent::ConstructionFinished {
                    item: 5,
                    shown: Some(text),
                },
            ),
        ] = &events[..]
        else {
            panic!("{events:?}");
        };
        assert!(text.ends_with(b"^Die Stadionrenovierung^ist abgeschlossen.^ ^"));
        assert_eq!(fmt.pad_width, 1, "message_post leaves the pad width at 1");
    }

    #[test]
    fn slot_start_without_a_competition_skips_the_squads() {
        let rt = Runtime::default();
        for raw in [0, 9] {
            let mut s = two_managers();
            s.week_competitions[0] = raw;
            let mut fmt = NumberFormat::default();
            let wf = season_slot_start(&mut s, &rt, &mut Rng::new(5), &mut fmt, &mut |_, _, _| {});
            assert_eq!(wf, 0);
            assert_eq!(s.lineup_state[0], [1, 4], "{raw}");
            assert_eq!(s.current_manager, 1);
        }
    }

    #[test]
    fn office_turn_start_draws_once_or_refreshes_the_market() {
        let mut s = two_managers();
        s.current_manager = 1;
        let seed_of = |hit: bool| {
            (0..1000)
                .find(|&seed| (Rng::new(seed).random_range(0, 5) == 0) == hit)
                .unwrap()
        };
        let mut rng = Rng::new(seed_of(false));
        let start = rng;
        let before = s.clone();
        office_turn_start(&mut s, &mut rng);
        assert_eq!(draws(start, rng), 1);
        assert_eq!(s.saved_manager, 1);
        assert!(s.squads == before.squads);

        let mut rng = Rng::new(seed_of(true));
        let mut want = s.clone();
        let mut want_rng = advanced(rng, 1);
        transfer_market_refresh(&mut want, &mut want_rng);
        office_turn_start(&mut s, &mut rng);
        assert!(s == want);
        assert_eq!(rng, want_rng);
    }

    /// One manager whose camp counter steps once per day of finances.
    fn finance_counter_state(day: i32) -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.start_year = 1965;
        s.day_counter = day;
        s.ds_0620[0] = 1000;
        calendar_compute_date(&mut s, 0);
        s
    }

    #[test]
    fn day_advance_ends_on_half_weeks() {
        for (day, next, days) in [(0, 4, 3), (4, 7, 2), (7, 11, 3)] {
            let mut s = finance_counter_state(day);
            let mut ui = Headless::new(&s);
            assert_eq!(
                day_advance(&mut s, &Runtime::default(), &mut Rng::new(1), &mut ui),
                Ok(true)
            );
            assert_eq!(s.day_counter, next, "from {day}");
            assert_eq!(1000 - s.ds_0620[0], days, "from {day}");
        }
        // Day 322 is the last match day: the next one would be day 326.
        let mut s = finance_counter_state(SEASON_LAST_DAY - 3);
        let mut ui = Headless::new(&s);
        assert_eq!(
            day_advance(&mut s, &Runtime::default(), &mut Rng::new(1), &mut ui),
            Ok(true)
        );
        assert_eq!(s.day_counter, SEASON_LAST_DAY);
        assert_eq!(
            day_advance(&mut s, &Runtime::default(), &mut Rng::new(1), &mut ui),
            Ok(false)
        );
        assert_eq!(s.day_counter, SEASON_LAST_DAY + 4);
    }

    #[test]
    fn day_advance_counts_rounds_from_the_raw_slot_byte() {
        let mut s = finance_counter_state(0);
        s.week_competitions[0] = 0x85;
        s.competition_rounds = [3, 3, 3];
        s.squads[0][2].suspended = 1;
        s.squads[0][2].lineup_position = 5;
        let mut ui = Headless::new(&s);
        day_advance(&mut s, &Runtime::default(), &mut Rng::new(1), &mut ui).unwrap();
        assert_eq!(s.competition_rounds, [4, 3, 4]);
        assert_eq!(s.squads[0][2].lineup_position, 0);
        assert_eq!(ui.winter_passes, 0);

        // Day 131 is inside the winter break, day 130 is not.
        for (day, passes) in [(130, 0), (131, 1), (206, 1), (207, 0)] {
            let mut s = finance_counter_state(day);
            let mut ui = Headless::new(&s);
            day_advance(&mut s, &Runtime::default(), &mut Rng::new(1), &mut ui).unwrap();
            assert_eq!(ui.winter_passes, passes, "day {day}");
        }
    }

    #[test]
    fn special_dates_draw_only_on_their_day() {
        let mut s = two_managers();
        // 23 December: no special date, no draw.
        s.day_counter = 147;
        calendar_compute_date(&mut s, 0);
        assert_eq!((s.day_of_month, s.month), (23, 11));
        let mut ui = Headless::new(&s);
        let mut rng = Rng::new(3);
        check_special_dates(&mut s, &mut rng, &mut ui).unwrap();
        assert_eq!(rng, Rng::new(3));

        // 24 December.
        s.day_counter = 148;
        calendar_compute_date(&mut s, 0);
        let seed = (0..1000)
            .find(|&seed| Rng::new(seed).random_range(0, 3) != 0)
            .unwrap();
        let mut rng = Rng::new(seed);
        check_special_dates(&mut s, &mut rng, &mut ui).unwrap();
        assert_eq!(draws(Rng::new(seed), rng), 1);
        assert!(ui.dates.is_empty());

        // A birthday that happens draws once and changes nothing.
        let seed = (0..1000)
            .find(|&seed| Rng::new(seed).random_range(0, 3) == 0)
            .unwrap();
        let mut rng = Rng::new(seed);
        let before = s.clone();
        let date = special_date_events(&mut s, &mut rng, 1);
        assert_eq!(
            date,
            Some(SpecialDate {
                idx: 1,
                christmas_bonus: None
            })
        );
        assert_eq!(draws(Rng::new(seed), rng), 1);
        assert!(s == before);
    }

    #[test]
    fn christmas_bonus_is_paid_by_tier() {
        let seed = (0..5000)
            .find(|&seed| {
                let mut r = Rng::new(seed);
                r.random_range(0, 3) == 0 && r.random_range(0, 1) != 0
            })
            .unwrap();
        let mut probe = advanced(Rng::new(seed), 2);
        let bonus = i32::from(probe.random_range(15, 85)) * 10_000;
        let mut s = two_managers();
        s.manager_count = 3;
        s.managers[2].league = 2;
        for m in &mut s.managers[..3] {
            m.balance = 1_000_000;
        }
        let mut rng = Rng::new(seed);
        let date = special_date_events(&mut s, &mut rng, 0).unwrap();
        assert_eq!(date.christmas_bonus, Some(bonus));
        assert_eq!(rng, probe);
        let paid: Vec<i32> = s.managers[..3]
            .iter()
            .map(|m| 1_000_000 - m.balance)
            .collect();
        assert_eq!(paid, [bonus, bonus / 2, bonus / 3]);
        assert_eq!(s.managers[3].balance, 0);
    }

    #[test]
    fn eternal_points_by_tier_and_cup() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        let rank = |s: &mut GameState, t: usize, r: u8| s.team_tables[t].table_position = r;
        rank(&mut s, 3, 3);
        rank(&mut s, 5, 5);
        rank(&mut s, 18, 0);
        rank(&mut s, 19, 1);
        rank(&mut s, 38, 0);
        rank(&mut s, 58, 0);
        rank(&mut s, 63, 7);
        s.team_tables[6].prestige = 1000;
        // Holders are stored as club + 1: club 6 won the DFB-Pokal and European cups 3, 2 and
        // 1 (bonus order 20, 30, 40, 50 for DS:07ac, 07af, 07ae, 07ad).
        s.winner_07ac = 7;
        s.winner_07af = 7;
        s.winner_07ae = 7;
        s.winner_07ad = 7;
        season_end_eternal_points(&mut s, &rt);
        let p = |t: usize| s.team_tables[t].prestige;
        // Bundesliga champion: 2 * 25 - 0 + 19 + 12.
        assert_eq!(p(0), 81);
        // Bundesliga rank 3 (0-based): 50 - 3 + 19 + 3.
        assert_eq!(p(3), 69);
        // Bundesliga rank 5: 50 - 5 + 19, no bonus.
        assert_eq!(p(5), 64);
        // Club 6, rank 0: 81 + 20 + 30 + 40 + 50 on top of 1000.
        assert_eq!(p(6), 1000 + 81 + 140);
        // 2. Bundesliga champion: 25 - 0 + 19 + 3; rank 1: 25 - 1 + 19.
        assert_eq!(p(18), 47);
        assert_eq!(p(19), 43);
        // Amateur-Oberliga champion: 19 + 3.
        assert_eq!(p(38), 22);
        // Outside the leagues nothing, not even for rank 0.
        assert_eq!(p(58), 0);
        assert_eq!(p(63), 0);
    }

    #[test]
    fn eternal_points_of_a_low_rank_go_negative() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.team_tables[40].table_position = 30;
        season_end_eternal_points(&mut s, &rt);
        assert_eq!(s.team_tables[40].prestige, 19 - 30);
    }

    fn record_state() -> GameState {
        let mut s = two_managers();
        let t = &mut s.team_tables[3];
        t.points = [40, 30, 0, 0];
        t.played = [17, 17, 0, 0];
        t.goals_for = [35, 20, 0, 0];
        t.goals_against = [10, 25, 0, 0];
        t.wins = [18, 12, 0, 0];
        t.losses = [1, 3, 0, 0];
        let m = &mut s.managers[0];
        m.position_history[33] = 0;
        m.values_132 = [5, 0x1e, 3, 0x1e, 0x1e];
        m.records = [500_000, 40_000, 9_000];
        m.values_13a[0] = 17;
        m.career_stats[0] = [0xfff0, 1];
        s.logged_rounds = 2;
        s
    }

    #[test]
    fn season_record_manager_logs_the_season() {
        let rt = Runtime::default();
        let mut s = record_state();
        s.winner_07ac = 4;
        s.winner_07ae = 4;
        let mut ui = Headless::new(&s);
        season_record_manager(&mut s, &rt, 0, &mut ui).unwrap();
        let m = &s.managers[0];
        // Points wrap at 16 bits: 0xfff0 + 40.
        assert_eq!(m.career_stats[0], [0x0018, 31]);
        assert_eq!(m.career_stats[1], [35, 20]);
        assert_eq!(m.career_stats[2], [10, 25]);
        // Points lost: 2 * 17 - 40 wraps, 2 * 17 - 30 = 4.
        assert_eq!(m.career_stats[3], [34u16.wrapping_sub(40), 4]);
        assert_eq!(m.career_stats[4], [17, 17]);
        assert_eq!(m.career_stats[5], [18, 12]);
        assert_eq!(m.career_stats[6], [1, 3]);
        // Draws: 17 - 1 - 18 wraps, 17 - 3 - 12 = 2.
        assert_eq!(m.career_stats[7], [17u16.wrapping_sub(19), 2]);
        assert_eq!(m.records, [0, 0, 99_999]);
        assert_eq!(m.values_13a[0], 0);
        // Champion of tier 0, DFB-Pokal winner after stage 5, European cup 2 winner at stage 3.
        assert_eq!(m.round_log[2], [1, 0, 0x80 + 6, 0x80 + (2 << 3) + 3]);
        assert_eq!((m.titles, m.cup_wins), (1, [1, 0, 1, 0]));
        assert_eq!(ui.champions, [0]);
    }

    #[test]
    fn season_record_manager_shifts_a_full_log() {
        let rt = Runtime::default();
        let mut s = record_state();
        s.logged_rounds = 0x32;
        let m = &mut s.managers[0];
        m.league = 1;
        m.position_history[37] = 4;
        m.values_132 = [0, 0x1e, 0x1e, 0, 0x1e];
        for (j, e) in m.round_log.iter_mut().enumerate() {
            *e = [j as u8; 4];
        }
        let mut ui = Headless::new(&s);
        season_record_manager(&mut s, &rt, 0, &mut ui).unwrap();
        let m = &s.managers[0];
        assert_eq!(m.round_log[0], [1; 4]);
        assert_eq!(m.round_log[0x30], [0x31; 4]);
        // Rank 5 of the 2. Bundesliga is global rank 18 + 5; stage 0 of the DFB-Pokal; no
        // European cup, so byte 3 keeps the shifted value.
        assert_eq!(m.round_log[0x31], [23, 1, 1, 0x31]);
        assert_eq!(m.values_13a[1], 1);
        assert_eq!((m.titles, m.cup_wins), (0, [0; 4]));
        assert!(ui.champions.is_empty());
    }

    #[test]
    fn board_verdict_carries_over_between_managers() {
        let mut s = two_managers();
        s.logged_rounds = 1;
        s.team_tables[3].prestige = 1300;
        s.team_tables[20].prestige = 50;
        s.managers[0].fans = 60;
        s.managers[1].fans = 40;
        // Manager 0: 60 + 300 * 100 / 108 / 2 - 60 / 2 = 168, clamped to 100. Manager 1 starts
        // from that, but its own value replaces it: 40 + 0 - 20 = 20.
        assert_eq!(board_verdict(&s, &[1000, 50]), 20);
        // A loss wraps the unsigned division: 40 + (0xffff_ff9c / 108 / 2) as u16 - 20.
        let q = ((-100i32 as u32) / 108 / 2) as u16;
        let v = 40u16.wrapping_add(q.wrapping_sub(20)) as i16;
        assert_eq!(board_verdict(&s, &[1000, 51]), v.clamp(0, 100));
        // From the fifth season on the divisor stays 5.
        s.logged_rounds = 9;
        assert_eq!(board_verdict(&s, &[1300, 50]), 40 - 8);
    }

    #[test]
    fn board_verdict_floors_carry_on() {
        let mut s = two_managers();
        s.manager_count = 3;
        s.managers[1].league = 0;
        s.managers[2].league = 2;
        board_verdict_store(&mut s, 10);
        // Manager 0 (Bundesliga) is floored to 61, which manager 1 and 2 inherit.
        assert_eq!(
            s.managers[..3].iter().map(|m| m.fans).collect::<Vec<_>>(),
            [61, 61, 61]
        );
        let mut s = two_managers();
        s.managers[0].league = 2;
        board_verdict_store(&mut s, 10);
        assert_eq!((s.managers[0].fans, s.managers[1].fans), (10, 30));
    }

    /// Manager 0 of club 3 with two squad players: one who played (lineup position 3, first
    /// skill higher) and one on the bench.
    fn gate_state() -> (GameState, Runtime) {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 3;
        s.managers[0].value_10a = 20;
        s.managers[0].stadium[0][0] = 20_000;
        s.managers[0].stadium[1][0] = 10_000;
        s.managers[0].records[2] = 99_999;
        s.month = 2;
        for (i, (pid, pos)) in [(7u8, 3u8), (8, 0)].into_iter().enumerate() {
            let rec = &mut s.squads[0][i];
            rec.player_id = pid;
            rec.lineup_position = pos;
            rec.exhaustion = 40;
        }
        s.players[7].skills = [60, 50, 0];
        s.players[8].skills = [50, 60, 0];
        for club in [3usize, 9] {
            s.clubs[club].strength = [90, 90, 90];
            s.clubs[club].base_strength = [90, 90, 90];
        }
        (s, Runtime::default())
    }

    fn gate(
        s: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        home: u8,
        away: u8,
        cup_mask: u8,
        comp: u8,
    ) -> (u8, Vec<u8>) {
        let mut forfeited = Vec::new();
        let r = match_gate_receipts_and_aftermath(
            s,
            rt,
            rng,
            home,
            away,
            cup_mask,
            comp,
            4,
            1,
            &mut |_, m| forfeited.push(m),
        );
        (r, forfeited)
    }

    #[test]
    fn gate_receipts_of_a_league_home_game() {
        let (mut s, mut rt) = gate_state();
        s.squads[0][0].exhaustion = 140;
        let start = Rng::new(8);
        let mut probe = start;
        let att = match_attendance_estimate(&s, &rt, &mut probe, 0, 3, 9, 1, false, 0);
        assert!(att > 1000, "{att}");
        let tired = probe.random_range(2, 4) - 1 + 6;
        probe.rand();
        let mut rng = start;
        assert_eq!(gate(&mut s, &mut rt, &mut rng, 3, 9, 0, 0), (0, vec![]));
        assert_eq!(rng, probe);
        let m = &s.managers[0];
        assert_eq!(m.home_attendance[0], (att / 1000) as u8);
        assert_eq!(m.records, [att, att, att]);
        assert_eq!(m.values_1f4, [9, 0, 9, 0]);
        assert_eq!(m.values_13a[0], 1);
        assert_eq!(m.balance, 20 * att);
        assert_eq!(s.squads[0][0].exhaustion, (140 + tired as u8).min(150));
        assert_eq!(s.squads[0][1].exhaustion, 50);
        assert_eq!(s.squads[0][0].appearances, [1, 0, 0]);
        assert_eq!(s.squads[0][0].value_terms, [1, 0, 0]);
        assert_eq!(s.players[7].counters[1], 1);
        assert_eq!(s.squads[0][1].appearances, [0; 3]);
        assert_eq!(rt.manager_plays_now[0], 1);
        assert_eq!((rt.yellow_cards_left[0], rt.red_card_possible[0]), (0, 0));
        assert_eq!(rt.booked_players[0], [NO_BOOKING; 6]);
        assert_eq!(rt.players_on_pitch[0], 11);
        assert_eq!(rt.substitutions_left[0], [1, 2]);
        assert_eq!(rt.live_match_shown, 1);
        assert_eq!(rt.live_matches, [LiveMatch { row: 0, slot: 4 }]);
        let rec = &rt.news_records[0];
        assert_eq!((rec.club_a, rec.club_b, rec.spectators), (3, 9, att));
    }

    #[test]
    fn exhaustion_wraps_at_eight_bits_then_clamps() {
        let (mut s, mut rt) = gate_state();
        s.squads[0][0].exhaustion = 250;
        gate(&mut s, &mut rt, &mut Rng::new(8), 3, 9, 0, 0);
        assert_eq!(s.squads[0][0].exhaustion, 50);
    }

    #[test]
    fn a_full_squad_gets_its_card_limits() {
        let (mut s, mut rt) = gate_state();
        for i in 2..6 {
            s.squads[0][i].player_id = 10 + i as u8;
        }
        gate(&mut s, &mut rt, &mut Rng::new(8), 3, 9, 0, 0);
        assert_eq!((rt.yellow_cards_left[0], rt.red_card_possible[0]), (6, 1));
    }

    #[test]
    fn a_manager_without_a_team_forfeits_until_the_10th_of_june() {
        for (day, forfeit) in [(9, true), (10, false)] {
            let (mut s, mut rt) = gate_state();
            s.managers[0].aggression = CANNOT_FIELD_A_TEAM;
            s.month = 5;
            s.day_of_month = day;
            let mut rng = Rng::new(8);
            let (r, dialogs) = gate(&mut s, &mut rt, &mut rng, 9, 3, 0, 0);
            if forfeit {
                assert_eq!((r, dialogs), (2, vec![0]));
                assert_eq!(s.managers[0].balance, -200_000);
                assert_eq!(rng, Rng::new(8));
                assert_eq!(rt.manager_plays_now[0], 0);
            } else {
                assert_eq!((r, dialogs), (0, vec![]));
                assert_eq!(rt.manager_plays_now[0], 1);
            }
        }
    }

    #[test]
    fn the_dfb_cup_final_pays_a_fixed_gate_to_both_clubs() {
        let (mut s, mut rt) = gate_state();
        s.manager_count = 2;
        s.managers[1].team = 9;
        s.managers[1].records[2] = 99_999;
        s.event_state[0] = 5;
        let mut rng = Rng::new(8);
        assert_eq!(gate(&mut s, &mut rt, &mut rng, 3, 9, 1, 0), (0, vec![]));
        assert_eq!(
            (s.managers[0].balance, s.managers[1].balance),
            (532_000, 532_000)
        );
        // One random_range(2, 4) for the player who played, one hooligan draw for the home side.
        assert_eq!(draws(Rng::new(8), rng), 2);
        // Cup games leave the league attendance records alone.
        assert_eq!(s.managers[0].records, [0, 0, 99_999]);
        assert_eq!(s.squads[0][0].appearances, [0, 1, 0]);
        assert_eq!(rt.news_records[0].spectators, FIXED_ATTENDANCE);
        assert_eq!(rt.news_records[1].spectators, FIXED_ATTENDANCE + 200_000);
        assert_eq!(
            (rt.news_records[1].club_a, rt.news_records[1].club_b),
            (9, 3)
        );
        assert_eq!(rt.live_matches.len(), 1);
    }

    #[test]
    fn dfb_cup_visitor_gets_half_the_gate() {
        let (mut s, mut rt) = gate_state();
        s.manager_count = 2;
        s.managers[1].team = 9;
        s.managers[1].value_10a = 99;
        let start = Rng::new(8);
        let mut probe = start;
        let att = match_attendance_estimate(&s, &rt, &mut probe, 0, 3, 9, 1, false, 0);
        let mut rng = start;
        gate(&mut s, &mut rt, &mut rng, 3, 9, 1, 0);
        assert_eq!(s.managers[0].balance, 20 * att / 2);
        // The visitor gets the home manager's price.
        assert_eq!(s.managers[1].balance, 20 * att / 2);
    }

    #[test]
    fn hooligans_are_drawn_from_the_seat_share() {
        let (mut s, mut rt) = gate_state();
        s.squads[0][0].lineup_position = 0;
        // Seats - standing = -10 000: h = 0 after the clamp, bound 30 * 4 + 15.
        let seed = (0..20_000u16)
            .find(|&seed| {
                let mut probe = Rng::new(seed);
                match_attendance_estimate(&s, &rt, &mut probe, 0, 3, 9, 1, false, 0);
                probe.random_range(0, 135) == 0
            })
            .unwrap();
        gate(&mut s, &mut rt, &mut Rng::new(seed), 3, 9, 0, 0);
        assert_eq!(s.managers[0].value_13e, 1);
        let (mut s, mut rt) = gate_state();
        s.squads[0][0].lineup_position = 0;
        gate(&mut s, &mut rt, &mut Rng::new(seed), 9, 3, 0, 0);
        assert_eq!(s.managers[0].value_13e, 0);
    }

    #[test]
    fn suspension_counts_down_on_the_weekly_day() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.squads[0][0].status = 2;
        s.squads[0][0].suspended = 2;
        for (day, left, status) in [(6, 2, 2), (7, 1, 2), (14, 0, 0)] {
            s.day_counter = day;
            player_suspension_countdown(&mut s, &rt, 0, 0);
            assert_eq!(
                (s.squads[0][0].suspended, s.squads[0][0].status),
                (left, status),
                "day {day}"
            );
        }
        // A league ban (bit 0) only ends when the count is 0.
        s.squads[0][1].status = 5;
        s.squads[0][1].suspended = 1;
        s.day_counter = 7;
        player_suspension_countdown(&mut s, &rt, 0, 1);
        assert_eq!((s.squads[0][1].suspended, s.squads[0][1].status), (1, 5));
        s.squads[0][1].suspended = 0;
        player_suspension_countdown(&mut s, &rt, 0, 1);
        assert_eq!(s.squads[0][1].status, 4);
    }

    /// [`two_managers`] in round `round` of a season whose tables hold every club in id order.
    fn league_season(round: u8) -> GameState {
        let mut s = two_managers();
        for (tier, &first) in TIER_FIRST.iter().enumerate() {
            for i in 0..20 {
                let club = first + i as u8;
                s.league_teams[tier][i] = club;
                if let Some(t) = s.team_tables.get_mut(usize::from(club)) {
                    t.table_position = i as u8;
                }
            }
        }
        s.competition_rounds = [round; 3];
        s
    }

    /// The callees of a match slot, in the order the slot runs them.
    #[derive(Clone, Copy)]
    enum SlotCall {
        Postpone(u8),
        /// A league day of the tiers in the mask; the match-day manager is set again after it.
        LeagueDay(u8),
        CupDay(u8),
        RelegationDay,
        RescheduledDay,
    }

    #[test]
    fn slot_matches_run_by_the_competition_bits() {
        use SlotCall::*;
        use Title::*;
        const WEEK: u8 = 0x2c;
        let cases: [(u8, &[Title], &[SlotCall]); 10] = [
            (0x00, &[], &[]),
            (
                0x01,
                &[League],
                &[Postpone(0), Postpone(1), Postpone(2), LeagueDay(1)],
            ),
            (0x06, &[League], &[Postpone(1), Postpone(2), LeagueDay(6)]),
            (0x08, &[DfbCup], &[CupDay(1)]),
            (0x10, &[Relegation], &[RelegationDay]),
            (0x20, &[EuropeanCup], &[CupDay(0x0e)]),
            (0x30, &[EuropeanCup], &[CupDay(0x0e)]),
            // `league_reschedule_match` marks a rescheduled day with this bit alone.
            (0x80, &[Rescheduled], &[RescheduledDay]),
            (
                0x1f,
                &[League, DfbCup, Relegation],
                &[
                    Postpone(0),
                    Postpone(1),
                    Postpone(2),
                    LeagueDay(0x1f),
                    CupDay(1),
                    RelegationDay,
                ],
            ),
            (
                0x4c,
                &[League, DfbCup, EuropeanCup],
                &[
                    Postpone(1),
                    Postpone(2),
                    LeagueDay(0x4c),
                    CupDay(1),
                    CupDay(0x0e),
                ],
            ),
        ];
        for (wf, titles, calls) in cases {
            let mut s = league_season(10);
            s.event_state[..2].copy_from_slice(&[3, 3]);
            // A winter slot, where the weather may postpone matches.
            s.season_week = WEEK;
            s.lineup_state = [[2, 4], [3, 1], [0, 0], [0, 0]];
            if wf & RESCHEDULED_GAMES_DAY != 0 {
                s.rescheduled_matches[2] = [0, WEEK, 1, 0, 3];
            }
            let mut rt = Runtime {
                manager_plays_now: [1; MANAGER_SLOTS],
                lineup_change_count: [2; MANAGER_SLOTS],
                ..Runtime::default()
            };
            let mut rng = Rng::new(29);
            let mut ui = Headless::new(&s);
            let (mut want, mut want_rt, mut want_rng) = (s.clone(), rt.clone(), rng);
            let mut want_ui = Headless::new(&s);
            play_slot_matches(&mut s, &mut rt, &mut rng, &mut ui, wf).unwrap();
            assert_eq!(ui.titles, titles, "wf {wf:#x}");

            for m in 0..2u8 {
                want.current_manager = m as i8;
                squad_recalc_team_strength(&mut want, &want_rt, &mut want_rng, m, true);
            }
            want.lineup_state[0] = [1, 2];
            want.lineup_state[1] = [1, 3];
            want_rt.manager_plays_now = [0, 0, 1, 1];
            want_rt.lineup_change_count = [0, 0, 2, 2];
            want.current_manager = MATCH_DAY_MANAGER;
            let (st, r, g) = (&mut want, &mut want_rt, &mut want_rng);
            for &call in calls {
                let hooks = &mut SeasonHooks(&mut want_ui);
                match call {
                    Postpone(tier) => league_postpone_matches_weather(st, r, g, WEEK, tier),
                    LeagueDay(mask) => {
                        matchday_play(st, r, g, hooks, mask, 0);
                        st.current_manager = MATCH_DAY_MANAGER;
                    }
                    CupDay(cup) => matchday_play(st, r, g, hooks, 0, cup),
                    RelegationDay => cups::matchday_relegation_match(st, r, g, hooks),
                    RescheduledDay => matchday_play_rescheduled(st, r, g, hooks, WEEK),
                }
            }
            assert!(s == want, "wf {wf:#x}");
            assert!(rt == want_rt, "wf {wf:#x}");
            assert_eq!(rng, want_rng, "wf {wf:#x}");
        }

        // The rescheduled bit without a game rescheduled to this slot plays nothing.
        let mut s = league_season(10);
        s.season_week = WEEK;
        s.rescheduled_matches[2] = [0, WEEK + 1, 1, 0, 3];
        let mut ui = Headless::new(&s);
        let want = s.clone();
        let (mut rt, mut rng) = (Runtime::default(), Rng::new(29));
        play_slot_matches(&mut s, &mut rt, &mut rng, &mut ui, RESCHEDULED_GAMES_DAY).unwrap();
        assert!(ui.titles.is_empty());
        assert_eq!(s.rescheduled_matches, want.rescheduled_matches);
    }

    #[test]
    fn a_resumed_game_gives_the_saved_manager_the_first_office_turn() {
        // On a match slot the saved manager's turn opens the normal office loop.
        let mut s = two_managers();
        s.season_week = 4;
        s.week_competitions[4] = 1;
        s.current_manager = 1;
        let mut ui = Headless::new(&s);
        ui.stop_at = 0;
        let end = season_main_loop(
            &mut s,
            &mut Runtime::default(),
            &mut Rng::new(7),
            &mut ui,
            true,
        );
        assert_eq!(end, Err(Done));
        assert_eq!(ui.office_turns, [(1, 4)]);
        assert_eq!(s.saved_manager, 1);

        // Without `resume` manager 0 starts.
        let mut s = two_managers();
        s.season_week = 4;
        s.week_competitions[4] = 1;
        s.current_manager = 1;
        let mut ui = Headless::new(&s);
        ui.stop_at = 0;
        let _ = season_main_loop(
            &mut s,
            &mut Runtime::default(),
            &mut Rng::new(7),
            &mut ui,
            false,
        );
        assert_eq!(ui.office_turns, [(0, 4)]);

        // A game saved on the last slot without a match resumes in the season-end office.
        let mut s = two_managers();
        s.day_counter = SEASON_LAST_DAY;
        s.current_manager = 1;
        let mut ui = Headless::new(&s);
        ui.stop_at = 0;
        let _ = season_main_loop(
            &mut s,
            &mut Runtime::default(),
            &mut Rng::new(7),
            &mut ui,
            true,
        );
        assert_eq!(ui.office_turns, [(1, 0xff)]);
        assert_eq!(s.day_counter, SEASON_LAST_DAY + 4);
    }

    #[test]
    fn a_manager_added_in_the_office_gets_a_turn_the_same_day() {
        // The office loop re-reads DS:07ab (2a58:3b2f) after every turn.
        let mut s = two_managers();
        s.season_week = 4;
        s.week_competitions[4] = 1;
        let mut ui = Headless::new(&s);
        ui.stop_at = 2;
        ui.add_manager_at = 1;
        let end = season_main_loop(
            &mut s,
            &mut Runtime::default(),
            &mut Rng::new(7),
            &mut ui,
            false,
        );
        assert_eq!(end, Err(Done));
        assert_eq!(ui.office_turns, [(0, 4), (1, 4), (2, 4)]);
    }

    /// Two managers in the middle of the season end, with points in the all-time table.
    fn season_final_state() -> GameState {
        let mut s = league_season(34);
        s.day_counter = SEASON_LAST_DAY + 4;
        s.season_week = 0x5c;
        s.logged_rounds = 2;
        s.scenario = 1970;
        s.team_tables[3].prestige = 900;
        s.team_tables[20].prestige = 300;
        s.managers[0].fans = 70;
        s.managers[1].fans = 45;
        s.team_tables[3].points = [30, 20, 0, 0];
        s.league_results[0][1][2] = 5;
        calendar_compute_date(&mut s, 0);
        s
    }

    #[test]
    fn season_final_runs_its_callees_in_order() {
        let mut s = season_final_state();
        s.options[cups::SHOW_DRAW_OPTION..][..2].copy_from_slice(&[1, 1]);
        let mut rt = Runtime::default();
        let mut rng = Rng::new(41);
        let mut ui = Headless::new(&s);
        let (mut want, mut want_rt, mut want_rng) = (s.clone(), rt.clone(), rng);
        assert_eq!(season_final(&mut s, &mut rt, &mut rng, &mut ui), Ok(false));
        assert_eq!(ui.titles, [Title::SeasonEnd]);
        assert_eq!(ui.highscore_inserts, [0, 1]);
        // The DFB-Pokal draw asks first, then the redisplayed European cup draws.
        assert_eq!(ui.draws_asked, [0, 1, 2, 3]);

        let old = [900, 300];
        let mut top_scorer = [false; 2];
        for m in 0..2 {
            want.current_manager = m;
            top_scorer[m as usize] = top_scorer_plays_for_current_manager(&mut want);
        }
        season_end_eternal_points(&mut want, &want_rt);
        let v = board_verdict(&want, &old);
        for t in &mut want.team_tables {
            t.points = [0; 4];
            t.played = [0; 4];
            t.goals_for = [0; 4];
            t.goals_against = [0; 4];
            t.wins = [0; 4];
            t.losses = [0; 4];
            strcpy(&mut t.form[0], FORM_PLACEHOLDER);
            strcpy(&mut t.form[1], FORM_PLACEHOLDER);
        }
        cups::season_promotion_relegation(&mut want, &want_rt, &mut want_rng);
        league_strength_drift(&mut want, &want_rt, &mut want_rng, 10);
        board_verdict_store(&mut want, v);
        for m in 0..2 {
            want.current_manager = m;
            advertising_offers_generate(&mut want, &mut want_rng);
        }
        cups::league_shuffle_teams(&mut want, &want_rt, &mut want_rng);
        cups::euro_cup_qualifiers_build(&mut want, &mut want_rt);
        let mut want_ui = Headless::new(&want);
        season_end_messages(
            &mut want,
            &want_rt,
            &mut want_rng,
            &top_scorer,
            &mut want_ui,
        );
        assert_eq!(ui.season_end, want_ui.season_end);
        want.logged_rounds = 3;
        // The finances of days 327 (21 June) to 364 (28 July).
        want.day_counter = SEASON_LAST_DAY + 5;
        for _ in 0..38 {
            want.loop_month = want.month as u8;
            calendar_compute_date(&mut want, 0);
            for m in 0..2 {
                want.current_manager = m;
                want.saved_manager = m;
                finance_daily_processing(&mut want, &mut want_rng, &mut NumberFormat::default());
            }
            want.day_counter += 1;
        }
        assert_eq!((want.day_of_month, want.month), (28, 6));
        // The date of day 365 (29 July 1966) stays until the next loop top.
        calendar_compute_date(&mut want, 0);
        want.day_counter = i32::from(SEASON_START_DAY);
        want.season_week = 0;
        want.start_year = 1966;
        want.league_results = [[[0; 20]; 38]; 3];
        for m in 0..2 {
            manager_compute_wage_budget(&mut want, &mut want_rng, m);
        }
        cups::cup_clear_match_goals(&mut want);
        let ask = &mut |_: u8| false;
        for cup in 0..4 {
            cups::cup_draw_first_round(&mut want, &want_rt, &mut want_rng, cup, false, ask);
        }
        for cup in 1..4 {
            cups::cup_draw_first_round(&mut want, &want_rt, &mut want_rng, cup, true, ask);
        }
        assert!(s == want);
        assert!(rt == want_rt);
        assert_eq!(rng, want_rng);
    }

    #[test]
    fn a_promotion_pays_a_bonus_and_cuts_the_advertising_income() {
        for (league, line, bonus) in [
            (1, PROMOTED_TO_SECOND.0, 800_000),
            (0, PROMOTED_TO_FIRST.0, 1_600_000),
        ] {
            for released in [false, true] {
                let mut s = weekly_state();
                let r = &mut s.managers[1];
                r.league = league;
                r.value_140 = 1 | 8;
                r.balance = 1000;
                s.manager_contracts[1] = [100, 200, 300, 400, 500, 600, 700, 800, 900];
                s.far_0000[2] = u8::from(released);
                s.far_4b9a[2 * (6 + 1)] = 3; // the second board's partner may cancel
                let mut lines = Vec::new();
                let mut stale = 0;
                let shown = season_end_bonuses(
                    &mut s,
                    &Runtime::default(),
                    1,
                    true,
                    &mut lines,
                    &mut stale,
                );
                assert_eq!(shown, released);
                assert_eq!(lines, [line, SEASON_END_FLAG_LINES[2].1, TOP_SCORER.0]);
                assert_eq!(s.managers[1].balance, 1000 + bonus + 250_000);
                let first = if released { 100 } else { 10 };
                assert_eq!(
                    s.manager_contracts[1],
                    [first, 20, 300, 40, 50, 60, 70, 800, 900]
                );
                assert_eq!((s.far_0000[2], s.far_4b9a[2 * (6 + 1)], stale), (0, 0, 6));
            }
        }
    }

    #[test]
    fn a_youth_player_may_join_a_hard_working_club() {
        let seed = (0..u16::MAX)
            .find(|&seed| Rng::new(seed).random_range(0, 2) == 0)
            .unwrap();
        let mut s = weekly_state();
        s.year = 1970;
        s.managers[1].fatigue_sum = 12 * 60;
        for p in &mut s.players[1..] {
            p.owner_manager = NO_OWNER;
        }
        let rt = Runtime::default();
        let p = season_end_youth_player(&mut s, &rt, &mut Rng::new(seed), 1).unwrap();
        assert_eq!(
            s.managers[1].fatigue_sum,
            12 * 40,
            "a third of the work is lost"
        );
        let person = &s.players[p];
        assert!((17..=19).contains(&person.age));
        assert!(
            person.skills[..2].iter().all(|v| (17..=23).contains(v)),
            "around 40 / 2"
        );
        assert_eq!((person.owner_manager, person.team), (1, 20));
        let (g, i) = find_player_by_id(&s, p as u8).unwrap();
        assert_eq!(g, 1);
        assert!((2..=3).contains(&s.squads[1][i].contract_years));

        // The first season of a classic game keeps the work and draws nothing.
        let mut s = weekly_state();
        s.year = 1966;
        s.managers[1].fatigue_sum = 12 * 60;
        let mut rng = Rng::new(seed);
        assert_eq!(season_end_youth_player(&mut s, &rt, &mut rng, 1), None);
        assert_eq!((s.managers[1].fatigue_sum, rng), (12 * 60, Rng::new(seed)));
    }

    #[test]
    fn separations_age_everyone_and_return_held_players() {
        for auto_play in [false, true] {
            let mut s = weekly_state();
            s.current_manager = 0;
            s.players[30].age = 40;
            // Player 9 of manager 1 sits on the transfer list; player 12 is in squad 0.
            s.squads[TRANSFER_LIST][0] = s.squads[1][0].clone();
            s.squads[1][0] = crate::state::SquadPlayer::zeroed();
            s.squads[TRANSFER_LIST][0].contract_years = 2;
            s.squads[TRANSFER_LIST][0].status = 0xc1;
            s.squads[0][0].player_id = 12;
            s.squads[0][0].contract_years = 3;
            s.squads[0][0].negotiation = 5;
            s.players[12].owner_manager = 0;
            separations(&mut s, &mut Rng::new(3), auto_play);
            assert_eq!(s.players[30].age, 41);
            assert_eq!(s.current_manager, 0);
            assert_eq!(squad_count_listed(&s, TRANSFER_LIST), 0);
            let (g, i) = find_player_by_id(&s, 9).unwrap();
            assert_eq!((g, s.players[9].owner_manager), (1, 1));
            assert_eq!(s.squads[1][i].status, 0x01, "the transfer flags clear");
            let own = &s.squads[0][0];
            if auto_play {
                assert!((18..=25).contains(&s.players[12].age));
                assert_eq!((own.contract_years, own.negotiation), (3, 0));
                assert_eq!(s.squads[1][i].contract_years, 2);
            } else {
                assert_eq!(s.players[12].age, 1);
                // The returned player's record index is cleared in squad 0 (season.md §7 Q9).
                assert_eq!((own.contract_years, own.negotiation), (2, 0));
                assert_eq!(s.squads[1][i].contract_years, 1);
            }
        }
    }

    #[test]
    fn retiring_players_leave_and_old_free_persons_are_replaced() {
        let mut s = weekly_state();
        s.manager_count = 2;
        s.saved_manager = 1;
        s.squads[1][0].contract_years = 0;
        s.squads[1][0].negotiation = LEAVES;
        s.players[9].age = 31;
        for p in &mut s.players[1..] {
            p.age = 20;
        }
        s.players[9].age = 31;
        s.players[40].age = 0x23; // older than any limit
        s.players[40].counters = [4, 9];
        let mut ui = Headless::new(&s);
        retirements(&mut s, &mut Rng::new(5), &mut ui, 1, 2, 77);
        assert_eq!(ui.season_end.len(), 1);
        assert!(ui.season_end[0].starts_with("Retirement"));
        assert!(find_player_by_id(&s, 9).is_none());
        assert_eq!(s.players[9].owner_manager, NO_OWNER);
        for p in [9, 40] {
            let person = &s.players[p];
            assert!((18..=25).contains(&person.age), "person {p} is new");
            assert!((25..=0x61).contains(&person.skills[0]));
        }
        assert_eq!(
            s.players[40].counters,
            [0, 0],
            "the last manager clears the league counters"
        );
        assert!(inbox(&s, 1).is_empty(), "the message was deleted again");
    }

    #[test]
    fn expiring_contracts_are_talked_over_and_a_leaving_player_brings_a_fee() {
        for keep in [true, false] {
            let mut s = weekly_state();
            s.squads[1][0].contract_years = 0;
            s.squads[1][0].loan = 0;
            s.squads[1][1] = s.squads[1][0].clone();
            s.squads[1][1].player_id = 10;
            s.squads[1][1].contract_years = 2;
            s.managers[1].balance = 0;
            let mut ui = Headless::new(&s);
            ui.keep_contracts = keep;
            let mut rng = Rng::new(6);
            s.current_manager = 1;
            let value = player_value_calc(&s, &mut rng.clone(), 0, 0);
            contract_talks(&mut s, &Runtime::default(), &mut rng, &mut ui, 1);
            assert_eq!(ui.negotiations, [(1, 0)]);
            if keep {
                assert_eq!(s.squads[1][0].player_id, 9);
                assert_eq!(s.managers[1].balance, 0);
            } else {
                let fee = value / 2 / 1000 * 1000;
                assert_eq!(s.managers[1].balance, fee);
                assert_eq!(
                    s.squads[1][0].player_id, 10,
                    "the leaving player is removed"
                );
                let n = ui.season_end.len();
                assert!(ui.season_end[n - 2].contains(&format!("fee: {fee}")));
                assert!(ui.season_end[n - 1].starts_with("ContractDone"));
            }
        }
    }

    #[test]
    fn the_option_matrix_follows_the_managers_tiers() {
        let mut s = weekly_state();
        s.managers[0].league = 0;
        s.managers[1].league = 2;
        s.options[..9].copy_from_slice(&[0, 1, 0, 1, 1, 1, 0, 0, 0]);
        let mut ui = Headless::new(&s);
        ui.auto_play = true;
        season_end_messages(
            &mut s,
            &Runtime::default(),
            &mut Rng::new(7),
            &[false; 2],
            &mut ui,
        );
        // Tier 0 keeps its row and gains the columns another tier uses, tier 1 has no manager,
        // tier 2 gets every used column.
        assert_eq!(s.options[..9], [1, 1, 1, 0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn rebalancing_moves_nothing_without_free_places() {
        let mut s = weekly_state();
        s.squads[1][0] = crate::state::SquadPlayer::zeroed();
        let rt = Runtime::default();
        for (p, person) in s.players.iter_mut().enumerate().skip(1) {
            person.owner_manager = NO_OWNER;
            person.team = (p % 58) as u8;
        }
        let (mut want, mut rng) = (s.clone(), Rng::new(8));
        let mut want_rng = rng;
        season_rebalance_player_clubs(&mut s, &rt, &mut rng);
        player_pool_season_shuffle(&mut want, &mut want_rng);
        player_pool_season_shuffle(&mut want, &mut want_rng);
        assert!(s == want);
        assert_eq!(rng, want_rng);
    }

    #[test]
    fn the_last_season_ends_the_game_before_the_summer_break() {
        let mut s = season_final_state();
        s.scenario = s.year as u16;
        s.managers[0].name[..4].copy_from_slice(b"ANNA");
        s.managers[1].name[..4].copy_from_slice(b"BERT");
        let mut rng = Rng::new(41);
        let mut ui = Headless::new(&s);
        assert_eq!(
            season_final(&mut s, &mut Runtime::default(), &mut rng, &mut ui),
            Ok(true)
        );
        assert!(ui.game_over);
        assert_eq!((s.day_counter, s.logged_rounds), (SEASON_LAST_DAY + 4, 2));
        assert!(ui.highscore_inserts.is_empty());
        let rows: Vec<&[u8]> = ui.table.entries[..3]
            .iter()
            .map(|e| e.manager_name())
            .collect();
        assert_eq!(rows[2], b"", "two entries");
        assert!(rows[..2].contains(&&b"ANNA"[..]) && rows[..2].contains(&&b"BERT"[..]));
        assert!(ui.table.entries[0].score >= ui.table.entries[1].score);
    }

    /// Every real save plays on into the first office turn of the next season, or to the end
    /// of the game. The run is capped at 1000 office turns.
    #[test]
    fn real_saves_play_to_the_next_season() {
        for (path, mut s) in save_states() {
            let what = path.display().to_string();
            let before = s.clone();
            let mut rt = Runtime::default();
            let mut rng = Rng::new(0x1234);
            let mut ui = Headless::new(&s);
            ui.stop_at = 1000;
            let end = season_main_loop(&mut s, &mut rt, &mut rng, &mut ui, true);
            assert!(ui.turns <= ui.stop_at, "{what}: turn cap reached");
            match end {
                Ok(GameOver) => {
                    assert!(ui.game_over, "{what}");
                    assert_eq!(i32::from(s.scenario), s.year, "{what}");
                }
                Err(Done) => {
                    assert_eq!(s.start_year, before.start_year + 1, "{what}");
                    assert_eq!(s.logged_rounds, before.logged_rounds + 1, "{what}");
                    assert_eq!(s.season_week, 0, "{what}");
                    assert_eq!(s.day_counter, i32::from(SEASON_START_DAY), "{what}");
                    assert_eq!(s.competition_rounds, [1; 3], "{what}");
                    let e = usize::from(before.logged_rounds.min(0x31));
                    for (m, old) in before.active_managers().iter().enumerate() {
                        assert_eq!(s.managers[m].round_log[e][1], old.league, "{what}");
                    }
                }
            }
            assert!(ui.titles.contains(&Title::SeasonEnd), "{what}");
            let bytes = s.to_savegame().to_bytes().unwrap();
            let back = GameState::from_savegame(&SaveGame::parse(&bytes).unwrap()).unwrap();
            assert_eq!(back.to_savegame(), s.to_savegame(), "{what}");
        }
    }
}
