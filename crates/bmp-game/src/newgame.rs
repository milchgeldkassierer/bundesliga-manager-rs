//! New-game world initialisation of `BMMAIN.EXE`: `main` up to the start menu ([`startup`]),
//! the start menu's finish for a new game ([`new_game`]) or for managers joining a running game
//! ([`add_managers`]), the club shuffle and the player pool built from MANA.DAT.
//!
//! The start menu itself (UI) runs between [`startup`] and [`new_game`] and draws no random
//! numbers. The cup draws `main` makes ([`crate::cups`]) run through [`CupInit`] at their place
//! in the random stream; [`CupDraws`] makes them.
//!
//! Full documentation with addresses: `docs/re/logic/flow.md` §2, §4.6–§4.9 and §5.1–§5.5.

use bmp_formats::mana::ManaDat;
use bmp_formats::savegame::{
    ONE_YEAR_SCENARIOS, SCENARIO_MODERN_OFFSET, SCENARIO_ONE_YEAR, SCENARIO_THREE_YEARS,
    THREE_YEAR_SCENARIOS,
};

use crate::ai::{league_strength_drift, squad_recalc_team_strength};
use crate::cups::{
    CupDrawUi, cup_clear_match_goals, cup_draw_first_round, euro_cup_qualifiers_build,
    league_shuffle_teams, round_pairings,
};
use crate::finance::interest_rates_update;
use crate::league_layout::layout;
use crate::rng::{Rng, ldiv, lmul};
use crate::runtime::{Runtime, SEASON_START_DAY};
use crate::season::pick_random_free_team_in_tier;
use crate::squad::{LineupRules, auto_lineup, squad_insert_player};
use crate::state::{
    GERMAN_CLUBS, GameState, Layout, MANAGER_SLOTS, Manager, MatchHistory, NO_OWNER, PLAYER_COUNT,
    SQUAD_SIZE, strcpy,
};
use crate::stats::{advertising_offers_generate, mana_load_club_table, player_pool_season_shuffle};
use crate::transfer::{player_value_calc, transfer_market_refresh};

/// DS:9438: players per position group (keepers, defenders, midfielders, forwards) of a new
/// human squad.
const SQUAD_GROUP_SIZES: [u8; 4] = [2, 5, 8, 5];
/// DS:943c: free records counted down from the top of each position group by the human
/// squads of a new game.
const FREE_GROUP_RECORDS: [u8; 4] = [0x13, 0x2c, 0x2a, 0x2d];
/// DS:07a6: first player record of each position group, and the end of the table.
const POOL_GROUP_BOUNDS: [u8; 5] = [1, 0x14, 0x40, 0x6a, 0x97];
/// DS:9432: first MANA.DAT name slot of each position group, and the end of the slots.
const NAME_SLOT_BOUNDS: [u8; 5] = [0, 2, 8, 15, 20];
/// DS:0286: initial training values of a manager.
const TRAINING_141: [u8; 5] = [5, 4, 6, 5, 6];
/// DS:028c: initial training values of a manager.
const TRAINING_146: [u8; 4] = [1, 3, 3, 3];
/// FAR:513c of an endless game.
const ENDLESS: u16 = 0x56ce;
/// DS:07e0 of a modern start (1992).
const MODERN_START_YEAR: u16 = 0x7c8;
/// DS:07e0 of a historical start (1963), as the program image holds it.
const HISTORICAL_START_YEAR: u16 = 0x7ab;
/// MOD2026: DS:07e0 of every new game with Mod 2026 data (season 2026/27).
pub const MOD2026_START_YEAR: u16 = 2026;

/// MOD2026: the year the running game's first season started in. `season_final` advances
/// `start_year` and `logged_rounds` together, so their difference stays fixed.
pub fn first_season_year(state: &GameState) -> u16 {
    state.start_year.wrapping_sub(state.logged_rounds)
}

/// MOD2026: the year whose season end ends the game, `None` for an endless game. FAR:513c
/// holds the original's end-year codes (1964/1966 or 1993/1995, endless above 2000); a mod
/// game counts their length from its own first season instead (`docs/mod2026/CHANGES.md`).
pub fn game_end_year(state: &GameState, mod2026: bool) -> Option<u16> {
    let code = state.scenario;
    if code > 2000 {
        return None;
    }
    if !mod2026 {
        return Some(code);
    }
    let code_start = if code > MODERN_START_YEAR {
        MODERN_START_YEAR
    } else {
        HISTORICAL_START_YEAR
    };
    Some(first_season_year(state).wrapping_add(code.wrapping_sub(code_start)))
}

/// The cup draws `main` makes while it sets up a new game (`docs/re/logic/cups.md`). Each hook
/// runs at the place in the random stream where `main` calls the cup functions.
pub trait CupInit {
    /// `euro_cup_qualifiers_build` 27c2:0ef2, `cup_clear_match_goals` 27c2:1b6d and
    /// `cup_draw_first_round(c, 0)` for c = 1..=3; `main` 18bc:0718..0750, after
    /// [`league_shuffle_teams`].
    fn before_start_menu(&mut self, state: &mut GameState, rt: &mut Runtime, rng: &mut Rng);
    /// `cup_draw_first_round(0, 0)`, then `cup_draw_first_round(c, 1)` for c = 1..=3; `main`
    /// 18bc:0888..08a5, after [`assign_pool_players_to_teams`].
    fn after_pool_assignment(&mut self, state: &mut GameState, rt: &mut Runtime, rng: &mut Rng);
}

/// The cup draws of the original, shown through the draw screens `U` (a plain
/// `FnMut(u8) -> bool` answers only the "An Alle … AUSLOSUNG BEIWOHNEN ?" question, see
/// [`crate::cups::cup_draw_screen_open`]).
pub struct CupDraws<U: CupDrawUi>(pub U);

impl<U: CupDrawUi> CupInit for CupDraws<U> {
    fn before_start_menu(&mut self, state: &mut GameState, rt: &mut Runtime, rng: &mut Rng) {
        euro_cup_qualifiers_build(state, rt);
        cup_clear_match_goals(state);
        for cup in 1..=3 {
            cup_draw_first_round(state, rt, rng, cup, false, &mut self.0);
        }
    }

    fn after_pool_assignment(&mut self, state: &mut GameState, rt: &mut Runtime, rng: &mut Rng) {
        cup_draw_first_round(state, rt, rng, 0, false, &mut self.0);
        for cup in 1..=3 {
            cup_draw_first_round(state, rt, rng, cup, true, &mut self.0);
        }
    }
}

/// The start-menu part of a manager record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerChoice {
    /// `+0x00` name, at most 28 bytes, without the NUL.
    pub name: Vec<u8>,
    /// `+0x1e` club slot, [`crate::state::NO_TEAM`] for none.
    pub team: u8,
    /// `+0x1d` portrait slot (1-based), swapped by clicking two portraits.
    pub slot: u8,
}

/// Game-mode bit "Endlosspiel" of [`GameOptions::mode_flags`].
pub const MODE_ENDLESS: u8 = 1;
/// Game-mode bit "1-Jahres-Spiel".
pub const MODE_ONE_YEAR: u8 = 2;
/// Game-mode bit "3-Jahres-Spiel".
pub const MODE_THREE_YEARS: u8 = 4;
/// Game-mode bit "Histor. Start".
pub const MODE_HISTORICAL: u8 = 8;
/// The game-length bits, of which the dialog keeps exactly one.
pub const MODE_LENGTHS: u8 = MODE_ENDLESS | MODE_ONE_YEAR | MODE_THREE_YEARS;

/// The new-game options dialog (flow.md §4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptions {
    /// Shown level 0..5 (`DS:4a28` before it is stored as `5 - level`).
    pub level: u8,
    /// Game-mode bits `MODE_*`.
    pub mode_flags: u8,
}

impl GameOptions {
    /// `FAR:513c`, the end year (save block 43).
    pub fn end_year(&self) -> u16 {
        let mut year = ENDLESS;
        if self.mode_flags & MODE_ONE_YEAR != 0 {
            year = SCENARIO_ONE_YEAR;
        }
        if self.mode_flags & MODE_THREE_YEARS != 0 {
            year = SCENARIO_THREE_YEARS;
        }
        if self.modern_start() {
            year += SCENARIO_MODERN_OFFSET;
        }
        year
    }

    /// `FAR:56f4`: modern start (bit 3 "Histor. Start" clear).
    pub fn modern_start(&self) -> bool {
        self.mode_flags & MODE_HISTORICAL == 0
    }

    /// `DS:4a28` as stored: `5 - shown level`.
    pub fn difficulty(&self) -> u8 {
        5u8.wrapping_sub(self.level)
    }
}

