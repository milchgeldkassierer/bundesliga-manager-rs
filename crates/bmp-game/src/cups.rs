//! Cup competitions of `BMMAIN.EXE` (`docs/re/logic/cups.md`): the DFB-Pokal (cup row 0),
//! the three European cups (rows 1..3), the relegation play-off and the promotion,
//! relegation and league shuffle of the season rollover.
//!
//! * Data model and helpers (§0, §2): [`team_is_euro_qualified`], [`cup_clear_match_goals`],
//!   [`cup_amateur_home_swap`], [`cup_tie_winner`]; `league_tier_of_team` 27c2:1bb5 is
//!   [`Runtime::league_of_club`].
//! * First-round draw (§3): [`cup_draw_first_round`], [`euro_cup_qualifiers_build`], and the
//!   decision part of the draw screen, [`cup_draw_screen_open`].
//! * Match-day processing (§4): [`cup_check_ties_extra_time`], [`cup_round_complete`],
//!   [`cup_draw_next_round`].
//! * Relegation play-off (§8): [`matchday_relegation_match`].
//! * Season rollover (§9.1, §9.2): [`season_promotion_relegation`], [`league_shuffle_teams`];
//!   the fixtures (§9.3) are [`crate::league::fixtures_build_round_pairings`].
//!
//! The match engine reaches §4 through [`MatchHooks`]; its implementer forwards the two cup
//! methods here:
//!
//! ```ignore
//! fn cup_check_ties_extra_time(&mut self, state, rt, rng, cup_mask, count, shootout) -> bool {
//!     cups::cup_check_ties_extra_time(state, rt, rng, self, cup_mask, count, shootout)
//! }
//! fn cup_round_complete(&mut self, state, rt, rng, euro) {
//!     cups::cup_round_complete(state, rt, rng, euro, &mut self.draw_screens)
//! }
//! ```
//!
//! Whether a draw is shown changes the logic (the amateur home rule of
//! [`cup_draw_next_round`]), and for most rounds the original asks the players first. The
//! draws therefore take a [`CupDrawUi`]: its [`CupDrawUi::ask`] receives the cup row and
//! answers the dialog "MÖCHTEN SIE DEM UNÜBERTROFFENEM SCHAUSPIEL ... AUSLOSUNG BEIWOHNEN ?"
//! (false under automatic play, DS:05d4); a plain `FnMut(u8) -> bool` is such a hook. Its other
//! methods are the screens (§5, §7), called where the original calls them and no-ops by
//! default; they draw no random numbers. The copy-protection check of the draw screen is
//! taken as passed, so it draws no random number either (cups.md §1).
//!
//! Call order at a season change (`season_main_loop`, #31): play-off, then
//! [`season_promotion_relegation`], [`league_shuffle_teams`], [`euro_cup_qualifiers_build`];
//! at the season start [`cup_clear_match_goals`], [`cup_draw_first_round`] `(c, false)` for
//! c = 0..=3, then `(c, true)` for c = 1..=3. A new game (#30) runs the shuffle, the
//! qualifiers, the goal clearing and the draws `(c, false)` for c = 1..=3, and later
//! `(0, false)` and `(c, true)` for c = 1..=3 (§9.4).

use crate::matchsim::{CUP_ROW_CELLS, MatchHooks, cup_row, match_penalty_shootout, matchday_play};
use crate::rng::Rng;
use crate::runtime::{Runtime, TIER_FIRST};
use crate::state::{CLUB_COUNT, GERMAN_CLUBS, GameState, MANAGER_SLOTS};

/// DS:07d6: clubs left after each cup round, i.e. the pairings played in round `idx >= 1`.
pub const CUP_ROUND_PAIRINGS: [u8; 6] = [32, 16, 8, 4, 2, 1];
/// [`crate::state::Manager::values_132`] stage of a manager whose club is not in the cup.
pub const NOT_IN_CUP: u8 = 0x1e;
/// [`cup_tie_winner`] result of a tie still level after both legs.
pub const TIE_LEVEL: u8 = 0x1e;
/// Free entry of [`GameState::competition_slots`].
pub const NO_QUALIFIER: u8 = 0x80;
/// First foreign club id; lower ids are German.
pub const FIRST_FOREIGN_TEAM: u8 = GERMAN_CLUBS as u8;
/// DS:2277: highest club id of the DFB-Pokal draw.
const DFB_LAST_TEAM: u8 = 57;
/// Highest foreign club id of the European draws (0xc7).
const LAST_FOREIGN_TEAM: u8 = (CLUB_COUNT - 1) as u8;
/// DS:226d: UEFA-Cup places of the Bundesliga table.
const UEFA_PLACES: u8 = 4;
/// DS:2266: relegation places per league tier (Bundesliga: two direct plus the play-off).
const RELEGATION_PLACES: [i8; 3] = [3, 4, 4];
/// DS:226a: the league tier has a relegation play-off.
const HAS_RELEGATION_PLAYOFF: [u8; 3] = [1, 0, 0];
/// Last day of the season on which a level European tie goes by away goals.
const PLAYOFF_DAY: i32 = 315;
/// Index of DS:0608 ("show the DFB-Pokal draw") in [`GameState::options`]; DS:0609 (European
/// draws) follows.
pub(crate) const SHOW_DRAW_OPTION: usize = 10;

/// Pairings played in cup round `round` (DS:07d6).
pub(crate) fn round_pairings(round: u8) -> u8 {
    *CUP_ROUND_PAIRINGS
        .get(usize::from(round))
        .unwrap_or_else(|| panic!("cup round {round} outside DS:07d6"))
}

/// Slots played in cup round `round`: two per pairing, at most the [`CUP_ROW_CELLS`] of one row.
// DEVIATION: round 0, which the game only holds at program start and inside
// `cup_draw_first_round`, gives 64 slots in the original, so a corrupt save with it on a cup
// day plays and completes ties past the 32-slot rows of the cup tables (`state.md` §1.1).
pub(crate) fn round_slots(round: u8) -> u8 {
    (round_pairings(round) << 1).min(CUP_ROW_CELLS as u8)
}

/// The 32 draw slots of cup row `cup`.
fn slots(state: &GameState, cup: u8) -> &[u8] {
    &state.cup_pairings.as_flattened()[cup_row(cup)]
}

fn slots_mut(state: &mut GameState, cup: u8) -> &mut [u8] {
    &mut state.cup_pairings.as_flattened_mut()[cup_row(cup)]
}

/// The 32 match goals of cup row `cup`.
fn goals(state: &GameState, cup: u8) -> &[u8] {
    &state.cup_results.as_flattened()[cup_row(cup)]
}

fn goals_mut(state: &mut GameState, cup: u8) -> &mut [u8] {
    &mut state.cup_results.as_flattened_mut()[cup_row(cup)]
}

/// Range of European cup `cup` in [`GameState::cup_first_leg_goals`]; row 0 (FAR:21c2) is not
/// saved and never used.
fn first_leg_range(cup: u8) -> std::ops::Range<usize> {
    assert!(cup >= 1, "the DFB-Pokal has no first legs");
    cup_row(cup - 1)
}

/// Whether `team` is among the first [`Runtime::euro_qualified_count`] entries of
/// [`GameState::euro_qualified_list`].
///
/// Origin: BMMAIN.EXE 27c2:11e7 (team_is_euro_qualified)
pub fn team_is_euro_qualified(state: &GameState, rt: &Runtime, team: u8) -> bool {
    state
        .euro_qualified_list
        .iter()
        .take(usize::from(rt.euro_qualified_count))
        .any(|&t| t == team)
}

/// Clears the match goals of all four cup rows.
///
/// Origin: BMMAIN.EXE 27c2:1b6d (cup_clear_match_goals)
pub fn cup_clear_match_goals(state: &mut GameState) {
    state.cup_results = [[0; 2]; 64];
}

/// DFB-Pokal home rule: an amateur club `team_s` drawn into the odd (away) slot `s` against
/// the professional club `team_prev` of slot `s - 1` swaps to home, until the semi-final
/// round counter is reached. The checks run in the original's order, so `team_prev` of
/// slot 0 never matters.
///
/// Origin: BMMAIN.EXE 27c2:1187 (cup_amateur_home_swap)
pub fn cup_amateur_home_swap(
    state: &mut GameState,
    rt: &Runtime,
    team_s: u8,
    team_prev: u8,
    s: u8,
) {
    if rt.league_of_club(team_s) != 4
        || rt.league_of_club(team_prev) >= 4
        || state.event_state[0] >= 4
        || s & 1 == 0
    {
        return;
    }
    slots_mut(state, 0).swap(usize::from(s) - 1, usize::from(s));
}

/// [`cup_amateur_home_swap`] over the first `n` DFB-Pokal slots, reading each slot as the
/// previous swap left it.
fn amateur_home_swaps(state: &mut GameState, rt: &Runtime, n: u8) {
    for s in 0..n {
        let row = slots(state, 0);
        let i = usize::from(s);
        // Slot 0 reads FAR:76c9, the byte before the table; the rule ignores it.
        let prev = if i == 0 { 0 } else { row[i - 1] };
        cup_amateur_home_swap(state, rt, row[i], prev, s);
    }
}

/// Decides a two-legged tie of cup row `cup >= 1` after the return leg; `slot` is even and
/// its club played the first leg away. Returns 1 when the club of `slot + 1` goes through,
/// 0 when the club of `slot` does, [`TIE_LEVEL`] when the tie is level. Aggregate goals
/// decide, then away goals, except after day 315 (the relegation play-off), where any level
/// aggregate is [`TIE_LEVEL`]. The return-leg goals of `slot` lose their extra-time and
/// penalty markers; the sums are 16-bit, as in the original.
///
/// Origin: BMMAIN.EXE 27c2:15e8 (cup_tie_winner)
pub fn cup_tie_winner(state: &GameState, cup: u8, slot: u8) -> u8 {
    let s = usize::from(slot);
    let goals = goals(state, cup);
    let first = &state.cup_first_leg_goals[first_leg_range(cup)];
    let mut e = goals[s];
    while e > 9 {
        e -= 10;
    }
    let e = u16::from(e);
    let a = u16::from(first[s]);
    let b = u16::from(goals[s + 1]);
    let d = u16::from(first[s + 1]);
    if state.day_counter > PLAYOFF_DAY && b.wrapping_add(d).wrapping_sub(a) == e {
        return TIE_LEVEL;
    }
    if (d + b) as i16 > (a + e) as i16 {
        return 1;
    }
    let level = b.wrapping_add(d).wrapping_sub(e) == a;
    if level && a < b {
        return 1;
    }
    if level && a == b {
        return TIE_LEVEL;
    }
    0
}

