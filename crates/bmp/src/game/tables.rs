//! The screens behind the office main icon "Tabellen" (segment 3a41): league
//! table, fixtures and results, strength table and the opponent info they all
//! open. Spec: `docs/re/logic/stats.md` §2.
//!
//! The table logic itself (round pairings, the table sort) lives in
//! [`bmp_game::league`]; this module ports the drawing and input handling and
//! the small text formatters of the segment.

use bmp_game::league::{
    LEAGUE_FIRST_TEAM, LEAGUE_ROUND_COUNT, LEAGUE_TEAM_COUNT, NO_MATCH, NOT_PLAYED, POINTS_PER_WIN,
    RoundMode, fixtures_build_round_pairings, league_apply_round_and_sort,
};
use bmp_game::rng::{NumberFormat, format_long_thousands, long_abs_diff};
use bmp_game::state::GameState;

use super::season::calendar_compute_date;
use super::widgets::{TextButtons, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, append_ordinal_suffix, text_to_upper_de};
use crate::gfx::{
    Gfx, Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, Run};

/// `UI[42..44]` (`DS:2388`): league names.
pub(crate) const LEAGUE_NAMES: [&[u8]; 3] = [b"Bundesliga", b"Zweite Liga", b"Am.-Oberliga"];
/// `LBL[75..77]` (`DS:4b94`): league table modes.
const TABLE_MODES: [&[u8]; 3] = [b"HEIM", b"GESAMT", b"AUSW[RTS"];
/// `LBL[78]`: league table column header.
const TABLE_HEADER: &[u8] = b"PL.     VEREIN                           BILANZ         SP       S-U-N      PUNKTE     TORE      TORDIFF.";
/// `LBL[79]`, `LBL[80]`.
const TABLE_TITLE: &[u8] = b"Tabelle ";
const MATCHDAY: &[u8] = b"SPIELTAG";
/// `DS:5538`: league table mode -> opponent info side.
const MODE_SIDE: [u8; 3] = [0, 2, 1];
/// `DS:226d`: European places marked in the Bundesliga table (rows 2..).
const EURO_PLACES: i16 = 4;
/// `DS:2265 + L`: promotion places marked in the table of league `L` (`L` > 0).
const PROMOTION_PLACES: [i16; 3] = [0, 3, 4];
/// `DS:2266`: relegation places per league.
const RELEGATION_PLACES: [i16; 3] = [3, 4, 4];

/// `LBL[70..73]`.
const FIXTURES_GAMES: &[u8] = b"SPIELE  ";
const FIXTURES_MATCHDAY: &[u8] = b"SPIELTAG  ";
const FIXTURES_RESCHEDULED: &[u8] = b"NACHHOLSPIELE  ";
const FIXTURES_MINUTE: &[u8] = b"MINUTE";
/// `W[2]`: suffix of a rescheduled match date.
const RESCHEDULED_SUFFIX: &[u8] = b" (N)";
/// `LBL[74]`.
const PLACE_STRENGTH: &[u8] = b"PLATZ, ST[RKE ";

/// `LBL[81]`, `LBL[82..85]` (`DS:4bb0`), `LBL[86]`.
const STRENGTH_HEADER: &[u8] = b"PL.     VEREIN               KONDITION   TECHNIK    FORM";
const STRENGTH_PARTS: [&[u8]; 4] = [b"GESAMT", b"ABWEHR", b"MITTELFELD", b"STURM"];
const STRENGTH_TITLE: &[u8] = b"ST[RKEN ";

/// `UI[408]`, `LBL[43..69]`: opponent info texts.
const INFO_EXIT: &[u8] = b"REICHT MIR";
const INFO_TITLE: &[u8] = b"Info ]ber ";
const INFO_FOREIGN: [&[u8]; 3] = [
    b"IHRE SP[HER K|NNEN LEIDER NUR",
    b"AUF DEUTSCHEM BODEN OPERIEREN...",
    b"WILL SAGEN: KEINE INFORMATIONEN VERF]GBAR.",
];
const INFO_FIXTURES: &[u8] = b"RESTPROGRAMM";
const INFO_SHOW: &[u8] = b"ANZEIGEN";
/// `DS:54dc`: colour of the "ANZEIGEN" button by bit 7 of the crest byte.
const INFO_SHOW_COLOR: [u8; 2] = [0x0a, 0x11];
const INFO_PLACE: &[u8] = b" PLATZ IN DER ";
const INFO_STRENGTH: &[u8] = b", ST[RKE: ";
const INFO_HOME_RECORD: &[u8] = b"HEIMBILANZ:";
const INFO_WINS: &[u8] = b" SIEGE, ";
const INFO_DRAWS: &[u8] = b" UNENT., ";
const INFO_LOSSES: &[u8] = b" NIED.";
const INFO_AWAY_RECORD: &[u8] = b"AUSW[RTSBILANZ:";
const INFO_LAST_GAMES: &[u8] = b"DIE LETZTEN SPIELE:";
const INFO_GOALS_PER_GAME: &[u8] = b"TORE PRO SPIEL ";
const INFO_SERIES: &[u8] = b"SERIEN ";
const INFO_IN_A_ROW: &[u8] = b" SPIELE IN FOLGE ";
const INFO_NO_SERIES: &[u8] = b"KEINE BEMERKENSWERTE SERIE VORHANDEN.";
const INFO_HOME_TAG: &[u8] = b" (H)";
const INFO_AWAY_TAG: &[u8] = b" (A)";
const INFO_HISTORY: &[u8] = b"HISTORISCHE ERGEBNISSE GEGEN ";
const INFO_HISTORY_HOME: &[u8] = b"H: ";
const INFO_HISTORY_NONE: &[u8] = b" NOCH KEINE";
const INFO_HISTORY_AWAY: &[u8] = b" -- A: ";
const INFO_ETERNAL: &[u8] = b"PUNKTE IN DER EWIGEN TABELLE: ";
const INFO_ETERNAL_NONE: &[u8] = b"NOCH KEINEN EINZIGEN...";
/// What a NULL far pointer reads as a string under DOSBox (the first bytes of
/// the interrupt vector table at 0000:0000 up to the first NUL).
const NULL_POINTER_TEXT: &[u8] = b"~";
/// `LBL[26..28]`: where a streak happened (total, home, away); used only by the
/// English game option (`DS:224e != DS:2252`), after the category.
const SERIES_PLACE: [&[u8]; 3] = [b" ", b"ZUHAUSE ", b"AUSW[RTS "];
/// `LBL[29..35]`: streak categories.
const SERIES_KIND: [&[u8]; 7] = [
    b"GEWONNEN",
    b"VERLOREN",
    b"UNENTSCHIEDEN",
    b"NICHT GEWONNEN",
    b"NICHT VERLOREN",
    b"OHNE GEGENTOR",
    b"OHNE TORERFOLG",
];
/// `LBL[36..38]`: column suffix by side (home, away, total).
const SIDE_SUFFIX: [&[u8]; 3] = [b"(HEIM):  ", b"(AUSW[RTS):  ", b"(GESAMT):  "];
/// `DS:54d8`: side -> streak column.
const SIDE_COLUMN: [usize; 3] = [1, 2, 0];
/// `UI[101..108]`: record labels of slots 0..7.
const RECORD_LABELS: [&[u8]; 8] = [
    b"H|CHSTER H-SIEG",
    b"H|CHSTE H-NIEDERL.",
    b"ERZIELTE H-TORE",
    b"KASSIERTE H-TORE",
    b"H|CHSTER A-SIEG",
    b"H|CHSTE A-NIEDERL.",
    b"ERZIELTE A-TORE",
    b"KASSIERTE A-TORE",
];
/// `LBL[39..42]`: record labels of the combined view (slots 4..7).
const RECORD_LABELS_BOTH: [&[u8]; 4] = [
    b"H|CHSTER SIEG",
    b"H|CHSTE NIEDERL.",
    b"ERZIELTE TORE",
    b"KASSIERTE TORE",
];
/// `LBL[69]`.
const NO_RECORD: &[u8] = b"NOCH KEINE EINTRAGUNG.";

