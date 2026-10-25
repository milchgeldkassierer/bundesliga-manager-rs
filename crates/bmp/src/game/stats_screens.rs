//! The read-only statistics screens behind the office main icon "Info"
//! (segment 362f): statistics, eternal table and the manager's history.
//! Spec: `docs/re/logic/stats.md` §4.2-§4.4.

use bmp_game::finance::{
    CreditField, credit_sum, finance_monthly_expenses, finance_monthly_income,
};
use bmp_game::league_layout::layout;
use bmp_game::newgame::first_season_year;
use bmp_game::rng::NumberFormat;
use bmp_game::state::{GameState, Manager};

use super::tables::{
    LEAGUE_NAMES, cat, centered, club_name, fill, format_goals_per_game, format_team_record, icon,
    line, num, panel, text_at,
};
use super::widgets::ui_wait_right_click_button;
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{Gfx, Ui, ui_draw_frame};
use crate::host::{Host, Run};

/// `DS:224d`: the "points against" variant flag; 0 in the shipped data.
const POINTS_AGAINST_HIDDEN: bool = false;

/// `UI[90..120]` (`DS:2448`): the statistics items; `'!'` marks a header.
const STAT_ITEMS: [&[u8]; 31] = [
    b"!SERIEN",
    b"GEWONNEN",
    b"VERLOREN",
    b"UNENTSCHIEDEN",
    b"NICHT GEWONNEN",
    b"NICHT VERLOREN",
    b"OHNE GEGENTOR",
    b"OHNE TORERFOLG",
    b"TORE/SPIEL",
    b"GEGENTORE/SPIEL",
    b"!REKORDE",
    b"H|CHSTER H-SIEG",
    b"H|CHSTE H-NIEDERL.",
    b"ERZIELTE H-TORE",
    b"KASSIERTE H-TORE",
    b"H|CHSTER A-SIEG",
    b"H|CHSTE A-NIEDERL.",
    b"ERZIELTE A-TORE",
    b"KASSIERTE A-TORE",
    b"!ZUSCHAUER",
    b"BESUCHERZAHL",
    b"SCHNITT/SPIEL",
    b"BEN|TIGTER SCHNITT",
    b"ZUSCHAUERREKORD: ",
    b"MINUSKULISSE: ",
    b"!FINANZEN",
    b"KONTOSTAND",
    b"SCHULDEN GESAMT",
    b"!EINNAHMEN+AUSGABEN / MONAT",
    b"EINNAHMEN (SPENDEN, SPONSOREN)",
    b"AUSGABEN (GEH[LTER, REISEKOSTEN)",
];
/// `LBL[0]`: statistics column header.
const STAT_COLUMNS: &[u8] = b"G       H       A";
/// `DS:54b2`: statistics column G/H/A -> `format_goals_per_game` side.
const STAT_COLUMN_SIDE: [u8; 3] = [2, 0, 1];
/// `DS:5485` / `DS:5482`: what `format_long_or_blank` shows for 0.
const BLANK_ZERO: &[u8] = b"^@";
const BLANK_ZERO_224D: &[u8] = b"^\xfc";
/// `DS:4a46`, `DS:4a56`.
const NONE_YET_F: &[u8] = b"(NOCH KEINE...)";
const NONE_YET_M: &[u8] = b"(NOCH KEINER...)";
/// `LBL[319]`, `LBL[324]` (empty: the currency prefix of the `DS:224d` variant).
const CURRENCY: &[u8] = b" DM";
const CURRENCY_PREFIX: &[u8] = b"";
/// `M(m)[0x1ec]` before the first home game: no lowest attendance yet.
const NO_LOWEST_ATTENDANCE: i32 = 99_999;
/// `DS:5498`: the 13 `0x0RGB` colours of the balance gauge, set into the
/// palette entries `0x1f` down to `0x13` (green .. red).
const GAUGE_PALETTE: [u16; 13] = [
    0x00c0, 0x02c0, 0x04c0, 0x06c0, 0x08c0, 0x0ac0, 0x0cc0, 0x0ca0, 0x0d80, 0x0d60, 0x0d40, 0x0d20,
    0x0d00,
];
/// The balance gauge starts at -2 375 000 DM and steps 50 000 DM per pixel.
const GAUGE_START: i32 = -2_375_000;
const GAUGE_STEP: i32 = 50_000;
/// Above this balance the right end of the gauge is marked.
const GAUGE_TOP: i32 = 2_325_000;

/// `LBL[1..5]`, `LBL[6..10]`, `LBL[11..14]`: eternal table texts.
const TROPHIES: [&[u8]; 5] = [
    b"MEISTERSCHAFTEN",
    b"DFB-POKALE",
    b"LANDESMEISTER-POKALE",
    b"POKALSIEGER-POKALE",
    b"UEFA-POKALE",
];
const CAREER_ROWS: [&[u8]; 5] = [b"PUN.", b"TORE", b"SIEGE", b"NIED.", b"UNEN."];
const ETERNAL_TITLE: &[u8] = b"Ewige Tabelle";
const CAREER_TITLE: &[u8] = b"Ewige Bilanz";
const NOT_YET: &[u8] = b" KOMMT VIELLEICHT NOCH...";
const CAREER_HEADER: &[u8] = b"GESAMT           HEIM         AUSW[RTS";
/// Rows of the eternal table shown before the human teams further down.
const ETERNAL_TOP_ROWS: usize = 23;

/// `LBL[19..25]`: history texts.
const HISTORY_HEADER: &[u8] = b"Sais. Pl.   Liga       DfB-Pokal       Europapokal";
const HISTORY_GRAPH_TITLE: &[u8] = b"Gesamtentwicklung";
const HISTORY_EMPTY: &[u8] =
    b"NOCH HABEN SIE KEINE HISTORISCH SONDERLICH WICHTIGEN ERFOLGE ERZIELT...";