/// What the player chose in the start menu for a new game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuChoices {
    /// All four manager slots; the inactive ones hold [`crate::state::NO_TEAM`].
    pub managers: [ManagerChoice; MANAGER_SLOTS],
    /// `DS:07ab`: number of active managers.
    pub count: u8,
    /// `[bp-4]`: the carousel position when the menu ended. The per-manager initialisation
    /// reuses this local (see [`new_game`]).
    pub carousel_pos: i16,
    pub options: GameOptions,
}

/// `main` from loading MANA.DAT up to the start menu: the club table, the first seed, the
/// cleared tables, the club shuffle and the European cup draws.
///
/// `rng` must be in its power-on state ([`Rng::default`]): MANA.DAT is read before the first
/// seed, with 800 draws.
///
/// Origin: BMMAIN.EXE 18bc:0418 (main), up to the start menu
pub fn startup(
    mana: &ManaDat,
    rt: &mut Runtime,
    rng: &mut Rng,
    time: i32,
    cups: &mut dyn CupInit,
) -> GameState {
    let mut state = GameState::program_start();
    mana_load_club_table(&mut state, rt, rng, mana);
    rng.seed_from_time(time);
    state.manager_count = 0;
    startup_clear_state(&mut state);
    league_shuffle_teams(&mut state, rt, rng);
    cups.before_start_menu(&mut state, rt, rng);
    state
}

/// The rest of a new game once the start menu has ended: the managers, the options, the
/// player pool, each manager's club, squad and finances, then `main`'s part with the second
/// seed `time2`, the free players' clubs, the DFB-Pokal draw and the transfer market.
///
/// Every manager's squad attributes are also written into pool record `carousel_pos` (7 after
/// a one-year manager), as in the original.
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load, finish with `n0 == 0`) and
/// 18bc:0418 (main, 18bc:0850..08b8)
pub fn new_game(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    mana: &ManaDat,
    choices: &MenuChoices,
    time2: i32,
    cups: &mut dyn CupInit,
) {
    for (m, choice) in state.managers.iter_mut().zip(&choices.managers) {
        write_choice(m, choice);
    }
    state.manager_count = choices.count;
    let saved = state.current_manager;
    apply_game_options(state, rt, &choices.options);
    newgame_init_player_pool(state, rt, rng, mana);
    let mut bp4 = choices.carousel_pos;
    manager_init_loop(state, rt, rng, 0, &mut bp4, None);
    state.current_manager = saved;
    interest_rates_update(state, rng);

    league_strength_drift(state, rt, rng, 10);
    rng.seed_from_time(time2);
    state.competition_rounds[0] = 1;
    assign_pool_players_to_teams(state, rt, rng);
    cups.after_pool_assignment(state, rt, rng);
    state.day_counter = i32::from(SEASON_START_DAY);
    transfer_market_refresh(state, rng);
    // The message table has one (empty) group per manager; the save format requires it.
    state.messages = vec![Vec::new(); usize::from(state.manager_count)];
}

/// Adds managers to a running game (start menu opened from the office): each gets 20 free
/// players, then a club, squad attributes, finances and a lineup as in a new game.
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load, finish with `n0 != 0`)
pub fn add_managers(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    new: &[ManagerChoice],
    rules: LineupRules,
) {
    let n0 = state.manager_count;
    for (m, choice) in state.managers[usize::from(n0)..].iter_mut().zip(new) {
        write_choice(m, choice);
    }
    state.manager_count = n0 + new.len() as u8;
    let saved = state.current_manager;
    add_manager_squads(state, rng, n0);
    // `[bp-4]` still holds the last group size of the squad loop.
    let mut bp4 = i16::from(SQUAD_GROUP_SIZES[3]);
    manager_init_loop(state, rt, rng, n0, &mut bp4, Some(rules));
    state.current_manager = saved;
    state
        .messages
        .resize(usize::from(state.manager_count), Vec::new());
}

/// The fields of manager record `m` the start menu edits.
fn write_choice(m: &mut Manager, choice: &ManagerChoice) {
    let n = choice.name.len().min(m.name.len() - 1);
    m.name = [0; 29];
    m.name[..n].copy_from_slice(&choice.name[..n]);
    m.team = choice.team;
    m.slot = choice.slot;
}

/// Clears the head-to-head and record tables, marks every head-to-head entry empty, resets
/// three cup stage bytes of the active managers and numbers the four manager slots.
///
/// Origin: BMMAIN.EXE 18bc:01fb (startup_clear_state)
pub fn startup_clear_state(state: &mut GameState) {
    state.match_history = MatchHistory::zeroed();
    let count = usize::from(state.manager_count).min(MANAGER_SLOTS);
    for m in &mut state.managers[..count] {
        m.values_132[2..5].fill(0x1e);
    }
    for (i, m) in state.managers.iter_mut().enumerate() {
        m.slot = i as u8 + 1;
    }
    let h = &mut state.match_history.recent_results;
    h.as_flattened_mut().as_flattened_mut().fill([0xff; 2]);
}

/// Swaps the league table records of clubs `a` and `b`.
///
/// Origin: BMMAIN.EXE 18bc:1fc4 (team_stats_swap)
pub fn team_stats_swap(state: &mut GameState, a: u8, b: u8) {
    state.team_tables.swap(usize::from(a), usize::from(b));
}

/// Whether `team` is drawn in one of the European cups selected by `mask` (bit `c - 1` for
/// cup `c` = 1..=3) in the current round (`event_state[1]`).
///
/// # Panics
/// When `event_state[1]` is above 5.
///
/// Origin: BMMAIN.EXE 18bc:1df3 (team_in_european_cup_lists)
pub fn team_in_european_cup_lists(state: &GameState, team: u8, mask: u8) -> bool {
    // TODO(re): a round index above 5 reads past DS:07d6 in the original.
    let n = usize::from(round_pairings(state.event_state[1]));
    // In the first round the UEFA cup list runs 32 bytes past FAR:76ca into the first squad
    // record (FAR:774a).
    let mut lists = state.cup_pairings.as_flattened().to_vec();
    lists.extend_from_slice(&state.squads[0][0].to_bytes()[..32]);
    (1..=3usize)
        .any(|c| mask & (1 << (c - 1)) != 0 && lists[c * 32..c * 32 + 2 * n].contains(&team))
}

/// Player name `idx` of club `team` in MANA.DAT, found by the club's name.
///
/// Origin: BMMAIN.EXE 422b:01d0 (mana_team_player_name_ptr)
pub fn mana_team_player_name_ptr<'a>(
    mana: &'a ManaDat,
    state: &GameState,
    team: u8,
    idx: usize,
) -> &'a [u8] {
    let name = state.clubs[usize::from(team)].name();
    mana.clubs[..GERMAN_CLUBS]
        .iter()
        .find(|c| c.name_bytes() == name)
        .and_then(|c| c.player_name_bytes(idx))
        // TODO(re): without a match the original reads past the 64 records into the
        // uninitialised rest of its 40000-byte buffer.
        .unwrap_or(b"")
}

/// A random MANA.DAT name of position group `cat` from a Bundesliga club no human manager
/// runs, not yet used by any pool player. With `rec != 0xff` the pool record `rec` gets that
/// club.
///
/// Origin: BMMAIN.EXE 422b:022e (mana_pick_unused_player_name)
pub fn mana_pick_unused_player_name(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    mana: &ManaDat,
    cat: usize,
    rec: u8,
) -> Vec<u8> {
    let count = usize::from(state.manager_count).min(MANAGER_SLOTS);
    loop {
        let slot = rng.random_range(
            i16::from(NAME_SLOT_BOUNDS[cat]),
            i16::from(NAME_SLOT_BOUNDS[cat + 1]) - 1,
        );
        let t = loop {
            let t = rng.random_range(0, i16::from(rt.tier_team_count[0]) - 1) as u8;
            if !state.managers[..count].iter().any(|m| m.team == t) {
                break t;
            }
        };
        let name = mana_team_player_name_ptr(mana, state, t, slot as usize);
        if name.is_empty() || state.players[1..].iter().any(|p| p.name() == name) {
            continue;
        }
        if rec != 0xff {
            state.players[usize::from(rec)].team = t;
        }
        return name.to_vec();
    }
}