pub(crate) fn num(fmt: NumberFormat, v: i32) -> Vec<u8> {
    format_long_thousands(v, fmt)
}

pub(crate) fn cat(buf: &mut Vec<u8>, s: &[u8]) {
    buf.extend_from_slice(s);
}

pub(crate) fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: u8) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, i16::from(color));
}

pub(crate) fn centered(
    host: &mut Host,
    text: &[u8],
    x0: i16,
    x1: i16,
    y: i16,
    color: u8,
    shadow: u8,
) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

pub(crate) fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

pub(crate) fn line(host: &mut Host, x0: i16, y0: u8, x1: i16, y1: u8, color: u8) {
    ui_draw_line(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

pub(crate) fn icon(host: &mut Host, x: i16, y: i16, code: i16) -> [i16; 4] {
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, y, 0, code, false, 8)
        .1
}

pub(crate) fn panel(host: &mut Host, x: i16, y: i16, w: i16, h: i16, save: bool) -> (u8, [i16; 4]) {
    if save {
        // The saved background is a bitmap from vga_load_picture, which also
        // resets the field width DS:079c to 1.
        host.number_pad = 1;
    }
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, y, w, h, save, 8)
}

/// True when one of the human managers runs club `t`.
fn is_human_team(state: &GameState, t: u8) -> bool {
    let n = usize::from(state.manager_count).min(4);
    state.managers[..n].iter().any(|m| m.team == t)
}

/// Text colour of a club's row: 0xb for human teams, else 1.
fn team_color(state: &GameState, t: u8) -> u8 {
    if is_human_team(state, t) { 0xb } else { 1 }
}

pub(crate) fn club_name(state: &GameState, t: u8) -> Vec<u8> {
    state.clubs[usize::from(t)].name().to_vec()
}

/// One column of a [`bmp_game::state::TeamTable`] counter as the table modes
/// sum it: 0 = home, 1 = home + away, 2 = away.
fn col(v: [u8; 4], cols: u8) -> i16 {
    let home = if cols < 2 { i16::from(v[0]) } else { 0 };
    let away = if cols != 0 { i16::from(v[1]) } else { 0 };
    home + away
}

/// League tier code of a club: 1 = Bundesliga, 2 = 2. Liga, 4 = any other club.
///
/// Origin: BMMAIN.EXE 27c2:1bb5 (league_tier_of_team)
pub(crate) fn league_tier_of_team(t: u8) -> u8 {
    if t < LEAGUE_FIRST_TEAM[1] {
        1
    } else if t < LEAGUE_FIRST_TEAM[2] {
        2
    } else {
        4
    }
}

/// Index of the fixture slot (two per week) of the `n`-th week whose
/// competition bits match `mask`.
///
/// Origin: BMMAIN.EXE 1000:2895 (fixture_week_of_nth_match)
pub fn fixture_week_of_nth_match(week_competitions: &[u8; 95], mask: u8, n: i8) -> u8 {
    let mut n = n;
    let mut w: usize = 0;
    while n != 0 {
        if week_competitions[w % 95] & mask != 0 {
            n = n.wrapping_sub(1);
        }
        w += 1;
    }
    (w as u8).wrapping_sub(1)
}

/// The record of slot `s` of club `t` as text (`"x:y (OPPONENT)"`,
/// `"n (OPPONENT)"` or "NOCH KEINE EINTRAGUNG."), and the raw record byte.
/// Sets the field width `DS:079c` to 1 when a record exists.
///
/// Origin: BMMAIN.EXE 3a41:0f0e (format_team_record)
pub fn format_team_record(
    state: &GameState,
    t: u8,
    s: usize,
    fmt: &mut NumberFormat,
) -> (u8, Vec<u8>) {
    let h = &state.match_history;
    let v = h.record_scores[usize::from(t)][s];
    if v == 0 {
        return (v, NO_RECORD.to_vec());
    }
    fmt.pad_width = 1;
    let (hi, lo) = (i32::from(v >> 4), i32::from(v & 15));
    let mut out = Vec::new();
    if matches!(s, 0 | 1 | 4 | 5) {
        let (x, y) = if s <= 3 { (hi, lo) } else { (lo, hi) };
        cat(&mut out, &num(*fmt, x));
        cat(&mut out, b":");
        cat(&mut out, &num(*fmt, y));
    } else {
        let x = if hi == 0 { lo } else { hi };
        cat(&mut out, &num(*fmt, x));
    }
    cat(&mut out, b" (");
    let opp = h.record_opponents[usize::from(t)][s];
    cat(&mut out, &club_name(state, opp));
    cat(&mut out, b")");
    (v, out)
}

