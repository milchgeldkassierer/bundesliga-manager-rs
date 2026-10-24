//! Player statistics of `BMMAIN.EXE` segment 24a4: the goal scorers of the
//! clubs no human manages, the top-scorer list, the yearly reshuffle of the
//! player pool, the stadium attraction and the advertising offers. Also the
//! club table loader of segment 362f.
//!
//! Full documentation with addresses: `docs/re/logic/stats.md` §3.2, §3.3,
//! §3.5, §3.6, §3.8 and §4.6.

use bmp_formats::mana::ManaDat;
use bmp_formats::savegame::{BLOCK_PLAYERS, BLOCK_SQUADS, BLOCKS};

use crate::league::{
    LEAGUE_ROUND_COUNT, LEAGUE_TEAM_COUNT, NO_MATCH, NOT_PLAYED, league_team_ids, round_results,
};
use crate::rng::{Rng, clamp_int, ldiv, lmul};
use crate::runtime::Runtime;
use crate::state::{
    CLUB_COUNT, GERMAN_CLUBS, GameState, Layout, PLAYER_COUNT, Player, SquadPlayer, strcpy,
};
use crate::transfer::player_value_of;

/// DS:5331 scorer selection weights, indexed by the candidate count.
const SCORER_WEIGHTS: [u8; 11] = [0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 46];
/// Rows of the top-scorer list.
const TOP_SCORER_ROWS: usize = 20;
/// DS:54c9 (and its copy DS:53c7): form string of a club that has not played yet.
pub(crate) const FORM_PLACEHOLDER: &[u8] = b"@@@@@@@@";
/// FAR address of the player table ([`GameState::players`]).
const PLAYERS_FAR: usize = BLOCKS[BLOCK_PLAYERS].offset as usize;
/// FAR address of the squad table ([`GameState::squads`]).
const SQUADS_FAR: usize = BLOCKS[BLOCK_SQUADS].offset as usize;

/// League goals this season (+0x22).
fn goals(p: &Player) -> u8 {
    p.counters[0]
}

/// League games this season (+0x23).
fn games(p: &Player) -> u8 {
    p.counters[1]
}

/// Distributes the goals of the last round of `league` over the pool players
/// of the clubs that no human manages, home club before away club, pair by
/// pair. A round counter beyond the season reads round 1.
///
/// Origin: BMMAIN.EXE 24a4:1662 (matchday_assign_ai_scorers)
pub fn matchday_assign_ai_scorers(state: &mut GameState, rng: &mut Rng, league: usize) {
    let mut r = state.competition_rounds[league];
    if (LEAGUE_ROUND_COUNT[league] as i8 as i16) < r as i16 {
        r = 1;
    }
    // TODO(re): for league 0 and round counter 0 the original reads the 20
    // bytes before FAR:6ddc as results; no goals are assigned here.
    let Some(results) = round_results(state, league, r.wrapping_sub(1) as i8) else {
        return;
    };
    for i in (0..LEAGUE_TEAM_COUNT[league] as usize).step_by(2) {
        let (gh, ga) = (results[i], results[i + 1]);
        // The end marker is tested on the result byte here, not the team id.
        if gh == NO_MATCH {
            break;
        }
        if gh == NOT_PLAYED {
            continue;
        }
        let [h, a] = state.matchday_pairings[league][i / 2];
        match_assign_ai_scorers(state, rng, h, gh);
        match_assign_ai_scorers(state, rng, a, ga);
    }
}

/// Distributes `goals` of `team` over its pool players, unless a human
/// manages the team. Each player of the team plays with chance 94/101 (one
/// league game more) and becomes a candidate when a talent-weighted draw
/// succeeds. With fewer than two candidates no goal is credited; otherwise
/// each goal goes to a random candidate or, with some chance, to nobody.
///
/// Origin: BMMAIN.EXE 24a4:14d4 (match_assign_ai_scorers)
pub fn match_assign_ai_scorers(state: &mut GameState, rng: &mut Rng, team: u8, goals: u8) {
    if goals == 0 || state.manager_index_of_team(team, 0xff) != 0 {
        return;
    }
    let mut cand = [0u8; PLAYER_COUNT];
    let mut n: u8 = 0;
    for p in 1..PLAYER_COUNT {
        let player = &mut state.players[p];
        if player.team != team {
            continue;
        }
        if rng.random_range(0, 100) < 94 {
            player.counters[1] = player.counters[1].wrapping_add(1);
            let x = rng.random_range(10, 99);
            let y = rng.random_range(5, 25) + player.position_value as i16;
            if y > x {
                cand[n as usize] = p as u8;
                n += 1;
            }
        }
    }
    if n < 2 {
        return;
    }
    for _ in 0..goals {
        let mut k = if n < 10 {
            SCORER_WEIGHTS[n as usize] - 1
        } else {
            10
        };
        if k > 2 {
            k -= 1;
        }
        if rng.random_range(0, 4) == 0 {
            continue;
        }
        loop {
            if rng.random_range(0, k as i16) != 0 {
                let idx = rng.random_range(0, n as i16 - 1) as u8;
                let scorer = &mut state.players[cand[idx as usize] as usize];
                scorer.counters[0] = scorer.counters[0].wrapping_add(1);
                break;
            }
            if rng.random_range(0, 1) + 2 >= n as i16 {
                break;
            }
        }
    }
}

/// Copies the goal counters of every human squad into the player pool and
/// returns the pool players 1..=150 ordered by goals (most first), ties by
/// games (fewer first). Element 0 is unused.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, data part before the
/// display)
pub fn top_scorers_order(state: &mut GameState) -> [u8; PLAYER_COUNT] {
    for m in 0..state.active_managers().len() {
        let count = state.squad_player_count(m) as usize;
        for i in 0..count {
            let slot = &state.squads[m][i];
            let (id, goals) = (slot.player_id as usize, slot.goals[0]);
            state.players[id].counters[0] = goals;
        }
    }
    let mut ord: [u8; PLAYER_COUNT] = std::array::from_fn(|p| p as u8);
    let players = &state.players;
    for i in 1..PLAYER_COUNT {
        for j in i + 1..PLAYER_COUNT {
            let (x, y) = (&players[ord[i] as usize], &players[ord[j] as usize]);
            if goals(x) < goals(y) || (goals(x) == goals(y) && games(y) < games(x)) {
                ord.swap(i, j);
            }
        }
    }
    ord
}