/// Builds the 150 pool players of a new game: random age, skills and character for all,
/// then 20 players for each human squad named after the club's MANA.DAT players, then names
/// and Bundesliga clubs for the rest (no club in an endless game), and finally the goal talent
/// by position group.
///
/// The human squads take their records from the top of each position group. The original
/// counts these records down in a global it never resets; this port starts from the image
/// values, which is the same as long as one new game is made per program run, as in the
/// original.
///
/// Origin: BMMAIN.EXE 422b:035c (newgame_init_player_pool)
pub fn newgame_init_player_pool(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    mana: &ManaDat,
) {
    for squad in &mut state.squads[..MANAGER_SLOTS] {
        for rec in &mut squad[..SQUAD_SIZE] {
            rec.player_id = 0;
        }
    }
    for p in &mut state.players[1..] {
        p.age = rng.random_range(0x12, 0x21) as u8;
        let mut t = rng.random_range(0x1e, 0x59);
        if t < 0x2d && rng.random_range(0, 1) != 0 {
            t = rng.random_range(0x1e, 0x59);
        }
        let t = t as u8;
        p.skills[1] = 10u8.wrapping_sub((rng.random_range(0, 0x14) as u8).wrapping_sub(t));
        p.skills[2] = rng.random_range(0x2d, 0x37) as u8;
        p.skills[0] = 10u8.wrapping_sub((rng.random_range(0, 0x14) as u8).wrapping_sub(t));
        p.average_skill = (p.skills.iter().map(|&s| u16::from(s)).sum::<u16>() / 3) as u8;
        p.attr_20 = rng.random_range(0, 6) as u8;
        p.owner_manager = NO_OWNER;
    }

    let mut free = FREE_GROUP_RECORDS;
    state.current_manager = 0;
    while i16::from(state.current_manager) < i16::from(state.manager_count) {
        let m = state.current_manager as usize;
        state.saved_manager = state.current_manager;
        let mut n = 0usize;
        for k in 0..4 {
            for _ in 0..SQUAD_GROUP_SIZES[k] {
                let p = usize::from(POOL_GROUP_BOUNDS[k + 1] - free[k]) + 1;
                free[k] -= 1;
                let team = state.managers[m].team;
                let mut name = mana_team_player_name_ptr(mana, state, team, n).to_vec();
                if name.is_empty() {
                    name = mana_pick_unused_player_name(state, rt, rng, mana, k, 0xff);
                }
                strcpy(&mut state.players[p].name, &name);
                n += 1;
                state.players[p].owner_manager = m as u8;
                state.players[p].age = rng.random_range(0x13, 0x1f) as u8;
                // The original ignores a refusal; a new squad never is full.
                let _ = squad_insert_player(state, rng, p as u8, 2, 0, 0x18);
                state.players[p].team = state.managers[m].team;
            }
        }
        state.current_manager += 1;
    }

    let mut k = 0;
    for p in 1..PLAYER_COUNT {
        if usize::from(POOL_GROUP_BOUNDS[k + 1]) <= p {
            k += 1;
        }
        if state.players[p].name[0] == 0 {
            let name = mana_pick_unused_player_name(state, rt, rng, mana, k, p as u8);
            strcpy(&mut state.players[p].name, &name);
            if state.scenario > 2000 {
                state.players[p].team = 0xff;
            }
        }
    }
    for p in 1..=0x14 {
        state.players[p].position_value = 0;
    }
    for (range, base) in [
        (0x15..=0x3f, 0x19),
        (0x40..=0x69, 0x32),
        (0x6a..=0x96, 0x4b),
    ] {
        for p in range {
            state.players[p].position_value = rng.random_range(0, 0x10) as u8 + base;
        }
    }
}

/// Gives the free pool players of a new game their clubs: after the pool reshuffle, every
/// player of no human club whose club is "none" (or every such player once the year is set)
/// gets a random club of each tier whose quota is left, the last one winning.
///
/// Origin: BMMAIN.EXE 24a4:19fb (assign_pool_players_to_teams)
pub fn assign_pool_players_to_teams(state: &mut GameState, rt: &Runtime, rng: &mut Rng) {
    let mut quota = player_pool_season_shuffle(state, rng);
    // The original ranks the tiers here for the club pick, which never reads the ranking and
    // draws nothing (rank_tier_teams_by_strength).
    for p in 1..PLAYER_COUNT {
        let team = state.players[p].team;
        if state.manager_index_of_team(team, 0xff) != 0 || (team != 0xff && state.year == 0) {
            continue;
        }
        for (k, q) in quota.iter_mut().enumerate() {
            if *q != 0 {
                state.players[p].team = pick_random_free_team_in_tier(state, rt, rng, k);
                *q = q.wrapping_sub(1);
            }
        }
    }
}

/// The new-game options: end year, start, season year and stored difficulty.
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load, options dialog tail)
fn apply_game_options(state: &mut GameState, rt: &mut Runtime, options: &GameOptions) {
    state.scenario = options.end_year();
    rt.modern_start = options.modern_start();
    if rt.modern_start {
        state.start_year = MODERN_START_YEAR;
    }
    // MOD2026: a mod game starts in season 2026/27 whatever "Histor. Start" says.
    if rt.mod2026 {
        state.start_year = MOD2026_START_YEAR;
    }
    state.level = options.difficulty();
}

/// The first 20 players of each manager `n0..` of a running game: free players drawn from
/// each position group (at most 1000 draws per player).
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load, finish with `n0 != 0`)
fn add_manager_squads(state: &mut GameState, rng: &mut Rng, n0: u8) {
    state.current_manager = n0 as i8;
    while i16::from(state.current_manager) < i16::from(state.manager_count) {
        let m = state.current_manager as usize;
        state.saved_manager = state.current_manager;
        let mgr = &mut state.managers[m];
        mgr.position_history = [10; 38];
        mgr.values_132 = [0x1e; 5];
        for entry in &mut mgr.round_log {
            entry[0] = 0xff;
        }
        for k in 0..4 {
            for _ in 0..SQUAD_GROUP_SIZES[k] {
                let mut tries = 0;
                let p = loop {
                    let p = rng.random_range(
                        i16::from(POOL_GROUP_BOUNDS[k]),
                        i16::from(POOL_GROUP_BOUNDS[k + 1]),
                    ) as usize;
                    tries += 1;
                    // DEVIATION: the forward range ends at 0x97, one record past the pool
                    // (FAR:6db0). The original reads that record's owner byte, and when it
                    // is 5 or the 1000 tries are used up it writes the manager into it and
                    // inserts player 151 into the squad (corrupting the data behind the
                    // pool). The port never takes record 151 and draws again instead; the
                    // random stream differs only where the original would corrupt memory.
                    let in_pool = p < state.players.len();
                    let free = in_pool && state.players[p].owner_manager == NO_OWNER;
                    if free || (in_pool && tries >= 1000) {
                        break p;
                    }
                };
                state.players[p].owner_manager = m as u8;
                // The original ignores a refusal.
                let _ = squad_insert_player(state, rng, p as u8, 1, 0, 0x18);
            }
        }
        state.current_manager += 1;
    }
    state.current_manager = n0 as i8;
}

/// The per-manager initialisation of the start menu for managers `n0..`: contracts, money,
/// a free club slot in the start tier, squad attributes and salaries, training, stadium,
/// fans, the wage budget and the advertising offers; with `n0 != 0` also the lineup.
///
/// `bp4` is the original's reused local `[bp-4]`: the pool record that also receives each
/// manager's squad attributes.
///
/// # Panics
/// When `n0 != 0` and `rules` is `None`.
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load, label 18bc:3950)
fn manager_init_loop(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    n0: u8,
    bp4: &mut i16,
    rules: Option<LineupRules>,
) {
    state.current_manager = n0 as i8;
    while i16::from(state.current_manager) < i16::from(state.manager_count) {
        let m = state.current_manager as usize;
        state.saved_manager = state.current_manager;
        init_manager(state, rt, rng, m, n0, bp4);
        if n0 != 0 {
            auto_lineup(
                state,
                rules.expect("adding managers needs the lineup rules"),
            );
            squad_recalc_team_strength(state, rt, rng, m as u8, false);
        }
        state.current_manager += 1;
    }
}