/// `"<pos>. (<strength>)"` or, `detailed`, `"<pos>. PLATZ, ST[RKE <s> (<d>,<m>,<a>)"`.
/// Leaves the field width `DS:079c` at 1.
///
/// Origin: BMMAIN.EXE 3a41:1d18 (format_position_strength)
pub fn format_position_strength(
    state: &GameState,
    t: u8,
    detailed: bool,
    fmt: &mut NumberFormat,
) -> Vec<u8> {
    fmt.pad_width = 2;
    let c = &state.clubs[usize::from(t)];
    let sum = |v: [u8; 3]| v.iter().map(|&b| i32::from(b)).sum::<i32>();
    let (a, b, d) = (sum(c.strength), sum(c.base_strength), sum(c.strength_1e));
    let pos = state.team_tables[usize::from(t)]
        .table_position
        .wrapping_add(1);
    let mut out = num(*fmt, i32::from(pos));
    append_ordinal_suffix(&mut out, pos, GAME_OPTION_A, GAME_OPTION_B);
    cat(&mut out, b" ");
    cat(&mut out, if detailed { PLACE_STRENGTH } else { b"(" });
    cat(&mut out, &num(*fmt, (a + b + d) / 9));
    if detailed {
        cat(&mut out, b" (");
        cat(&mut out, &num(*fmt, a / 3));
        cat(&mut out, b",");
        cat(&mut out, &num(*fmt, b / 3));
        cat(&mut out, b",");
        cat(&mut out, &num(*fmt, d / 3));
    }
    cat(&mut out, b")");
    fmt.pad_width = 1;
    out
}

/// Goals per game of club `t` as `"x.y"` (or `"x.y:x.y"` for `what == 2`);
/// `side` 0 = home, 1 = away, 2 = both, `what` 0 = scored, 1 = conceded,
/// 2 = both. Leaves the field width `DS:079c` at 2.
///
/// Origin: BMMAIN.EXE 362f:21e1 (format_goals_per_game)
pub fn format_goals_per_game(
    state: &GameState,
    t: u8,
    side: u8,
    what: u8,
    fmt: &mut NumberFormat,
) -> Vec<u8> {
    let tt = &state.team_tables[usize::from(t)];
    let pick = |v: [u8; 4]| -> u16 {
        if side < 2 {
            u16::from(v[usize::from(side)])
        } else {
            u16::from(v[0]) + u16::from(v[1])
        }
    };
    let (lo, hi) = if what == 2 { (0, 2) } else { (what, what + 1) };
    let mut out = Vec::new();
    for w in lo..hi {
        let g = pick(tt.played);
        let mut v: i32 = 0;
        if g != 0 {
            let goals = pick(if w != 0 {
                tt.goals_against
            } else {
                tt.goals_for
            });
            v = i32::from(goals) * 10 / i32::from(g);
        }
        fmt.pad_width = 1;
        cat(&mut out, &num(*fmt, v / 10));
        cat(&mut out, b".");
        cat(&mut out, &num(*fmt, v % 10));
        fmt.pad_width = 2;
        if what == 2 && w == 0 {
            cat(&mut out, b":");
        }
    }
    out
}

/// The numbers string of one league table row (form, games, wins/draws/
/// losses, points, goals, goal difference) for the columns `cols`.
///
/// Origin: BMMAIN.EXE 3a41:214c (screen_league_table, row text at 3a41:2731)
pub fn league_table_row_text(
    state: &GameState,
    t: u8,
    cols: u8,
    fmt: &mut NumberFormat,
) -> Vec<u8> {
    let tt = &state.team_tables[usize::from(t)];
    let mut out = bmp_game::state::c_str(&tt.form[0]).to_vec();
    cat(&mut out, b"   ");
    fmt.pad_width = 2;
    let games = col(tt.played, cols);
    cat(&mut out, &num(*fmt, i32::from(games)));
    cat(&mut out, b"     ");
    let wins = col(tt.wins, cols);
    cat(&mut out, &num(*fmt, i32::from(wins)));
    cat(&mut out, b" ");
    let losses = col(tt.losses, cols);
    cat(&mut out, &num(*fmt, i32::from(games - wins - losses)));
    cat(&mut out, b" ");
    cat(&mut out, &num(*fmt, i32::from(losses)));
    cat(&mut out, b"    ");
    let points = col(tt.points, cols);
    if POINTS_PER_WIN > 2 {
        cat(&mut out, b"     ");
    }
    cat(&mut out, &num(*fmt, i32::from(points)));
    if POINTS_PER_WIN < 3 {
        cat(&mut out, b":");
        cat(&mut out, &num(*fmt, i32::from(2 * games - points)));
    } else {
        cat(&mut out, b" ");
    }
    cat(&mut out, b"    ");
    let gf = col(tt.goals_for, cols);
    fmt.pad_width = 3;
    cat(&mut out, &num(*fmt, i32::from(gf)));
    cat(&mut out, b":");
    let ga = col(tt.goals_against, cols);
    cat(&mut out, &num(*fmt, i32::from(ga)));
    cat(&mut out, b"      ");
    fmt.pad_width = 2;
    let diff = gf - ga;
    cat(
        &mut out,
        match diff.signum() {
            0 => b"#",
            1 => b"+",
            _ => b"-",
        },
    );
    cat(&mut out, &num(*fmt, long_abs_diff(0, i32::from(diff))));
    out
}

/// Club order of the strength table: the teams of `league` (0-based index
/// into the league) sorted by the summed values of the parts `part` selects
/// (0 = all, 1..3 = defence, midfield, attack), best first.
///
/// Origin: BMMAIN.EXE 3a41:3891 (screen_strength_table, sort at 3a41:3acc)
pub fn strength_order(state: &GameState, league: usize, part: u8) -> Vec<u8> {
    let (start, end) = part_range(part);
    let n = usize::from(LEAGUE_TEAM_COUNT[league]);
    let first = usize::from(LEAGUE_FIRST_TEAM[league]);
    let score: Vec<i16> = (0..n)
        .map(|k| {
            let c = &state.clubs[first + k];
            (start..=end)
                .map(|j| {
                    i16::from(c.strength_1e[j])
                        + i16::from(c.strength[j])
                        + i16::from(c.base_strength[j])
                })
                .sum()
        })
        .collect();
    let mut ord: Vec<u8> = (0..n as u8).collect();
    for i in 0..n.saturating_sub(1) {
        for j in i + 1..n {
            if score[usize::from(ord[j])] > score[usize::from(ord[i])] {
                ord.swap(i, j);
            }
        }
    }
    ord
}

/// Inclusive part range of a strength-table mode.
fn part_range(part: u8) -> (usize, usize) {
    if part == 0 {
        (0, 2)
    } else {
        (usize::from(part) - 1, usize::from(part) - 1)
    }
}

/// Average of a club value over the parts of `part`, clamped to 0..99.
fn part_average(v: [u8; 3], part: u8) -> i32 {
    let (start, end) = part_range(part);
    let n = if part != 0 { 1 } else { 3 };
    let sum: i16 = (start..=end).map(|j| i16::from(v[j])).sum();
    i32::from((sum / n).clamp(0, 0x63))
}