/// Whether the top scorer of the current manager's league plays for the
/// manager's club: the first player of the league with at least two goals in
/// [`top_scorers_order`] (which also copies the squads' goals into the pool).
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, `no_display != 0`)
pub fn top_scorer_plays_for_current_manager(state: &mut GameState) -> bool {
    let order = top_scorers_order(state);
    let m = current_manager(state);
    let (tier, team) = (usize::from(m.league), m.team);
    top_scorers_rows(state, &order, tier, false)
        .first()
        .is_some_and(|r| state.players[usize::from(r.player)].team == team)
}

/// One row of the top-scorer list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopScorerRow {
    /// 1-based rank among all scorers of the tier, also in the human-teams
    /// list.
    pub rank: u8,
    /// Index into [`GameState::players`].
    pub player: u8,
}

/// The rows of the top-scorer list of league `tier`, built from
/// [`top_scorers_order`]: the players of the tier's clubs with at least two
/// goals, at most 20 rows. With `human_teams_only` ("SPIELER" view) only the
/// players of the human managers' clubs are listed, keeping their rank in the
/// whole tier.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, list loop)
pub fn top_scorers_rows(
    state: &GameState,
    order: &[u8; PLAYER_COUNT],
    tier: usize,
    human_teams_only: bool,
) -> Vec<TopScorerRow> {
    let teams = league_team_ids(tier);
    let managers = state.active_managers();
    let mut rows = Vec::new();
    let mut rank: u8 = 0;
    for &p in &order[1..] {
        let player = &state.players[p as usize];
        if !teams.contains(&player.team) {
            continue;
        }
        if goals(player) < 2 {
            break;
        }
        rank = rank.wrapping_add(1);
        let human = managers.iter().any(|m| m.team == player.team);
        if rows.len() < TOP_SCORER_ROWS && (!human_teams_only || human) {
            rows.push(TopScorerRow { rank, player: p });
        }
    }
    rows
}

/// Goals per game in tenths (`goals * 10 / games`, unsigned, `goals * 10`
/// without games), shown as `"<q / 10>.<q % 10>"`.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, TORE/SPIEL column)
pub fn goals_per_game_tenths(player: &Player) -> u16 {
    let q = goals(player) as u16 * 10;
    match games(player) {
        0 => q,
        g => q / g as u16,
    }
}

/// Fills the club table from MANA.DAT: crests and an empty form string for the
/// 64 German clubs; strength, base strength, a random third strength part
/// (5 lower for clubs above 64) and a random morale for all 200 clubs; the
/// names; and the league order lists `0..18`, `18..38`, `38..58`.
///
/// RNG: per club `random_range(45, 55)` for each of the three parts, then one
/// for the morale (800 draws). `main` calls it before the first seed, so the
/// draws run on the generator's power-on state.
///
/// Origin: BMMAIN.EXE 362f:36ec (mana_load_club_table)
pub fn mana_load_club_table(state: &mut GameState, rt: &Runtime, rng: &mut Rng, mana: &ManaDat) {
    for i in 0..GERMAN_CLUBS {
        state.clubs[i].crest = mana.crest_index(i);
        strcpy(&mut state.team_tables[i].form[0], FORM_PLACEHOLDER);
    }
    for i in 0..CLUB_COUNT {
        let club = &mut state.clubs[i];
        for j in 0..3 {
            let v = mana.strength[i][j];
            club.base_strength[j] = v;
            club.strength[j] = v;
            let lower = if i > 0x40 { 5 } else { 0 };
            club.strength_1e[j] = (rng.random_range(0x2d, 0x37) as u8).wrapping_sub(lower);
        }
        club.morale = rng.random_range(0x2d, 0x37) as u8;
    }
    for i in 0..CLUB_COUNT {
        strcpy(&mut state.clubs[i].name, &mana.clubs[i].name);
    }
    let mut next = 0u8;
    for (t, row) in state.league_teams.iter_mut().enumerate() {
        for slot in &mut row[..rt.tier_team_count[t] as usize] {
            *slot = next;
            next = next.wrapping_add(1);
        }
    }
}

/// Moves a skill by -3..=+3 and clamps it to 30..=99.
///
/// Origin: BMMAIN.EXE 24a4:174e (player_skill_drift)
pub fn player_skill_drift(rng: &mut Rng, v: &mut u8) {
    let mut x = i16::from(*v) + rng.random_range(1, 7) - 4;
    clamp_int(&mut x, 30, 99);
    *v = x as u8;
}

/// A random club no human manager runs whose strength suits a player of
/// `value` (the player's value in 10 000s): a German club, or a foreign one
/// with `foreign`. The club's strength is `(sum of strength and base
/// strength + 150) / 9`; it suits when `strength - 2 < value < strength +
/// 20`. After 500 tries any free club is taken.
///
/// The original's first argument, the player, is not read.
///
/// Origin: BMMAIN.EXE 24a4:24bf (pick_team_for_player)
pub fn pick_team_for_player(state: &GameState, rng: &mut Rng, value: u8, foreign: bool) -> u8 {
    let (lo, hi) = if foreign { (0x40, 0xc7) } else { (0, 0x3f) };
    let value = i16::from(value);
    let mut tries: i16 = 0;
    loop {
        let t = rng.random_range(lo, hi) as u8;
        if state.manager_index_of_team(t, 0xff) == 0 {
            let club = &state.clubs[t as usize];
            let s = (0..3).fold(0i16, |acc, j| {
                acc + i16::from(club.strength[j]) + i16::from(club.base_strength[j]) + 50
            }) / 9;
            if tries > 500 || (value < s + 20 && value + 2 > s) {
                return t;
            }
        }
        tries += 1;
    }
}

/// A club index of a foreign club (not German, not "no club").
fn is_foreign(team: u8) -> bool {
    team > 0x3f && team != 0xff
}