/// One pass of [`manager_init_loop`] for manager `m`, up to the advertising offers.
fn init_manager(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    m: usize,
    n0: u8,
    bp4: &mut i16,
) {
    let sc = state.scenario;
    let one_year = ONE_YEAR_SCENARIOS.contains(&sc);
    let three_years = THREE_YEAR_SCENARIOS.contains(&sc);
    if one_year {
        let c = &mut state.manager_contracts[m];
        c[8] = 30000;
        c[0] = 180_000;
        c[1..7].fill(45000);
        *bp4 = 7;
    }
    if state.managers[0].name[0] == 0 {
        state.managers[m].team = (m * 11) as u8;
    }
    let endless = if rt.modern_start {
        sc > THREE_YEAR_SCENARIOS[1]
    } else {
        sc > THREE_YEAR_SCENARIOS[0]
    };
    let tf: u8 = if endless { 2 } else { 0 };
    let level = state.level as i8;
    let mgr = &mut state.managers[m];
    mgr.balance = if level == 4 { 1_900_000 } else { 1_500_000 };
    mgr.records[2] = 99999;
    let mut base = i16::from(level) / 2 + 0x1b;
    if tf == 0 {
        base += 0x29;
    }
    if one_year {
        base += 0x0c;
    }

    let team = state.managers[m].team;
    let keep = tf != 0
        && rt.league_of_club(team) >> 1 == i16::from(tf)
        && team <= rt.tier_club_ranges[2][1];
    if !keep {
        let r = draw_tier_slot(state, rt, rng, m, tf, |s, r| {
            !team_in_european_cup_lists(s, r, 7)
        });
        move_club(state, m, r);
    }
    if one_year && n0 == 0 {
        state.managers[m].values_132[3] = 1;
        let r = draw_tier_slot(state, rt, rng, m, tf, |s, r| {
            team_in_european_cup_lists(s, r, 4)
        });
        move_club(state, m, r);
    }

    // The count comes from squad `i`, the records written are always those of squad `m`.
    let pool = *bp4 as usize;
    for i in 0..MANAGER_SLOTS {
        for k in 0..usize::from(state.squad_player_count(i)) {
            for j in 0..3 {
                let v = rng.random_range(base, base + 5) as u8;
                state.squads[m][k].skills[j] = v;
                let w = rng.random_range(0x28, 0x3c) as u8;
                if j == 2 {
                    state.squads[m][k].skills[2] = w;
                }
                state.players[pool].skills[j] = if j == 2 { w } else { v };
            }
        }
    }
    for k in 0..state.squad_player_count(m) {
        state.squads[m][usize::from(k)].market_value = player_value_calc(state, rng, k, 1);
    }

    let tfl = i32::from(tf);
    let y1 = i32::from(one_year);
    let mgr = &mut state.managers[m];
    mgr.training_141 = TRAINING_141;
    mgr.training_146 = TRAINING_146;
    mgr.league = tf;
    mgr.value_10a = ((u8::from(one_year) + 8) * 2).wrapping_sub(tf * 3);
    mgr.stadium[1][0] = (5 - tfl) * 4000;
    mgr.stadium[0][0] = (2 - tfl) * 3000;
    if one_year {
        mgr.stadium[0][0] += 5000;
        mgr.stadium[1][0] += 10000;
    }
    mgr.stadium[2][0] = (2 - tfl) * 3500;
    mgr.stadium[5][0] = y1 + 4;
    mgr.stadium[6][0] = y1 + 3;
    if sc < 2000 {
        mgr.stadium[4][0] = 3 - 2 * i32::from(three_years);
        mgr.stadium[3][0] = 3 - i32::from(three_years);
    }
    mgr.value_131 = 0x10;
    mgr.fans = u16::from(tf).wrapping_mul(0xffec).wrapping_add(0x32);
    manager_compute_wage_budget(state, rng, m);
    advertising_offers_generate(state, rng);
}

/// A random slot of tier `tf` that no manager before `m` holds and that `accept`s.
fn draw_tier_slot(
    state: &GameState,
    rt: &Runtime,
    rng: &mut Rng,
    m: usize,
    tf: u8,
    accept: impl Fn(&GameState, u8) -> bool,
) -> u8 {
    let t = usize::from(tf);
    let lo = i16::from(layout().first_team[t] as i8);
    let hi = lo + i16::from(rt.tier_team_count[t]) - 1;
    loop {
        let r = rng.random_range(lo, hi) as u8;
        if !state.managers[..m].iter().any(|x| x.team == r) && accept(state, r) {
            return r;
        }
    }
}

/// Moves manager `m`'s club into slot `r`; the league table records stay with the slots.
fn move_club(state: &mut GameState, m: usize, r: u8) {
    let t = state.managers[m].team;
    state.team_swap_slots(t, r, false);
    team_stats_swap(state, t, r);
}