/// The screens of the cup draws (cups.md §5) and of a won cup final (§7). The logic calls them
/// where the original does; none of them may touch the random generator.
pub trait CupDrawUi {
    /// The office background with the cup name and the question "An Alle: MÖCHTEN SIE DEM
    /// UNÜBERTROFFENEM SCHAUSPIEL EINER DFB-POKAL-/EUROPA-POKAL- AUSLOSUNG BEIWOHNEN ?" of cup
    /// row `cup`'s draw. On "yes" the draw board is shown; on "no" (and under automatic play)
    /// the screen is cleared (`cup_draw_screen_close` with FAR:57c8 = 0).
    ///
    /// Origin: BMMAIN.EXE 27c2:0006 (cup_draw_screen_open), the screen
    fn ask(&mut self, cup: u8) -> bool;

    /// The club `team` drawn into slot `slot` of a shown draw (`state.event_state[cup]` is the
    /// round being drawn).
    ///
    /// Origin: BMMAIN.EXE 27c2:0378 (cup_draw_animate_slot)
    fn animate_slot(&mut self, _state: &GameState, _cup: u8, _slot: u8, _team: u8) {}

    /// The end of a shown draw: "TASTE DRÜCKEN" when the board was shown, then the new
    /// pairings (`screen_cup_matches`, which overwrites the date globals).
    ///
    /// Origin: BMMAIN.EXE 27c2:089a (cup_draw_screen_close)
    fn close(&mut self, _state: &mut GameState, _cup: u8) {}

    /// Manager `m`'s club won the final of competition `comp` (1 = DFB-Pokal, 2..4 = European
    /// cups 1..3).
    ///
    /// Origin: BMMAIN.EXE 27c2:274d (screen_title_winner)
    fn title_winner(&mut self, _state: &GameState, _m: u8, _comp: u8) {}
}

impl<F: FnMut(u8) -> bool> CupDrawUi for F {
    fn ask(&mut self, cup: u8) -> bool {
        self(cup)
    }
}

/// Whether the draw of cup row `cup` is shown (FAR:57c8): never when the settings switch it
/// off (DS:0608 for the DFB-Pokal, DS:0609 for the European cups), always for the DFB-Pokal
/// final draw, otherwise as the players answer `ask(cup)`.
///
/// Origin: BMMAIN.EXE 27c2:0006 (cup_draw_screen_open), without the screen
pub fn cup_draw_screen_open(state: &GameState, cup: u8, ui: &mut dyn CupDrawUi) -> bool {
    if state.options[SHOW_DRAW_OPTION + usize::from(cup != 0)] == 0 {
        return false;
    }
    let teams = round_pairings(state.event_state[usize::from(cup)]);
    if cup == 0 && teams < 4 {
        return true;
    }
    ui.ask(cup)
}

/// Draws the first round of cup row `cup`, or with `redisplay` only shows an existing
/// European draw (the screen opens, nothing is drawn).
///
/// DFB-Pokal: every human manager's club gets a distinct random slot `random_range(0, 31)`,
/// every other slot a club `random_range(0, 57)` re-drawn until it is in no earlier slot and
/// no manager's club; then the amateur home rule. European cups: every slot gets a foreign
/// club `random_range(64, 199)` re-drawn until it is in no earlier slot of this cup, no slot
/// of the other European cups (which may still hold last season's draw) and no manager's
/// club; then each German qualifier replaces the foreign club of slot `random_range(1, 31)`,
/// re-drawn while that slot holds a German club. Managers whose club takes part get stage 1.
///
/// Origin: BMMAIN.EXE 27c2:09e0 (cup_draw_first_round)
pub fn cup_draw_first_round(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    cup: u8,
    redisplay: bool,
    ui: &mut dyn CupDrawUi,
) {
    let c = usize::from(cup);
    let mut shown = false;
    state.event_state[c] = 0;
    if cup == 0 || redisplay {
        shown = cup_draw_screen_open(state, cup, ui);
    }
    if cup == 0 || !redisplay {
        let euro = cup != 0;
        let (lo, hi) = if euro {
            (FIRST_FOREIGN_TEAM, LAST_FOREIGN_TEAM)
        } else {
            (0, DFB_LAST_TEAM)
        };
        let compared = if euro { 1..4 } else { 0..1 };
        if cup == 1 {
            state.cup_first_leg_goals = [0; 96];
        }
        let managers = state.active_managers().len();
        for m in &mut state.managers[..managers] {
            m.values_132[c] = NOT_IN_CUP;
        }
        let mut human_slot = [0u8; MANAGER_SLOTS];
        if cup == 0 {
            for m in 0..managers {
                loop {
                    human_slot[m] = rng.random_range(0, 31) as u8;
                    if !human_slot[..m].contains(&human_slot[m]) {
                        break;
                    }
                }
                let manager = &mut state.managers[m];
                manager.values_132[0] = 1;
                let team = manager.team;
                slots_mut(state, 0)[usize::from(human_slot[m])] = team;
            }
        }
        for i in 0..32u8 {
            // A human manager's slot keeps its club but is animated like the others.
            let human = cup == 0 && human_slot[..managers].contains(&i);
            if !human {
                loop {
                    let t = rng.random_range(lo.into(), hi.into()) as u8;
                    slots_mut(state, cup)[usize::from(i)] = t;
                    let drawn = compared.clone().any(|j| {
                        let lim = if j == cup { usize::from(i) } else { 32 };
                        slots(state, j)[..lim].contains(&t)
                    });
                    let managed = state.active_managers().iter().any(|m| m.team == t);
                    if !drawn && !managed {
                        break;
                    }
                }
            }
            if cup == 0 && shown {
                let team = slots(state, 0)[usize::from(i)];
                ui.animate_slot(state, 0, i, team);
            }
        }
        if cup == 0 {
            amateur_home_swaps(state, rt, 32);
        } else {
            state.euro_second_leg_pending[c - 1] = 0;
            // The original runs on into the next row until it meets 0x80; a row is never full.
            let qualifiers: Vec<u8> = state.competition_slots[(c - 1) * 6..]
                .iter()
                .copied()
                .take_while(|&t| t != NO_QUALIFIER)
                .collect();
            for team in qualifiers {
                let s = loop {
                    let s = rng.random_range(1, 31) as usize;
                    if slots(state, cup)[s] >= FIRST_FOREIGN_TEAM {
                        break s;
                    }
                };
                slots_mut(state, cup)[s] = team;
                for m in &mut state.managers[..managers] {
                    if m.team == team {
                        m.values_132[c] = 1;
                    }
                }
            }
        }
    }
    if cup != 0 && shown {
        for i in 0..32u8 {
            let team = slots(state, cup)[usize::from(i)];
            ui.animate_slot(state, cup, i, team);
        }
    }
    state.event_state[c] = 1;
    if shown {
        ui.close(state, cup);
    }
}

/// Builds the German qualifiers of the European cups from the final tables
/// ([`GameState::league_teams`]) and the title holders: Champions' Cup the champion and the
/// German holder (or, if already qualified, the runner-up); Cup Winners' Cup the DFB-Pokal
/// winner (else its runner-up) and the German holder (else the DFB-Pokal runner-up); UEFA
/// Cup the German holder and the next four unqualified Bundesliga clubs from rank 2 (one
/// more when the holder already qualified). Rows are padded with [`NO_QUALIFIER`]; every
/// qualifier is also appended to [`GameState::euro_qualified_list`], whose stale tail stays.
///
/// Like the original, a holder byte of 0 for the DFB-Pokal winner becomes club 0xff, the
/// second Champions' Cup entry and the DFB-Pokal runner-up as second Cup Winners' Cup entry
/// do not advance their row, and the UEFA places may run on into the 2. Bundesliga table.
///
/// Origin: BMMAIN.EXE 27c2:0ef2 (euro_cup_qualifiers_build)
pub fn euro_cup_qualifiers_build(state: &mut GameState, rt: &mut Runtime) {
    fn qualify(state: &mut GameState, rt: &mut Runtime, cell: usize, team: u8) {
        state.competition_slots[cell] = team;
        state.euro_qualified_list[usize::from(rt.euro_qualified_count)] = team;
        rt.euro_qualified_count += 1;
    }
    const CHAMPIONS: usize = 0;
    const CUP_WINNERS: usize = 6;
    const UEFA: usize = 12;
    let table: [u8; 60] = state.league_teams.as_flattened().try_into().unwrap();
    let mut uefa = UEFA_PLACES;
    rt.euro_qualified_count = 0;
    state.competition_slots = [NO_QUALIFIER; 18];

    qualify(state, rt, CHAMPIONS, table[0]);
    if state.winner_07ad != 0 {
        let holder = state.winner_07ad - 1;
        let team = if team_is_euro_qualified(state, rt, holder) {
            table[1]
        } else {
            holder
        };
        qualify(state, rt, CHAMPIONS + 1, team);
    }

    let mut n1 = 0;
    let dfb_winner = state.winner_07ac.wrapping_sub(1);
    let dfb_runner_up = state.dfb_cup_runner_up.wrapping_sub(1);
    if !team_is_euro_qualified(state, rt, dfb_winner) {
        qualify(state, rt, CUP_WINNERS, dfb_winner);
        n1 += 1;
    } else if !team_is_euro_qualified(state, rt, dfb_runner_up) {
        qualify(state, rt, CUP_WINNERS, dfb_runner_up);
        n1 += 1;
    }
    if state.winner_07ae != 0 {
        let holder = state.winner_07ae - 1;
        if !team_is_euro_qualified(state, rt, holder) {
            qualify(state, rt, CUP_WINNERS + n1, holder);
        } else if !team_is_euro_qualified(state, rt, dfb_runner_up) {
            qualify(state, rt, CUP_WINNERS + n1, dfb_runner_up);
        }
    }

    let mut n2 = 0;
    if state.winner_07af != 0 {
        let holder = state.winner_07af - 1;
        if !team_is_euro_qualified(state, rt, holder) {
            qualify(state, rt, UEFA, holder);
            n2 += 1;
        } else {
            uefa += 1;
        }
    }
    let mut src = 1;
    // DEVIATION: the original runs on past the 60 table entries while places are left, which
    // only a corrupt save with duplicate `league_teams` ids reaches; the port stops at the
    // table's end and leaves those places free (`state.md` §1.1).
    while uefa != 0 && src < table.len() {
        let team = table[src];
        if !team_is_euro_qualified(state, rt, team) {
            qualify(state, rt, UEFA + n2, team);
            n2 += 1;
            uefa -= 1;
        }
        src += 1;
    }
}