/// The value byte [`pick_team_for_player`] gets for pool player `p`:
/// `player_value_calc(p, 0) / 10000`. The original passes the pool index as a
/// squad slot, so the value is that of record `FAR:304a * 25 + p` of the flat
/// squad table, which runs past the 125 squad records into the following far
/// data; the age is read the same way, through the record's player id.
/// `player_value_calc` sign-extends the slot (`CBW`), so a pool index of 128 or
/// more is a negative slot and reads a record *before* the squad table (in the
/// player table); the record offset is 16-bit and wraps inside the segment.
/// The far data is read as it stands at the call: a record in the player table
/// overlaps club bytes (`P[q][0x24]`) that earlier moves of the same reshuffle
/// have already rewritten.
///
/// Origin: BMMAIN.EXE 3277:25de (player_value_calc, record address 3277:25eb..2603)
fn pool_value_byte(state: &GameState, rng: &mut Rng, p: u8) -> u8 {
    let img = state.far_data_image();
    let at = pool_record_offset(state.current_manager, p);
    let read = |at: u16, len: usize| -> Vec<u8> {
        (0..len)
            .map(|k| img[usize::from(at.wrapping_add(k as u16))])
            .collect()
    };
    let rec = SquadPlayer::from_bytes(&read(at, SquadPlayer::LEN)).unwrap();
    let at = (PLAYERS_FAR as u16)
        .wrapping_add(u16::from(rec.player_id).wrapping_mul(Player::LEN as u16));
    let age = Player::from_bytes(&read(at, Player::LEN)).unwrap().age;
    ldiv(player_value_of(rng, &rec, age, 0), 10000) as u8
}

/// Far offset of the squad record `player_value_calc` reads for slot `slot` of
/// manager `FAR:304a = cm`: `(25 * (i8)cm + (i8)slot) * 0x34 + 0x774a`, all
/// 16-bit (`IMUL byte`, `CBW`, `IMUL CX`, `ADD AX,0x774a`).
///
/// Origin: BMMAIN.EXE 3277:25de (player_value_calc, 3277:25eb..2603)
fn pool_record_offset(cm: i8, slot: u8) -> u16 {
    let index = i16::from(cm)
        .wrapping_mul(25)
        .wrapping_add(i16::from(slot as i8));
    (index.wrapping_mul(SquadPlayer::LEN as i16) as u16).wrapping_add(SQUADS_FAR as u16)
}

/// The yearly reshuffle of the player pool: every player's two skills drift,
/// 5..=15 random players of clubs no human runs get a new German club (while
/// more than four players play abroad, the drawn foreigners go back to a
/// German club instead), and the result is the number of free players each
/// league tier may take: the tier's share of the human squads applied to the
/// players neither abroad nor in a human squad.
///
/// RNG: 2 per player (skill drift), the move count, then per move the player
/// draws, the value draws of [`player_value_calc`](crate::transfer::player_value_calc)
/// and the club draws of [`pick_team_for_player`].
///
/// Origin: BMMAIN.EXE 24a4:1798 (player_pool_season_shuffle)
pub fn player_pool_season_shuffle(state: &mut GameState, rng: &mut Rng) -> [i16; 3] {
    let mut per_tier = [0u8; 3];
    let mut total = 0u8;
    let mut foreign = 0u8;
    for (tier, n) in per_tier.iter_mut().enumerate() {
        for m in 0..state.active_managers().len() {
            if usize::from(state.managers[m].league) == tier {
                *n = n.wrapping_add(state.squad_player_count(m));
            }
        }
        total = total.wrapping_add(*n);
    }
    for player in &mut state.players[1..] {
        player_skill_drift(rng, &mut player.skills[0]);
        player_skill_drift(rng, &mut player.skills[1]);
        if is_foreign(player.team) {
            foreign += 1;
        }
    }
    let moves = rng.random_range(5, 15) as u8;
    for _ in 0..moves {
        let p = loop {
            let mut p = rng.random_range(1, 150) as u8;
            let old = state.players[p as usize].team;
            if foreign > 4 && is_foreign(old) {
                foreign -= 1;
                let v = pool_value_byte(state, rng, p);
                state.players[p as usize].team = pick_team_for_player(state, rng, v, false);
                p = 0;
            }
            if state.manager_index_of_team(old, 0xff) == 0 && p != 0 && foreign <= 4 {
                break p;
            }
        };
        let v = pool_value_byte(state, rng, p);
        state.players[p as usize].team = pick_team_for_player(state, rng, v, false);
    }
    for player in &state.players[1..] {
        if is_foreign(player.team) {
            total = total.wrapping_add(1);
        }
    }
    let rest = i32::from(150u8.wrapping_sub(total));
    let total = i32::from(total.max(1));
    per_tier.map(|n| ldiv(lmul(ldiv(lmul(i32::from(n), 100), total), rest), 100) as i16)
}

/// How attractive the current manager's stadium is to advertisers, 0..=100:
/// the places (covered places count half), capped at 80 000, plus the
/// weighted stadium extras, capped at 100 000, in thousands.
///
/// Origin: BMMAIN.EXE 24a4:2bba (stadium_attraction)
pub fn stadium_attraction(state: &GameState) -> i32 {
    let st = &current_manager(state).stadium;
    let s = |k: usize| st[k][0];
    let mut v = (s(2) >> 1).wrapping_add(s(1)).wrapping_add(s(0));
    if v > 80_000 {
        v = 80_000;
    }
    let extras = lmul(s(6).wrapping_mul(2).wrapping_add(s(5)), 1000).wrapping_add(lmul(s(3), 1500));
    v = v.wrapping_add(lmul(s(4), 1900).wrapping_add(extras));
    if v > 100_000 {
        v = 100_000;
    }
    ldiv(v, 1000)
}

/// The human manager `FAR:304a`.
fn current_manager(state: &GameState) -> &crate::state::Manager {
    state.current().expect("FAR:304a is not a human manager")
}

/// Draws the current manager's 2 x 10 advertising offers: the amounts
/// `far_002e` (`long`, 0 = no interest) and the contract lengths `far_2ec2`
/// (1..=3 seasons) of offer kind `k` (0, 1) and offer `i` at index
/// `(2 * m + k) * 10 + i`. Better stadiums, more fans, a higher league and
/// the difficulty raise the amounts; later offers are less likely to come.
///
/// RNG per offer: `random_range(15, 20)`, the base amount, the interest
/// draw, then either the refusal draw (when the interest draw is high) or
/// the fan factor, the length and the two spreads.
///
/// Origin: BMMAIN.EXE 24a4:2cb4 (advertising_offers_generate)
pub fn advertising_offers_generate(state: &mut GameState, rng: &mut Rng) {
    let m = current_manager(state);
    let km = i32::from(m.fans);
    let tier2 = 2i16.wrapping_sub(i16::from(m.league));
    let mi = state.current_manager as usize;
    let level = i32::from(state.level as i8);
    for k in 0..2usize {
        for i in 0..10i16 {
            let idx = (2 * mi + k) * 10 + i as usize;
            let offer = advertising_offer(state, rng, km, tier2, level, k, i);
            let (amount, years) = offer.unwrap_or((0, state.far_2ec2[idx]));
            state.far_002e[4 * idx..4 * idx + 4].copy_from_slice(&amount.to_le_bytes());
            state.far_2ec2[idx] = years;
        }
    }
}