const HISTORY_NOT_YET: &[u8] = b"EXISTIERTEN SIE NOCH GAR NICHT ALS MANAGER...";
const CUP_WINNER: &[u8] = b"SIEGER";
const EURO_WINNER: &[u8] = b"SIEGER ";
const NOT_TAKING_PART: &[u8] = b"nicht im wettbewerb";
/// `UI[120..127]` (`DS:24c0`): the DFB-Pokal stage texts by `stage & 7`
/// (stage 0 reads the item before the table).
pub(crate) const DFB_STAGES: [&[u8]; 8] = [
    b"AUSGABEN (GEH[LTER, REISEKOSTEN)",
    b"",
    b"1.Runde ",
    b"Achtelfinale ",
    b"Viertelfinale ",
    b"Halbfinale ",
    b"Endspiel ",
    b"DfB-Pokal",
];
/// `UI[127..130]` (`DS:24dc`): competition names by `(stage >> 3) & 3`.
pub(crate) const CUP_NAMES: [&[u8]; 4] = [
    b"DfB-Pokal",
    b"Pokal der Landesmeister",
    b"Pokal der Pokalsieger",
    b"UEFA-Pokal",
];

/// `UI[120..131)`: the stage texts followed by the competition names, as
/// one run of the string table. `UI[127]` is both the last stage text and
/// the first competition name. Stage 7 of a European cup (`UI[128]`) reads
/// past the stage texts into the competition names, like the original.
fn ui_stage_text(index: usize) -> &'static [u8] {
    match index.checked_sub(120) {
        Some(i) if i < DFB_STAGES.len() => DFB_STAGES[i],
        Some(i) => CUP_NAMES
            .get(i + 1 - DFB_STAGES.len())
            .copied()
            .unwrap_or(b""),
        None => b"",
    }
}
/// The history shows at most this many seasons.
const HISTORY_MAX: i16 = 50;
/// First season year (`0x7ab`); later starts add 28 years.
const HISTORY_FIRST_YEAR: i16 = 1963;

/// MOD2026: the season starting in `year` in two digits each (`26/27`, `99/00`); the
/// 30 px year column of the history holds five micro4 glyphs, not seven.
fn season_label(year: u16) -> Vec<u8> {
    format!("{:02}/{:02}", year % 100, year.wrapping_add(1) % 100).into_bytes()
}

/// The year column of history row `k` for a manager who joined in season `joined` of the game
/// (`values_13a[1]`).
fn history_year_text(
    state: &GameState,
    joined: u8,
    k: u8,
    mod2026: bool,
    fmt: NumberFormat,
) -> Vec<u8> {
    // MOD2026: mod games show the season of the real year (`26/27`).
    if mod2026 {
        return season_label(
            first_season_year(state)
                .wrapping_add(u16::from(joined))
                .wrapping_add(u16::from(k)),
        );
    }
    let later_start = state.year.wrapping_sub(i32::from(state.logged_rounds)) > 1970;
    let year =
        0x1c * i16::from(later_start) + i16::from(joined) + i16::from(k) + HISTORY_FIRST_YEAR;
    num(fmt, i32::from(year))
}

fn current(state: &GameState) -> usize {
    (state.current_manager as usize).min(3)
}

/// The copy protection trap of every screen (one RNG call when the
/// protection was not passed, then the endless fade loop).
///
/// Origin: BMMAIN.EXE 16c7:09f9 (copy_protection_penalty)
fn copy_protection(host: &mut Host, session: &mut Session) -> Run<()> {
    if !session.copy_protection_ok && session.rng.random_range(0, 100) == 0 {
        loop {
            let on = session.rng.random_range(0, 1) as u8;
            host.screen_fade(on)?;
        }
    }
    Ok(())
}

/// Waits for a left click inside one of the first `n` rects and returns its
/// 1-based index (`ui_hit_test_buttons(rects, n, 1)`).
fn wait_hit(host: &mut Host, rects: &[[i16; 4]], n: i16) -> Run<i16> {
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let si = Ui::hit_test_buttons(rects, n, mx, my, Some(ev as i16));
        if si != 0 {
            return Ok(si);
        }
    }
}