/// `"H: g:g,g:g -- A: ..."`: the last results of manager `m` against club `t`.
///
/// Origin: BMMAIN.EXE 3a41:000e (screen_opponent_info, history at 3a41:0b7e)
pub fn head_to_head_text(state: &GameState, m: usize, t: u8, fmt: NumberFormat) -> Vec<u8> {
    let log = &state.match_history.recent_results[m][usize::from(t)];
    let mut out = INFO_HISTORY_HOME.to_vec();
    for pass in 0..2usize {
        // The first group reads the side-1 bytes, the second the side-0 bytes.
        let side = 1 - pass;
        for (k, entry) in log.iter().enumerate() {
            let e = entry[side];
            if e == 0xff {
                continue;
            }
            if k != 0 {
                cat(&mut out, b",");
            }
            cat(&mut out, &num(fmt, i32::from(e >> 4)));
            cat(&mut out, b":");
            cat(&mut out, &num(fmt, i32::from(e & 15)));
        }
        if pass == 0 {
            if out.len() == 3 {
                cat(&mut out, INFO_HISTORY_NONE);
            }
            cat(&mut out, INFO_HISTORY_AWAY);
        }
    }
    if out.last() == Some(&b' ') {
        cat(&mut out, INFO_HISTORY_NONE);
    }
    out
}

/// Runs `league_apply_round_and_sort(league, 1, cols)` with the round counter
/// clamped to the last round, as the league table does around every sort.
fn sort_table_clamped(state: &mut GameState, session: &mut Session, league: usize, cols: u8) {
    let saved = state.competition_rounds[league];
    let last = LEAGUE_ROUND_COUNT[league] as i8;
    let clamp = last <= saved as i8;
    if clamp {
        state.competition_rounds[league] = (last - 1) as u8;
    }
    // DS:224d == 0 in the shipped data: the games played are a sort key.
    league_apply_round_and_sort(
        state,
        &mut session.rng,
        league,
        RoundMode::Snapshot,
        cols,
        false,
    );
    if clamp {
        state.competition_rounds[league] = saved;
    }
}