/// One offer of [`advertising_offers_generate`]: amount and length, `None`
/// when the advertiser has no interest (the length stays).
fn advertising_offer(
    state: &GameState,
    rng: &mut Rng,
    km: i32,
    tier2: i16,
    level: i32,
    k: usize,
    i: i16,
) -> Option<(i32, u8)> {
    let mut s = stadium_attraction(state);
    if s < 50 && km > 29 {
        s += 20;
    }
    let r1 = rng.random_range(15, 20);
    let mut base = (s >> 1).wrapping_add(50);
    let thr = base.wrapping_add(i32::from(r1)).wrapping_sub(50);
    let q = (ldiv(base, 5) as i16).wrapping_add(2 * i);
    let mut o = i32::from(rng.random_range(
        q.wrapping_mul(11).wrapping_add(0x3a),
        q.wrapping_mul(12).wrapping_add(0x3e),
    ));
    if k == 0 {
        base = lmul(base, 5);
    }
    let p = ((i * 7) / 10 + 3) * 3;
    let v = 10 * i + rng.random_range(3, 20);
    if i32::from(v) >= thr && rng.random_range(0, p) != 0 {
        return None;
    }
    let r = rng.random_range(
        (ldiv(km, 7) as i16).wrapping_add(100),
        (ldiv(km, 5) as i16).wrapping_add(100),
    );
    o = ldiv(lmul(lmul(o, base), i32::from(r)), 100);
    let d = rng.random_range(0, 2);
    if tier2 == 1 {
        o = ldiv(lmul(o, 107), 100);
    }
    let spread = rng.random_range((d + 9) * 10, d * 15 + 92);
    o = ldiv(lmul(o, i32::from(spread)), 100);
    let spread = rng.random_range(
        tier2.wrapping_add(5).wrapping_mul(20),
        tier2.wrapping_mul(23).wrapping_add(102),
    );
    o = ldiv(lmul(o, i32::from(spread)), 100)
        .wrapping_add(level * 800)
        .wrapping_add(16500);
    if k == 0 {
        o = o.wrapping_add(level * 2000 - 11000);
    }
    Some((o, d as u8 + 1))
}

/// The injury names `UI[53..71]`, by injury type ([`SquadPlayer::injury`]).
pub const INJURY_NAMES: [&[u8]; 18] = [
    b"Oberschenkelzerrung",
    b"Prellung d. Kn|chels",
    b"Platzwunde am Kopf",
    b"Stauchung d. Fu~gelenkes",
    b"Sprunggelenkprellung",
    b"B{nderdehnung",
    b"Bauchmuskelzerrung",
    b"Bluterguss am Knie",
    b"Nasenbeinbruch",
    b"Au~enbandanri~",
    b"Schienbeinbruch",
    b"Muskelfaserri~",
    b"Meniskusverletzung",
    b"Innenbandabri~",
    b"Wadenbeinbruch",
    b"Kapselri~",
    b"Leistenbruch",
    b"Kreuzbandri~",
];
/// DS:05bc: the least number of games each injury type lasts.
const INJURY_GAMES: [u8; 18] = [1, 2, 1, 2, 2, 4, 2, 1, 1, 6, 12, 5, 7, 10, 11, 9, 20, 14];

/// Injures the player of record `slot` of squad `group`: his third skill drops by 12..=19
/// when above 20, a random injury type (a serious one, with chance 1/2 and when it lasts
/// longer than a draw of 2..=4 games, is drawn again) keeps him out for its games plus up to
/// a quarter of them and one more, and he leaves the lineup. Returns `" (<injury>)"`.
///
/// Origin: BMMAIN.EXE 24a4:30cf (player_injure)
pub fn player_injure(state: &mut GameState, rng: &mut Rng, group: usize, slot: usize) -> Vec<u8> {
    let p = &mut state.players[usize::from(state.squads[group][slot].player_id)];
    if p.skills[2] > 20 {
        p.skills[2] = p.skills[2].wrapping_sub(rng.random_range(12, 19) as u8);
    }
    let mut kind = rng.random_range(0, 17) as usize;
    if rng.random_range(0, 1) != 0 && i16::from(INJURY_GAMES[kind]) > rng.random_range(2, 4) {
        kind = rng.random_range(0, 17) as usize;
    }
    let games = INJURY_GAMES[kind];
    let extra = rng.random_range(0, i16::from(games >> 2) + 1) as u8;
    let s = &mut state.squads[group][slot];
    s.suspended = extra.wrapping_add(games);
    s.status |= 2;
    s.injury = kind as u8;
    s.lineup_position = 0;
    [&b" ("[..], INJURY_NAMES[kind], b")"].concat()
}