fn frame(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_draw_frame(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

/// `fmt(v)`, or `"^@"` for 0 (a blank digit and a dash).
///
/// Origin: BMMAIN.EXE 362f:0a9f (format_long_or_blank)
pub fn format_long_or_blank(fmt: NumberFormat, v: i32) -> Vec<u8> {
    if v != 0 {
        num(fmt, v)
    } else if POINTS_AGAINST_HIDDEN {
        BLANK_ZERO_224D.to_vec()
    } else {
        BLANK_ZERO.to_vec()
    }
}

/// `"<current>(<best>)"` of streak category `k` (0..6) in column `c` (0 =
/// total, 1 = home, 2 = away): the current streak of club `t` and the best
/// streak of manager `m`.
///
/// Origin: BMMAIN.EXE 362f:0af7 (screen_statistics, 362f:0e32)
pub fn streak_text(
    state: &GameState,
    m: usize,
    t: u8,
    k: usize,
    c: usize,
    fmt: NumberFormat,
) -> Vec<u8> {
    let h = &state.match_history;
    let mut buf = format_long_or_blank(fmt, i32::from(h.team_series[usize::from(t)][k][c]));
    cat(&mut buf, b"(");
    cat(
        &mut buf,
        &format_long_or_blank(fmt, i32::from(h.manager_series_best[m][k][c])),
    );
    cat(&mut buf, b")");
    buf
}

/// The needed average attendance per remaining home game: twelve months of
/// expenses minus income, divided by the home games of a season and the
/// club's popularity, at least 0.
///
/// # Panics
/// When the popularity `M(m)[0x10a]` is 0 (R6003 in the original). The game
/// never writes 0 and `GameState::from_savegame` refuses a save with 0
/// (`state.md` §1.1).
///
/// Origin: BMMAIN.EXE 24a4:2b15 (finance_weekly_balance_per_round)
pub fn finance_needed_attendance(state: &mut GameState) -> i32 {
    let m = current(state);
    let tier = usize::from(state.managers[m].league);
    // DS:225e[tier] - 1 as a signed byte (the table continues with DS:2261).
    let games = i32::from((*layout().team_count.get(tier).unwrap_or(&34) as i8).wrapping_sub(1));
    let expenses = finance_monthly_expenses(state).wrapping_mul(12);
    let income = finance_monthly_income(state).wrapping_mul(12);
    let d = expenses.wrapping_sub(income);
    let r = d / games / i32::from(state.managers[m].value_10a);
    r.max(0)
}

/// Bar heights (2..13 pixels) of the attendance graph: each value scaled
/// between the smallest and the largest one, with the original's signed
/// byte minimum and maximum.
///
/// Origin: BMMAIN.EXE 362f:0af7 (screen_statistics, 362f:1639..1749)
pub fn attendance_bar_heights(values: &[u8]) -> Vec<i16> {
    let mut max: i8 = -127;
    let mut min: i8 = 127;
    for &v in values {
        if i16::from(v) > i16::from(max) {
            max = v as i8;
        }
        if i16::from(min) > i16::from(v) {
            min = v as i8;
        }
    }
    let mut range = max.wrapping_sub(min);
    if range == 0 {
        range = 1;
    }
    values
        .iter()
        .map(|&v| {
            let d = (i16::from(v) - i16::from(min)).wrapping_mul(11);
            d / i16::from(range) + 2
        })
        .collect()
}

/// The 96 strokes of the balance gauge: `(x, tall, colour)`. The stroke at
/// the balance (the first threshold above it) is tall.
///
/// Origin: BMMAIN.EXE 362f:0af7 (screen_statistics, 362f:176d..1822)
pub fn balance_gauge(value: i32) -> Vec<(i16, bool, u8)> {
    let mut out = Vec::new();
    let mut color: u8 = 0;
    let mut x: i16 = 0xd1;
    let mut thr = GAUGE_START;
    while x < 0x131 {
        let tall = value < thr && value.wrapping_add(GAUGE_STEP) >= thr;
        out.push((x, tall, color + 0x13));
        x += 1;
        if (x - 0xd1) % 7 == 0 && color < 0xc {
            color += 1;
        }
        thr = thr.wrapping_add(GAUGE_STEP);
    }
    out
}

/// The text below (or, for items 23/24, also beside) attendance and finance
/// item `i` (20..27). Returns `(text, value)`, where `value` is the number
/// shown beside the label (0 = none).
///
/// Origin: BMMAIN.EXE 362f:0af7 (screen_statistics, 362f:0edd..11a6)
fn attendance_finance_text(
    state: &mut GameState,
    i: usize,
    debt: i32,
    fmt: NumberFormat,
    prev: &[u8],
) -> (Vec<u8>, i32) {
    let m = current(state);
    let games = i32::from(state.managers[m].values_13a[0]);
    let records = state.managers[m].records;
    let club = |state: &GameState, w: i16| -> Vec<u8> {
        state
            .clubs
            .get(w as u16 as usize)
            .map_or_else(Vec::new, |c| c.name().to_vec())
    };
    let money = |v: i32| -> Vec<u8> {
        let mut buf = CURRENCY_PREFIX.to_vec();
        cat(&mut buf, &num(fmt, v));
        cat(&mut buf, CURRENCY);
        buf
    };
    match i {
        20 if games == 0 => (NONE_YET_F.to_vec(), 0),
        20 => (num(fmt, records[0]), 0),
        21 if games == 0 => (NONE_YET_M.to_vec(), 0),
        21 => (num(fmt, records[0] / games), 0),
        22 => (num(fmt, finance_needed_attendance(state)), 0),
        23 if records[1] == 0 => (NONE_YET_M.to_vec(), records[1]),
        24 if records[2] == NO_LOWEST_ATTENDANCE => (NONE_YET_F.to_vec(), records[2]),
        23 | 24 => {
            let w = state.managers[m].values_1f4[if i == 23 { 0 } else { 2 }];
            let mut buf = b"(".to_vec();
            cat(&mut buf, &club(state, w));
            cat(&mut buf, b")");
            (buf, records[i - 22])
        }
        26 => {
            let v = state.managers[m].balance.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A;
            (money(v), 0)
        }
        27 => {
            // strcpy instead of strcat: the prefix is overwritten.
            let v = debt.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A;
            let mut buf = num(fmt, v.wrapping_neg());
            cat(&mut buf, CURRENCY);
            (buf, 0)
        }
        _ => (prev.to_vec(), 0),
    }
}

/// Streaks, records, attendance and finances of the own club.
///
/// Origin: BMMAIN.EXE 362f:0af7 (screen_statistics)
pub fn screen_statistics(host: &mut Host, session: &mut Session, state: &mut GameState) -> Run<()> {
    copy_protection(host, session)?;
    // All four buttons write their rect to the same slot: the exit icon wins.
    panel(host, 5, 8, 0xc3, 0xb4, false);
    panel(host, 0xcd, 8, 0x6c, 0xb4, false);
    panel(host, 5, 0xc0, 0xc3, 0x27, false);
    let rects = [icon(host, 0x10c, 0xc1, 0x22)];
    let m = current(state);
    let t = state.managers[m].team;
    let mut debt = credit_sum(&state.managers[m], CreditField::Principal, None).wrapping_neg();
    for (i, &w) in GAUGE_PALETTE.iter().enumerate() {
        let c = |s: u16| ((s & 0xf) as u8) << 2;
        host.g
            .palette_set_entry(0x1f - i as u8, c(w >> 8), c(w >> 4), c(w));
    }
    let (mut x, mut y): (i16, i16) = (8, 0x10);
    let mut buf: Vec<u8> = Vec::new();
    for (i, &label) in STAT_ITEMS.iter().enumerate() {
        if i == 19 {
            (x, y) = (0xd1, 0x10);
        }
        if i == 28 {
            (x, y) = (8, 0xc5);
        }
        if label[0] == b'!' {
            host.set_font(1);
            y += 6;
            text_at(host, &label[1..], x, y - 2, 0xb);
            let w = host.g.rastport_text_width(&host.rp, &label[1..]);
            line(host, x, y as u8, x + w, y as u8, 0xb);
            host.set_font(2);
            y += 0xb;
            continue;
        }
        text_at(host, label, x, y, 1);
        if i > 0x1c {
            x += 0x8c;
        }
        if i < 19 {
            host.number_pad = 2;
            if i < 10 {
                for (c, &side) in STAT_COLUMN_SIDE.iter().enumerate() {
                    if i >= 8 {
                        buf = b"  ".to_vec();
                        let mut f = host.number_format();
                        let g = format_goals_per_game(state, t, side, (i - 8) as u8, &mut f);
                        host.number_pad = f.pad_width;
                        cat(&mut buf, &g);
                    } else {
                        buf = streak_text(state, m, t, i - 1, c, host.number_format());
                    }
                    text_at(host, &buf, 0x22 * c as i16 + 0x54, y, 2);
                }
            } else {
                let mut f = host.number_format();
                buf = format_team_record(state, t, i - 11, &mut f).1;
                host.number_pad = f.pad_width;
                let x = (7 + i16::from(GAME_OPTION_B == 3)) * 12;
                text_at(host, &buf, x, y, 2);
            }
        } else if i < 0x1c {
            host.number_no_separators = false;
            let (text, v) = attendance_finance_text(state, i, debt, host.number_format(), &buf);
            buf = text;
            if (i == 23 || i == 24) && v != NO_LOWEST_ATTENDANCE && v != 0 {
                let s = num(host.number_format(), v);
                let w = host.g.rastport_text_width(&host.rp, label);
                text_at(host, &s, x + w, y, 2);
            }
            text_at(host, &buf, x, y + 6, 2);
        } else {
            host.number_pad = 7;
            host.number_no_separators = false;
            buf = CURRENCY_PREFIX.to_vec();
            let fmt = host.number_format();
            match i {
                // The income is scaled by B / A, the expenses by B / B.
                0x1d => {
                    let v = finance_monthly_income(state).wrapping_mul(GAME_OPTION_B);
                    cat(&mut buf, &num(fmt, v / GAME_OPTION_A));
                }
                0x1e => {
                    let v = finance_monthly_expenses(state).wrapping_mul(GAME_OPTION_B);
                    cat(&mut buf, &num(fmt, v / GAME_OPTION_B));
                }
                _ => {}
            }
            cat(&mut buf, CURRENCY);
            text_at(host, &buf, x, y, 2);
            host.number_pad = 1;
            x -= 0x8c;
        }
        if i > 0x1b {
            y += 8;
        } else {
            y += if i > 0x12 { 15 } else { 8 };
            if i == 0x18 {
                y += 0x12;
            }
        }
    }
    host.set_font(1);
    text_at(host, STAT_COLUMNS, 0x5c, 0x14, 0xb);
    host.set_font(2);
    fill(host, 0xd1, 0x69, 0x130, 0x76, 0);
    frame(host, 0xd0, 0x68, 0x131, 0x77, 6);
    fill(host, 0xd1, 0xab, 0x130, 0xb8, 0);
    frame(host, 0xd0, 0xaa, 0x131, 0xb9, 6);
    debt = debt.wrapping_add(state.managers[m].balance);
    let mgr = &state.managers[m];
    let games = usize::from(mgr.values_13a[0]);
    // TODO(#31): the original reads `games` bytes from +0x14a on; only the 20
    // bytes up to the stadium block are modelled here.
    let bars = attendance_bar_heights(&mgr.home_attendance[..games.min(mgr.home_attendance.len())]);
    for (k, h) in bars.into_iter().enumerate() {
        let x = 5 * k as i16;
        frame(host, x + 0xd1, 0x76 - h, x + 0xd6, 0x76, 0x15);
        fill(host, x + 0xd2, 0x77 - h, x + 0xd6, 0x76, 0x11);
    }
    for (x, tall, color) in balance_gauge(debt) {
        let (y0, y1) = if tall { (0xab, 0xb8) } else { (0xae, 0xb5) };
        line(host, x, y0, x, y1, color);
    }
    if debt <= GAUGE_START {
        line(host, 0xd1, 0xab, 0xd1, 0xb8, 0x13);
    }
    if debt > GAUGE_TOP {
        line(host, 0x130, 0xab, 0x130, 0xb8, 0x1f);
    }
    host.screen_fade(1)?;
    wait_hit(host, &rects, 1)?;
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    host.number_pad = 1;
    Ok(())
}

/// The 64 German clubs ordered by eternal-table points, most first (an
/// exchange sort on the unsigned `long`).
///
/// Origin: BMMAIN.EXE 362f:18e7 (screen_eternal_table, 362f:18f4..197b)
pub fn eternal_table_order(state: &GameState) -> [u8; 64] {
    let mut ord: [u8; 64] = std::array::from_fn(|i| i as u8);
    let pts = |t: u8| state.team_tables[usize::from(t)].prestige as u32;
    for i in 0..63 {
        for j in i..64 {
            if pts(ord[i]) < pts(ord[j]) {
                ord.swap(i, j);
            }
        }
    }
    ord
}

/// One row of the eternal table: rank index into the order, highlighted
/// (a human team), and whether the dotted gap follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EternalRow {
    pub rank: usize,
    pub human: bool,
    pub gap_after: bool,
}

/// The rows the eternal table shows: the top 23 (plus one for every human
/// team among them) and every human team below them; the dotted gap
/// follows the last top row when human teams are still to come.
///
/// Origin: BMMAIN.EXE 362f:18e7 (screen_eternal_table, 362f:1efa..20c3)
pub fn eternal_table_rows(state: &GameState, ord: &[u8; 64]) -> Vec<EternalRow> {
    let humans = state.active_managers();
    let mut shown_humans: i16 = 0;
    let mut rows = Vec::new();
    for (si, &t) in ord.iter().enumerate() {
        let human = humans.iter().any(|m| m.team == t);
        if human && si < ETERNAL_TOP_ROWS {
            shown_humans += 1;
        }
        if (shown_humans + ETERNAL_TOP_ROWS as i16) as usize > si || human {
            let gap_after = shown_humans - si as i16 == -(ETERNAL_TOP_ROWS as i16 - 1)
                && i16::from(state.manager_count) != shown_humans;
            rows.push(EternalRow {
                rank: si,
                human,
                gap_after,
            });
        }
    }
    rows
}

/// Word `k` of the career counters `M(m)+0x1a4` (pairs of home and away
/// words).
fn career_word(mgr: &Manager, k: usize) -> u16 {
    mgr.career_stats[k / 2][k % 2]
}

/// The career line `row` (points, goals, wins, losses, draws) in column `c`
/// (0 = total, 1 = home, 2 = away): `"a"` or `"a:b"` for the first two rows.
///
/// Origin: BMMAIN.EXE 362f:18e7 (screen_eternal_table, 362f:1d0f..1eb0)
pub fn career_text(mgr: &Manager, row: usize, c: usize, fmt: NumberFormat) -> (Vec<u8>, bool) {
    // Word index of the "for" and the "against" pair (M+0x1a4 + 2 * index).
    let (p, q) = match row {
        // MOD2026: with 3 points for a win the points show as one number, like the league
        // table.
        0 => (
            0,
            (!POINTS_AGAINST_HIDDEN && !layout().single_points_column()).then_some(6),
        ),
        1 => (2, Some(4)),
        _ => (6 + 2 * row, None),
    };
    let pick = |base: usize| match c {
        0 => career_word(mgr, base).wrapping_add(career_word(mgr, base + 1)),
        1 => career_word(mgr, base),
        _ => career_word(mgr, base + 1),
    };
    let mut buf = num(fmt, i32::from(pick(p)));
    match q {
        Some(q) => {
            cat(&mut buf, b":");
            cat(&mut buf, &num(fmt, i32::from(pick(q))));
            (buf, true)
        }
        None => (buf, false),
    }
}

/// The all-time league table and the manager's career record.
///
/// Origin: BMMAIN.EXE 362f:18e7 (screen_eternal_table)
pub fn screen_eternal_table(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
) -> Run<()> {
    let ord = eternal_table_order(state);
    let pic = host.picture(0x24)?;
    copy_protection(host, session)?;
    panel(host, 5, 8, 0x87, 0xde, false);
    panel(host, 0x91, 8, 0xa8, 0xb4, false);
    let rects = [icon(host, 0x10c, 0xc1, 0x22)];
    host.set_font(1);
    centered(host, ETERNAL_TITLE, 5, 0x8c, 0x11, 0xb, 0xff);
    line(host, 6, 0x13, 0x8b, 0x13, 0xb);
    centered(host, CAREER_TITLE, 0x91, 0x139, 0x11, 0xb, 0xff);
    line(host, 0x92, 0x13, 0x138, 0x13, 0xb);
    let screen = host.screen();
    host.g
        .blit_bitmap(pic, 0, 0, screen, 0x9b, 0x16, 0x15, 0x71);
    host.g
        .blit_bitmap(pic, 0, 0, screen, 0x119, 0x16, 0x15, 0x71);
    host.set_font(2);
    let m = current(state);
    let mgr = state.managers[m].clone();
    for (si, (&trophy, &row)) in TROPHIES.iter().zip(&CAREER_ROWS).enumerate() {
        let k = si as i16;
        centered(host, trophy, 0xb0, 0x11e, 0x17 * k + 0x1e, 1, 0);
        // M(m)[0x39 + si]: titles won.
        let won = [
            mgr.titles,
            mgr.cup_wins[0],
            mgr.cup_wins[1],
            mgr.cup_wins[2],
            mgr.cup_wins[3],
        ][si];
        let (buf, color) = if won == 0 {
            (NOT_YET.to_vec(), 1)
        } else {
            (num(host.number_format(), i32::from(won)), 0xb)
        };
        centered(host, &buf, 0xb0, 0x119, 0x17 * k + 0x26, color, 0);
        text_at(host, row, 0x94, 7 * k + 0x9c, 1);
    }
    text_at(host, CAREER_HEADER, 0xb7, 0x93, 1);
    host.number_pad = 4;
    for row in 0..5usize {
        for c in 0..3usize {
            let (buf, pair) = career_text(&mgr, row, c, host.number_format());
            let x = 0x30 * c as i16 + if pair { 0xa9 } else { 0xb3 };
            text_at(host, &buf, x, 7 * row as i16 + 0x9c, 1);
        }
    }
    host.number_no_separators = false;
    let mut y: i16 = 0x1b;
    for row in eternal_table_rows(state, &ord) {
        let t = ord[row.rank];
        let color = if row.human { 0xb } else { 1 };
        host.number_pad = 2;
        let mut buf = num(host.number_format(), row.rank as i32 + 1);
        cat(&mut buf, b". ");
        cat(&mut buf, &club_name(state, t));
        text_at(host, &buf, 9, y, color);
        host.number_pad = 4;
        let pts = (state.team_tables[usize::from(t)].prestige as u32 % 100_000) as i32;
        let buf = num(host.number_format(), pts);
        text_at(host, &buf, 0x6f, y, color);
        y += 7;
        if row.gap_after {
            Gfx::rastport_set_color(&mut host.rp, 1);
            y += 0xe;
            let mut p = (y - 0x13) as u8;
            while i16::from(p) < y - 7 {
                host.g.rastport_put_pixel(&host.rp, 0xd, i16::from(p));
                p = p.wrapping_add(2);
            }
        }
    }
    host.number_pad = 1;
    host.number_no_separators = true;
    host.g.free(pic);
    // FAR:4bec[m] (not in the save, 0 after loading): the title is out of reach.
    let flags_4bec = session.rt.goal_message_flags[m & 3];
    for si in 0..5usize {
        let active = if si == 0 {
            flags_4bec & 1 == 0 && mgr.league == 0
        } else {
            mgr.values_132[si - 1] == state.event_state[si - 1]
        };
        Gfx::rastport_set_color(&mut host.rp, if active { 0xd } else { 0x12 });
        host.g
            .draw_circle(&host.rp, 0xa5, 0x17 * si as i16 + 0x20, 0xb);
    }
    host.screen_fade(1)?;
    wait_hit(host, &rects, 1)?;
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)
}