/// Marks the ties of the cups in `mask` (1 = DFB-Pokal, 2 = relegation, 0x0e = European
/// cups) that are level after a match (`shootout == false`, after 90 minutes) or still level
/// after extra time (`shootout == true`, which then plays the penalties): +10 on the home
/// goals of each such tie. `count` is the number of slots played. A level DFB-Pokal tie of a
/// human manager's home club sets that manager's [`Runtime::manager_plays_now`]; any human
/// tie sets [`Runtime::live_match_shown`]. European ties count only after the return leg
/// and when [`cup_tie_winner`] finds them level. Returns whether a tie was level.
///
/// Origin: BMMAIN.EXE 27c2:1226 (cup_check_ties_extra_time)
pub fn cup_check_ties_extra_time<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    mask: u8,
    count: u8,
    shootout: bool,
) -> bool {
    let mut any = false;
    rt.manager_plays_now[..state.active_managers().len()].fill(0);
    for cup in 0..4u8 {
        if (1 << cup) & mask == 0 {
            continue;
        }
        for s in (0..count).step_by(2) {
            let i = usize::from(s);
            let (home, away) = (slots(state, cup)[i], slots(state, cup)[i + 1]);
            if mask == 1 {
                let (g0, g1) = (goals(state, cup)[i], goals(state, cup)[i + 1]);
                if !(g1 == g0 || u16::from(g0).wrapping_sub(u16::from(g1)) == 10) {
                    continue;
                }
                any = true;
                let g = &mut goals_mut(state, 0)[i];
                *g = g.wrapping_add(10);
                let r = state.manager_index_of_team(home, away);
                if r != 0 {
                    // TODO(re): for a human away club (r = 0x81 + m) the original sets
                    // FAR:1d94 + m in the high-score buffer FAR:1d52, which is not modelled
                    // (cups.md §11.3).
                    if r & 0x80 == 0 {
                        rt.manager_plays_now[usize::from(r - 1)] = 1;
                    }
                    rt.live_match_shown = 1;
                }
            } else {
                assert!(
                    cup != 0,
                    "cup mask {mask:#x} mixes the DFB-Pokal into a European day"
                );
                if state.euro_second_leg_pending[usize::from(cup) - 1] == 0
                    || cup_tie_winner(state, cup, s) != TIE_LEVEL
                {
                    continue;
                }
                any = true;
                let g = &mut goals_mut(state, cup)[i];
                *g = g.wrapping_add(10);
                if state.manager_index_of_team(home, away) != 0 {
                    rt.live_match_shown = 1;
                }
            }
            if shootout {
                match_penalty_shootout(state, rt, rng, hooks, cup, s);
            }
        }
    }
    any
}

/// Draws the next round of cup row `cup` unless the final was played: the winners (even
/// slots) move to the front, then 20 swaps of slots `random_range(1, n) - 1` and
/// `random_range(1, n) - 1` mix the `n` clubs. A European draw clears the first-leg goals of
/// all three European cups. For the DFB-Pokal the amateur home rule follows; when the draw is
/// shown it also runs once with the old round counter, which gives amateurs home advantage
/// in the semi-final too.
///
/// Origin: BMMAIN.EXE 27c2:13a2 (cup_draw_next_round)
pub fn cup_draw_next_round(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    cup: u8,
    ui: &mut dyn CupDrawUi,
) {
    let c = usize::from(cup);
    let round = state.event_state[c];
    if round == 5 {
        return;
    }
    let n = round_pairings(round);
    if cup != 0 {
        state.cup_first_leg_goals = [0; 96];
    }
    let shown = cup_draw_screen_open(state, cup, ui);
    state.event_state[c] = round.wrapping_add(1);
    let row = slots_mut(state, cup);
    for i in 0..usize::from(n).min(16) {
        row[i] = row[2 * i];
    }
    for _ in 0..20 {
        let a = (rng.random_range(1, n.into()) - 1) as usize;
        let b = (rng.random_range(1, n.into()) - 1) as usize;
        row.swap(a, b);
    }
    if shown {
        state.event_state[c] = round;
        for i in 0..n {
            let team = slots(state, cup)[usize::from(i)];
            ui.animate_slot(state, cup, i, team);
        }
        if cup == 0 {
            amateur_home_swaps(state, rt, n);
        }
        state.event_state[c] = round.wrapping_add(1);
        ui.close(state, cup);
    }
    if cup == 0 {
        amateur_home_swaps(state, rt, n);
    }
}

/// Completes a cup round after its match day; `euro` is the match day's cup mask - 1 (0 =
/// DFB-Pokal). Each tie's winner ends up in its even slot and its manager's stage becomes
/// round + 1.
///
/// European cups (only cup 1 in relegation mode, [`Runtime::relegation_mode`]): after a first
/// leg the goals are stored as first legs in return-leg order and the venues swap; after a
/// return leg [`cup_tie_winner`] decides (a non-zero result puts the club of `s + 1`
/// through), and a final sets the German holder byte (0 for a foreign winner). DFB-Pokal:
/// the home goals lose their markers for good and the away club wins only with strictly more
/// goals; the final sets the winner and runner-up. A decided final keeps its pairing and
/// goals for display. Outside relegation mode the goals are then cleared (not after a final)
/// and the next rounds drawn (European cups only after their return legs).
///
/// Origin: BMMAIN.EXE 27c2:16dc (cup_round_complete)
pub fn cup_round_complete(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    euro: u8,
    ui: &mut dyn CupDrawUi,
) {
    let n2 = usize::from(round_slots(state.event_state[usize::from(euro != 0)]));
    let managers = state.active_managers().len();
    let playing = rt.relegation_mode == 0;
    if euro != 0 {
        let last = if playing { 4 } else { 2 };
        for cup in 1..last {
            let c = usize::from(cup);
            if state.euro_second_leg_pending[c - 1] == 0 {
                state.euro_second_leg_pending[c - 1] = 1;
                for s in (0..n2).step_by(2) {
                    let [g0, g1] = [goals(state, cup)[s], goals(state, cup)[s + 1]];
                    let first = &mut state.cup_first_leg_goals[first_leg_range(cup)];
                    first[s] = g1;
                    first[s + 1] = g0;
                    slots_mut(state, cup).swap(s, s + 1);
                }
            } else {
                state.euro_second_leg_pending[c - 1] = 0;
                let final_pair = [slots(state, cup)[0], slots(state, cup)[1]];
                for s in (0..n2).step_by(2) {
                    if cup_tie_winner(state, cup, s as u8) != 0 {
                        slots_mut(state, cup).swap(s, s + 1);
                    }
                    let winner = slots(state, cup)[s];
                    let stage = state.event_state[c].wrapping_add(1);
                    for m in &mut state.managers[..managers] {
                        if m.team == winner && playing {
                            m.values_132[c] = stage;
                        }
                    }
                }
                if n2 == 2 && playing {
                    let winner = slots(state, cup)[0];
                    let holder = match cup {
                        1 => &mut state.winner_07ad,
                        2 => &mut state.winner_07ae,
                        _ => &mut state.winner_07af,
                    };
                    *holder = if winner < FIRST_FOREIGN_TEAM {
                        winner + 1
                    } else {
                        0
                    };
                    let r = state.manager_index_of_team(winner, 0xff);
                    if r != 0 {
                        ui.title_winner(state, r - 1, cup + 1);
                    }
                    slots_mut(state, cup)[..2].copy_from_slice(&final_pair);
                }
            }
        }
    } else {
        let final_pair = [slots(state, 0)[0], slots(state, 0)[1]];
        let mut decided = (0, 0);
        for s in (0..n2).step_by(2) {
            let (mut winner, mut loser) = (slots(state, 0)[s], slots(state, 0)[s + 1]);
            let g = goals_mut(state, 0);
            while g[s] > 9 {
                g[s] -= 10;
            }
            if g[s + 1] > g[s] {
                let row = slots_mut(state, 0);
                row[s] = row[s + 1];
                std::mem::swap(&mut winner, &mut loser);
            }
            let stage = state.event_state[0].wrapping_add(1);
            for m in &mut state.managers[..managers] {
                if m.team == winner && playing {
                    m.values_132[0] = stage;
                }
            }
            decided = (winner, loser);
        }
        if n2 == 2 {
            state.winner_07ac = decided.0.wrapping_add(1);
            state.dfb_cup_runner_up = decided.1.wrapping_add(1);
            let r = state.manager_index_of_team(slots(state, 0)[0], 0xff);
            if r != 0 {
                ui.title_winner(state, r - 1, 1);
            }
            slots_mut(state, 0)[..2].copy_from_slice(&final_pair);
        }
    }

    let clear = playing
        && if euro != 0 {
            state.euro_second_leg_pending[0] != 0 || state.event_state[1] < 5
        } else {
            state.event_state[0] < 5
        };
    if clear {
        cup_clear_match_goals(state);
    }
    let cups = if euro != 0 { 1..4 } else { 0..1 };
    for cup in cups {
        if playing && (cup == 0 || state.euro_second_leg_pending[usize::from(cup) - 1] == 0) {
            cup_draw_next_round(state, rt, rng, cup, ui);
        }
    }
}