/// The league table of `league` with the home / total / away modes; a click
/// on a row opens the opponent info. `arg` is 1 from the office: the exit
/// icon is 0x22 and the shown round is the last one played.
///
/// Origin: BMMAIN.EXE 3a41:214c (screen_league_table)
pub fn screen_league_table(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    league: usize,
    arg: u8,
) -> Run<()> {
    let mut redraw = true;
    let mut cols: u8 = 1;
    let mut title = TABLE_TITLE.to_vec();
    cat(&mut title, LEAGUE_NAMES[league]);
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    host.set_font(1);
    centered(host, &title, 0, 0x10c, 0xdc, 0xb, 0);
    let frame = panel(host, 5, 8, 0x134, 0xb4, false).1;
    let exit = icon(host, 0x10c, 0xc1, 0x22 + i16::from(arg < 1));
    let rects = [frame, exit];
    line(host, 7, 0x19, 0x137, 0x19, 1);
    line(host, 7, 0xa9, 0x137, 0xa9, 1);
    host.set_font(2);
    text_at(host, TABLE_HEADER, 9, 0x17, 1);
    let teams = i16::from(LEAGUE_TEAM_COUNT[league]);
    loop {
        if redraw {
            host.screen_fade(0)?;
            let mut buttons = TextButtons::default();
            for i in 0..3i16 {
                let color = if i16::from(cols) == i { 0x12 } else { 1 };
                buttons.register(host, i * 0x66 + 0x1e, 0xae, TABLE_MODES[i as usize], color);
            }
            host.number_pad = 1;
            host.set_font(1);
            fill(host, 6, 9, 0x137, 0x11, 8);
            let mut head = TABLE_TITLE.to_vec();
            cat(&mut head, TABLE_MODES[usize::from(cols)]);
            cat(&mut head, b"   ");
            let cur = state.competition_rounds[league] as i8;
            let back = i8::from(arg == 1 && cur > 1);
            let round = cur.wrapping_sub(back);
            cat(&mut head, &num(host.number_format(), i32::from(round)));
            append_ordinal_suffix(&mut head, round as u8, GAME_OPTION_A, GAME_OPTION_B);
            cat(&mut head, MATCHDAY);
            centered(host, &head, 5, 0x139, 0x10, 1, 7);
            host.set_font(2);
            sort_table_clamped(state, session, league, cols);
            fill(host, 9, 0x1a, 0x137, 0xa8, 8);
            if league == 0 {
                fill(host, 9, 0x1c, 0x13, 0x22, 0xf);
                fill(host, 9, 0x23, 0x13, 7 * EURO_PLACES + 0x22, 0xc);
            }
            let rel = RELEGATION_PLACES[league];
            fill(host, 9, 7 * (teams - rel + 4), 0x13, 7 * teams + 0x1b, 0x11);
            if league != 0 {
                fill(
                    host,
                    9,
                    0x1c,
                    0x13,
                    7 * PROMOTION_PLACES[league] + 0x1b,
                    0xc,
                );
            }
            for p in 0..teams {
                let y = p * 7 + 0x21;
                let t = state.league_teams[league][p as usize];
                let color = team_color(state, t);
                host.number_pad = 2;
                let mut pos = num(host.number_format(), i32::from(p + 1));
                cat(&mut pos, b".");
                text_at(host, &pos, 9, y, 0x1d);
                let name = club_name(state, t);
                text_at(host, &name, 0x15, y, color);
                let mut fmt = host.number_format();
                let row = league_table_row_text(state, t, cols, &mut fmt);
                host.number_pad = fmt.pad_width;
                text_at(host, &row, 0x67, y, color);
            }
            redraw = false;
        }
        host.screen_fade(1)?;
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let si = Ui::hit_test_buttons(&rects, 2, mx, my, Some(ev as i16));
        if si == 1 && my > 0xa9 {
            let mode = if mx < 0x87 {
                0
            } else if mx < 0xd7 {
                1
            } else {
                2
            };
            if cols != mode {
                redraw = true;
                cols = mode;
            }
        }
        if si == 1 && my < 0xa9 && my > 0x1a {
            let row = (my - 0x1a) / 7;
            if row >= 0 && row < teams {
                let t = state.league_teams[league][row as usize];
                let side = MODE_SIDE[usize::from(cols)];
                screen_opponent_info(host, state, t, side, false, u8::from(arg == 0))?;
            }
        }
        if si == 2 || session.auto_play {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    sort_table_clamped(state, session, league, 1);
    Ok(())
}

/// The fixtures and results of `league`, one round at a time; the arrow
/// icons page through the rounds and a click on a club opens its info.
/// `mode` 0 is the office view (2 = the live minute display of the match
/// day), `by_round` 0 lists the rescheduled matches instead of a round.
///
/// Origin: BMMAIN.EXE 3a41:120a (screen_fixtures)
pub fn screen_fixtures(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    league: usize,
    mode: u8,
    arg: u8,
    by_round: bool,
) -> Run<()> {
    if mode != 0 {
        // TODO(#25): manager_ai_weekly 1f9d:0002 for every manager (match-day views).
    }
    let mut redraw = true;
    let mut count: i16 = 2;
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    let mut rects = [[0i16; 4]; 4];
    rects[0] = panel(host, 1, 2, 0x13a, 0xb9, false).1;
    rects[1] = icon(host, 0x10c, 0xc1, 0x22 + i16::from(mode == 2));
    if mode == 0 {
        rects[2] = icon(host, 1, 0xc1, 0x28);
        rects[3] = icon(host, 0x3b, 0xc1, 0x27);
        count += 2;
    }
    line(host, 3, 0xe, 0x139, 0xe, 0xb);
    let rounds = LEAGUE_ROUND_COUNT[league] as i8;
    let mut round = state.competition_rounds[league] as i8;
    if round > rounds {
        round = 1;
    }
    let teams = i16::from(LEAGUE_TEAM_COUNT[league]);
    loop {
        if redraw {
            fill(host, 6, 3, 0x13a, 0xd, 8);
            let mut head;
            if by_round {
                head = FIXTURES_GAMES.to_vec();
                cat(&mut head, LEAGUE_NAMES[league]);
                cat(&mut head, b"   ");
                cat(&mut head, &num(host.number_format(), i32::from(round)));
                append_ordinal_suffix(&mut head, round as u8, GAME_OPTION_A, GAME_OPTION_B);
                cat(&mut head, FIXTURES_MATCHDAY);
                if mode != 2 {
                    let mut w = u16::from(fixture_week_of_nth_match(
                        &state.week_competitions,
                        1 << league,
                        round,
                    ));
                    if w == 0 {
                        w = 0x78;
                    }
                    calendar_compute_date(state, w);
                    append_date(host, state, &mut head);
                }
            } else {
                head = FIXTURES_RESCHEDULED.to_vec();
            }
            if mode == 2 {
                host.number_pad = 1;
                cat(&mut head, b"  ");
                cat(&mut head, &num(host.number_format(), i32::from(arg)));
                append_ordinal_suffix(&mut head, arg, GAME_OPTION_A, GAME_OPTION_B);
                cat(&mut head, FIXTURES_MINUTE);
            }
            host.set_font(1);
            centered(host, &head, 7, 0x139, 0xb, 1, 7);
            if by_round {
                fixtures_build_round_pairings(state, league, round as u8);
            }
            fill(host, 3, 0xf, 0x13a, 0xb9, 8);
            draw_fixture_rows(host, state, league, round, mode, teams);
            redraw = false;
        }
        host.screen_fade(1)?;
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let si = Ui::hit_test_buttons(&rects, count, mx, my, Some(ev as i16));
        if mode == 0 && si > 2 {
            if si == 4 {
                round += 1;
                if rounds < round {
                    round = 1;
                }
                ui_wait_right_click_button(host, 0x3b, 0xc1, session.auto_play)?;
            } else {
                round -= 1;
                if round == 0 {
                    round = rounds;
                }
                ui_wait_right_click_button(host, 1, 0xc1, session.auto_play)?;
            }
            redraw = true;
        }
        if si == 1 && my > 0xe {
            let side = i16::from(mx > 0x8c);
            let pair = (my - 0xe) / 0x11 * 2 + side;
            if pair >= 0 && pair < teams {
                let flag = u8::from(mode == 2 && arg == 0x5a);
                let saved = state.matchday_pairings;
                let t = state.matchday_pairings[league][(pair / 2) as usize][(pair % 2) as usize];
                screen_opponent_info(host, state, t, side as u8, true, flag)?;
                state.matchday_pairings = saved;
            }
        }
        if si == 2 || session.auto_play {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    if by_round {
        let cur = state.competition_rounds[league];
        fixtures_build_round_pairings(state, league, cur);
    }
    if mode != 0 {
        // TODO(#25): manager_ai_weekly 1f9d:0002 for every manager.
    }
    Ok(())
}

/// Appends `"<day>.<month>."` of the date globals.
fn append_date(host: &Host, state: &GameState, buf: &mut Vec<u8>) {
    let fmt = host.number_format();
    cat(buf, &num(fmt, state.day_of_month));
    cat(buf, b".");
    cat(buf, &num(fmt, state.month + 1));
    cat(buf, b".");
}

/// The pairs of the fixtures screen with positions, strengths and results.
///
/// Origin: BMMAIN.EXE 3a41:120a (screen_fixtures, rows at 3a41:1623)
fn draw_fixture_rows(
    host: &mut Host,
    state: &mut GameState,
    league: usize,
    round: i8,
    mode: u8,
    teams: i16,
) {
    for si in (0..teams).step_by(2) {
        let y = (si >> 1) * 0x11 + 0x1a;
        let [h, a] = state.matchday_pairings[league][(si / 2) as usize];
        if h == NO_MATCH {
            break;
        }
        let color = if is_human_team(state, h) || is_human_team(state, a) {
            0xb
        } else {
            1
        };
        let name = club_name(state, h);
        text_at(host, &name, 3, y - 2, color);
        Gfx::rastport_move_to(&mut host.rp, 0x88, y - 2);
        host.g.rastport_draw_text(&host.rp, b"-");
        let name = club_name(state, a);
        text_at(host, &name, 0x92, y - 2, color);
        host.set_font(2);
        let mut fmt = host.number_format();
        let s = format_position_strength(state, h, true, &mut fmt);
        text_at(host, &s, 5, y + 5, 4);
        let s = format_position_strength(state, a, true, &mut fmt);
        host.number_pad = fmt.pad_width;
        text_at(host, &s, 0x94, y + 5, 4);
        host.set_font(1);
        let res = round_result(state, league, round, si as usize);
        if res[0] == NOT_PLAYED {
            let found = state.rescheduled_matches.iter().copied().find(|e| {
                i16::from(e[2]) == i16::from(round)
                    && usize::from(e[3]) == league
                    && i16::from(e[4]) * 2 == si
            });
            if let Some(e) = found {
                calendar_compute_date(state, u16::from(e[1]));
                host.number_pad = 2;
                let mut buf = Vec::new();
                append_date(host, state, &mut buf);
                cat(&mut buf, RESCHEDULED_SUFFIX);
                host.set_font(2);
                centered(host, &buf, 0x128, 0x128, y - 3, color, 0xff);
                host.set_font(1);
                host.number_pad = 1;
            }
            continue;
        }
        let shown = i16::from(state.competition_rounds[league] as i8) + i16::from(mode == 2);
        if i16::from(round) < shown {
            let fmt = host.number_format();
            Gfx::rastport_set_color(&mut host.rp, color);
            Gfx::rastport_move_to(&mut host.rp, 0x120, y);
            host.g
                .rastport_draw_text(&host.rp, &num(fmt, i32::from(res[0] as i8)));
            Gfx::rastport_move_to(&mut host.rp, 0x129, y);
            host.g.rastport_draw_text(&host.rp, b":");
            Gfx::rastport_move_to(&mut host.rp, 0x12e, y);
            host.g
                .rastport_draw_text(&host.rp, &num(fmt, i32::from(res[1] as i8)));
        } else {
            text_at(host, b"-:-", 0x120, y, color);
        }
    }
}

/// Result bytes `[home, away]` of pair offset `si` in the 1-based `round` of
/// `league`, read like `FAR:6dc8 + 20 * (38 * league + round) + si` from the
/// flat result table.
fn round_result(state: &GameState, league: usize, round: i8, si: usize) -> [u8; 2] {
    let flat = (league * 38) as isize + isize::from(round) - 1;
    let Ok(flat) = usize::try_from(flat) else {
        return [0, 0];
    };
    state
        .league_results
        .get(flat / 38)
        .map_or([0, 0], |l| [l[flat % 38][si], l[flat % 38][si + 1]])
}

/// The strength table of `league`; a click below the list cycles the parts
/// (all, defence, midfield, attack), a click on a row opens the club info.
///
/// Origin: BMMAIN.EXE 3a41:3891 (screen_strength_table)
pub fn screen_strength_table(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    league: usize,
) -> Run<()> {
    let mut part: u8 = 0;
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    let frame = panel(host, 0x3d, 0x11, 0xbb, 0xb2, false).1;
    let exit = icon(host, 0x10c, 0xc1, 0x22);
    let rects = [frame, exit];
    line(host, 0x3f, 0x23, 0xf6, 0x23, 1);
    line(host, 0x3f, 0xb2, 0xf6, 0xb2, 1);
    host.set_font(1);
    let mut title = STRENGTH_TITLE.to_vec();
    cat(&mut title, LEAGUE_NAMES[league]);
    centered(host, &title, 0x3d, 0xf8, 0x19, 1, 7);
    host.set_font(2);
    text_at(host, STRENGTH_HEADER, 0x41, 0x21, 1);
    let teams = usize::from(LEAGUE_TEAM_COUNT[league]);
    let first = LEAGUE_FIRST_TEAM[league];
    let mut redraw = true;
    let mut ord = Vec::new();
    loop {
        if redraw {
            let mut buttons = TextButtons::default();
            buttons.register(host, 0x85, 0xb5, STRENGTH_PARTS[usize::from(part)], 0x12);
            fill(host, 0x3e, 0x24, 0xf6, 0xb0, 8);
            ord = strength_order(state, league, part);
            host.number_pad = 2;
            for (row, &k) in ord.iter().enumerate() {
                let t = first + k;
                let color = team_color(state, t);
                let fmt = host.number_format();
                let y = (row as i16 + 6) * 7;
                let mut buf = num(fmt, row as i32 + 1);
                cat(&mut buf, b". ");
                cat(&mut buf, &club_name(state, t));
                text_at(host, &buf, 0x41, y, color);
                let c = &state.clubs[usize::from(t)];
                let mut buf = num(fmt, part_average(c.strength, part));
                cat(&mut buf, b"            ");
                cat(&mut buf, &num(fmt, part_average(c.base_strength, part)));
                cat(&mut buf, b"            ");
                cat(&mut buf, &num(fmt, part_average(c.strength_1e, part)));
                text_at(host, &buf, 0xa2, y, color);
                redraw = false;
            }
        }
        host.screen_fade(1)?;
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let si = Ui::hit_test_buttons(&rects, 2, mx, my, Some(ev as i16));
        if si == 1 && my > 0xb1 {
            redraw = true;
            part = (part + 1) % 4;
        }
        if si == 1 && my < 0xb2 && my > 0x25 {
            let row = (my - 0x25) / 7;
            if row >= 0 && (row as usize) < teams {
                let t = first + ord[row as usize];
                screen_opponent_info(host, state, t, 2, false, 0)?;
            }
        }
        if si == 2 {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)
}

/// The info window about club `t` over the current screen: place, home and
/// away record, form, goals per game, streaks, records, the manager's
/// results against the club and its eternal-table points. `side` 0 shows the
/// home view, 1 the away view, 2 both; `from_list` shifts the window by two
/// pixels. The window's background is restored on exit.
///
/// Origin: BMMAIN.EXE 3a41:000e (screen_opponent_info)
pub fn screen_opponent_info(
    host: &mut Host,
    state: &mut GameState,
    t: u8,
    side: u8,
    from_list: bool,
    _arg: u8,
) -> Run<()> {
    let x0: i16 = 0x59 + 2 * i16::from(from_list);
    let y0: i16 = 0x37;
    let (slot, _) = panel(host, x0, y0, 0xe0, 0xae, true);
    let mut buttons;
    let mut count;
    'draw: loop {
        host.set_font(1);
        let mut title = INFO_TITLE.to_vec();
        cat(&mut title, &club_name(state, t));
        centered(host, &title, x0, x0 + 0xe0, y0 + 8, 0xb, 0);
        line(
            host,
            x0 + 1,
            (y0 + 0xa) as u8,
            x0 + 0xdf,
            (y0 + 0xa) as u8,
            0xb,
        );
        buttons = TextButtons::default();
        host.set_font(2);
        buttons.register(host, x0 + 4, y0 + 0x9f, INFO_EXIT, 0xa);
        count = 2;
        if t > 0x3f {
            for (text, dy) in INFO_FOREIGN.iter().zip([0x44, 0x4c, 0x56]) {
                centered(host, text, x0, x0 + 0xe0, y0 + dy, 1, 0);
            }
            count = 1;
        } else {
            buttons.register(host, x0 + 0x54, y0 + 0x9f, INFO_FIXTURES, 0xa);
        }
        let show = INFO_SHOW_COLOR[usize::from(state.clubs[usize::from(t)].crest >> 7)];
        buttons.register(host, x0 + 0xa4, y0 + 0x9f, INFO_SHOW, show);
        count += 1;
        if t <= 0x3f {
            draw_opponent_info_body(host, state, t, side, x0, y0);
        }
        loop {
            let ev = host.input_poll_mouse()?;
            let (mx, my) = host.mouse_xy();
            let mut hit = buttons.hit(mx, my);
            if hit > count {
                hit = 0;
            }
            if ev == LEFT_DOWN && buttons.count() == hit {
                buttons.drop_last();
                let crest = &mut state.clubs[usize::from(t)].crest;
                *crest ^= 0x80;
                let show = INFO_SHOW_COLOR[usize::from(*crest >> 7)];
                buttons.register(host, x0 + 0xa4, y0 + 0x9f, INFO_SHOW, show);
            }
            if ev == LEFT_DOWN && hit == 2 && buttons.count() != 2 {
                // TODO(#15): screen_team_fixtures 1000:28c4 ("RESTPROGRAMM",
                // finance.md §3.10); the original redraws the info unless it
                // returns 1.
                continue 'draw;
            }
            if ev == LEFT_DOWN && hit == 1 {
                break 'draw;
            }
        }
    }
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, slot, 0);
    Ok(())
}

/// The text lines of the opponent info of a German club.
///
/// Origin: BMMAIN.EXE 3a41:000e (screen_opponent_info, 3a41:0271..0e04)
fn draw_opponent_info_body(host: &mut Host, state: &GameState, t: u8, side: u8, x0: i16, y0: i16) {
    let ti = usize::from(t);
    let tt = state.team_tables[ti].clone();
    let fmt = host.number_format();
    let pos = tt.table_position.wrapping_add(1);
    let mut buf = num(fmt, i32::from(pos));
    append_ordinal_suffix(&mut buf, pos, GAME_OPTION_A, GAME_OPTION_B);
    cat(&mut buf, INFO_PLACE);
    let tier = usize::from(league_tier_of_team(t) >> 1);
    cat(&mut buf, &text_to_upper_de(LEAGUE_NAMES[tier]));
    cat(&mut buf, INFO_STRENGTH);
    let c = &state.clubs[ti];
    let total: i16 = (0..3)
        .map(|j| {
            i16::from(c.strength_1e[j]) + i16::from(c.base_strength[j]) + i16::from(c.strength[j])
        })
        .sum();
    cat(&mut buf, &num(fmt, i32::from(total / 9)));
    centered(host, &buf, x0, x0 + 0xe0, y0 + 0x12, 1, 0);
    let mut y = y0 + 0x1c;
    for (label, s) in [(INFO_HOME_RECORD, 0usize), (INFO_AWAY_RECORD, 1)] {
        text_at(host, label, x0 + 3, y, 1);
        let mut buf = num(fmt, i32::from(tt.wins[s]));
        cat(&mut buf, INFO_WINS);
        let draws = i32::from(tt.played[s]) - i32::from(tt.wins[s]) - i32::from(tt.losses[s]);
        cat(&mut buf, &num(fmt, draws));
        cat(&mut buf, INFO_DRAWS);
        cat(&mut buf, &num(fmt, i32::from(tt.losses[s])));
        cat(&mut buf, INFO_LOSSES);
        text_at(host, &buf, x0 + 0x6b, y, 1);
        y += 8;
    }
    text_at(host, INFO_LAST_GAMES, x0 + 3, y, 1);
    for i in 0..8i16 {
        let ch = [tt.form[0][i as usize]];
        let ch: &[u8] = if ch[0] == 0 { b"" } else { &ch };
        text_at(host, ch, 7 * i + x0 + 0x6b, y, 1);
    }
    y += 8;
    let mut buf = INFO_GOALS_PER_GAME.to_vec();
    cat(&mut buf, SIDE_SUFFIX[usize::from(side)]);
    text_at(host, &buf, x0 + 3, y, 1);
    let mut f = host.number_format();
    let gpg = format_goals_per_game(state, t, side, 2, &mut f);
    host.number_pad = f.pad_width;
    text_at(host, &gpg, x0 + 0x6b, y, 1);
    let mut buf = INFO_SERIES.to_vec();
    cat(&mut buf, SIDE_SUFFIX[usize::from(side)]);
    y += 8;
    text_at(host, &buf, x0 + 3, y, 1);
    y += 8;
    let first_y = y as u8;
    let fmt = host.number_format();
    let column = SIDE_COLUMN[usize::from(side)];
    let mut lines = 0;
    for (k, kind) in SERIES_KIND.iter().enumerate() {
        let n = state.match_history.team_series[ti][k][column];
        if lines >= 4 || n <= 1 {
            continue;
        }
        let mut buf = num(fmt, i32::from(n));
        cat(&mut buf, INFO_IN_A_ROW);
        if GAME_OPTION_A == GAME_OPTION_B {
            // The place comes from the far pointer table FAR:2f2c, which is never
            // written: the null pointer reads the interrupt vector table at
            // 0000:0000, a one-glyph string under DOSBox.
            cat(&mut buf, NULL_POINTER_TEXT);
            cat(&mut buf, kind);
        } else {
            cat(&mut buf, kind);
            cat(&mut buf, SERIES_PLACE[column]);
        }
        text_at(host, &buf, x0 + 3, y, 1);
        y += 8;
        lines += 1;
    }
    if first_y as i16 == y {
        text_at(host, INFO_NO_SERIES, x0 + 0xa, y, 2);
        y += 8;
    }
    y += 2;
    for s in 0..8usize {
        let wanted = if side == 0 { s < 4 } else { s > 3 };
        if !wanted {
            continue;
        }
        let label = if side == 2 {
            RECORD_LABELS_BOTH[s - 4]
        } else {
            RECORD_LABELS[s]
        };
        text_at(host, label, x0 + 3, y, 1);
        let mut f = host.number_format();
        let text = if side != 2 {
            format_team_record(state, t, s, &mut f).1
        } else {
            let (home, mut home_text) = format_team_record(state, t, s - 4, &mut f);
            let (away, away_text) = format_team_record(state, t, s, &mut f);
            if home == 0 && away == 0 {
                home_text
            } else {
                let (mut h, mut a) = (i16::from(home), i16::from(away));
                if s < 6 {
                    a = (a >> 4) - (a & 0xf);
                    h = (h >> 4) - (h & 0xf);
                }
                if a < h {
                    cat(&mut home_text, INFO_HOME_TAG);
                    home_text
                } else {
                    let mut t = away_text;
                    cat(&mut t, INFO_AWAY_TAG);
                    t
                }
            }
        };
        host.number_pad = f.pad_width;
        text_at(host, &text, x0 + 0x6b, y, 1);
        y += 8;
    }
    y += 2;
    let cur = (state.current_manager as usize).min(3);
    let own = state.managers[cur].team;
    if own != t {
        let mut buf = INFO_HISTORY.to_vec();
        cat(&mut buf, &club_name(state, own));
        text_at(host, &buf, x0 + 3, y, 1);
        y += 8;
        let buf = head_to_head_text(state, cur, t, host.number_format());
        text_at(host, &buf, x0 + 3, y, 1);
        y += 0xa;
    }
    let mut buf = INFO_ETERNAL.to_vec();
    let color = if tt.prestige != 0 {
        cat(&mut buf, &num(host.number_format(), tt.prestige));
        1
    } else {
        cat(&mut buf, INFO_ETERNAL_NONE);
        2
    };
    text_at(host, &buf, x0 + 3, y, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_game::state::GameState;

    fn fmt() -> NumberFormat {
        NumberFormat {
            no_separators: true,
            pad_width: 1,
            ..Default::default()
        }
    }

    fn state() -> Box<GameState> {
        let mut s = Box::new(GameState::zeroed());
        for (i, c) in s.clubs.iter_mut().enumerate() {
            c.name = [0; 23];
            let n = format!("CLUB{i}");
            c.name[..n.len()].copy_from_slice(n.as_bytes());
        }
        s
    }

    #[test]
    fn week_of_nth_match_counts_matching_weeks() {
        let mut wc = [0u8; 95];
        wc[3] = 1;
        wc[5] = 3;
        wc[8] = 2;
        assert_eq!(fixture_week_of_nth_match(&wc, 1, 1), 3);
        assert_eq!(fixture_week_of_nth_match(&wc, 1, 2), 5);
        assert_eq!(fixture_week_of_nth_match(&wc, 2, 2), 8);
    }

    #[test]
    fn team_records_format_results_and_counts() {
        let mut s = state();
        let h = &mut s.match_history;
        h.record_scores[3][0] = 0x51;
        h.record_opponents[3][0] = 7;
        h.record_scores[3][4] = 0x13;
        h.record_opponents[3][4] = 8;
        h.record_scores[3][3] = 0x04;
        h.record_opponents[3][3] = 9;
        let mut f = fmt();
        assert_eq!(format_team_record(&s, 3, 0, &mut f).1, b"5:1 (CLUB7)");
        assert_eq!(format_team_record(&s, 3, 4, &mut f).1, b"3:1 (CLUB8)");
        assert_eq!(format_team_record(&s, 3, 3, &mut f).1, b"4 (CLUB9)");
        assert_eq!(
            format_team_record(&s, 3, 1, &mut f),
            (0, NO_RECORD.to_vec())
        );
    }

    #[test]
    fn position_strength_detailed_and_short() {
        let mut s = state();
        s.team_tables[2].table_position = 4;
        let c = &mut s.clubs[2];
        c.strength = [50, 60, 70];
        c.base_strength = [40, 40, 40];
        c.strength_1e = [30, 33, 36];
        let mut f = fmt();
        assert_eq!(
            format_position_strength(&s, 2, true, &mut f),
            b"^5. PLATZ, ST[RKE 44 (60,40,33)"
        );
        assert_eq!(f.pad_width, 1);
        assert_eq!(format_position_strength(&s, 2, false, &mut f), b"^5. (44)");
    }

    #[test]
    fn goals_per_game_in_tenths() {
        let mut s = state();
        let t = &mut s.team_tables[1];
        t.played = [4, 3, 0, 0];
        t.goals_for = [9, 2, 0, 0];
        t.goals_against = [3, 6, 0, 0];
        let mut f = fmt();
        assert_eq!(format_goals_per_game(&s, 1, 0, 2, &mut f), b"2.2:0.7");
        assert_eq!(format_goals_per_game(&s, 1, 2, 2, &mut f), b"1.5:1.2");
        assert_eq!(format_goals_per_game(&s, 1, 1, 1, &mut f), b"2.0");
        assert_eq!(f.pad_width, 2);
    }

    #[test]
    fn league_row_text_uses_the_selected_columns() {
        let mut s = state();
        let t = &mut s.team_tables[0];
        t.form[0][..8].copy_from_slice(b"sSuUnN@@");
        t.played = [3, 2, 0, 0];
        t.wins = [2, 0, 0, 0];
        t.losses = [0, 1, 0, 0];
        t.points = [5, 1, 0, 0];
        t.goals_for = [7, 1, 0, 0];
        t.goals_against = [2, 3, 0, 0];
        let mut f = fmt();
        assert_eq!(
            league_table_row_text(&s, 0, 1, &mut f),
            b"sSuUnN@@   ^5     ^2 ^2 ^1    ^6:^4    ^^8:^^5      +^3"
        );
        assert_eq!(
            league_table_row_text(&s, 0, 2, &mut f),
            b"sSuUnN@@   ^2     ^0 ^1 ^1    ^1:^3    ^^1:^^3      -^2"
        );
    }

    #[test]
    fn strength_order_sorts_by_the_selected_parts() {
        let mut s = state();
        for k in 0..18 {
            s.clubs[k].strength = [k as u8, 0, 0];
        }
        s.clubs[3].strength = [0, 0, 90];
        let all = strength_order(&s, 0, 0);
        assert_eq!(all[0], 3);
        assert_eq!(all[1], 17);
        let defence = strength_order(&s, 0, 1);
        assert_eq!(defence[0], 17);
        assert!(defence[16..].contains(&0) && defence[16..].contains(&3));
        assert_eq!(part_average([10, 20, 31], 0), 20);
        assert_eq!(part_average([10, 200, 31], 2), 99);
    }

    #[test]
    fn head_to_head_lists_both_sides() {
        let mut s = state();
        let log = &mut s.match_history.recent_results[0][5];
        for e in log.iter_mut() {
            *e = [0xff, 0xff];
        }
        assert_eq!(
            head_to_head_text(&s, 0, 5, fmt()),
            b"H:  NOCH KEINE -- A:  NOCH KEINE"
        );
        let log = &mut s.match_history.recent_results[0][5];
        log[0] = [0x21, 0x03];
        log[1] = [0xff, 0x11];
        assert_eq!(head_to_head_text(&s, 0, 5, fmt()), b"H: 0:3,1:1 -- A: 2:1");
    }
}