/// The cup columns of a history line: `(DFB-Pokal text, colour)` and
/// `(European cup text, colour)`, both upper case. `league` is the league
/// byte of the line; its name is what the European cup text starts with
/// when the stage of a lost European cup is 0 after a won DFB-Pokal.
///
/// Origin: BMMAIN.EXE 362f:390e (screen_manager_history, 362f:3de1..3fa6)
pub fn history_cup_texts(league: u8, dfb: u8, euro: u8) -> ((Vec<u8>, u8), (Vec<u8>, u8)) {
    // The buffer FAR [BP-0x9e] keeps the last string copied into it.
    let mut scratch: Vec<u8> = LEAGUE_NAMES
        .get(usize::from(league))
        .map_or_else(Vec::new, |s| s.to_vec());
    let dfb_col = if dfb & 0x80 != 0 {
        (CUP_WINNER.to_vec(), 0xb)
    } else {
        scratch = DFB_STAGES[usize::from(dfb & 7)].to_vec();
        (text_to_upper_de(&scratch), 1)
    };
    let mut color = 1;
    if euro & 0x80 != 0 {
        scratch = EURO_WINNER.to_vec();
        color = 0xb;
    } else if euro & 7 != 0 {
        // UI[121 + stage]: the stage names without the empty first entry.
        scratch = ui_stage_text(121 + usize::from(euro & 7)).to_vec();
    }
    if euro & 0xf8 == 0 {
        scratch = NOT_TAKING_PART.to_vec();
    } else {
        let comp = usize::from((euro & 0x18) >> 3);
        let mut name = CUP_NAMES[comp].to_vec();
        if (comp == 1 || comp == 2) && GAME_OPTION_A == GAME_OPTION_B {
            name.truncate(18 - comp);
            name.push(b'.');
        }
        cat(&mut scratch, &name);
    }
    (dfb_col, (text_to_upper_de(&scratch), color))
}