/// Plays one leg of the relegation play-off between the 16th of the Bundesliga
/// (`league_teams[0][15]`) and the 3rd of the 2. Bundesliga (`league_teams[1][2]`), as pairing
/// 0 of cup row 1 in relegation mode: the 2. Bundesliga club is at home in the first leg.
/// The first leg's goals go to [`GameState::relegation_first_leg`]; after the return leg
/// [`GameState::relegation_result`] is [`cup_tie_winner`] (0 = the Bundesliga club stays up).
/// The borrowed cup cells are restored; the European round counter is left at 5.
///
/// Origin: BMMAIN.EXE 1310:02bf (matchday_relegation_match)
pub fn matchday_relegation_match<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
) {
    state.event_state[1] = 5;
    rt.relegation_mode = 1;
    // UI: the title "Relegationsspiel" and, without automatic play, delay_ticks(50).
    let return_leg = state.euro_second_leg_pending[0] != 0;
    let saved_slots = [slots(state, 1)[0], slots(state, 1)[1]];
    let saved_goals = [goals(state, 1)[0], goals(state, 1)[1]];
    let saved_first = [state.cup_first_leg_goals[0], state.cup_first_leg_goals[1]];
    goals_mut(state, 1)[..2].fill(0);
    state.cup_first_leg_goals[..2].copy_from_slice(&if return_leg {
        state.relegation_first_leg
    } else {
        [0; 2]
    });
    let (bundesliga, second_tier) = (state.league_teams[0][15], state.league_teams[1][2]);
    let pairing = if return_leg {
        [bundesliga, second_tier]
    } else {
        [second_tier, bundesliga]
    };
    slots_mut(state, 1)[..2].copy_from_slice(&pairing);

    matchday_play(state, rt, rng, hooks, 0, 2);

    if state.euro_second_leg_pending[0] == 0 {
        state.relegation_result = cup_tie_winner(state, 1, 0);
    } else {
        state.relegation_first_leg = [state.cup_first_leg_goals[0], state.cup_first_leg_goals[1]];
    }
    slots_mut(state, 1)[..2].copy_from_slice(&saved_slots);
    goals_mut(state, 1)[..2].copy_from_slice(&saved_goals);
    state.cup_first_leg_goals[..2].copy_from_slice(&saved_first);
    rt.relegation_mode = 0;
}

/// Shuffles which club plays in which fixture slot: per league tier 55 swaps of two random
/// club ids of the tier ([`GameState::team_swap_slots`], `random_range(lo, hi)` twice), each
/// also swapping the two ids' positions in the tier's table order.
///
/// Origin: BMMAIN.EXE 1310:09c5 (league_shuffle_teams)
pub fn league_shuffle_teams(state: &mut GameState, rt: &Runtime, rng: &mut Rng) {
    for (tier, &lo) in TIER_FIRST.iter().enumerate() {
        let count = rt.tier_team_count[tier];
        let hi = lo.wrapping_add(count as u8).wrapping_sub(1);
        for _ in 0..55 {
            let a = rng.random_range(lo.into(), hi.into()) as u8;
            let b = rng.random_range(lo.into(), hi.into()) as u8;
            state.team_swap_slots(a, b, true);
            let row = &mut state.league_teams[tier][..count.max(0) as usize];
            let i = row.iter().position(|&t| t == a);
            let j = row.iter().position(|&t| t == b);
            if let (Some(i), Some(j)) = (i, j) {
                row.swap(i, j);
            }
        }
    }
}