/// Manager `m`'s monthly extra costs and contract item 7 from the league tier and the fans.
/// The costs use 16-bit arithmetic: below the three league tiers the tier difference wraps.
///
/// Origin: BMMAIN.EXE 18bc:0a63 (manager_compute_wage_budget)
pub fn manager_compute_wage_budget(state: &mut GameState, rng: &mut Rng, m: usize) {
    let mgr = &state.managers[m];
    let (league, fans) = (mgr.league, mgr.fans);
    let d = 2u16.wrapping_sub(u16::from(league));
    let r = rng.random_range(15, 20) as u16;
    let a = d.wrapping_mul(r.wrapping_add(40));
    let b = (d.wrapping_mul(0x6e_u16.wrapping_sub(fans)) & 0xfffe).wrapping_mul(500);
    let costs = a.wrapping_add(b);
    let di = d as i16;
    let r2 = rng.random_range(
        di.wrapping_add(10).wrapping_mul(10),
        di.wrapping_add(4).wrapping_mul(25),
    );
    state.managers[m].extra_costs = ldiv(lmul(i32::from(costs), i32::from(r2)), 100) as u16;
    let x = i32::from(2i16.wrapping_sub(i16::from(league))) * 30 + i32::from(fans);
    state.manager_contracts[m][7] = lmul(x, 1000);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::{CLUB_LISTED, NO_TEAM, TRANSFER_LIST, c_str};
    use bmp_formats::mana;
    use bmp_formats::savegame::SaveGame;

    /// A MANA.DAT with clubs `"CLUB nnn"`, 20 players `"Pcc-nn"` per German club (slot 7 is
    /// empty for even clubs) and strengths 20..=79.
    pub(crate) fn synthetic_mana() -> ManaDat {
        let field = |text: String, len: usize| {
            let mut v = vec![0u8; len];
            v[..text.len()].copy_from_slice(text.as_bytes());
            v
        };
        let clubs = (0..mana::CLUB_COUNT)
            .map(|i| mana::Club {
                name: field(format!("CLUB {i:03}"), mana::CLUB_NAME_LEN)
                    .try_into()
                    .unwrap(),
                players: if i < mana::GERMAN_CLUB_COUNT {
                    (0..mana::PLAYERS_PER_CLUB)
                        .map(|k| {
                            let text = if k == 7 && i % 2 == 0 {
                                String::new()
                            } else {
                                format!("P{i:02}-{k:02}")
                            };
                            field(text, mana::PLAYER_NAME_LEN).try_into().unwrap()
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            })
            .collect();
        ManaDat {
            crest: std::array::from_fn(|i| 0x50 + i as u8),
            strength: std::array::from_fn(|i| [20 + (i % 60) as u8, 40, 60]),
            points: [[0; 2]; mana::GERMAN_CLUB_COUNT],
            goals: [[0; 2]; mana::GERMAN_CLUB_COUNT],
            rank: [0; mana::GERMAN_CLUB_COUNT],
            clubs,
        }
    }

    /// Draws no cups: the cup tables keep their values and no random numbers are drawn.
    struct NoCups;

    impl CupInit for NoCups {
        fn before_start_menu(&mut self, _: &mut GameState, _: &mut Runtime, _: &mut Rng) {}
        fn after_pool_assignment(&mut self, _: &mut GameState, _: &mut Runtime, _: &mut Rng) {}
    }

    /// Puts clubs 0..=5 into the first-round UEFA cup list and foreign clubs everywhere else
    /// in the European cup lists, so that one-year games find their clubs.
    struct TestCups;

    impl CupInit for TestCups {
        fn before_start_menu(&mut self, state: &mut GameState, _: &mut Runtime, _: &mut Rng) {
            for (i, pair) in state.cup_pairings[16..].iter_mut().enumerate() {
                *pair = [0x80 + i as u8, 0xc0];
            }
            for (i, pair) in state.cup_pairings[48..51].iter_mut().enumerate() {
                *pair = [2 * i as u8, 2 * i as u8 + 1];
            }
        }
        fn after_pool_assignment(&mut self, _: &mut GameState, _: &mut Runtime, _: &mut Rng) {}
    }

    pub(crate) fn choices(
        teams: &[u8],
        mode_flags: u8,
        level: u8,
        carousel_pos: i16,
    ) -> MenuChoices {
        MenuChoices {
            managers: std::array::from_fn(|m| ManagerChoice {
                name: if m < teams.len() {
                    format!("MANAGER {m}").into_bytes()
                } else {
                    Vec::new()
                },
                team: teams.get(m).copied().unwrap_or(NO_TEAM),
                slot: m as u8 + 1,
            }),
            count: teams.len() as u8,
            carousel_pos,
            options: GameOptions { level, mode_flags },
        }
    }

    /// `startup` and the start menu's finish up to the per-manager loop.
    fn up_to_manager_loop(mana: &ManaDat, seed: i32, c: &MenuChoices) -> (GameState, Runtime, Rng) {
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let mut s = startup(mana, &mut rt, &mut rng, seed, &mut TestCups);
        for (m, choice) in s.managers.iter_mut().zip(&c.managers) {
            write_choice(m, choice);
        }
        s.manager_count = c.count;
        apply_game_options(&mut s, &mut rt, &c.options);
        newgame_init_player_pool(&mut s, &rt, &mut rng, mana);
        (s, rt, rng)
    }

    fn full_game(
        mana: &ManaDat,
        seed: i32,
        c: &MenuChoices,
        cups: &mut dyn CupInit,
    ) -> (GameState, Runtime) {
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let mut s = startup(mana, &mut rt, &mut rng, seed, cups);
        new_game(
            &mut s,
            &mut rt,
            &mut rng,
            mana,
            c,
            seed.wrapping_add(7),
            cups,
        );
        (s, rt)
    }

    fn draws_between(mut from: Rng, to: Rng) -> usize {
        let mut n = 0;
        while from != to {
            from.rand();
            n += 1;
        }
        n
    }

    #[test]
    fn game_options_end_year_follows_mode_and_start() {
        let o = |mode_flags, level| GameOptions { level, mode_flags };
        assert_eq!(o(MODE_ENDLESS | MODE_HISTORICAL, 2).end_year(), 0x56ce);
        assert_eq!(o(MODE_ONE_YEAR | MODE_HISTORICAL, 2).end_year(), 0x7ac);
        assert_eq!(o(MODE_THREE_YEARS | MODE_HISTORICAL, 2).end_year(), 0x7ae);
        assert_eq!(o(MODE_ENDLESS, 2).end_year(), 0x56eb);
        assert_eq!(o(MODE_ONE_YEAR, 2).end_year(), 0x7c9);
        assert_eq!(o(MODE_THREE_YEARS, 2).end_year(), 0x7cb);
        assert!(o(MODE_ENDLESS, 0).modern_start());
        assert!(!o(MODE_ENDLESS | MODE_HISTORICAL, 0).modern_start());
        assert_eq!(o(MODE_ENDLESS, 2).difficulty(), 3);
    }

    #[test]
    fn startup_clears_the_history_and_numbers_the_slots() {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.match_history.record_scores[3][1] = 9;
        s.managers[2].slot = 9;
        startup_clear_state(&mut s);
        assert_eq!(s.match_history.record_scores[3][1], 0);
        assert!(
            s.match_history
                .recent_results
                .as_flattened()
                .as_flattened()
                .iter()
                .all(|e| *e == [0xff; 2])
        );
        assert_eq!(
            s.managers.iter().map(|m| m.slot).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert_eq!(s.managers[0].values_132, [0, 0, 0x1e, 0x1e, 0x1e]);
        assert_eq!(s.managers[1].values_132, [0; 5]);
    }

    #[test]
    fn league_shuffle_stays_inside_the_tiers() {
        let mana = synthetic_mana();
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let s = startup(&mana, &mut rt, &mut rng, 1234, &mut NoCups);
        let mut probe = Rng::default();
        for _ in 0..800 {
            probe.rand();
        }
        probe.seed_from_time(1234);
        assert_eq!(draws_between(probe, rng), 330);
        for t in 0..3 {
            let ids = layout().team_ids(t);
            let mut clubs: Vec<u8> = ids
                .clone()
                .map(|c| {
                    s.clubs[c as usize].name()[5..]
                        .iter()
                        .fold(0, |a, d| a * 10 + d - b'0')
                })
                .collect();
            clubs.sort_unstable();
            assert!(clubs.into_iter().eq(ids.clone()), "tier {t}");
            let n = ids.len();
            let mut order = s.league_teams[t][..n].to_vec();
            order.sort_unstable();
            assert!(order.into_iter().eq(ids), "league order of tier {t}");
        }
        // The league order lists the slots by club: the club at slot league_teams[t][i] is
        // the one MANA.DAT has at index first_team[t] + i.
        for (t, &first) in layout().first_team.iter().enumerate() {
            for i in 0..rt.tier_team_count[t] as usize {
                let slot = s.league_teams[t][i] as usize;
                let want = format!("CLUB {:03}", first as usize + i);
                assert_eq!(s.clubs[slot].name(), want.as_bytes());
            }
        }
        // Clubs 58..63 never move.
        s.clubs[58..64].iter().zip(58..).for_each(|(c, i)| {
            assert_eq!(c.name(), format!("CLUB {i:03}").as_bytes());
        });
    }

    #[test]
    fn european_cup_lists_follow_mask_and_round() {
        let mut s = GameState::zeroed();
        s.cup_pairings.fill([0x80; 2]);
        s.cup_pairings[16][0] = 7; // byte 32: cup 1
        s.cup_pairings[40][1] = 8; // byte 81: cups 1 and 2
        s.cup_pairings[63][1] = 9; // byte 127: cups 2 and 3
        assert!(team_in_european_cup_lists(&s, 7, 1));
        assert!(!team_in_european_cup_lists(&s, 7, 6));
        assert!(team_in_european_cup_lists(&s, 8, 2));
        assert!(!team_in_european_cup_lists(&s, 9, 1));
        assert!(team_in_european_cup_lists(&s, 9, 4));
        // The UEFA cup list of the first round ends 32 bytes into the first squad record.
        s.squads[0][0].player_id = 11; // byte 0x0f of the record
        assert!(team_in_european_cup_lists(&s, 11, 4));
        s.squads[0][0].market_value = 12; // byte 0x28: past the list
        assert!(!team_in_european_cup_lists(&s, 12, 4));
        // Round 1 has 16 pairings per cup: cup 1 covers bytes 32..64 only.
        s.event_state[1] = 1;
        assert!(team_in_european_cup_lists(&s, 7, 1));
        assert!(!team_in_european_cup_lists(&s, 8, 1));
        assert!(!team_in_european_cup_lists(&s, 11, 4));
    }

    /// The pool records of manager `m`'s squad: the top of each position group.
    fn squad_records(m: usize) -> Vec<usize> {
        let mut v = Vec::new();
        for k in 0..4 {
            let n = usize::from(SQUAD_GROUP_SIZES[k]);
            let first = usize::from(POOL_GROUP_BOUNDS[k + 1] - FREE_GROUP_RECORDS[k]) + 1 + n * m;
            v.extend(first..first + n);
        }
        v
    }

    #[test]
    fn player_pool_gives_each_manager_twenty_named_players() {
        let mana = synthetic_mana();
        let c = choices(&[4, 40], MODE_THREE_YEARS | MODE_HISTORICAL, 2, 0);
        let (s, _, _) = up_to_manager_loop(&mana, 99, &c);
        assert_eq!(squad_records(0)[..3], [2, 3, 21]);
        assert_eq!(squad_records(1)[..3], [4, 5, 26]);
        let mut used = std::collections::HashSet::new();
        for m in 0..2 {
            let team = s.managers[m].team;
            let club = s.clubs[team as usize].name();
            let mana_club = mana
                .clubs
                .iter()
                .position(|c| c.name_bytes() == club)
                .unwrap();
            for (n, p) in squad_records(m).into_iter().enumerate() {
                let player = &s.players[p];
                let own = mana.clubs[mana_club].player_name_bytes(n).unwrap();
                if own.is_empty() {
                    // A name of a Bundesliga club no human runs, slot of the position group.
                    let name = std::str::from_utf8(player.name()).unwrap();
                    let (c, k): (u8, u8) =
                        (name[1..3].parse().unwrap(), name[4..].parse().unwrap());
                    assert!(
                        s.clubs[..18]
                            .iter()
                            .any(|x| x.name() == format!("CLUB {c:03}").as_bytes())
                    );
                    let group = [2, 7, 15, 20].iter().position(|&end| n < end).unwrap();
                    let slots = NAME_SLOT_BOUNDS[group]..NAME_SLOT_BOUNDS[group + 1];
                    assert!(slots.contains(&k), "{name}");
                } else {
                    assert_eq!(player.name(), own);
                }
                assert_eq!((player.owner_manager, player.team), (m as u8, team));
                assert!((19..=31).contains(&player.age));
            }
            assert_eq!(s.squad_player_count(m), 20);
        }
        for (p, player) in s.players.iter().enumerate().skip(1) {
            assert!(!player.name().is_empty(), "record {p}");
            assert!(
                used.insert(player.name().to_vec())
                    || squad_records(0).contains(&p)
                    || squad_records(1).contains(&p)
            );
            if !squad_records(0).contains(&p) && !squad_records(1).contains(&p) {
                assert_eq!(player.owner_manager, NO_OWNER);
                assert!(player.team < 18 && s.manager_index_of_team(player.team, 0xff) == 0);
            }
            let talent = player.position_value;
            match p {
                1..=20 => assert_eq!(talent, 0),
                21..=63 => assert!((0x19..=0x29).contains(&talent)),
                64..=105 => assert!((0x32..=0x42).contains(&talent)),
                _ => assert!((0x4b..=0x5b).contains(&talent)),
            }
            assert_eq!(
                u16::from(player.average_skill),
                player.skills.iter().map(|&x| u16::from(x)).sum::<u16>() / 3
            );
        }
    }

    #[test]
    fn endless_games_leave_free_players_without_a_club() {
        let mana = synthetic_mana();
        for start in [0, MODE_HISTORICAL] {
            let (s, _, _) =
                up_to_manager_loop(&mana, 5, &choices(&[4], MODE_ENDLESS | start, 2, 0));
            for (p, player) in s.players.iter().enumerate().skip(1) {
                let human = squad_records(0).contains(&p);
                assert_eq!(player.team == 0xff, !human, "start {start}, record {p}");
            }
        }
        // End years up to 2000 keep the Bundesliga club of the name, also in a modern start.
        let (s, _, _) = up_to_manager_loop(&mana, 5, &choices(&[4], MODE_THREE_YEARS, 2, 0));
        assert!(s.players[1..].iter().all(|p| p.team < 18));
    }

    #[test]
    fn pool_assignment_in_year_zero_only_places_clubless_players() {
        let mana = synthetic_mana();
        let c = choices(&[4, 10], MODE_ENDLESS, 2, 0);
        let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 5, &c);
        assert_eq!(s.year, 0);
        let before = s.clone();
        assign_pool_players_to_teams(&mut s, &rt, &mut rng);
        let humans = [s.managers[0].team, s.managers[1].team];
        for (p, player) in s.players.iter().enumerate().skip(1) {
            if humans.contains(&before.players[p].team) {
                assert_eq!(player.team, before.players[p].team);
            } else {
                assert!(
                    player.team == 0xff || player.team < 64,
                    "record {p}: {}",
                    player.team
                );
                assert!(!humans.contains(&player.team));
            }
        }
        assert!(s.players[1..].iter().filter(|p| p.team != 0xff).count() > 100);
    }

    #[test]
    fn pool_assignment_in_year_zero_keeps_the_clubs_of_a_three_year_game() {
        let mana = synthetic_mana();
        let c = choices(&[4, 10], MODE_THREE_YEARS | MODE_HISTORICAL, 2, 0);
        let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 5, &c);
        let mut shuffled = s.clone();
        let quota = player_pool_season_shuffle(&mut shuffled, &mut rng.clone());
        assert!(quota.iter().any(|&q| q != 0));
        assign_pool_players_to_teams(&mut s, &rt, &mut rng);
        for (p, player) in s.players.iter().enumerate().skip(1) {
            let team = shuffled.players[p].team;
            if team != 0xff {
                assert_eq!(player.team, team, "record {p}");
            }
        }
        assert!(s.players[1..].iter().all(|p| p.team != 0xff));
    }

    #[test]
    fn wage_budget_in_the_bundesliga() {
        let mut s = GameState::zeroed();
        s.managers[0].fans = 50;
        let mut rng = Rng::new(31);
        let mut p = rng;
        manager_compute_wage_budget(&mut s, &mut rng, 0);
        let r = i32::from(p.random_range(15, 20));
        let r2 = i32::from(p.random_range(120, 150));
        // d = 2: 2 * (r + 40) + (2 * 60 & 0xfffe) * 500 = 60 080 + 2r, times 120..150 %,
        // kept in 16 bits.
        let costs = 2 * (r + 40) + 60_000;
        assert_eq!(s.managers[0].extra_costs, (costs * r2 / 100) as u16);
        assert!(costs * r2 / 100 > 0xffff, "the costs wrap");
        assert_eq!(s.manager_contracts[0][7], 110_000);
        assert_eq!(rng, p);
    }

    #[test]
    fn wage_budget_below_the_leagues_wraps() {
        let mut s = GameState::zeroed();
        s.managers[0].league = 3;
        s.managers[0].fans = 0x70;
        let mut rng = Rng::new(8);
        let mut p = rng;
        manager_compute_wage_budget(&mut s, &mut rng, 0);
        let r = i32::from(p.random_range(15, 20));
        // d = 0xffff: a = -(r + 40); (0xffff * 0xfffe) & 0xfffe = 2, * 500 = 1000.
        // The second range is reversed: random_range(90, 75) gives 90..=103.
        let r2 = i32::from(p.random_range(90, 75));
        assert!((90..=103).contains(&r2));
        assert_eq!(i32::from(s.managers[0].extra_costs), (960 - r) * r2 / 100);
        assert_eq!(s.manager_contracts[0][7], (-30 + 0x70) * 1000);
    }

    #[test]
    fn wage_budget_division_truncates_toward_zero() {
        // Tier 200: d = -198, the factor is drawn from random_range(-1880, -4850).
        let mut checked = 0;
        for seed in 0..200 {
            let mut s = GameState::zeroed();
            s.managers[0].league = 200;
            s.managers[0].fans = 50;
            let mut rng = Rng::new(seed);
            let mut p = rng;
            manager_compute_wage_budget(&mut s, &mut rng, 0);
            let r = p.random_range(15, 20) as u16;
            let d = 2u16.wrapping_sub(200);
            let costs = d
                .wrapping_mul(r + 40)
                .wrapping_add((d.wrapping_mul(60) & 0xfffe).wrapping_mul(500));
            let r2 = i32::from(p.random_range(-1880, -4850));
            let product = i32::from(costs) * r2;
            if product < 0 && product % 100 != 0 {
                assert_eq!(s.managers[0].extra_costs, (product / 100) as u16);
                assert_ne!(product / 100, product.div_euclid(100));
                checked += 1;
            }
        }
        assert!(checked > 10);
    }

    #[test]
    fn one_year_manager_values() {
        let mana = synthetic_mana();
        let c = choices(&[30], MODE_ONE_YEAR | MODE_HISTORICAL, 2, 33);
        let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 17, &c);
        let mut bp4 = c.carousel_pos;
        manager_init_loop(&mut s, &rt, &mut rng, 0, &mut bp4, None);
        assert_eq!(bp4, 7);
        let mut contracts = [180_000, 45000, 45000, 45000, 45000, 45000, 45000, 0, 30000];
        contracts[7] = s.manager_contracts[0][7];
        assert_eq!(s.manager_contracts[0], contracts);
        let m = &s.managers[0];
        assert_eq!(m.values_132[3], 1);
        assert_eq!(
            (m.league, m.fans, m.value_10a, m.value_131),
            (0, 50, 18, 0x10)
        );
        assert_eq!(m.stadium.map(|x| x[0]), [11000, 30000, 7000, 3, 3, 5, 4]);
        assert_eq!((m.balance, m.records[2]), (1_500_000, 99999));
        assert_eq!(
            (m.training_141, m.training_146),
            (TRAINING_141, TRAINING_146)
        );
        // In the UEFA cup list of TestCups, a Bundesliga slot.
        assert!(m.team <= 5);
        // Base 3 / 2 + 0x1b + 0x29 + 0x0c = 81.
        let squad = &s.squads[0][..20];
        assert!(squad.iter().all(|r| (81..=86).contains(&r.skills[0])
            && (81..=86).contains(&r.skills[1])
            && (40..=60).contains(&r.skills[2])
            && r.market_value > 0));
        assert_eq!(s.players[7].skills, squad[19].skills);
        assert_ne!(s.players[33].skills, squad[19].skills);
    }

    #[test]
    fn three_year_manager_writes_the_carousel_record() {
        let mana = synthetic_mana();
        let c = choices(&[30, 12], MODE_THREE_YEARS | MODE_HISTORICAL, 1, 33);
        let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 17, &c);
        for (slot, table) in s.team_tables.iter_mut().enumerate() {
            table.prestige = slot as i32;
        }
        let mut bp4 = c.carousel_pos;
        manager_init_loop(&mut s, &rt, &mut rng, 0, &mut bp4, None);
        assert_eq!(bp4, 33);
        // Manager 0's club moved from the 2. Bundesliga; the league table records stayed at
        // their slots.
        assert_ne!(s.managers[0].team, 30);
        for (slot, table) in s.team_tables.iter().enumerate() {
            assert_eq!(table.prestige, slot as i32);
        }
        // Manager 1 ran last; its loop over squad 0 and squad 1 ends with its own record 19.
        assert_eq!(s.players[33].skills, s.squads[1][19].skills);
        let m = &s.managers[1];
        assert_eq!(m.balance, 1_900_000);
        assert_eq!(m.stadium.map(|x| x[0]), [6000, 20000, 7000, 2, 1, 4, 3]);
        assert_eq!(m.values_132[3], 0);
        assert!(s.managers[0].team < 18 && m.team < 18 && s.managers[0].team != m.team);
    }

    /// Both starts take the same manager setup per game length: the one-year contracts and
    /// carousel position, the amateur tier only in the endless game, and the three-year stadium.
    #[test]
    fn manager_setup_follows_the_game_length_in_both_starts() {
        let mana = synthetic_mana();
        for start in [MODE_HISTORICAL, 0] {
            for length in [MODE_ONE_YEAR, MODE_THREE_YEARS, MODE_ENDLESS] {
                let what = format!("length {length}, start {start}");
                let c = choices(&[40], length | start, 2, 33);
                let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 3, &c);
                let mut bp4 = c.carousel_pos;
                manager_init_loop(&mut s, &rt, &mut rng, 0, &mut bp4, None);
                let one_year = length == MODE_ONE_YEAR;
                let league = if length == MODE_ENDLESS { 2 } else { 0 };
                assert_eq!(s.managers[0].league, league, "{what}");
                assert_eq!(bp4, if one_year { 7 } else { 33 }, "{what}");
                assert_eq!(s.manager_contracts[0][0] == 180_000, one_year, "{what}");
                if length != MODE_ENDLESS {
                    let floodlight_and_scoreboard =
                        (s.managers[0].stadium[3][0], s.managers[0].stadium[4][0]);
                    let expected = if length == MODE_THREE_YEARS {
                        (2, 1)
                    } else {
                        (3, 3)
                    };
                    assert_eq!(floodlight_and_scoreboard, expected, "{what}");
                }
            }
        }
    }

    #[test]
    fn endless_manager_keeps_an_amateur_club() {
        let mana = synthetic_mana();
        let c = choices(&[40], MODE_ENDLESS | MODE_HISTORICAL, 2, 0);
        let (mut s, rt, mut rng) = up_to_manager_loop(&mana, 3, &c);
        let club = s.clubs[40].clone();
        let mut p = rng;
        let mut bp4 = 0;
        manager_init_loop(&mut s, &rt, &mut rng, 0, &mut bp4, None);
        let m = &s.managers[0];
        assert_eq!((m.team, m.league, m.fans, m.value_10a), (40, 2, 10, 10));
        assert_eq!(s.clubs[40], club);
        // No slot draw: the first draws are the squad attributes with base 0x1c.
        let v = p.random_range(0x1c, 0x21) as u8;
        assert_eq!(s.squads[0][0].skills[0], v);
    }

    #[test]
    fn added_manager_gets_twenty_free_players() {
        let mana = synthetic_mana();
        let (mut s, _) = full_game(
            &mana,
            21,
            &choices(&[30], MODE_THREE_YEARS | MODE_HISTORICAL, 2, 0),
            &mut NoCups,
        );
        let before = s.clone();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(4);
        let rules = LineupRules {
            suspended_allowed: false,
            max_lineup_slot: 15,
        };
        let new = ManagerChoice {
            name: b"NEW".to_vec(),
            team: 50,
            slot: 2,
        };
        add_managers(&mut s, &mut rt, &mut rng, &[new], rules);
        assert_eq!(s.manager_count, 2);
        assert_eq!(s.squad_player_count(1), 20);
        assert_eq!(
            s.players[1..]
                .iter()
                .filter(|p| p.owner_manager == 1)
                .count(),
            20
        );
        assert_eq!(s.squads[0], before.squads[0]);
        assert_eq!(s.managers[0], before.managers[0]);
        let m = &s.managers[1];
        assert_eq!(m.name(), b"NEW");
        assert_eq!(m.position_history, [10; 38]);
        assert!(m.team < 18 && m.team != s.managers[0].team);
        let starters = s.squads[1]
            .iter()
            .filter(|r| (1..=11).contains(&r.lineup_position))
            .count();
        assert_eq!(starters, 11);
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.current_manager, before.current_manager);
        // `[bp-4]` = 5: the last squad record's attributes also land in pool record 5.
        assert_eq!(s.players[5].skills, s.squads[1][19].skills);
        // The club strength already matches the new lineup.
        let mut again = s.clone();
        squad_recalc_team_strength(&mut again, &rt, &mut rng, 1, false);
        assert_eq!(again, s);
    }

    #[test]
    fn an_added_manager_never_gets_the_record_past_the_pool() {
        // All forwards (records 0x6a..=0x96) are taken: the 1000-tries fallback must not end
        // on record 151, which lies past the pool (DEVIATION, flow.md §4.7b).
        let mana = synthetic_mana();
        let (mut s, _) = full_game(
            &mana,
            21,
            &choices(&[30], MODE_THREE_YEARS | MODE_HISTORICAL, 2, 0),
            &mut NoCups,
        );
        for p in &mut s.players[0x6a..] {
            if p.owner_manager == NO_OWNER {
                p.owner_manager = 4;
            }
        }
        let rules = LineupRules {
            suspended_allowed: false,
            max_lineup_slot: 15,
        };
        let new = ManagerChoice {
            name: b"NEW".to_vec(),
            team: 50,
            slot: 2,
        };
        for seed in 0..20 {
            let mut t = s.clone();
            let mut rng = Rng::new(seed);
            add_managers(
                &mut t,
                &mut Runtime::default(),
                &mut rng,
                std::slice::from_ref(&new),
                rules,
            );
            assert_eq!(t.manager_count, 2);
        }
    }

    /// Records the game state and the generator at each [`CupInit`] hook.
    #[derive(Default)]
    struct SpyCups {
        seen: Vec<(GameState, Rng)>,
    }

    impl CupInit for SpyCups {
        fn before_start_menu(&mut self, state: &mut GameState, rt: &mut Runtime, rng: &mut Rng) {
            TestCups.before_start_menu(state, rt, rng);
            self.seen.push((state.clone(), *rng));
        }
        fn after_pool_assignment(&mut self, state: &mut GameState, _: &mut Runtime, rng: &mut Rng) {
            self.seen.push((state.clone(), *rng));
        }
    }

    /// `new_game` runs its steps in `main`'s order, with the cup hooks at their places in the
    /// random stream: replayed step by step, the state and the generator match at each hook
    /// and at the end.
    #[test]
    fn new_game_runs_mains_sequence() {
        let mana = synthetic_mana();
        let (seed, time2) = (77, 1234);
        let mut c = choices(&[3, 20], MODE_THREE_YEARS, 2, 9);
        c.managers[0].slot = 2;
        c.managers[1].slot = 1;
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let mut spy = SpyCups::default();
        let mut s = startup(&mana, &mut rt, &mut rng, seed, &mut spy);
        assert_eq!((&spy.seen[0].0, spy.seen[0].1), (&s, rng));
        // The DGROUP image already holds round 1; cleared, `new_game` must set it.
        s.competition_rounds[0] = 0;
        new_game(&mut s, &mut rt, &mut rng, &mana, &c, time2, &mut spy);
        assert_eq!(spy.seen.len(), 2);

        let (mut want, want_rt, mut want_rng) = up_to_manager_loop(&mana, seed, &c);
        let mut bp4 = c.carousel_pos;
        manager_init_loop(&mut want, &want_rt, &mut want_rng, 0, &mut bp4, None);
        want.current_manager = spy.seen[0].0.current_manager;
        interest_rates_update(&mut want, &mut want_rng);
        league_strength_drift(&mut want, &want_rt, &mut want_rng, 10);
        want_rng.seed_from_time(time2);
        want.competition_rounds[0] = 1;
        assign_pool_players_to_teams(&mut want, &want_rt, &mut want_rng);
        assert!(spy.seen[1].0 == want, "state at after_pool_assignment");
        assert_eq!(spy.seen[1].1, want_rng);
        want.day_counter = i32::from(SEASON_START_DAY);
        transfer_market_refresh(&mut want, &mut want_rng);
        want.messages = vec![Vec::new(); 2];
        assert!(s == want, "state at the end");
        assert_eq!(rng, want_rng);
        assert_eq!(rt, want_rt);

        assert_eq!(s.competition_rounds[0], 1);
        assert_eq!(s.start_year, MODERN_START_YEAR);
        assert_eq!((s.managers[0].slot, s.managers[1].slot), (2, 1));
    }

    #[test]
    fn mod_games_start_in_2026() {
        let mana = synthetic_mana();
        for (mode, mod_end) in [
            (MODE_ONE_YEAR, 2027),
            (MODE_THREE_YEARS, 2029),
            (MODE_ENDLESS, 0),
        ] {
            for start in [0, MODE_HISTORICAL] {
                for mod2026 in [false, true] {
                    let c = choices(&[4], mode | start, 2, 0);
                    let mut rt = Runtime {
                        mod2026,
                        ..Runtime::default()
                    };
                    let mut rng = Rng::default();
                    let mut s = startup(&mana, &mut rt, &mut rng, 5, &mut TestCups);
                    new_game(&mut s, &mut rt, &mut rng, &mana, &c, 12, &mut TestCups);
                    let what = format!("mode {mode}, start {start}, mod {mod2026}");
                    let code = c.options.end_year();
                    assert_eq!(s.scenario, code, "{what}");
                    let want_start = match (mod2026, start) {
                        (true, _) => 2026,
                        (false, 0) => MODERN_START_YEAR,
                        (false, _) => HISTORICAL_START_YEAR,
                    };
                    assert_eq!(s.start_year, want_start, "{what}");
                    let want_end = match (mod2026, mod_end) {
                        (_, 0) => None,
                        (true, _) => Some(mod_end),
                        (false, _) => Some(code),
                    };
                    assert_eq!(game_end_year(&s, mod2026), want_end, "{what}");
                }
            }
        }
    }

    #[test]
    fn game_end_year_off_mod_is_the_code() {
        let mut s = GameState::program_start();
        for code in [0x7ac, 0x7ae, 0x7c9, 0x7cb] {
            s.scenario = code;
            assert_eq!(game_end_year(&s, false), Some(code));
        }
        for code in [0x56ce, 0x56eb] {
            s.scenario = code;
            assert_eq!(game_end_year(&s, false), None);
            assert_eq!(game_end_year(&s, true), None);
        }
        (s.start_year, s.logged_rounds, s.scenario) = (2028, 2, 0x7c9);
        assert_eq!(game_end_year(&s, true), Some(2027));
        (s.start_year, s.logged_rounds, s.scenario) = (2027, 1, 0x7ae);
        assert_eq!(game_end_year(&s, true), Some(2029));
        // An original end-year code keeps its year (the loader refuses an original save
        // on mod data, #218).
        (s.start_year, s.logged_rounds, s.scenario) = (1994, 2, 0x7cb);
        assert_eq!(game_end_year(&s, true), Some(0x7cb));
        (s.start_year, s.logged_rounds, s.scenario) = (1965, 2, 0x7ac);
        assert_eq!(game_end_year(&s, true), Some(0x7ac));
    }

    #[test]
    fn cup_draws_finish_every_game() {
        let mana = synthetic_mana();
        for mode in [MODE_ENDLESS, MODE_ONE_YEAR, MODE_THREE_YEARS] {
            for start in [0, MODE_HISTORICAL] {
                for n in 1..=4 {
                    let c = choices(&[3, 20, 45, 60][..n], mode | start, 2, 9);
                    let what = format!("mode {mode} start {start} {n} managers");
                    let (s, rt) = full_game(&mana, 35372, &c, &mut CupDraws(|_| false));
                    assert_consistent(&s, &rt, &what);
                }
            }
        }
    }

    /// The hooks of [`CupDraws`] are `main`'s cup calls: replayed by hand on clones, the state
    /// and the generator match after each hook.
    #[test]
    fn cup_draws_run_mains_cup_sequence() {
        let mana = synthetic_mana();
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let mut s = startup(&mana, &mut rt, &mut rng, 77, &mut NoCups);
        let (mut want, mut want_rt, mut want_rng) = (s.clone(), rt.clone(), rng);
        let mut asked = Vec::new();
        let mut draws = CupDraws(|c| {
            asked.push(c);
            false
        });
        draws.before_start_menu(&mut s, &mut rt, &mut rng);
        euro_cup_qualifiers_build(&mut want, &mut want_rt);
        cup_clear_match_goals(&mut want);
        for cup in 1..=3 {
            cup_draw_first_round(&mut want, &want_rt, &mut want_rng, cup, false, &mut |_| {
                false
            });
        }
        assert!(s == want, "state after before_start_menu");
        assert_eq!((rng, &rt), (want_rng, &want_rt));
        assert_ne!(rng, Rng::default());

        draws.after_pool_assignment(&mut s, &mut rt, &mut rng);
        cup_draw_first_round(&mut want, &want_rt, &mut want_rng, 0, false, &mut |_| false);
        for cup in 1..=3 {
            cup_draw_first_round(&mut want, &want_rt, &mut want_rng, cup, true, &mut |_| {
                false
            });
        }
        assert!(s == want, "state after after_pool_assignment");
        assert_eq!((rng, &rt), (want_rng, &want_rt));
        // Only the DFB-Pokal draw and the redisplayed European draws ask.
        assert_eq!(asked, [0, 1, 2, 3]);
    }

    /// The owner's acceptance check of a finished new game.
    fn assert_consistent(s: &GameState, rt: &Runtime, what: &str) {
        let count = usize::from(s.manager_count);
        for m in 0..count {
            let mgr = &s.managers[m];
            let tier = usize::from(mgr.league);
            let first = layout().first_team[tier];
            assert!(
                (first..first + rt.tier_team_count[tier] as u8).contains(&mgr.team),
                "{what}: manager {m}"
            );
            assert!(
                s.managers[..m].iter().all(|x| x.team != mgr.team),
                "{what}: manager {m}"
            );
            assert_eq!(s.squad_player_count(m), 20, "{what}: manager {m}");
            let owned = s.players[1..]
                .iter()
                .filter(|p| p.owner_manager == m as u8)
                .count();
            assert_eq!(owned, 20, "{what}: manager {m}");
            for rec in &s.squads[m][..20] {
                let p = &s.players[usize::from(rec.player_id)];
                assert_eq!(
                    (p.owner_manager, p.team),
                    (m as u8, mgr.team),
                    "{what}: manager {m}"
                );
            }
        }
        for p in &s.players[1..] {
            if usize::from(p.owner_manager) >= count {
                assert!(
                    p.owner_manager == NO_OWNER || p.owner_manager == CLUB_LISTED,
                    "{what}"
                );
            }
        }
        for rec in &s.squads[TRANSFER_LIST][..s.squad_player_count(TRANSFER_LIST) as usize] {
            assert_eq!(
                s.players[usize::from(rec.player_id)].owner_manager,
                CLUB_LISTED,
                "{what}"
            );
        }
        for (t, &first) in layout().first_team.iter().enumerate() {
            let n = rt.tier_team_count[t] as usize;
            let mut order = s.league_teams[t][..n].to_vec();
            order.sort_unstable();
            assert!(
                order.into_iter().eq(first..first + n as u8),
                "{what}: tier {t}"
            );
        }
        let bytes = s.to_savegame().to_bytes().unwrap();
        let back = GameState::from_savegame(&SaveGame::parse(&bytes).unwrap()).unwrap();
        assert_eq!(back.to_savegame(), s.to_savegame(), "{what}");
    }

    #[test]
    fn new_games_are_consistent() {
        let mana = synthetic_mana();
        for seed in [1, 35372, 0xbeef] {
            for mode in [MODE_ENDLESS, MODE_ONE_YEAR, MODE_THREE_YEARS] {
                for start in [0, MODE_HISTORICAL] {
                    for teams in [&[7][..], &[3, 20, 45, 60]] {
                        let c = choices(teams, mode | start, 2, 9);
                        let what = format!("seed {seed} mode {mode} start {start} {teams:?}");
                        let mut rt = Runtime::default();
                        let mut rng = Rng::default();
                        let mut s = startup(&mana, &mut rt, &mut rng, seed, &mut TestCups);
                        new_game(
                            &mut s,
                            &mut rt,
                            &mut rng,
                            &mana,
                            &c,
                            seed + 60,
                            &mut TestCups,
                        );
                        assert_eq!(rt.modern_start, start == 0, "{what}");
                        assert_eq!(s.day_counter, 0);
                        assert_eq!(s.current_manager, s.saved_manager, "{what}");
                        assert_consistent(&s, &rt, &what);
                    }
                }
            }
        }
    }

    /// Seed 35372 was recovered from a DOSBox run of the original: its carousel showed exactly
    /// these clubs, and no other 16-bit seed produces them. The MANA.DAT draws come before the
    /// seed, so they do not change the carousel.
    #[test]
    fn league_shuffle_matches_original_carousel() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let mana = ManaDat::parse(&std::fs::read(dir.join("MANA.DAT")).unwrap()).unwrap();
        let mut rt = Runtime::default();
        let s = startup(&mana, &mut rt, &mut Rng::default(), 35372, &mut NoCups);
        let name = |i: usize| c_str(&s.clubs[i].name).to_vec();
        assert_eq!(name(0), b"DYNAMO DRESDEN");
        assert_eq!(name(2), b"BORUSSIA DORTMUND");
        assert_eq!(name(3), b"HAMBURGER SV");
        assert_eq!(name(4), b"VFL BOCHUM");
        assert_eq!(name(5), b"WATTENSCHEID 09");
    }

    /// No `.MAN` save holds a game that has just started, so the real data is checked for
    /// consistency and against the real save format only.
    #[test]
    fn new_game_from_the_real_mana_dat() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let mana = ManaDat::parse(&std::fs::read(dir.join("MANA.DAT")).unwrap()).unwrap();
        let c = choices(&[2, 5], MODE_THREE_YEARS | MODE_HISTORICAL, 2, 3);
        let mut rt = Runtime::default();
        let mut rng = Rng::default();
        let mut draws = CupDraws(|_| false);
        let mut s = startup(&mana, &mut rt, &mut rng, 35372, &mut draws);
        new_game(&mut s, &mut rt, &mut rng, &mana, &c, 35400, &mut draws);
        assert_consistent(&s, &rt, "MANA.DAT");
        for p in &s.players[1..] {
            assert!(!p.name().is_empty());
        }
    }
}