/// The bars of the development graph: one 5-pixel column per season from
/// the table rank (0 = best of all league clubs) down to the bottom, as
/// `(x0, x1, y, colour)` lines; brown in the amateur range, then the 2.
/// Liga and Bundesliga colours.
///
/// Origin: BMMAIN.EXE 362f:390e (screen_manager_history, 362f:3ad5..3b92)
pub fn history_graph_lines(ranks: &[u8]) -> Vec<(i16, i16, u8, u8)> {
    let mut out = Vec::new();
    for (k, &v) in ranks.iter().enumerate() {
        let x = 5 * k as i16;
        let mut color = 0x1b;
        // MOD2026: the original starts at its 58 league clubs (0x3a), the last rank.
        let mut h = i16::from(layout().league_club_count());
        while h > 0 {
            if i16::from(layout().first_team[2] as i8) + 1 > h {
                color = 0x1a;
            }
            if i16::from(layout().first_team[1] as i8) + 1 > h {
                color = 0x19;
            }
            if i16::from(v) <= h {
                out.push((x + 7, x + 0xb, (h - 0x56) as u8, color));
            }
            h -= 1;
        }
    }
    out
}

/// The manager's past seasons and the development graph. A click on the
/// list shows the next 16 seasons.
///
/// Origin: BMMAIN.EXE 362f:390e (screen_manager_history)
pub fn screen_manager_history(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
) -> Run<()> {
    let m = current(state);
    copy_protection(host, session)?;
    // The graph panel writes its rect into the slot the list panel then
    // overwrites: button 1 is the list, button 2 the exit icon.
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 5, 0x9d, 0x102, 0x49, false, 0);
    let list = panel(host, 5, 8, 0x134, 0x8f, false).1;
    let rects = [list, icon(host, 0x10c, 0xc1, 0x22)];
    host.set_font(1);
    text_at(
        host,
        HISTORY_HEADER,
        10 - 3 * i16::from(GAME_OPTION_B == 3),
        0x11,
        0xb,
    );
    line(host, 5, 0x13, 0x138, 0x13, 0xb);
    centered(host, HISTORY_GRAPH_TITLE, 6, 0x106, 0xa5, 0xb, 0xff);
    line(host, 6, 0xa7, 0x106, 0xa7, 0xb);
    let logged = state.logged_rounds;
    let n = (logged as i16).min(HISTORY_MAX);
    let mgr = state.managers[m].clone();
    let ranks: Vec<u8> = (0..n.max(0) as usize)
        .map(|k| mgr.round_log[k][0])
        .collect();
    for (x0, x1, y, color) in history_graph_lines(&ranks) {
        line(host, x0, y, x1, y, color);
    }
    host.set_font(2);
    if logged == 0 {
        centered(host, HISTORY_EMPTY, 0, 0x140, 0x1d, 1, 7);
        host.screen_fade(1)?;
        while wait_hit(host, &rects, 2)? != 2 {}
    } else {
        let mut y: i16 = 0x1d;
        let mut k: u8 = 0;
        loop {
            let log = mgr.round_log[usize::from(k)];
            let buf = history_year_text(
                state,
                mgr.values_13a[1],
                k,
                session.rt.mod2026,
                host.number_format(),
            );
            text_at(host, &buf, 0xc, y, 1);
            host.number_pad = 2;
            if log[0] == 0xff {
                text_at(host, HISTORY_NOT_YET, 0x2a, y, 1);
            } else {
                let first = layout()
                    .first_team
                    .get(usize::from(log[1]))
                    .copied()
                    .unwrap_or(0);
                let pos = i16::from(log[0]) - i16::from(first as i8);
                let mut buf = num(host.number_format(), i32::from(pos));
                cat(&mut buf, b".");
                let color = if log[0] == 1 && log[1] == 0 { 0xb } else { 1 };
                text_at(host, &buf, 0x2a, y, color);
                let league = LEAGUE_NAMES
                    .get(usize::from(log[1]))
                    .copied()
                    .unwrap_or(b"");
                centered(host, &text_to_upper_de(league), 0x55, 0x55, y, 1, 0xff);
                let ((dfb, dfb_color), (euro, euro_color)) =
                    history_cup_texts(log[1], log[2], log[3]);
                centered(host, &dfb, 0x90, 0x90, y, dfb_color, 0xff);
                centered(host, &euro, 0xb4, 0x139, y, euro_color, 0xff);
            }
            k += 1;
            if k & 0xf != 0 {
                line(host, 6, (y + 2) as u8, 0x138, (y + 2) as u8, 6);
            }
            y += 8;
            if k & 0xf == 0 || i16::from(k) == n {
                host.screen_fade(1)?;
                let hit = loop {
                    let hit = wait_hit(host, &rects, 2)?;
                    if i16::from(k) != n || hit == 2 {
                        break hit;
                    }
                };
                if hit == 2 {
                    break;
                }
                y = 0x1d;
                host.screen_fade(0)?;
                fill(host, 6, 0x18, 0x138, 0x96, 8);
            }
            if i16::from(k) >= n {
                break;
            }
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    host.number_pad = 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> NumberFormat {
        NumberFormat {
            pad_width: 1,
            ..Default::default()
        }
    }

    #[test]
    fn season_labels_use_the_real_year() {
        for (year, label) in [
            (2026, &b"26/27"[..]),
            (2027, b"27/28"),
            (2099, b"99/00"),
            (2100, b"00/01"),
        ] {
            let got = season_label(year);
            assert_eq!(got, label, "{year}");
            assert!(!got.windows(2).any(|w| w == b"19"), "{year}");
        }
    }

    #[test]
    fn history_rows_show_the_season_played() {
        let mut s = Box::new(GameState::zeroed());
        (s.start_year, s.logged_rounds, s.year) = (2028, 2, 2028);
        assert_eq!(history_year_text(&s, 1, 0, true, fmt()), b"27/28");
        assert_eq!(history_year_text(&s, 1, 1, true, fmt()), b"28/29");
        assert_eq!(history_year_text(&s, 0, 0, true, fmt()), b"26/27");
        for (year, joined, k, want) in [(1965, 1, 1, 1965), (1994, 0, 1, 1992)] {
            (s.year, s.logged_rounds) = (year, 2);
            assert_eq!(
                history_year_text(&s, joined, k, false, fmt()),
                num(fmt(), want),
                "{year}"
            );
        }
    }

    #[test]
    fn blank_for_zero() {
        assert_eq!(format_long_or_blank(fmt(), 0), b"^@");
        let f = NumberFormat {
            pad_width: 2,
            ..Default::default()
        };
        assert_eq!(format_long_or_blank(f, 4), b"^4");
    }

    #[test]
    fn streaks_show_current_and_best() {
        let mut s = Box::new(GameState::zeroed());
        s.match_history.team_series[5][2][1] = 3;
        s.match_history.manager_series_best[1][2][1] = 7;
        assert_eq!(streak_text(&s, 1, 5, 2, 1, fmt()), b"3(7)");
        assert_eq!(streak_text(&s, 1, 5, 2, 0, fmt()), b"^@(^@)");
    }

    #[test]
    fn bars_scale_between_min_and_max() {
        assert_eq!(attendance_bar_heights(&[10, 20, 15]), vec![2, 13, 7]);
        // All equal: range forced to 1.
        assert_eq!(attendance_bar_heights(&[9, 9]), vec![2, 2]);
        // The running maximum and minimum are signed bytes, the values are
        // not: 200 becomes the maximum -56, then 100 replaces it.
        assert_eq!(attendance_bar_heights(&[200, 100]), vec![1102, 2]);
        assert!(attendance_bar_heights(&[]).is_empty());
    }

    #[test]
    fn gauge_marks_the_balance() {
        let g = balance_gauge(0);
        assert_eq!(g.len(), 96);
        assert_eq!(g[0], (0xd1, false, 0x13));
        assert_eq!(g[7].2, 0x14);
        assert_eq!(g[95].2, 0x1f);
        let tall: Vec<i16> = g.iter().filter(|s| s.1).map(|s| s.0).collect();
        // -2 375 000 + 48 * 50 000 = 25 000 is the first threshold above 0.
        assert_eq!(tall, vec![0xd1 + 48]);
        assert!(balance_gauge(-3_000_000).iter().all(|s| !s.1));
    }

    #[test]
    fn eternal_order_and_rows() {
        let mut s = Box::new(GameState::zeroed());
        for t in 0..64 {
            s.team_tables[t].prestige = 1000 - t as i32;
        }
        s.team_tables[40].prestige = -1; // unsigned: the largest
        let ord = eternal_table_order(&s);
        assert_eq!(ord[0], 40);
        assert_eq!(ord[1], 0);
        assert_eq!(ord[63], 63);
        s.manager_count = 2;
        s.managers[0].team = 50;
        s.managers[1].team = 2;
        let rows = eternal_table_rows(&s, &ord);
        // Rank 3 (team 2) is human and inside the top: one extra top row,
        // then the gap and rank 50.
        assert_eq!(rows.len(), 25);
        assert!(rows[3].human);
        assert!(rows[23].gap_after);
        assert_eq!(rows[23].rank, 23);
        let last = rows[24];
        assert!(last.human && !last.gap_after);
        assert_eq!(ord[last.rank], 50);
        // No humans below the top: no gap.
        s.manager_count = 1;
        s.managers[0].team = 2;
        let rows = eternal_table_rows(&s, &ord);
        assert_eq!(rows.len(), 24);
        assert!(rows.iter().all(|r| !r.gap_after));
    }

    #[test]
    fn career_columns() {
        let mut mgr = Box::new(GameState::zeroed()).managers[0].clone();
        mgr.career_stats[0] = [30, 61]; // points for: home 30, away 61
        mgr.career_stats[3] = [4, 5]; // points against
        mgr.career_stats[5] = [9, 7]; // M+0x1b8 wins
        assert_eq!(career_text(&mgr, 0, 0, fmt()), (b"91:9".to_vec(), true));
        assert_eq!(career_text(&mgr, 0, 2, fmt()), (b"61:5".to_vec(), true));
        assert_eq!(career_text(&mgr, 2, 1, fmt()), (b"9".to_vec(), false));
    }

    #[test]
    fn career_points_show_one_number_with_three_points() {
        use bmp_game::league_layout::{MOD2026, ORIGINAL, set_layout};
        let mut mgr = Box::new(GameState::zeroed()).managers[0].clone();
        mgr.career_stats[0] = [30, 61];
        mgr.career_stats[3] = [4, 5];
        set_layout(&MOD2026);
        let text = career_text(&mgr, 0, 0, fmt());
        set_layout(&ORIGINAL);
        assert_eq!(text, (b"91".to_vec(), false));
    }

    #[test]
    fn history_cup_columns() {
        let ((d, dc), (e, ec)) = history_cup_texts(2, 2, 0);
        assert_eq!((d.as_slice(), dc), (b"1.RUNDE ".as_slice(), 1));
        assert_eq!((e.as_slice(), ec), (b"NICHT IM WETTBEWERB".as_slice(), 1));
        let ((d, dc), (e, ec)) = history_cup_texts(0, 0x86, 0x8a);
        assert_eq!((d.as_slice(), dc), (b"SIEGER".as_slice(), 0xb));
        assert_eq!(
            (e.as_slice(), ec),
            (b"SIEGER POKAL DER LANDESM.".as_slice(), 0xb)
        );
        // The European stages are UI[121 + stage], one entry later than
        // the DFB-Pokal stages.
        let (_, (e, _)) = history_cup_texts(0, 3, 0x1c);
        assert_eq!(e, b"HALBFINALE UEFA-POKAL");
        let (_, (e, _)) = history_cup_texts(0, 3, 0x13);
        assert_eq!(e, b"VIERTELFINALE POKAL DER POKALS.");
        // Stage 7 reads UI[128], past the stage texts: the name of the
        // Pokal der Landesmeister.
        let (_, (e, _)) = history_cup_texts(0, 3, 0x0f);
        assert_eq!(e, b"POKAL DER LANDESMEISTERPOKAL DER LANDESM.");
        let (_, (e, _)) = history_cup_texts(0, 3, 0x17);
        assert_eq!(e, b"POKAL DER LANDESMEISTERPOKAL DER POKALS.");
        let (_, (e, _)) = history_cup_texts(0, 3, 0x1f);
        assert_eq!(e, b"POKAL DER LANDESMEISTERUEFA-POKAL");
        assert_eq!(ui_stage_text(127), CUP_NAMES[0]);
        assert_eq!(ui_stage_text(129), CUP_NAMES[2]);
        // Stage 0 of a cup keeps the buffer: the league name after a won
        // DFB-Pokal.
        let (_, (e, _)) = history_cup_texts(1, 0x80, 0x18);
        assert_eq!(e, b"ZWEITE LIGAUEFA-POKAL");
    }

    #[test]
    fn graph_bars_hang_from_the_rank() {
        let lines = history_graph_lines(&[0, 57]);
        // Rank 0: 58 lines from y 0xe4 up to 0xab.
        let first: Vec<_> = lines.iter().filter(|l| l.0 == 7).collect();
        assert_eq!(first.len(), 58);
        assert_eq!(first[0], &(7, 0xb, 0xe4, 0x1b));
        assert_eq!(first[57], &(7, 0xb, 0xab, 0x19));
        assert_eq!(first[19].3, 0x1b);
        assert_eq!(first[20].3, 0x1a);
        assert_eq!(first[40].3, 0x19);
        let second: Vec<_> = lines.iter().filter(|l| l.0 == 12).collect();
        assert_eq!(second.len(), 2);
    }

    #[test]
    fn the_last_league_club_draws_one_line_in_both_layouts() {
        use bmp_game::league_layout::{MOD2026, ORIGINAL, set_layout};
        for l in [&ORIGINAL, &MOD2026] {
            set_layout(l);
            let lines = history_graph_lines(&[l.league_club_count()]);
            assert_eq!(lines.len(), 1);
        }
        set_layout(&ORIGINAL);
    }
}