/// How much a player wants to leave: his age, times 85 % for a goalkeeper or 100 % otherwise,
/// minus half of how far his second skill is above his first, in percent.
///
/// Origin: BMMAIN.EXE 24a4:2588 (player_value_percent)
pub fn player_value_percent(state: &GameState, s: &SquadPlayer) -> i32 {
    let p = &state.players[usize::from(s.player_id)];
    let diff = i16::from(s.skills[1]) - i16::from(s.skills[0]);
    let mut pct = 100 - 15 * i16::from(p.position_value < 25);
    if diff > 0 {
        pct -= diff >> 1;
    }
    ldiv(i32::from(p.age) * i32::from(pct), 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::save_files;
    use bmp_formats::savegame::SaveGame;

    #[test]
    fn pool_record_offset_sign_extends_the_slot() {
        // Squad record 25 + 4 of manager 1.
        assert_eq!(pool_record_offset(1, 4), 0x774a + 29 * 0x34);
        // Past the 125 squad records (slot 99): FAR:907a.
        assert_eq!(pool_record_offset(1, 99), 0x907a);
        // Pool index 138 is slot -118: record -93, inside the player table.
        assert_eq!(pool_record_offset(1, 138), 0x774a - 93 * 0x34);
        // The 16-bit offset wraps inside the segment.
        assert_eq!(
            pool_record_offset(-128, 0),
            (0x774a_i32 - 3200 * 0x34).rem_euclid(0x10000) as u16
        );
    }

    /// Writes `bytes` at far address `at` inside the player table FAR:57dd.
    fn poke_players(s: &mut GameState, at: usize, bytes: &[u8]) {
        for (k, &b) in bytes.iter().enumerate() {
            let o = at + k - PLAYERS_FAR;
            let q = o / Player::LEN;
            let mut raw = s.players[q].to_bytes();
            raw[o % Player::LEN] = b;
            s.players[q] = Player::from_bytes(&raw).unwrap();
        }
    }

    #[test]
    fn pool_value_byte_reads_a_negative_slot_before_the_squads() {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.current_manager = 1;
        let mut rng = Rng::new(1);
        let empty = pool_value_byte(&s, &mut rng, 138);
        // The record at slot -118 of manager 1 lies in the player table; make it a player
        // record of id 5 (age 24) with some skill and stamina so its value is not the empty one.
        let at = usize::from(pool_record_offset(1, 138));
        let mut rec = SquadPlayer::zeroed();
        rec.player_id = 5;
        rec.skills = [60, 60, 60];
        poke_players(&mut s, at, &rec.to_bytes());
        s.players[5].age = 24;
        let negative = pool_value_byte(&s, &mut rng, 138);
        assert_ne!(negative, empty);
        assert_eq!(
            rng,
            Rng::new(1),
            "a mode-0 value draws nothing for an unlisted record"
        );
    }

    #[test]
    fn pool_value_byte_sees_clubs_moved_earlier_in_the_reshuffle() {
        // Slot -118 of manager 1 starts 27 bytes into player 86, so the record's status
        // byte (+0x09) is player 86's club (+0x24). The reshuffle moves players one by one
        // and the original reads the live far data, so a club set by an earlier move counts.
        assert_eq!(
            usize::from(pool_record_offset(1, 138)) + 0x09,
            PLAYERS_FAR + 86 * Player::LEN + 0x24
        );
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.current_manager = 1;
        s.players[86].team = 0x10;
        let mut rng = Rng::new(7);
        pool_value_byte(&s, &mut rng, 138);
        assert_eq!(rng, Rng::new(7), "German club: not listed, no draw");
        // A foreign club >= 0x80 sets the "listed" bit 7, which draws the offer percent.
        s.players[86].team = 0xc0;
        pool_value_byte(&s, &mut rng, 138);
        assert_ne!(rng, Rng::new(7));
    }

    /// Players 1..=150 belong to no club; `team` gets the given players.
    fn pool(team: u8, members: &[usize], talent: u8) -> GameState {
        let mut s = GameState::zeroed();
        for p in &mut s.players {
            p.team = 0xff;
        }
        for &p in members {
            s.players[p].team = team;
            s.players[p].position_value = talent;
        }
        s
    }

    #[test]
    fn the_top_scorer_flag_needs_the_leagues_first_scorer() {
        // Manager 0 runs club 20 in the 2. Bundesliga; player 5 plays for it, player 6 for
        // club 21, player 7 for Bundesliga club 1.
        let flag = |own: u8, other: u8, top_league: u8| {
            let mut s = pool(0xff, &[], 0);
            s.manager_count = 1;
            s.managers[0].team = 20;
            s.managers[0].league = 1;
            for (p, team, goals) in [(5, 20, own), (6, 21, other), (7, 1, top_league)] {
                s.players[p].team = team;
                s.players[p].counters[0] = goals;
            }
            top_scorer_plays_for_current_manager(&mut s)
        };
        assert!(flag(3, 2, 9));
        assert!(!flag(3, 4, 0));
        assert!(!flag(1, 0, 0), "fewer than two goals do not count");
    }

    #[test]
    fn an_injury_keeps_the_player_out_for_its_games() {
        for seed in 0..40 {
            let mut s = GameState::zeroed();
            s.squads[1][3].player_id = 7;
            s.squads[1][3].lineup_position = 4;
            s.squads[1][3].status = 0x40;
            s.players[7].skills = [50, 50, 30];
            let mut rng = Rng::new(seed);
            let mut want = rng;
            let drop = want.random_range(12, 19) as u8;
            let mut kind = want.random_range(0, 17) as usize;
            if want.random_range(0, 1) != 0
                && i16::from(INJURY_GAMES[kind]) > want.random_range(2, 4)
            {
                kind = want.random_range(0, 17) as usize;
            }
            let games = INJURY_GAMES[kind];
            let extra = want.random_range(0, i16::from(games / 4) + 1) as u8;

            let text = player_injure(&mut s, &mut rng, 1, 3);
            assert_eq!(rng, want, "seed {seed}");
            assert_eq!(s.players[7].skills, [50, 50, 30 - drop]);
            let r = &s.squads[1][3];
            assert_eq!(
                (r.suspended, r.status, r.injury),
                (games + extra, 0x42, kind as u8)
            );
            assert!((games..=games + games / 4 + 1).contains(&r.suspended));
            assert_eq!(r.lineup_position, 0);
            assert_eq!(text, [&b" ("[..], INJURY_NAMES[kind], b")"].concat());
        }
    }

    #[test]
    fn a_weak_third_skill_is_not_lowered_by_an_injury() {
        let mut s = GameState::zeroed();
        s.squads[0][0].player_id = 1;
        s.players[1].skills = [50, 50, 20];
        player_injure(&mut s, &mut Rng::new(3), 0, 0);
        assert_eq!(s.players[1].skills[2], 20);
    }

    #[test]
    fn the_wish_to_leave_grows_with_age() {
        let mut s = GameState::zeroed();
        let mut q = SquadPlayer::zeroed();
        q.player_id = 2;
        s.players[2].age = 30;
        s.players[2].position_value = 40;
        q.skills = [60, 50, 0];
        assert_eq!(player_value_percent(&s, &q), 30);
        q.skills = [50, 61, 0]; // half of 11 rounds down: 95 %
        assert_eq!(player_value_percent(&s, &q), 28);
        s.players[2].position_value = 0; // a goalkeeper: 85 % - 5
        assert_eq!(player_value_percent(&s, &q), 24);
    }

    #[test]
    fn no_randomness_without_goals_or_for_human_clubs() {
        let mut s = pool(5, &[1, 2, 3], 80);
        let mut rng = Rng::new(3);
        match_assign_ai_scorers(&mut s, &mut rng, 5, 0);
        s.manager_count = 1;
        s.managers[0].team = 5;
        match_assign_ai_scorers(&mut s, &mut rng, 5, 4);
        assert_eq!(rng, Rng::new(3));
        assert!(s.players.iter().all(|p| p.counters == [0, 0]));
    }

    #[test]
    fn scorer_call_order_for_two_certain_candidates() {
        // Talent 255: y >= 5 + 255 > 99 >= x, so every playing player is a
        // candidate.
        let mut s = pool(7, &[10, 20], 255);
        let mut rng = Rng::new(77);
        let mut expect = rng;
        match_assign_ai_scorers(&mut s, &mut rng, 7, 1);

        let mut played = Vec::new();
        for p in [10u8, 20] {
            if expect.random_range(0, 100) < 94 {
                expect.random_range(10, 99);
                expect.random_range(5, 25);
                played.push(p);
            }
        }
        let mut scorer = None;
        // Two candidates: k = DS:5331[2] - 1 = 2; the retry test
        // `random_range(0, 1) + 2 < 2` never holds.
        if played.len() == 2 && expect.random_range(0, 4) != 0 {
            if expect.random_range(0, 2) != 0 {
                scorer = Some(played[expect.random_range(0, 1) as usize]);
            } else {
                expect.random_range(0, 1);
            }
        }
        assert_eq!(rng, expect);
        for p in [10u8, 20] {
            let c = s.players[p as usize].counters;
            assert_eq!(c[1], played.contains(&p) as u8, "games of {p}");
            assert_eq!(c[0], (scorer == Some(p)) as u8, "goals of {p}");
        }
    }

    /// Replays `match_assign_ai_scorers` for `members` of talent 255 (every
    /// playing member is a candidate) and returns the candidate count and the
    /// goals per member.
    fn replay_scorers(rng: &mut Rng, members: &[usize], goals: u8) -> (usize, Vec<u8>) {
        // k per candidate count, hand-derived: DS:5331[n] - 1 below 10
        // candidates, else 10; one less when above 2.
        const K: [i16; 11] = [0, 0, 2, 2, 3, 4, 5, 6, 7, 8, 9];
        let cand: Vec<usize> = (0..members.len())
            .filter(|_| {
                let plays = rng.random_range(0, 100) < 94;
                if plays {
                    rng.random_range(10, 99);
                    rng.random_range(5, 25);
                }
                plays
            })
            .collect();
        let n = cand.len();
        let mut scored = vec![0; members.len()];
        if n < 2 {
            return (n, scored);
        }
        let k = K[n.min(10)];
        for _ in 0..goals {
            if rng.random_range(0, 4) == 0 {
                continue;
            }
            loop {
                if rng.random_range(0, k) != 0 {
                    scored[cand[rng.random_range(0, n as i16 - 1) as usize]] += 1;
                    break;
                }
                if rng.random_range(0, 1) + 2 >= n as i16 {
                    break;
                }
            }
        }
        (n, scored)
    }

    #[test]
    fn scorer_weights_follow_the_candidate_count() {
        let mut counts = std::collections::BTreeSet::new();
        for size in [2, 3, 4, 5, 9, 10, 12, 14] {
            let members: Vec<usize> = (30..30 + size).collect();
            for seed in 0..64 {
                let mut s = pool(7, &members, 255);
                let mut rng = Rng::new(seed);
                let mut expect = rng;
                match_assign_ai_scorers(&mut s, &mut rng, 7, 6);
                let (n, scored) = replay_scorers(&mut expect, &members, 6);
                assert_eq!(rng, expect, "{size} members, seed {seed}");
                let goals: Vec<u8> = members.iter().map(|&p| s.players[p].counters[0]).collect();
                assert_eq!(goals, scored, "{size} members, seed {seed}");
                counts.insert(n);
            }
        }
        for n in [2, 3, 4, 5, 9, 10, 12, 14] {
            assert!(counts.contains(&n), "no seed with {n} candidates");
        }
    }

    #[test]
    fn a_single_candidate_never_scores() {
        let mut s = pool(7, &[10], 255);
        let mut rng = Rng::new(5);
        match_assign_ai_scorers(&mut s, &mut rng, 7, 9);
        assert_eq!(s.players[10].counters[0], 0);
    }

    #[test]
    fn scorers_never_exceed_the_goals() {
        let members: Vec<usize> = (40..56).collect();
        let mut s = pool(3, &members, 60);
        let mut rng = Rng::new(2024);
        for _ in 0..50 {
            match_assign_ai_scorers(&mut s, &mut rng, 3, 3);
        }
        let goals: u32 = s.players.iter().map(|p| p.counters[0] as u32).sum();
        assert!(goals > 0 && goals <= 150, "{goals}");
        assert!(s.players[40..56].iter().all(|p| p.counters[1] <= 50));
    }

    #[test]
    fn matchday_walks_the_last_round() {
        let mut s = pool(2, &[1, 2, 3], 255);
        for p in [4, 5, 6] {
            s.players[p].team = 3;
            s.players[p].position_value = 255;
        }
        s.competition_rounds[0] = 2;
        s.matchday_pairings[0][0] = [2, 3];
        s.league_results[0][1] = [NOT_PLAYED; 20];
        s.league_results[0][1][..2].copy_from_slice(&[2, 0]);
        let mut rng = Rng::new(8);
        let mut expect = rng;
        let mut t = s.clone();
        matchday_assign_ai_scorers(&mut s, &mut rng, 0);
        match_assign_ai_scorers(&mut t, &mut expect, 2, 2);
        match_assign_ai_scorers(&mut t, &mut expect, 3, 0);
        assert_eq!((rng, &s.players), (expect, &t.players));
        // A counter past the season reads round 1 (index 0): unplayed here.
        s.competition_rounds[0] = 35;
        s.league_results[0][0] = [NOT_PLAYED; 20];
        matchday_assign_ai_scorers(&mut s, &mut rng, 0);
        assert_eq!(rng, expect);
    }

    #[test]
    fn top_scorers_sort_by_goals_then_fewer_games() {
        let mut s = GameState::zeroed();
        for (p, goals, games) in [(3, 5, 4), (7, 5, 3), (2, 1, 1), (9, 0, 0)] {
            s.players[p].counters = [goals, games];
        }
        // A human squad's goal counter overrides the pool counter.
        s.manager_count = 1;
        s.squads[0][0].player_id = 9;
        s.squads[0][0].goals[0] = 6;
        let ord = top_scorers_order(&mut s);
        assert_eq!(ord[1..5], [9, 7, 3, 2]);
        assert_eq!(s.players[9].counters[0], 6);
    }

    #[test]
    fn top_scorer_rows_filter_by_tier_and_stop_below_two_goals() {
        let mut s = GameState::zeroed();
        let rows = [
            (1, 18, 9, 4),
            (2, 8, 8, 1),
            (3, 5, 7, 2),
            (4, 12, 2, 3),
            (5, 3, 1, 5),
        ];
        for (p, team, goals, games) in rows {
            let player = &mut s.players[p];
            (player.team, player.counters) = (team, [goals, games]);
        }
        s.players[6].team = 0xff;
        s.manager_count = 1;
        s.managers[0].team = 5;
        let ord = top_scorers_order(&mut s);
        let list = top_scorers_rows(&s, &ord, 0, false);
        // Player 1 (team 18) is in tier 1; player 5 has a single goal.
        let ranked: Vec<(u8, u8)> = list.iter().map(|r| (r.rank, r.player)).collect();
        assert_eq!(ranked, [(1, 2), (2, 3), (3, 4)]);
        let human = top_scorers_rows(&s, &ord, 0, true);
        assert_eq!(human, [TopScorerRow { rank: 2, player: 3 }]);
        assert_eq!(top_scorers_rows(&s, &ord, 1, false)[0].player, 1);
    }

    #[test]
    fn top_scorer_list_has_at_most_twenty_rows() {
        let mut s = GameState::zeroed();
        for p in 1..40 {
            s.players[p].team = 40;
            s.players[p].counters = [5, 5];
        }
        let ord = top_scorers_order(&mut s);
        let rows = top_scorers_rows(&s, &ord, 2, false);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[19].rank, 20);
    }

    #[test]
    fn goals_per_game_truncates() {
        let mut p = GameState::zeroed().players[1].clone();
        p.counters = [7, 3];
        assert_eq!(goals_per_game_tenths(&p), 23);
        p.counters = [7, 0];
        assert_eq!(goals_per_game_tenths(&p), 70);
        p.counters = [255, 1];
        assert_eq!(goals_per_game_tenths(&p), 2550);
    }

    #[test]
    fn real_saves_top_scorers() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            let mut s = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
            let ord = top_scorers_order(&mut s);
            let mut ids = ord[1..].to_vec();
            ids.sort_unstable();
            assert!(ids.iter().copied().eq(1..=150), "{path:?}");
            for w in ord[1..].windows(2) {
                let (a, b) = (&s.players[w[0] as usize], &s.players[w[1] as usize]);
                assert!(goals(a) >= goals(b), "{path:?}");
                assert!(goals(a) > goals(b) || games(a) <= games(b), "{path:?}");
            }
            // Pool players play at most one league game per round.
            let rounds = s.competition_rounds.iter().max().copied().unwrap_or(0);
            assert!(
                s.players[1..].iter().all(|p| games(p) <= rounds),
                "{path:?}"
            );
            for tier in 0..LEAGUE_TEAM_COUNT.len() {
                let rows = top_scorers_rows(&s, &ord, tier, false);
                for r in &rows {
                    let p = &s.players[r.player as usize];
                    assert!(
                        league_team_ids(tier).contains(&p.team) && goals(p) >= 2,
                        "{path:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn club_table_comes_from_mana() {
        let mana = crate::newgame::tests::synthetic_mana();
        let rt = Runtime::default();
        let mut s = GameState::zeroed();
        let mut rng = Rng::default();
        mana_load_club_table(&mut s, &rt, &mut rng, &mana);
        let mut probe = Rng::default();
        let draws: Vec<u8> = (0..800)
            .map(|_| probe.random_range(0x2d, 0x37) as u8)
            .collect();
        assert_eq!(rng, probe, "800 draws");
        assert_eq!(s.clubs[0].crest, (mana.crest[0] + 0x30) & 0x7f);
        assert_eq!(s.clubs[63].crest, mana.crest_index(63));
        assert_eq!(s.clubs[64].crest, 0);
        for i in [0, 64, 65, 199] {
            let c = &s.clubs[i];
            assert_eq!(c.strength, mana.strength[i]);
            assert_eq!(c.base_strength, mana.strength[i]);
            let lower = if i > 64 { 5 } else { 0 };
            for j in 0..3 {
                assert_eq!(c.strength_1e[j], draws[4 * i + j] - lower, "club {i}");
            }
            assert_eq!(c.morale, draws[4 * i + 3]);
            assert_eq!(c.name(), mana.clubs[i].name_bytes());
        }
        assert_eq!(&s.team_tables[63].form[0], b"@@@@@@@@\0");
        assert_eq!(s.team_tables[63].form[1], [0; 9]);
        assert!(s.league_teams[0][..18].iter().copied().eq(0..18));
        assert!(s.league_teams[1].iter().copied().eq(18..38));
        assert!(s.league_teams[2].iter().copied().eq(38..58));
        assert_eq!(&s.league_teams[0][18..], &[0, 0]);
    }

    #[test]
    fn skill_drift_moves_by_three_and_clamps() {
        let mut rng = Rng::new(11);
        for _ in 0..200 {
            let (mut hi, mut mid, mut lo) = (99u8, 60u8, 0u8);
            player_skill_drift(&mut rng, &mut hi);
            player_skill_drift(&mut rng, &mut mid);
            player_skill_drift(&mut rng, &mut lo);
            assert!((96..=99).contains(&hi));
            assert!((57..=63).contains(&mid));
            assert_eq!(lo, 30);
        }
    }

    #[test]
    fn pick_team_takes_any_free_club_after_500_tries() {
        let s = GameState::zeroed();
        // Every club has strength (0 + 0 + 150) / 9 = 16; value 0 never suits.
        let mut rng = Rng::new(5);
        let mut probe = rng;
        let t = pick_team_for_player(&s, &mut rng, 0, false);
        let mut draws = 0;
        while probe != rng {
            probe.rand();
            draws += 1;
        }
        assert_eq!(draws, 502);
        assert!(t <= 0x3f);
        // Value 20 suits strength 16 at once (14 < 20 < 36): one draw, a foreign club.
        let mut rng = Rng::new(5);
        let t = pick_team_for_player(&s, &mut rng, 20, true);
        assert!((0x40..=0xc7).contains(&t));
        let mut probe = Rng::new(5);
        probe.rand();
        assert_eq!(rng, probe);
    }

    /// Players 1..=150 at German club 30, the first 20 in squad `m` of each listed manager.
    fn shuffle_state(tiers: &[u8]) -> GameState {
        let mut s = GameState::zeroed();
        for p in &mut s.players[1..] {
            p.team = 30;
            p.skills = [50; 3];
        }
        s.manager_count = tiers.len() as u8;
        for (m, &tier) in tiers.iter().enumerate() {
            s.managers[m].league = tier;
            s.managers[m].team = 60 + m as u8;
            for k in 0..20 {
                let pid = 20 * m + k + 1;
                s.squads[m][k].player_id = pid as u8;
                s.players[pid].team = 60 + m as u8;
            }
        }
        s
    }

    #[test]
    fn pool_quota_is_the_tier_share_of_the_rest() {
        // No foreigners: total = human squad players, rest = 150 - total.
        let q =
            |tiers: &[u8]| player_pool_season_shuffle(&mut shuffle_state(tiers), &mut Rng::new(9));
        assert_eq!(q(&[]), [0, 0, 0]);
        assert_eq!(q(&[0]), [130, 0, 0]);
        assert_eq!(q(&[0, 2]), [55, 0, 55]);
        // 40 * 100 / 60 = 66, * 90 / 100 = 59; 20 * 100 / 60 = 33, * 90 / 100 = 29.
        assert_eq!(q(&[0, 0, 1]), [59, 29, 0]);
    }

    #[test]
    fn pool_shuffle_keeps_human_clubs_and_counts_foreigners() {
        for seed in 0..20 {
            let mut s = shuffle_state(&[0]);
            for p in 100..110 {
                s.players[p].team = 0x80;
            }
            let q = player_pool_season_shuffle(&mut s, &mut Rng::new(seed));
            let foreign = s.players[1..].iter().filter(|p| is_foreign(p.team)).count();
            assert!(foreign <= 10, "seed {seed}");
            // total = 20 + foreigners, rest = 150 - total.
            let total = 20 + foreign as i32;
            assert_eq!(i32::from(q[0]), 20 * 100 / total * (150 - total) / 100);
            for (i, p) in s.players.iter().enumerate().skip(1) {
                assert_eq!(p.team == 60, i <= 20, "seed {seed}: player {i}");
                assert!(!is_foreign(p.team) || (100..110).contains(&i));
            }
        }
    }

    #[test]
    fn pool_value_reads_past_the_squad_table() {
        let mut s = GameState::zeroed();
        s.players[7].age = 21;
        let mut rec = SquadPlayer::zeroed();
        rec.player_id = 7;
        rec.skills = [80, 90, 50];
        rec.value_terms = [100, 0, 0];
        // Record 25 + 116 = 141 of the flat squad table (manager 1, pool index 116) starts
        // 0xb8 bytes into FAR:9336.
        let mut h = s.match_history.to_bytes();
        h[0xb8..0xb8 + SquadPlayer::LEN].copy_from_slice(&rec.to_bytes());
        s.match_history = crate::state::MatchHistory::from_bytes(&h).unwrap();
        s.current_manager = 1;
        let mut rng = Rng::new(1);
        let v = pool_value_byte(&s, &mut rng, 116);
        let expected = ldiv(player_value_of(&mut Rng::new(1), &rec, 21, 0), 10000) as u8;
        assert_eq!(v, expected);
        assert_ne!(v, 0);
        // Inside the table the record is the squad slot itself.
        s.squads[4][10] = rec.clone();
        s.current_manager = 0;
        assert_eq!(pool_value_byte(&s, &mut rng, 110), expected);
    }

    fn stadium_state(items: [i32; 7]) -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        for (k, v) in items.into_iter().enumerate() {
            s.managers[0].stadium[k][0] = v;
        }
        s
    }

    #[test]
    fn stadium_attraction_caps_places_and_total() {
        // 301 >> 1 + 500 + 1000 = 1650.
        assert_eq!(
            stadium_attraction(&stadium_state([1000, 500, 301, 0, 0, 0, 0])),
            1
        );
        // Places capped at 80 000, then 10 * 1900 = 19 000 more.
        assert_eq!(
            stadium_attraction(&stadium_state([90_000, 0, 0, 0, 10, 0, 0])),
            99
        );
        // 80 000 + 1500 + (2 * 100 + 3) * 1000 is capped at 100 000.
        assert_eq!(
            stadium_attraction(&stadium_state([90_000, 0, 0, 1, 0, 3, 100])),
            100
        );
        // Negative sums stay negative and truncate toward zero.
        assert_eq!(
            stadium_attraction(&stadium_state([-1999, 0, 0, 0, 0, 0, 0])),
            -1
        );
    }

    #[test]
    fn advertising_offer_follows_the_formula() {
        let mut s = stadium_state([0; 7]);
        s.managers[0].fans = 50;
        s.level = 3;
        s.far_2ec2 = [0xee; 80];
        let mut rng = Rng::new(77);
        let mut p = rng;
        advertising_offers_generate(&mut s, &mut rng);
        // Offer (k = 0, i = 0): stadium 0 + 20 (fans > 29), base 60, threshold 10 + r1,
        // q = 12; the interest draw 3..=20 is always below the threshold.
        let r1 = i32::from(p.random_range(15, 20));
        let mut o = i32::from(p.random_range(190, 206));
        let v = p.random_range(3, 20);
        assert!(i32::from(v) < 10 + r1);
        let r = i32::from(p.random_range(107, 110));
        o = o * 300 * r / 100;
        let d = p.random_range(0, 2);
        o = o * i32::from(p.random_range((d + 9) * 10, d * 15 + 92)) / 100;
        o = o * i32::from(p.random_range(140, 148)) / 100 + 3 * 800 + 16500;
        o += 3 * 2000 - 11000;
        assert_eq!(&s.far_002e[..4], &o.to_le_bytes());
        assert_eq!(s.far_2ec2[0], d as u8 + 1);
        for i in 0..20 {
            let amount = i32::from_le_bytes(s.far_002e[4 * i..4 * i + 4].try_into().unwrap());
            if amount == 0 {
                assert_eq!(s.far_2ec2[i], 0xee, "offer {i} keeps its length");
            } else {
                assert!((1..=3).contains(&s.far_2ec2[i]));
            }
        }
        assert!(s.far_002e[80..].iter().all(|&b| b == 0), "only manager 0");
    }
}