/// Promotion and relegation at the season end, on the final tables
/// ([`GameState::league_teams`]).
///
/// A human manager with a balance below -2 000 000 in one of the two Bundesligas loses the
/// licence: status 4, and the club moves to the bottom of its table, onto a relegation
/// place. Per tier the bottom clubs then swap slots ([`GameState::team_swap_slots`]) with the
/// top clubs of the tier below; the amateur tier swaps with the reserve clubs 61 or 62
/// (`random_range(61, 62)`, the season's only random number), 60, 59 and 58. A Bundesliga
/// club that won the play-off ([`GameState::relegation_result`] 0) keeps its slot. The tier
/// relegates its places plus the licence withdrawals (in the Bundesliga plus one for a
/// play-off it won) that exceed those places. A human manager going down gets status 2
/// (status 8 and 500 000 DM when dropping out of the amateur tier, whose two swaps cancel),
/// one coming up status 1. Every league round counter restarts at 1.
///
/// Origin: BMMAIN.EXE 2a58:3d99 (season_main_loop, promotion and relegation)
pub fn season_promotion_relegation(state: &mut GameState, rt: &Runtime, rng: &mut Rng) {
    let mut extra = [0i8; 3];
    for m in 0..state.active_managers().len() {
        let manager = &mut state.managers[m];
        manager.value_140 = 0;
        let league = manager.league as i8;
        if manager.balance >= -2_000_000 || league >= 2 {
            continue;
        }
        manager.value_140 = 4;
        let (l, team) = (league as usize, manager.team);
        let count = rt.tier_team_count[l] as usize;
        let row = &mut state.league_teams[l][..count];
        let Some(p) = row.iter().position(|&t| t == team) else {
            continue;
        };
        row.copy_within(p + 1.., p);
        row[count - 1] = team;
        extra[l] += 1;
        state.managers[m].league += 1;
    }

    for l in 0..3 {
        let places = RELEGATION_PLACES[l];
        if l == 0 && state.relegation_result == 0 {
            extra[0] += 1;
        }
        extra[l] = if extra[l] > places {
            extra[l] - places
        } else {
            0
        };
        state.competition_rounds[l] = 1;
        let count = rt.tier_team_count[l] as usize;
        let relegated = i16::from(extra[l]) + i16::from(places);
        let mut k: u8 = 0;
        while i16::from(k) < relegated {
            let table = state.league_teams.as_flattened();
            let down = table[l * 20 + count - 1 - usize::from(k)];
            let up = if l < 2 {
                table[(l + 1) * 20 + usize::from(k)]
            } else {
                let n = rt
                    .tier_team_count
                    .iter()
                    .fold(0u8, |n, &c| n.wrapping_add(c as u8));
                if k == 0 && (n as i8) < 60 {
                    let n = i16::from(n as i8);
                    rng.random_range(n + 3, n + 4) as u8
                } else {
                    n.wrapping_add(3).wrapping_sub(k)
                }
            };
            let playoff_slot = l == 0
                && relegated - i16::from(k) == 1
                && state.relegation_result == 0
                && HAS_RELEGATION_PLAYOFF[0] != 0;
            if !playoff_slot {
                let r = state.manager_index_of_team(down, 0xff);
                if r != 0 && state.managers[usize::from(r - 1)].value_140 == 0 {
                    let manager = &mut state.managers[usize::from(r - 1)];
                    if l < 2 {
                        manager.value_140 = 2;
                        manager.league = manager.league.wrapping_add(1);
                    } else {
                        manager.value_140 = 8;
                        manager.balance = 500_000;
                        state.team_swap_slots(down, up, true);
                    }
                }
                let r = state.manager_index_of_team(up, 0xff);
                if r != 0 && state.managers[usize::from(r - 1)].value_140 == 0 {
                    let manager = &mut state.managers[usize::from(r - 1)];
                    manager.league = manager.league.wrapping_sub(1);
                    manager.value_140 = 1;
                }
                state.team_swap_slots(down, up, true);
            }
            k += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matchsim::TickerEvent;
    use crate::test_saves::save_states;
    use bmp_formats::savegame::SaveGame;

    /// Advances a copy of `rng` by `n` calls of `rand()`.
    fn advanced(rng: Rng, n: usize) -> Rng {
        let mut r = rng;
        for _ in 0..n {
            r.rand();
        }
        r
    }

    /// Number of `rand()` calls that took `start` to `end`.
    fn draws(start: Rng, end: Rng) -> usize {
        (0..10_000).find(|&n| advanced(start, n) == end).unwrap()
    }

    fn never_asked(cup: u8) -> bool {
        panic!("draw dialog of cup {cup}")
    }

    /// Match hooks that play cup days through this module: no gate money, no ticker, no
    /// newspaper, draws never shown. Records the gate pairings and the state after each cup
    /// round.
    #[derive(Default)]
    struct CupHooks {
        gates: Vec<(u8, u8)>,
        after_round: Vec<GameState>,
    }

    impl MatchHooks for CupHooks {
        fn match_gate_receipts_and_aftermath(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            home: u8,
            away: u8,
            _: u8,
            _: u8,
            _: u8,
            _: u8,
        ) -> u8 {
            self.gates.push((home, away));
            0
        }

        fn match_ticker_events(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            _: TickerEvent,
        ) {
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
            cup_check_ties_extra_time(state, rt, rng, self, cup_mask, count, shootout)
        }

        fn cup_round_complete(
            &mut self,
            state: &mut GameState,
            rt: &mut Runtime,
            rng: &mut Rng,
            euro: u8,
        ) {
            cup_round_complete(state, rt, rng, euro, &mut |_| false);
            self.after_round.push(state.clone());
        }

        fn match_rate_players_and_newspaper(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            _: u8,
        ) {
        }
    }

    /// One tie of European cup 1 at slot 0: first legs `[a, d]`, return goals `[e, b]`.
    fn tie(a: u8, d: u8, e: u8, b: u8, day: i32) -> u8 {
        let mut s = GameState::zeroed();
        s.day_counter = day;
        s.cup_first_leg_goals[..2].copy_from_slice(&[a, d]);
        goals_mut(&mut s, 1)[..2].copy_from_slice(&[e, b]);
        cup_tie_winner(&s, 1, 0)
    }

    #[test]
    fn cup_tie_winner_cases() {
        // Slot 1 wins on aggregate 2:1.
        assert_eq!(tie(0, 2, 1, 0, 100), 1);
        // 2:2, slot 0 scored its away goal: slot 0 through.
        assert_eq!(tie(1, 2, 1, 0, 100), 0);
        // 2:2, slot 1 scored more away goals.
        assert_eq!(tie(0, 1, 2, 1, 100), 1);
        // Fully level.
        assert_eq!(tie(1, 1, 1, 1, 100), TIE_LEVEL);
        // After day 315 a level aggregate ignores away goals; on day 315 it does not.
        assert_eq!(tie(1, 2, 1, 0, 316), TIE_LEVEL);
        assert_eq!(tie(1, 2, 1, 0, 315), 0);
        // Extra-time and penalty markers of the home goals are stripped.
        assert_eq!(tie(0, 3, 13, 0, 100), TIE_LEVEL);
        assert_eq!(tie(0, 3, 23, 0, 100), TIE_LEVEL);
        assert_eq!(tie(0, 3, 14, 0, 100), 0);
        // b + d - a wraps to 0xfffb and never equals a stripped goal count.
        assert_eq!(tie(5, 0, 0, 0, 316), 0);
    }

    #[test]
    #[should_panic(expected = "no first legs")]
    fn the_dfb_cup_has_no_ties_over_two_legs() {
        cup_tie_winner(&GameState::zeroed(), 0, 0);
    }

    #[test]
    fn amateur_home_swap_rule() {
        let rt = Runtime::default();
        let swapped = |round: u8, pair: [u8; 2]| {
            let mut s = GameState::zeroed();
            s.event_state[0] = round;
            slots_mut(&mut s, 0)[..2].copy_from_slice(&pair);
            amateur_home_swaps(&mut s, &rt, 2);
            [slots(&s, 0)[0], slots(&s, 0)[1]] != pair
        };
        for round in 1..=3 {
            assert!(swapped(round, [3, 40]), "round {round}");
        }
        assert!(!swapped(4, [3, 40]));
        assert!(swapped(1, [20, 40]), "2. Bundesliga club is professional");
        assert!(!swapped(1, [41, 40]), "amateur v amateur");
        assert!(!swapped(1, [40, 3]), "amateur already at home");
        assert!(!swapped(1, [3, 20]), "no amateur");
        let mut s = GameState::zeroed();
        slots_mut(&mut s, 0)[..2].copy_from_slice(&[40, 3]);
        cup_amateur_home_swap(&mut s, &rt, 40, 3, 0);
        assert_eq!(slots(&s, 0)[..2], [40, 3], "slot 0 is a home slot");
    }

    /// Final tables 0..17 and 18..37; no holders.
    fn table_state() -> GameState {
        let mut s = GameState::zeroed();
        for i in 0..18 {
            s.league_teams[0][i] = i as u8;
        }
        for i in 0..20 {
            s.league_teams[1][i] = 18 + i as u8;
        }
        s
    }

    fn qualifiers(s: &GameState) -> [&[u8]; 3] {
        let rows = &s.competition_slots;
        let row = |c: usize| {
            let r = &rows[c * 6..c * 6 + 6];
            &r[..r.iter().position(|&t| t == NO_QUALIFIER).unwrap()]
        };
        [row(0), row(1), row(2)]
    }

    #[test]
    fn qualifiers_build_cases() {
        // (a) The champion also won the DFB-Pokal: its runner-up (club 20) plays the Cup
        // Winners' Cup; the UEFA Cup takes ranks 2..5.
        let mut s = table_state();
        s.winner_07ac = 1;
        s.dfb_cup_runner_up = 21;
        s.euro_qualified_list = [99; 10];
        let mut rt = Runtime {
            euro_qualified_count: 3,
            ..Runtime::default()
        };
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s), [&[0][..], &[20], &[1, 2, 3, 4]]);
        assert_eq!(s.competition_slots[1..6], [NO_QUALIFIER; 5]);
        assert_eq!(rt.euro_qualified_count, 6);
        assert_eq!(s.euro_qualified_list, [0, 20, 1, 2, 3, 4, 99, 99, 99, 99]);
        let base = s.clone();

        // (b) The champion holds the Champions' Cup: the runner-up takes the second place.
        let mut s = base.clone();
        s.winner_07ad = 1;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s), [&[0, 1][..], &[20], &[2, 3, 4, 5]]);
        // A holder from the table's middle qualifies itself.
        let mut s = base.clone();
        s.winner_07ad = 6;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s), [&[0, 5][..], &[20], &[1, 2, 3, 4]]);

        // (c) DS:07ac == 0 is not tested: club 0xff enters the Cup Winners' Cup.
        let mut s = base.clone();
        s.winner_07ac = 0;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s)[1], [0xff]);

        // The Cup Winners' Cup holder joins the DFB-Pokal entry.
        let mut s = base.clone();
        s.winner_07ae = 31;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s)[1], [20, 30]);

        // (d) The UEFA Cup holder is already qualified: five clubs from the table instead.
        let mut s = base.clone();
        s.winner_07af = 1;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s)[2], [1, 2, 3, 4, 5]);
        // An unqualified holder takes the first UEFA place.
        let mut s = base.clone();
        s.winner_07af = 11;
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s)[2], [10, 1, 2, 3, 4]);
        assert!(team_is_euro_qualified(&s, &rt, 10));
        assert!(!team_is_euro_qualified(&s, &rt, 5));
    }

    #[test]
    fn duplicate_table_ids_leave_uefa_places_free() {
        // Every entry is club 0, so only the champion qualifies from the table.
        let save = GameState::zeroed().to_savegame().to_bytes().unwrap();
        let mut s = GameState::from_savegame(&SaveGame::parse(&save).unwrap()).unwrap();
        let mut rt = Runtime::default();
        euro_cup_qualifiers_build(&mut s, &mut rt);
        assert_eq!(qualifiers(&s), [&[0][..], &[0xff], &[]]);
        assert_eq!(rt.euro_qualified_count, 2);
    }

    #[test]
    fn round_0_on_a_cup_day_completes_32_slots() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 31;
        for (i, t) in slots_mut(&mut s, 0).iter_mut().enumerate() {
            *t = i as u8;
        }
        goals_mut(&mut s, 0)[30..].copy_from_slice(&[0, 1]);
        cup_round_complete(&mut s, &rt, &mut Rng::new(9), 0, &mut |_| false);
        assert_eq!(s.managers[0].values_132[0], 1);
        assert_eq!(s.event_state[0], 1);
    }

    #[test]
    fn dfb_first_round_draw() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 5;
        s.managers[0].values_132 = [0; 5];
        s.options[SHOW_DRAW_OPTION] = 1;
        s.event_state[0] = 3;
        let start = Rng::new(7);
        let mut rng = start;
        let mut asked = Vec::new();
        cup_draw_first_round(&mut s, &rt, &mut rng, 0, false, &mut |c| {
            asked.push(c);
            false
        });
        assert_eq!(asked, [0]);
        assert_eq!(s.event_state[0], 1);
        assert_eq!(s.managers[0].values_132[0], 1);

        // Replay: the human slot, then per other slot a club until it is new and unmanaged.
        let mut r = start;
        let human = r.random_range(0, 31) as usize;
        let mut drawn = [0u8; 32];
        drawn[human] = 5;
        for i in (0..32).filter(|&i| i != human) {
            drawn[i] = loop {
                let t = r.random_range(0, 57) as u8;
                if t != 5 && !drawn[..i].contains(&t) {
                    break t;
                }
            };
        }
        assert_eq!(rng, r);
        let mut sorted = slots(&s, 0).to_vec();
        sorted.sort();
        let mut expected = drawn.to_vec();
        expected.sort();
        assert_eq!(sorted, expected);
        assert!(sorted.windows(2).all(|w| w[0] != w[1]));
        assert!(sorted.iter().all(|&t| t <= DFB_LAST_TEAM));
        // After the amateur rule no amateur is away against a professional club.
        let row = slots(&s, 0);
        for s in (1..32).step_by(2) {
            assert!(!(row[s] >= 38 && row[s - 1] < 38), "slot {s}: {row:?}");
        }

        // Switched off, the dialog never appears.
        let mut s2 = GameState::zeroed();
        cup_draw_first_round(&mut s2, &rt, &mut Rng::new(7), 0, false, &mut never_asked);
    }

    /// Cups 2 and 3 hold foreign clubs 64..=127; manager 0's club 7 qualified for cup 1.
    fn euro_draw_state() -> GameState {
        let mut s = GameState::zeroed();
        for (i, t) in (64..128).enumerate() {
            slots_mut(&mut s, 2 + (i / 32) as u8)[i % 32] = t;
        }
        s.competition_slots = [NO_QUALIFIER; 18];
        s.competition_slots[0] = 7;
        s.manager_count = 1;
        s.managers[0].team = 7;
        s.euro_second_leg_pending = [1; 3];
        s.cup_first_leg_goals = [9; 96];
        s
    }

    #[test]
    fn euro_first_round_draw() {
        let rt = Runtime::default();
        let mut s = euro_draw_state();
        let mut rng = Rng::new(11);
        cup_draw_first_round(&mut s, &rt, &mut rng, 1, false, &mut never_asked);
        let row = slots(&s, 1);
        assert_eq!(row.iter().filter(|&&t| t == 7).count(), 1);
        assert_ne!(row[0], 7, "slot 0 never gets a German qualifier");
        let foreign: Vec<u8> = row.iter().copied().filter(|&t| t != 7).collect();
        assert_eq!(foreign.len(), 31);
        assert!(foreign.iter().all(|&t| (128..=199).contains(&t)));
        let mut sorted = foreign.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 31);
        assert_eq!(s.managers[0].values_132[1], 1);
        assert_eq!(s.euro_second_leg_pending, [0, 1, 1]);
        assert_eq!(s.cup_first_leg_goals, [0; 96]);
        assert_eq!(s.event_state[1], 1);

        // Cup 2 compares with cups 1 and 3 and keeps the first legs.
        let mut s = euro_draw_state();
        cup_draw_first_round(&mut s, &rt, &mut rng, 2, false, &mut never_asked);
        assert_eq!(s.cup_first_leg_goals, [9; 96]);
        assert_eq!(s.managers[0].values_132[2], NOT_IN_CUP);
        assert!(
            slots(&s, 2)
                .iter()
                .all(|&t| t >= 64 && !slots(&s, 3).contains(&t))
        );

        // Redisplay: the dialog opens, nothing is drawn.
        let mut s = euro_draw_state();
        s.options[SHOW_DRAW_OPTION + 1] = 1;
        let before = s.clone();
        let mut rng = Rng::new(11);
        let mut asked = 0;
        cup_draw_first_round(&mut s, &rt, &mut rng, 1, true, &mut |_| {
            asked += 1;
            true
        });
        assert_eq!(asked, 1);
        assert_eq!(rng, Rng::new(11));
        let mut expected = before;
        expected.event_state[1] = 1;
        assert_eq!(s, expected);
    }

    #[test]
    fn ties_extra_time_marks_level_ties() {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.managers[0].team = 10;
        s.managers[1].team = 15;
        slots_mut(&mut s, 0)[..6].copy_from_slice(&[10, 11, 12, 13, 14, 15]);
        goals_mut(&mut s, 0)[..6].copy_from_slice(&[1, 1, 2, 1, 0, 0]);
        let mut rt = Runtime {
            manager_plays_now: [1; MANAGER_SLOTS],
            ..Runtime::default()
        };
        let mut rng = Rng::new(5);
        let mut hooks = CupHooks::default();
        assert!(cup_check_ties_extra_time(
            &mut s, &mut rt, &mut rng, &mut hooks, 1, 6, false
        ));
        assert_eq!(goals(&s, 0)[..6], [11, 1, 2, 1, 10, 0]);
        // The away club's manager is not flagged (cups.md §11.3); slots 2.. are untouched.
        assert_eq!(rt.manager_plays_now, [1, 0, 1, 1]);
        assert_eq!(rt.live_match_shown, 1);
        assert_eq!(rng, Rng::new(5));

        // Still level after extra time (+10 apart): penalties decide.
        goals_mut(&mut s, 0)[4] = 0;
        assert!(cup_check_ties_extra_time(
            &mut s, &mut rt, &mut rng, &mut hooks, 1, 2, true
        ));
        let g = goals(&s, 0);
        assert!(g[0] >= 0x16 && g[0] - 0x14 != g[1], "{g:?}");

        // European cups: only return legs of level ties count; no manager flag.
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 20;
        slots_mut(&mut s, 1)[..2].copy_from_slice(&[64, 65]);
        slots_mut(&mut s, 2)[..2].copy_from_slice(&[20, 66]);
        s.euro_second_leg_pending = [0, 1, 1];
        s.cup_first_leg_goals = [1; 96];
        for cup in 1..4 {
            goals_mut(&mut s, cup)[..2].copy_from_slice(&[1, 1]);
        }
        goals_mut(&mut s, 3)[0] = 2;
        let mut rt = Runtime::default();
        assert!(cup_check_ties_extra_time(
            &mut s, &mut rt, &mut rng, &mut hooks, 0x0e, 2, false
        ));
        assert_eq!(goals(&s, 1)[0], 1, "first leg");
        assert_eq!(goals(&s, 2)[0], 11);
        assert_eq!(goals(&s, 3)[0], 2, "decided on aggregate");
        assert_eq!(rt.manager_plays_now, [0; MANAGER_SLOTS]);
        assert_eq!(rt.live_match_shown, 1);
    }

    #[test]
    fn dfb_round_complete_and_next_draw() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 13;
        s.event_state[0] = 4;
        slots_mut(&mut s, 0)[..4].copy_from_slice(&[10, 11, 12, 13]);
        goals_mut(&mut s, 0)[..4].copy_from_slice(&[12, 1, 1, 3]);
        let start = Rng::new(9);
        let mut rng = start;
        cup_round_complete(&mut s, &rt, &mut rng, 0, &mut never_asked);
        assert_eq!(s.managers[0].values_132[0], 5);
        assert_eq!(s.event_state[0], 5);
        assert_eq!(s.cup_results, [[0; 2]; 64]);
        let mut finalists = [slots(&s, 0)[0], slots(&s, 0)[1]];
        finalists.sort();
        assert_eq!(finalists, [10, 13]);
        assert_eq!(draws(start, rng), 40);
        assert_eq!((s.winner_07ac, s.dfb_cup_runner_up), (0, 0));
    }

    /// Records the screen hooks of the draws.
    #[derive(Default)]
    struct DrawLog {
        answer: bool,
        asked: Vec<u8>,
        /// (cup, slot, team, round counter while animating)
        slots: Vec<(u8, u8, u8, u8)>,
        closed: Vec<(u8, u8)>,
        winners: Vec<(u8, u8)>,
    }

    impl CupDrawUi for DrawLog {
        fn ask(&mut self, cup: u8) -> bool {
            self.asked.push(cup);
            self.answer
        }
        fn animate_slot(&mut self, state: &GameState, cup: u8, slot: u8, team: u8) {
            self.slots
                .push((cup, slot, team, state.event_state[usize::from(cup)]));
        }
        fn close(&mut self, state: &mut GameState, cup: u8) {
            self.closed.push((cup, state.event_state[usize::from(cup)]));
        }
        fn title_winner(&mut self, _: &GameState, m: u8, comp: u8) {
            self.winners.push((m, comp));
        }
    }

    #[test]
    fn shown_first_round_animates_every_slot_with_the_same_draws() {
        let rt = Runtime::default();
        let setup = || {
            let mut s = GameState::zeroed();
            s.manager_count = 1;
            s.managers[0].team = 5;
            s.options[SHOW_DRAW_OPTION] = 1;
            s
        };
        let mut plain = setup();
        let mut rng_plain = Rng::new(7);
        cup_draw_first_round(&mut plain, &rt, &mut rng_plain, 0, false, &mut |_: u8| {
            false
        });
        let mut shown = setup();
        let mut rng_shown = Rng::new(7);
        let mut log = DrawLog {
            answer: true,
            ..Default::default()
        };
        cup_draw_first_round(&mut shown, &rt, &mut rng_shown, 0, false, &mut log);
        assert_eq!(rng_shown, rng_plain, "watching draws no random number");
        assert_eq!(shown, plain);
        assert_eq!(log.asked, [0]);
        // Every slot in order, the human club's too, before the amateur rule; round 0.
        assert_eq!(log.slots.len(), 32);
        assert!(
            log.slots
                .iter()
                .enumerate()
                .all(|(i, x)| x.1 == i as u8 && x.3 == 0)
        );
        assert!(log.slots.iter().any(|x| x.2 == 5));
        let mut teams: Vec<u8> = log.slots.iter().map(|x| x.2).collect();
        let mut row = slots(&shown, 0).to_vec();
        teams.sort();
        row.sort();
        assert_eq!(teams, row);
        assert_eq!(log.closed, [(0, 1)]);

        // Declined: no animation and no close.
        let mut log = DrawLog::default();
        cup_draw_first_round(&mut setup(), &rt, &mut Rng::new(7), 0, false, &mut log);
        assert_eq!(
            (log.asked.len(), log.slots.len(), log.closed.len()),
            (1, 0, 0)
        );
    }

    #[test]
    fn shown_euro_redisplay_animates_the_existing_draw() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.options[SHOW_DRAW_OPTION + 1] = 1;
        for (i, t) in slots_mut(&mut s, 2).iter_mut().enumerate() {
            *t = 64 + i as u8;
        }
        let before = slots(&s, 2).to_vec();
        let mut rng = Rng::new(3);
        let mut log = DrawLog {
            answer: true,
            ..Default::default()
        };
        cup_draw_first_round(&mut s, &rt, &mut rng, 2, true, &mut log);
        assert_eq!(rng, Rng::new(3));
        assert_eq!(slots(&s, 2), before);
        let teams: Vec<u8> = log.slots.iter().map(|x| x.2).collect();
        assert_eq!(teams, before);
        assert_eq!(log.closed, [(2, 1)]);
    }

    #[test]
    fn shown_next_round_animates_with_the_old_round_counter() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.options[SHOW_DRAW_OPTION] = 1;
        s.event_state[0] = 2;
        for i in 0..8 {
            slots_mut(&mut s, 0)[2 * i] = i as u8;
        }
        let mut log = DrawLog {
            answer: true,
            ..Default::default()
        };
        cup_draw_next_round(&mut s, &rt, &mut Rng::new(5), 0, &mut log);
        assert_eq!(log.slots.len(), 8);
        assert!(log.slots.iter().all(|x| x.3 == 2));
        assert_eq!(log.closed, [(0, 3)]);
    }

    #[test]
    fn won_finals_show_the_title_screen() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.managers[1].team = 11;
        s.event_state[0] = 5;
        slots_mut(&mut s, 0)[..2].copy_from_slice(&[10, 11]);
        goals_mut(&mut s, 0)[..2].copy_from_slice(&[0, 2]);
        let mut log = DrawLog::default();
        cup_round_complete(&mut s, &rt, &mut Rng::new(9), 0, &mut log);
        assert_eq!(log.winners, [(1, 1)]);
        assert!(log.asked.is_empty());

        // European cup 2, return leg of the final won by manager 0's club.
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 3;
        s.event_state[1] = 5;
        s.euro_second_leg_pending = [1; 3];
        slots_mut(&mut s, 2)[..2].copy_from_slice(&[3, 70]);
        goals_mut(&mut s, 2)[..2].copy_from_slice(&[2, 0]);
        let mut log = DrawLog::default();
        cup_round_complete(&mut s, &rt, &mut Rng::new(9), 0x0d, &mut log);
        assert!(log.winners.contains(&(0, 3)), "{:?}", log.winners);
    }

    #[test]
    fn dfb_final_sets_winner_and_runner_up() {
        let rt = Runtime::default();
        let final_state = |g: [u8; 2]| {
            let mut s = GameState::zeroed();
            s.manager_count = 1;
            s.managers[0].team = 11;
            s.event_state[0] = 5;
            slots_mut(&mut s, 0)[..2].copy_from_slice(&[10, 11]);
            goals_mut(&mut s, 0)[..2].copy_from_slice(&g);
            s
        };
        let mut s = final_state([0, 2]);
        let mut rng = Rng::new(9);
        cup_round_complete(&mut s, &rt, &mut rng, 0, &mut never_asked);
        assert_eq!((s.winner_07ac, s.dfb_cup_runner_up), (12, 11));
        assert_eq!(s.managers[0].values_132[0], 6);
        assert_eq!(slots(&s, 0)[..2], [10, 11], "the final's pairing is kept");
        assert_eq!(goals(&s, 0)[..2], [0, 2], "the final's goals are kept");
        assert_eq!(rng, Rng::new(9));

        // Penalties 4:3: the home byte loses its +20 for good.
        let mut s = final_state([24, 3]);
        cup_round_complete(&mut s, &rt, &mut rng, 0, &mut never_asked);
        assert_eq!((s.winner_07ac, s.dfb_cup_runner_up), (11, 12));
        assert_eq!(goals(&s, 0)[..2], [4, 3]);
        assert_eq!(
            s.managers[0].values_132[0], 0,
            "a lost final changes no stage"
        );
    }

    #[test]
    fn euro_legs_and_final() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 9;
        s.event_state[1..4].copy_from_slice(&[5; 3]);
        s.euro_second_leg_pending = [0, 1, 1];
        s.winner_07ae = 5;
        // Cup 1: first leg of the final, 2:1 for club 70.
        slots_mut(&mut s, 1)[..2].copy_from_slice(&[70, 7]);
        goals_mut(&mut s, 1)[..2].copy_from_slice(&[2, 1]);
        // Cup 2: return leg; club 80 (first leg away, 0:1) wins 3:0, a foreign holder.
        slots_mut(&mut s, 2)[..2].copy_from_slice(&[80, 81]);
        s.cup_first_leg_goals[32..34].copy_from_slice(&[0, 1]);
        goals_mut(&mut s, 2)[..2].copy_from_slice(&[3, 0]);
        // Cup 3: return leg; club 9 wins 0:2 after 0:0.
        slots_mut(&mut s, 3)[..2].copy_from_slice(&[90, 9]);
        goals_mut(&mut s, 3)[..2].copy_from_slice(&[0, 2]);
        let mut rng = Rng::new(9);
        cup_round_complete(&mut s, &rt, &mut rng, 0x0d, &mut never_asked);
        assert_eq!(s.euro_second_leg_pending, [1, 0, 0]);
        assert_eq!(
            s.cup_first_leg_goals[..2],
            [1, 2],
            "re-oriented to the return leg"
        );
        assert_eq!(slots(&s, 1)[..2], [7, 70], "venues swapped");
        assert_eq!((s.winner_07ae, s.winner_07af), (0, 10));
        assert_eq!(s.managers[0].values_132[3], 6);
        assert_eq!(slots(&s, 2)[..2], [80, 81]);
        assert_eq!(slots(&s, 3)[..2], [90, 9], "the final's pairing is kept");
        assert_eq!(s.cup_results, [[0; 2]; 64], "cleared after a first leg");
        assert_eq!(rng, Rng::new(9), "no draw after the finals");

        // A level semi-final return leg: the club of slot s + 1 goes through, the next round
        // of every European cup is drawn.
        let mut s = GameState::zeroed();
        s.event_state[1..4].copy_from_slice(&[4; 3]);
        s.euro_second_leg_pending = [1; 3];
        for cup in 1..4 {
            slots_mut(&mut s, cup)[..4].copy_from_slice(&[64, 65, 66, 67]);
        }
        let start = rng;
        cup_round_complete(&mut s, &rt, &mut rng, 0x0d, &mut never_asked);
        assert_eq!(draws(start, rng), 120);
        for cup in 1..4 {
            let mut f = [slots(&s, cup)[0], slots(&s, cup)[1]];
            f.sort();
            assert_eq!(f, [65, 67]);
        }
        assert_eq!(s.event_state[1..4], [5; 3]);
    }

    #[test]
    fn shown_semi_final_draw_gives_amateurs_home_advantage() {
        let rt = Runtime::default();
        let quarter_finals_played = || {
            let mut s = GameState::zeroed();
            s.event_state[0] = 3;
            for (i, t) in [40, 1, 41, 2].into_iter().enumerate() {
                slots_mut(&mut s, 0)[2 * i] = t;
            }
            s
        };
        let seed = (0..1000)
            .find(|&seed| {
                let mut s = quarter_finals_played();
                cup_draw_next_round(&mut s, &rt, &mut Rng::new(seed), 0, &mut never_asked);
                let row = slots(&s, 0);
                (1..4).step_by(2).any(|i| row[i] >= 38 && row[i - 1] < 38)
            })
            .expect("a draw with an amateur away against a professional");
        let mut unshown = quarter_finals_played();
        let mut rng_unshown = Rng::new(seed);
        cup_draw_next_round(&mut unshown, &rt, &mut rng_unshown, 0, &mut never_asked);
        let mut shown = quarter_finals_played();
        shown.options[SHOW_DRAW_OPTION] = 1;
        let mut rng_shown = Rng::new(seed);
        cup_draw_next_round(&mut shown, &rt, &mut rng_shown, 0, &mut |_| true);
        assert_eq!(rng_shown, rng_unshown);
        let row = slots_mut(&mut unshown, 0);
        for i in (1..4).step_by(2) {
            if row[i] >= 38 && row[i - 1] < 38 {
                row.swap(i - 1, i);
            }
        }
        unshown.options[SHOW_DRAW_OPTION] = 1;
        assert_eq!(shown, unshown);
        assert_eq!(shown.event_state[0], 4);
    }

    #[test]
    fn dfb_final_draw_is_shown_without_asking() {
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        s.options[SHOW_DRAW_OPTION] = 1;
        s.event_state[0] = 4;
        assert!(cup_draw_screen_open(&s, 0, &mut never_asked));
        cup_draw_next_round(&mut s, &rt, &mut Rng::new(3), 0, &mut never_asked);
        assert_eq!(s.event_state[0], 5);
    }

    #[test]
    fn relegation_round_leaves_cups_two_and_three_alone() {
        let rt = Runtime {
            relegation_mode: 1,
            ..Runtime::default()
        };
        let mut s = GameState::zeroed();
        s.event_state[1] = 5;
        for cup in 1..4 {
            slots_mut(&mut s, cup)[..2].copy_from_slice(&[0x40 + cup, 0x50 + cup]);
            goals_mut(&mut s, cup)[..2].copy_from_slice(&[cup, 2 * cup]);
        }
        for (i, g) in s.cup_first_leg_goals.iter_mut().enumerate() {
            *g = i as u8;
        }
        let untouched = |s: &GameState| {
            (
                s.euro_second_leg_pending[1..].to_vec(),
                s.cup_pairings.as_flattened()[64..].to_vec(),
                s.cup_results.as_flattened()[64..].to_vec(),
                s.cup_first_leg_goals[32..].to_vec(),
            )
        };
        let before = untouched(&s);
        let mut rng = Rng::new(5);
        cup_round_complete(&mut s, &rt, &mut rng, 1, &mut never_asked);
        assert_eq!(untouched(&s), before);
        assert_eq!(
            s.euro_second_leg_pending[0], 1,
            "cup row 1 played its first leg"
        );
        assert_eq!(rng, Rng::new(5));
    }

    #[test]
    fn relegation_match_borrows_and_restores_cup_one() {
        let mut s = GameState::zeroed();
        s.day_counter = 320;
        s.league_teams[0][15] = 15;
        s.league_teams[1][2] = 20;
        for club in [15, 20] {
            let c = &mut s.clubs[club];
            c.strength = [60; 3];
            c.base_strength = [50; 3];
            c.strength_1e = [50; 3];
            c.morale = 44;
        }
        s.event_state[1] = 2;
        slots_mut(&mut s, 1)[..2].copy_from_slice(&[100, 101]);
        goals_mut(&mut s, 1)[..2].copy_from_slice(&[3, 4]);
        s.cup_first_leg_goals[..2].copy_from_slice(&[5, 6]);
        s.relegation_result = 0x55;
        let borrowed = |s: &GameState| {
            (
                [slots(s, 1)[0], slots(s, 1)[1]],
                [goals(s, 1)[0], goals(s, 1)[1]],
                [s.cup_first_leg_goals[0], s.cup_first_leg_goals[1]],
            )
        };
        let before = borrowed(&s);
        let mut rt = Runtime::default();
        let mut rng = Rng::new(21);

        let mut hooks = CupHooks::default();
        matchday_relegation_match(&mut s, &mut rt, &mut rng, &mut hooks);
        assert_eq!(hooks.gates, [(20, 15)]);
        let after = &hooks.after_round[0];
        assert_eq!(
            s.relegation_first_leg,
            [after.cup_first_leg_goals[0], after.cup_first_leg_goals[1]]
        );
        assert_eq!(
            s.relegation_first_leg,
            [goals(after, 1)[1], goals(after, 1)[0]]
        );
        assert_eq!(s.euro_second_leg_pending[0], 1);
        assert_eq!(s.relegation_result, 0x55);
        assert_eq!(borrowed(&s), before);
        assert_eq!((rt.relegation_mode, s.event_state[1]), (0, 5));

        let mut hooks = CupHooks::default();
        matchday_relegation_match(&mut s, &mut rt, &mut rng, &mut hooks);
        assert_eq!(hooks.gates, [(15, 20)]);
        let after = &hooks.after_round[0];
        assert_eq!(after.cup_first_leg_goals[..2], s.relegation_first_leg);
        assert_eq!(s.relegation_result, cup_tie_winner(after, 1, 0));
        assert!(
            [0, 1].contains(&s.relegation_result),
            "decided by penalties at the latest"
        );
        assert_eq!(s.euro_second_leg_pending[0], 0);
        assert_eq!(borrowed(&s), before);
        assert_eq!(rt.relegation_mode, 0);
    }

    /// Identity final tables, club records marked with their id.
    fn identity_state() -> GameState {
        let mut s = GameState::zeroed();
        for (t, &first) in TIER_FIRST.iter().enumerate() {
            for i in 0..20 {
                s.league_teams[t][i] = first + i as u8;
            }
        }
        for id in 0..64 {
            s.clubs[id].name[0] = id as u8;
        }
        s.competition_rounds = [5; 3];
        s
    }

    fn marker(s: &GameState, id: u8) -> u8 {
        s.clubs[usize::from(id)].name[0]
    }

    #[test]
    fn promotion_relegation_swaps_the_right_slots() {
        let rt = Runtime::default();
        let start = Rng::new(4);
        let reserve = Rng::new(4).random_range(61, 62) as u8;
        let mut s = identity_state();
        let mut rng = start;
        season_promotion_relegation(&mut s, &rt, &mut rng);
        assert_eq!(draws(start, rng), 1);
        let swaps = [
            (17, 18),
            (16, 19),
            (37, 38),
            (36, 39),
            (35, 40),
            (34, 41),
            (57, reserve),
            (56, 60),
            (55, 59),
            (54, 58),
        ];
        for (down, up) in swaps {
            assert_eq!(
                (marker(&s, down), marker(&s, up)),
                (up, down),
                "{down}/{up}"
            );
        }
        assert_eq!(marker(&s, 15), 15, "the play-off winner stays up");
        assert_eq!(s.competition_rounds, [1, 1, 1]);

        let mut s = identity_state();
        s.relegation_result = 1;
        season_promotion_relegation(&mut s, &rt, &mut start.clone());
        assert_eq!((marker(&s, 15), marker(&s, 20)), (20, 15));

        let managed = |team: u8, league: u8, balance: i32| {
            let mut s = identity_state();
            s.manager_count = 1;
            let m = &mut s.managers[0];
            (m.team, m.league, m.balance, m.value_140) = (team, league, balance, 9);
            season_promotion_relegation(&mut s, &rt, &mut start.clone());
            s
        };
        let s = managed(17, 0, 0);
        let m = &s.managers[0];
        assert_eq!((m.value_140, m.league, m.team), (2, 1, 18));
        let s = managed(18, 1, 0);
        let m = &s.managers[0];
        assert_eq!((m.value_140, m.league, m.team), (1, 0, 17));
        let s = managed(57, 2, -5);
        let m = &s.managers[0];
        assert_eq!((m.value_140, m.balance, m.team), (8, 500_000, 57));
        assert_eq!(marker(&s, 57), 57, "both swaps cancel");

        // Licence withdrawn: club 3 drops to the bottom and is relegated first; its manager
        // keeps status 4 and goes down one tier only.
        let s = managed(3, 0, -2_000_001);
        let m = &s.managers[0];
        assert_eq!((m.value_140, m.league, m.team), (4, 1, 18));
        let mut table: Vec<u8> = (0..18).filter(|&t| t != 3).collect();
        table.push(3);
        assert_eq!(s.league_teams[0][..18], table[..]);
        assert_eq!((marker(&s, 3), marker(&s, 18)), (18, 3));
        assert_eq!((marker(&s, 17), marker(&s, 19)), (19, 17));
        assert_eq!(marker(&s, 16), 16, "now 16th: the play-off winner");
        assert_eq!(managed(3, 0, -2_000_000).managers[0].value_140, 0);
        let s = managed(25, 1, -2_000_001);
        let m = &s.managers[0];
        assert_eq!(
            (m.value_140, m.league),
            (4, 2),
            "the 2. Bundesliga withdraws licences too"
        );
        assert_eq!(managed(45, 2, -2_000_001).managers[0].value_140, 0);
    }

    #[test]
    fn league_shuffle_keeps_records_with_their_clubs() {
        let rt = Runtime::default();
        let mut s = identity_state();
        s.manager_count = 1;
        s.managers[0].team = 5;
        s.winner_07ac = 6;
        let start = Rng::new(13);
        let mut rng = start;
        league_shuffle_teams(&mut s, &rt, &mut rng);
        assert_eq!(draws(start, rng), 330);
        for (t, &first) in TIER_FIRST.iter().enumerate() {
            let n = usize::from(rt.tier_team_count[t] as u8);
            let mut ids = s.league_teams[t][..n].to_vec();
            ids.sort();
            assert_eq!(ids, (first..first + n as u8).collect::<Vec<_>>());
            for (i, &id) in s.league_teams[t][..n].iter().enumerate() {
                assert_eq!(marker(&s, id), first + i as u8, "tier {t} rank {i}");
            }
        }
        assert_eq!(marker(&s, s.managers[0].team), 5);
        assert_eq!(marker(&s, s.winner_07ac - 1), 5);
        assert_ne!(s.managers[0].team, 5, "seed 13 moves club 5");
    }

    #[test]
    fn league_shuffle_draws_from_the_whole_tier() {
        let rt = Runtime::default();
        let mut moved = [[false; 2]; 3];
        for seed in 0..40 {
            let mut s = identity_state();
            league_shuffle_teams(&mut s, &rt, &mut Rng::new(seed));
            for (t, &first) in TIER_FIRST.iter().enumerate() {
                let last = usize::from(rt.tier_team_count[t] as u8) - 1;
                moved[t][0] |= s.league_teams[t][0] != first;
                moved[t][1] |= s.league_teams[t][last] != first + last as u8;
            }
        }
        assert_eq!(moved, [[true; 2]; 3], "first and last club of every tier");
    }

    #[test]
    fn real_saves_qualifier_list_matches_the_rows() {
        for (path, s) in save_states() {
            let rows: Vec<u8> = s
                .competition_slots
                .iter()
                .copied()
                .filter(|&t| t != NO_QUALIFIER)
                .collect();
            assert_eq!(
                s.euro_qualified_list[..rows.len()],
                rows[..],
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn real_saves_euro_draws_place_every_qualifier() {
        let rt = Runtime::default();
        for (path, mut s) in save_states() {
            let mut rng = Rng::new(17);
            for cup in 1..4 {
                cup_draw_first_round(&mut s, &rt, &mut rng, cup, false, &mut never_asked);
            }
            let mut foreign = Vec::new();
            for cup in 1..4u8 {
                let c = usize::from(cup);
                let row = slots(&s, cup);
                let qualifiers: Vec<u8> = s.competition_slots[(c - 1) * 6..c * 6]
                    .iter()
                    .copied()
                    .take_while(|&t| t != NO_QUALIFIER)
                    .collect();
                for &q in &qualifiers {
                    assert_eq!(row[1..].iter().filter(|&&t| t == q).count(), 1);
                }
                let others: Vec<u8> = row
                    .iter()
                    .copied()
                    .filter(|t| !qualifiers.contains(t))
                    .collect();
                assert_eq!(others.len(), 32 - qualifiers.len(), "{}", path.display());
                assert!(others.iter().all(|&t| t >= FIRST_FOREIGN_TEAM));
                foreign.extend(others);
            }
            let n = foreign.len();
            foreign.sort();
            foreign.dedup();
            assert_eq!(foreign.len(), n, "{}", path.display());
        }
    }

    #[test]
    fn real_saves_dfb_round_completes() {
        let rt = Runtime::default();
        for (path, mut s) in save_states() {
            let round = s.event_state[0];
            if !(1..5).contains(&round) {
                continue;
            }
            let n2 = usize::from(round_pairings(round)) * 2;
            let mut winners: Vec<u8> = slots(&s, 0)[..n2].iter().step_by(2).copied().collect();
            for s2 in (0..n2).step_by(2) {
                goals_mut(&mut s, 0)[s2..s2 + 2].copy_from_slice(&[1, 0]);
            }
            let start = Rng::new(3);
            let mut rng = start;
            cup_round_complete(&mut s, &rt, &mut rng, 0, &mut |_| false);
            assert_eq!(s.event_state[0], round + 1, "{}", path.display());
            let mut drawn = slots(&s, 0)[..n2 / 2].to_vec();
            drawn.sort();
            winners.sort();
            assert_eq!(drawn, winners, "{}", path.display());
            assert_eq!(draws(start, rng), 40);
        }
    }

    #[test]
    fn real_saves_league_shuffle_keeps_managers_with_their_clubs() {
        let rt = Runtime::default();
        for (path, before) in save_states() {
            let mut s = before.clone();
            league_shuffle_teams(&mut s, &rt, &mut Rng::new(29));
            for (m, old) in s.active_managers().iter().zip(before.active_managers()) {
                if usize::from(old.team) < 64 {
                    assert_eq!(
                        s.clubs[usize::from(m.team)].name,
                        before.clubs[usize::from(old.team)].name,
                        "{}",
                        path.display()
                    );
                }
            }
            for t in 0..3 {
                let n = usize::from(rt.tier_team_count[t] as u8);
                let (mut a, mut b) = (
                    s.league_teams[t][..n].to_vec(),
                    before.league_teams[t][..n].to_vec(),
                );
                a.sort();
                b.sort();
                assert_eq!(a, b);
            }
        }
    }
}
