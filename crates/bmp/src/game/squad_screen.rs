//! The squad / lineup screen `screen_squad_lineup` with the squad list, the
//! tactic slider and the tactics board. Spec: `docs/re/logic/squad.md` §4.
//!
//! The lineup logic itself (automatic lineup, board placement, renumbering)
//! lives in [`bmp_game::squad`]; this module ports the drawing and input
//! handling of segment 2ecd around it.

use bmp_game::ai::squad_recalc_team_strength;
use bmp_game::rng::{Rng, format_long_thousands};
use bmp_game::squad::{
    LineupRules, auto_lineup, board_auto_place_player, board_find_starter_at,
    lineup_renumber_starters, squad_count_listed,
};
use bmp_game::state::{BOARD_UNPLACED, GameState, SquadPlayer, TRANSFER_LIST};
use bmp_game::stats::INJURY_NAMES;
use bmp_game::transfer::player_value_of;

use super::negotiation::{Browse, contract_negotiation_browse};
use super::widgets::{ui_dialog_box, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{
    BitmapId, Gfx, RastPort, Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow,
    ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// `DS:07c4`: list column widths in pixels, columns 0..17.
const COLUMN_WIDTH: [i16; 18] = [
    0x0b, 0x12, 0x39, 0x0d, 0x0c, 0x0c, 0x0e, 0x24, 0x0c, 0x0e, 0x0c, 0x0c, 0x0d, 0x1b, 0x28, 0x0d,
    0x19, 0x21,
];
/// Number of list columns; a column index of 18 means "no column".
const COLUMNS: usize = 18;
/// `DS:07b4`: text colour per position (TOR, ABW, MIT, ANG).
const POSITION_COLOR: [u8; 4] = [0x15, 0x1a, 0x19, 0x18];
/// Column mask of the squad screen: NR, ART, NAME, SP, STÄRKEN, TO, GK RK,
/// STATUS, TD.
const SQUAD_COLUMNS: u32 = 0x0000_f0c7;
/// `DS:5410`: icons of the list, tactics-board and contracts buttons.
const SCREEN_ICONS: [i16; 3] = [0x15, 0x25, 0x16];
/// `DS:4c48..4c7c`: list column headers.
const COLUMN_HEADERS: [&[u8]; 18] = [
    b"NR ",
    b"ART   ",
    b"NAME                    ",
    b"SP  ",
    b"      ",
    b"      ",
    b"SP  ",
    b"ST[RKEN  ",
    b"ST  ",
    b"TO  ",
    b"      ",
    b"      ",
    b"TO  ",
    b"GK RK  ",
    b"STATUS      ",
    b"TD ",
    b"V.DAUER  ",
    b"  GEHALT",
];
/// `DS:4c78`: value header of the transfer-market modes.
const VALUE_HEADER: &[u8] = b"     WERT";
/// `"Auto-Aufstellung"` (LBL 187).
const AUTO_LINEUP: &[u8] = b"Auto-Aufstellung";

/// Entry `k` of the main UI string table (`DS:22e0 + 4*k`), for the entries the
/// squad screen uses; others are empty.
pub(super) fn ui_string(k: i16) -> &'static [u8] {
    match k {
        4 => b"TOR",
        5 => b"ABW",
        6 => b"MIT",
        7 => b"ANG",
        8 => b"GESP.",
        9 => b"VERL.",
        10 => b" ",
        11 => b"IM TEAM",
        12 => b"RESERVE",
        13 => b"-",
        14 => b"O",
        15 => b"+",
        16 => b"LIGA",
        17 => b"SPIELER",
        20 => b"R]CKENNUMMER DES SPIELERS",
        21 => b"ART DES SPIELERS: ",
        22 => b"NAME: ",
        23 => b"ANZAHL DER LIGASPIELE",
        24 => b"ANZAHL DER SPIELE IM DFB-POKAL",
        25 => b"ANZAHL DER SPIELE IM EUROPAPOKAL",
        26 => b"SPIELE GESAMT (LIGA U. POKAL)",
        27 => b"KONDITION,TECHNIK,FORM ",
        28 => b"DURCHSCHNTTS-ST[RKE",
        29 => b"ANZAHL DER TORE IN DER LIGA",
        30 => b"ANZAHL DER TORE IM DFB-POKAL",
        31 => b"ANZAHL DER TORE IM EUROPAPOKAL",
        32 => b"TORE GESAMT (LIGA U. POKAL)",
        33 => b"ANZAHL DER GELBEN UND ROTEN KARTEN",
        34 => b"MOMENTANER STATUS DES SPIELERS",
        35 => b"TENDENZ DES SPIELERS: ",
        36 => b"DAUER DES VERTRAGES MIT DEM SPIELER (IN SAISONS)",
        37 => b"MONATLICHES GEHALT DES SPIELERS",
        38 => b"TORWART",
        39 => b"ABWEHRSPIELER",
        40 => b"MITTELFELDSPIELER",
        41 => b"ANGREIFER",
        53..=70 => INJURY_NAMES[(k - 53) as usize],
        128 => b"Pokal der Landesmeister",
        129 => b"Pokal der Pokalsieger",
        130 => b"UEFA-Pokal",
        131 => b"VERLETZT ",
        132 => b"GESPERRT.",
        133 => b"FALLEND.",
        134 => b"KONSTANT.",
        135 => b"STEIGEND.",
        216 => b"NOCH ",
        217 => b" SPIELE ",
        221 => b" JAHR",
        222 => b"E",
        302 => b" (AUSGEL.)",
        303 => b" JAHRE)",
        304 => b"STATUS: ",
        305 => b"H[NGT HALT SO RUM...",
        306 => b" (ERSCH|PFUNG:",
        307 => b"TRANSFERMARKT",
        308 => b"IHRE MANNSCHAFT",
        309 => b"Nicht mehr als ",
        310 => b"ST[RKE:",
        311 => b" JAHRE",
        _ => b"",
    }
}

/// Substitution counters of a live match (`FAR:5396`, `FAR:90c6`), needed when
/// the screen is opened from the match (`p1 != 0`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchSubs {
    /// `FAR:5396 + 2m + k`: substitutions left for goalkeepers (k = 0) and
    /// outfield players (k = 1).
    pub remaining: [u8; 2],
    /// `FAR:90c6 + m`: substitutions made.
    pub made: u8,
}

/// Hover state of the squad list helpers (`player_card`).
#[derive(Debug, Clone, Copy)]
pub(super) struct ListHover {
    /// `DS:53e6` / `DS:53e8`: last hovered column and row of the list.
    pub(super) col: i16,
    pub(super) row: i16,
    /// `FAR:513e`: suspended players count as available.
    pub(super) suspended_allowed: bool,
}

impl ListHover {
    pub(super) fn new(suspended_allowed: bool) -> Self {
        Self {
            col: 0,
            row: -1,
            suspended_allowed,
        }
    }
}

/// Pictures of the screen and the state of the list and board helpers that the
/// original keeps in globals.
pub(super) struct Screen {
    /// `PIC\6.VGA`: tactics board, badges and cursor sprites.
    pub(super) pic6: BitmapId,
    /// The rastport of `pic6` (badge numbers are drawn into the picture).
    pic6_rp: RastPort,
    /// Empty 13x13 bitmap: background under the board cursor.
    pub(super) save13: BitmapId,
    /// Hover state of the squad list.
    pub(super) list: ListHover,
    /// `FAR:54a0`: no player is held on the board.
    nothing_held: bool,
    /// `DS:a38e`: list row highlighted from the board (-1 = none).
    board_hl_row: i8,
    /// `DS:5416` / `DS:a38a`: centre of the board cursor (x = -1: none).
    cursor_x: i16,
    cursor_y: i16,
    /// `DS:a38c`: record of the held player.
    held_idx: u8,
    /// `DS:a388` / `DS:a389`: board cell the held player came from.
    held_from: (u8, u8),
}

/// The squad of `FAR:304a` (`TRANSFER_LIST` = the transfer market).
pub(super) fn group(state: &GameState) -> usize {
    (state.current_manager as usize).min(TRANSFER_LIST)
}

fn rec(state: &GameState, i: usize) -> &SquadPlayer {
    &state.squads[group(state)][i]
}

fn rec_mut(state: &mut GameState, i: usize) -> &mut SquadPlayer {
    let g = group(state);
    &mut state.squads[g][i]
}

/// Position 0..3 of the player of a squad record (`position code / 25`).
fn position(state: &GameState, r: &SquadPlayer) -> u8 {
    state.players[r.player_id as usize].position_value / 25
}

fn position_color(pos: u8) -> u8 {
    POSITION_COLOR.get(pos as usize).copied().unwrap_or(0)
}

/// `(K + T + F) / 3`.
fn average_skill(r: &SquadPlayer) -> i32 {
    (i32::from(r.skills[0]) + i32::from(r.skills[1]) + i32::from(r.skills[2])) / 3
}

fn is_starter(r: &SquadPlayer) -> bool {
    r.lineup_position != 0 && r.lineup_position < 12
}

fn num(host: &Host, v: i32) -> Vec<u8> {
    format_long_thousands(v, host.number_format())
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: u8) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, i16::from(color));
}

/// An XOR fill (`rastport_set_draw_mode` 2, fill, mode 0).
fn xor_fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16) {
    Gfx::rastport_set_draw_mode(&mut host.rp, 2);
    fill(host, x0, y0, x1, y1, 0x1e);
    Gfx::rastport_set_draw_mode(&mut host.rp, 0);
}

/// Appends `" (L)"`, `" (R)"` or `" (L+R)"` for the preferred foot of record `i`.
///
/// Origin: BMMAIN.EXE 1310:1c22 (player_append_foot_text)
fn player_append_foot_text(buf: &mut Vec<u8>, state: &GameState, i: usize) {
    player_append_foot_text_of(buf, state, group(state), i);
}

/// [`player_append_foot_text`] for record `i` of squad `g`.
///
/// Origin: BMMAIN.EXE 1310:1c22 (player_append_foot_text)
pub(super) fn player_append_foot_text_of(buf: &mut Vec<u8>, state: &GameState, g: usize, i: usize) {
    let foot = state.players[state.squads[g][i].player_id as usize].attr_20;
    buf.extend_from_slice(b" (");
    let left = foot < 5;
    if left {
        buf.push(b'L');
    }
    if foot > 1 {
        if left {
            buf.push(b'+');
        }
        buf.push(b'R');
    }
    buf.push(b')');
}

/// `squad_list_draw` mode: the own squad with the transfer-flag highlight.
const LIST_MODE_TRANSFER_FLAGS: u8 = 4;

/// Draws rows `first..=last` of the squad list of the current group with the
/// columns in `mask`. Mode 0 = own squad, 1/2 = transfer market, 3 = offers,
/// 4 = own squad with the transfer-flag highlight.
///
/// In the transfer modes the value of a player listed by a human manager
/// (owner != 4) is recomputed with `player_value_calc(si, 0)` on the market
/// squad, which may draw a random number.
///
/// Origin: BMMAIN.EXE 2ecd:06af (squad_list_draw)
#[allow(clippy::too_many_arguments)]
pub(super) fn squad_list_draw(
    host: &mut Host,
    state: &GameState,
    rng: &mut Rng,
    x0: i16,
    mut y: i16,
    first: i8,
    last: i8,
    x1: i16,
    y1: i16,
    mask: u32,
    mut mode: u8,
    suspended_allowed: bool,
) {
    let highlight = mode == LIST_MODE_TRANSFER_FLAGS;
    if highlight {
        mode = 0;
    }
    let transfer = mode == 1 || mode == 2;
    if first != last {
        host.set_font(1);
        let title = if transfer {
            ui_string(307)
        } else {
            ui_string(308)
        };
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, title, x0, x1, y - 2, 0xb, 7);
        y += 6;
        if y + 3 > 0x1b {
            fill(host, x0, y + 3, x1, y1 - 1, 8);
        }
    } else {
        y += 6 * i16::from(first) + 10;
    }
    host.set_font(2);
    let saved_manager = state.saved_manager as i16;
    let mut si = i16::from(first) - 1;
    while si <= i16::from(last) {
        let mut buf: Vec<u8> = Vec::new();
        let mut x = x0;
        for col in 0..COLUMNS {
            if (mask >> col) & 1 == 0 {
                continue;
            }
            if si == -1 && first != last {
                let mut h = if col == 17 && transfer {
                    VALUE_HEADER.to_vec()
                } else {
                    COLUMN_HEADERS[col].to_vec()
                };
                if col == 2 && mode != 0 {
                    h.truncate(h.len() - 3);
                }
                buf.extend_from_slice(&h);
                continue;
            }
            if i16::from(first) > si {
                continue;
            }
            let r = rec(state, si as usize);
            if r.player_id == 0 {
                continue;
            }
            let pl = &state.players[r.player_id as usize];
            let pos = pl.position_value / 25;
            let mut c = position_color(pos);
            let offer = r.negotiation;
            if mode == 3 && offer != 0 && offer < 100 {
                c = 3;
            }
            if mode == 3 && offer & 0x80 != 0 {
                c = 0x11;
            }
            host.number_pad = 2;
            let sum = |c: [u8; 3]| c.iter().map(|&n| i32::from(n)).sum::<i32>();
            let mut text: Option<Vec<u8>> = None;
            match col {
                0 => {
                    if r.lineup_position > 11 {
                        c = 4;
                    }
                    if r.lineup_position != 0 {
                        text = Some(num(host, i32::from(r.lineup_position)));
                    }
                }
                1 => {
                    text_at(host, ui_string(4 + i16::from(pos)), x, y, c);
                    if r.loan != 0 || (i16::from(pl.owner_manager) == saved_manager && transfer) {
                        host.g
                            .rastport_put_pixel(&host.rp, COLUMN_WIDTH[1] + x - 3, y - 2);
                    }
                }
                2 => {
                    if (transfer || highlight)
                        && r.status & 0xc0 != 0
                        && i16::from(pl.owner_manager) == saved_manager
                    {
                        c = 0xb;
                    }
                    if r.message_ptr != 0 && offer & 0x7f > 0x63 && mode == 3 {
                        c = 0xb;
                    }
                    text = Some(pl.name().to_vec());
                }
                3..=5 => text = Some(num(host, i32::from(r.appearances[col - 3]))),
                6 => {
                    if mask == 0x0003_d146 {
                        x -= 6;
                    }
                    text = Some(num(host, sum(r.appearances)));
                }
                7 => {
                    let mut s = num(host, i32::from(r.skills[0]));
                    s.push(b' ');
                    s.extend(num(host, i32::from(r.skills[1])));
                    s.push(b' ');
                    s.extend(num(host, i32::from(r.skills[2])));
                    if mode != 0 {
                        x -= 6;
                    }
                    text = Some(s);
                }
                8 => text = Some(num(host, average_skill(r))),
                9..=11 => text = Some(num(host, i32::from(r.goals[col - 9]))),
                12 => text = Some(num(host, sum(r.goals))),
                13 => {
                    let mut s = num(host, i32::from(r.cards[1]));
                    s.push(b' ');
                    s.extend(num(host, i32::from(r.cards[0])));
                    text = Some(s);
                }
                14 => {
                    let st = r.status & 3;
                    let s = if r.lineup_position > 11 {
                        ui_string(12).to_vec()
                    } else if st == 0 || (st == 1 && suspended_allowed) {
                        ui_string(10 + i16::from(r.lineup_position != 0)).to_vec()
                    } else {
                        let mut s = ui_string(7 + i16::from(st)).to_vec();
                        c = 0x12;
                        if st == 1 {
                            s.push(b'(');
                            host.number_pad = 1;
                            s.extend(num(host, i32::from(r.suspended)));
                            s.push(b')');
                        }
                        s
                    };
                    text = Some(s);
                }
                15 => {
                    if r.exhaustion > 0x82 {
                        c = 0x12;
                    }
                    let class = (i16::from(r.tendency) - 0x1e) / 0xd;
                    text = Some(ui_string(13 + class).to_vec());
                }
                16 => {
                    host.number_pad = 1;
                    let mut s = num(host, i32::from(r.contract_years));
                    s.extend_from_slice(ui_string(221));
                    if (s[0] as i8) > b'1' as i8 {
                        s.extend_from_slice(ui_string(222));
                    }
                    text = Some(s);
                }
                _ => {
                    // DS:224d (currency mode) is 0 in the shipped data.
                    host.number_pad = 7;
                    let mut v = r.market_value;
                    if transfer && pl.owner_manager != 4 {
                        v = player_value_of(rng, r, pl.age, 0);
                    }
                    if mode == 2 {
                        v /= 3;
                    }
                    let shown = v.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A;
                    let mut s = num(host, shown);
                    s.extend_from_slice(b" DM");
                    text = Some(s);
                }
            }
            if let Some(t) = text {
                text_at(host, &t, x, y, c);
            }
            x += COLUMN_WIDTH[col];
        }
        host.number_pad = 1;
        if si == -1 && first != last {
            text_at(host, &buf, x0, y, 0xb);
            let (g, rp) = (&mut host.g, &mut host.rp);
            let yb = (y as u8).wrapping_add(2);
            ui_draw_line(g, rp, x0, yb, x1, yb, 0xb);
            ui_draw_line(g, rp, x0, y1 as u8, x1, y1 as u8, 0xb);
            y += 10;
        } else {
            y += 6;
        }
        si += 1;
    }
    host.number_pad = 1;
}

/// The hover text of list cell (`col`, `row`).
fn hover_text(
    host: &mut Host,
    state: &GameState,
    col: i16,
    row: usize,
    x1: i16,
    suspended_allowed: bool,
) -> Vec<u8> {
    let r = rec(state, row).clone();
    let pl = &state.players[r.player_id as usize];
    let mut s: Vec<u8>;
    match col {
        1 => {
            s = ui_string(38 + i16::from(pl.position_value / 25)).to_vec();
            if r.loan != 0 {
                s.extend_from_slice(ui_string(302));
            }
        }
        2 => {
            s = ui_string(22).to_vec();
            s.extend_from_slice(pl.name());
            s.extend_from_slice(b" (");
            s.extend(num(host, i32::from(pl.age)));
            s.extend_from_slice(ui_string(303));
            player_append_foot_text(&mut s, state, row);
        }
        14 => {
            s = ui_string(304).to_vec();
            let st = r.status & 3;
            if r.lineup_position > 11 {
                s.extend_from_slice(ui_string(12));
            } else if st == 0 || (st == 1 && suspended_allowed) {
                s.extend_from_slice(if r.lineup_position != 0 {
                    ui_string(11)
                } else {
                    ui_string(305)
                });
            } else {
                host.number_pad = 1;
                if st == 1 {
                    s.extend_from_slice(ui_string(216));
                    s.extend(num(host, i32::from(r.suspended)));
                    s.extend_from_slice(ui_string(217));
                }
                s.extend_from_slice(ui_string(133 - i16::from(st)));
                if st == 2 {
                    s.push(b'(');
                    s.extend(text_to_upper_de(ui_string(53 + i16::from(r.injury))));
                    s.push(b')');
                }
            }
        }
        15 => {
            s = ui_string(35).to_vec();
            s.extend_from_slice(ui_string(133 + (i16::from(r.tendency) - 0x1e) / 0xd));
            if x1 == 0xec {
                s.extend_from_slice(ui_string(306));
                s.extend(num(host, i32::from(r.exhaustion)));
                s.push(b')');
            }
        }
        _ => s = ui_string(20 + col).to_vec(),
    }
    if col == 7 {
        s.push(b'(');
        s.extend(num(host, average_skill(&r)));
        s.push(b')');
    }
    s
}

/// The list column under x offset `px` (from the left list edge) for the
/// columns in `mask`, or 18 when `px` lies right of the last one.
fn list_column_at(mask: u32, px: i16) -> i16 {
    let mut acc: i16 = 0;
    let mut col = 0usize;
    while col < COLUMNS {
        if (mask >> col) & 1 != 0 {
            acc += COLUMN_WIDTH[col];
            if acc > px {
                break;
            }
        }
        col += 1;
    }
    col as i16
}

/// The number a substitute numbered above the bench limit gets: the first of
/// 12..=16 that none of records `0..=cnt+1` holds, where a holder at `cnt` or
/// beyond does not count; one past the limit becomes 0 (no bench place).
///
/// Origin: BMMAIN.EXE 2ecd:19ea (screen_squad_lineup, bench renumbering)
fn free_bench_number(state: &GameState, cnt: i16, max_lineup_slot: i8) -> u8 {
    let mut t: i8 = 12;
    loop {
        let mut j: i16 = 0;
        while i16::from(rec(state, j as usize).lineup_position) != i16::from(t) {
            if cnt < j {
                break;
            }
            j += 1;
        }
        if cnt > j && t < 16 {
            t += 1;
        } else {
            break;
        }
    }
    if i16::from(max_lineup_slot) - i16::from(t) + 1 == 0 {
        t = 0;
    }
    t as u8
}

/// Hover and click handling of the squad list. Returns `0x64 + n` for button
/// `n`, the row on a click, else 0x7f. `x0 == -1` resets the hover state.
///
/// Origin: BMMAIN.EXE 2ecd:000e (squad_list_hover_info)
#[allow(clippy::too_many_arguments)]
pub(super) fn squad_list_hover_info(
    host: &mut Host,
    state: &GameState,
    lh: &mut ListHover,
    x0: i16,
    y0: i16,
    min_row: i8,
    max_row: i8,
    x1: i16,
    y1: i16,
    mask: u32,
    btns: &[[i16; 4]],
) -> Run<i16> {
    if x0 == -1 {
        lh.col = 0;
        lh.row = -1;
        return Ok(0);
    }
    host.set_font(2);
    let ev = host.input_poll_mouse()?;
    let (mx, my) = host.mouse_xy();
    let h = Ui::hit_test_buttons(btns, btns.len() as i16, mx, my, Some(ev as i16));
    if h != 0 {
        return Ok(h + 0x64);
    }
    let mut row = my.wrapping_sub(y0) / 6;
    let px = mx.wrapping_sub(x0);
    let mut col = list_column_at(mask, px);
    let valid = mx >= 0
        && mx <= x1
        && i16::from(min_row) <= row
        && i16::from(max_row) >= row
        && col != COLUMNS as i16;
    if !valid {
        row = 0x7f;
        col = 0x7f;
    }
    if (host.button == LEFT_DOWN || host.button == RIGHT_DOWN) && col != 0x7f {
        let t = 6 * lh.row + y0;
        if t > 0x1b {
            xor_fill(host, x0, t, x1, t + 4);
        }
        lh.row = -1;
        return Ok(row);
    }
    if col != lh.col || row != lh.row {
        fill(host, x0, y1 - 7, x1, y1 + 1, 8);
        if col != 0x7f && row != 0x7f {
            let s = hover_text(host, state, col, row as usize, x1, lh.suspended_allowed);
            ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &s, x0, x1, y1, 1, 0xff);
        }
        lh.col = col;
    }
    if row != lh.row {
        if lh.row != -1 {
            let t = 6 * lh.row + y0;
            if t > 0x1b {
                xor_fill(host, x0, t, x1, t + 4);
            }
        }
        if row != 0x7f {
            let t = 6 * row + y0;
            if t > 0x1b {
                xor_fill(host, x0, t, x1, t + 4);
            }
        }
        lh.row = row;
    }
    Ok(0x7f)
}

/// The status cell of row `i`: "IM TEAM", "RESERVE", "GESP.(n)", "VERL.".
///
/// Origin: BMMAIN.EXE 2ecd:3170 (squad_draw_status_cell)
fn squad_draw_status_cell(host: &mut Host, state: &GameState, sc: &Screen, i: i16) {
    let suspended_allowed = sc.list.suspended_allowed;
    fill(host, 0xb1, (i + 5) * 6, 0xd8, i * 6 + 0x23, 8);
    let r = rec(state, i as usize).clone();
    let mut k: i16 = 2 + i16::from(r.lineup_position != 0);
    if r.lineup_position > 11 {
        k = 4;
    }
    let st = r.status & 3;
    let (s, c) = if st == 0 || (st == 1 && suspended_allowed) {
        (
            ui_string(8 + k).to_vec(),
            position_color(position(state, &r)),
        )
    } else {
        let mut s = ui_string(7 + i16::from(st)).to_vec();
        if st == 1 {
            s.push(b'(');
            host.number_pad = 1;
            s.extend(num(host, i32::from(r.suspended)));
            s.push(b')');
        }
        (s, 0x12)
    };
    text_at(host, &s, 0xb6, i * 6 + 0x22, c);
}

/// Renumbers the starters 1..k and, with `draw`, redraws the number column.
///
/// Origin: BMMAIN.EXE 2ecd:24cd (lineup_renumber_starters)
fn lineup_renumber_draw(host: &mut Host, state: &mut GameState, draw: bool) {
    host.number_pad = 2;
    lineup_renumber_starters(state);
    if draw {
        for i in 0..24i16 {
            fill(host, 6, (i + 5) * 6, 16, i * 6 + 0x23, 8);
            let r = rec(state, i as usize).clone();
            let s = r.lineup_position;
            if s == 0 {
                continue;
            }
            let c = if s <= 11 {
                position_color(position(state, &r))
            } else {
                4
            };
            let t = num(host, i32::from(s));
            text_at(host, &t, 6, i * 6 + 0x22, c);
        }
    }
    host.number_pad = 1;
}

/// "STÄRKE:avg, age JAHRE (foot)" for record `i` below the list.
///
/// Origin: BMMAIN.EXE 2ecd:2897 (squad_draw_strength_age_line)
fn squad_draw_strength_age_line(host: &mut Host, state: &GameState, i: usize, board_open: bool) {
    let r = rec(state, i).clone();
    let mut s = ui_string(310).to_vec();
    s.extend(num(host, average_skill(&r)));
    s.extend_from_slice(b", ");
    s.extend(num(
        host,
        i32::from(state.players[r.player_id as usize].age),
    ));
    s.extend_from_slice(ui_string(311));
    player_append_foot_text(&mut s, state, i);
    let x1 = if board_open { 0xaf } else { 0xec };
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &s, 6, x1, 0xb8, 1, 0xff);
}

/// The "EINSATZ" tactic slider with a bar of length `v`.
///
/// Origin: BMMAIN.EXE 2ecd:1435 (squad_draw_tactic_slider)
fn squad_draw_tactic_slider(host: &mut Host, pic37: BitmapId, v: i16) {
    let screen = host.screen();
    host.g
        .blit_bitmap(pic37, 0, 0, screen, 0x10b, 5, 0x33, 0x1f);
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_line(g, rp, 0x113, 0x19, 0x113 + v, 0x19, 0x1d);
    ui_draw_line(g, rp, 0x113, 0x1a, 0x113 + v, 0x1a, 0x1d);
}

/// Switches the automatic lineup off after a manual change (with a message).
///
/// Origin: BMMAIN.EXE 2ecd:14c7 (auto_lineup_disable)
fn auto_lineup_disable(host: &mut Host, state: &mut GameState, board_open: bool) -> Run<()> {
    let g = group(state);
    let f = state.lineup_state[g][0] as i8;
    if f > 1 {
        if board_open {
            let x = 0x3c * i16::from(f) - 0x74;
            host.ui
                .draw_button(&mut host.g, &mut host.rp, x, 0xc0, 0, 0x2000, false, 8);
        }
        ui_dialog_box(host, AUTO_LINEUP, b"ist deaktiviert !")?;
        state.lineup_state[g][0] = 1;
    }
    Ok(())
}

/// Draws the badge of starter `i` on the board.
///
/// Origin: BMMAIN.EXE 2ecd:132a (board_draw_player_badge)
fn board_draw_player_badge(host: &mut Host, state: &GameState, sc: &mut Screen, i: usize) {
    let r = rec(state, i).clone();
    let slot = r.lineup_position;
    if slot == 0 {
        return;
    }
    host.g
        .blit_bitmap(sc.pic6, 0xd, 0x79, sc.pic6, 0, 0x79, 0xd, 0xd);
    host.number_pad = 1;
    let t = num(host, i32::from(slot));
    let x = if slot < 10 { 5 } else { 2 };
    ui_draw_text_at(&mut host.g, &mut sc.pic6_rp, &t, x, 0x81, 0xb);
    let screen = host.screen();
    let dx = i16::from(r.board_col) * 16 + 0xc2;
    let dy = 0xe * i16::from(r.board_row) + 0x63;
    host.g
        .blit_bitmap_transparent(sc.pic6, 0, 0x79, screen, dx, dy, 0xd, 0xd);
}

/// Redraws the empty board and the badges of all starters.
///
/// Origin: BMMAIN.EXE 2ecd:2658 (board_draw)
pub(super) fn board_draw(host: &mut Host, state: &GameState, sc: &mut Screen) {
    let screen = host.screen();
    host.g
        .blit_bitmap(sc.pic6, 0, 0, screen, 0xbc, 0x5d, 0x78, 0x78);
    for i in 0..24 {
        if is_starter(rec(state, i)) {
            board_draw_player_badge(host, state, sc, i);
        }
    }
}

/// The open board: frame (with saved background), board, formation buttons
/// (only with `full`) and badges. Returns the frame's button slot and the
/// formation button rects.
///
/// Origin: BMMAIN.EXE 2ecd:26e6 (board_open)
pub(super) fn board_open(
    host: &mut Host,
    state: &GameState,
    sc: &mut Screen,
    full: bool,
) -> (u8, Option<[[i16; 4]; 3]>) {
    let (frame_id, _) =
        host.ui
            .draw_button(&mut host.g, &mut host.rp, 0xb5, 0x56, 0x84, 0x84, true, 8);
    let screen = host.screen();
    host.g
        .blit_bitmap(sc.pic6, 0, 0, screen, 0xbc, 0x5d, 0x78, 0x78);
    Host::set_rastport_font(&mut sc.pic6_rp, 2);
    Gfx::rastport_set_draw_mode(&mut sc.pic6_rp, 0);
    let mut rects = None;
    if full {
        let f = state.lineup_state[group(state)][0] as i8;
        let mut r = [[0i16; 4]; 3];
        for k in 0..3i16 {
            let pressed = if i16::from(f) - k == 2 { 0x8000u16 } else { 0 };
            r[k as usize] = host
                .ui
                .draw_button(
                    &mut host.g,
                    &mut host.rp,
                    k * 0x3c + 4,
                    0xc0,
                    0,
                    pressed as i16,
                    false,
                    8,
                )
                .1;
            host.g.blit_bitmap(
                sc.pic6,
                0x79,
                k * 0x18,
                screen,
                k * 0x3c + 0xd,
                0xc9,
                0x20,
                0x17,
            );
        }
        rects = Some(r);
    }
    for i in 0..24 {
        if is_starter(rec(state, i)) {
            board_draw_player_badge(host, state, sc, i);
        }
    }
    (frame_id, rects)
}

/// Resets the board input state (`board_handle_input` with `pic6 == NULL`).
pub(super) fn board_reset(sc: &mut Screen) {
    sc.nothing_held = true;
    sc.board_hl_row = -1;
    sc.cursor_x = -1;
}

/// Saves the screen under the cursor into `save13`.
fn board_save_under_cursor(host: &mut Host, sc: &Screen) {
    let screen = host.screen();
    host.g.blit_bitmap(
        screen,
        sc.cursor_x - 7,
        sc.cursor_y - 7,
        sc.save13,
        0,
        0,
        0xd,
        0xd,
    );
}

/// Restores the screen under the cursor from `save13`.
fn board_restore_under_cursor(host: &mut Host, sc: &Screen) {
    let screen = host.screen();
    host.g.blit_bitmap(
        sc.save13,
        0,
        0,
        screen,
        sc.cursor_x - 7,
        sc.cursor_y - 7,
        0xd,
        0xd,
    );
}

/// Formation buttons, board cursor, list highlight of the starter under the
/// cursor, and picking up / dropping / swapping starters on the board. Returns
/// 1 after a pick, a drop or a formation change.
///
/// Origin: BMMAIN.EXE 2ecd:29c6 (board_handle_input)
#[allow(clippy::too_many_arguments)]
pub(super) fn board_handle_input(
    host: &mut Host,
    state: &mut GameState,
    rules: LineupRules,
    sc: &mut Screen,
    form_rects: Option<&[[i16; 4]; 3]>,
    hl_x1: i16,
    board_open: bool,
    p1: u8,
) -> Run<i16> {
    let mut shown = false;
    let mut ret: i16 = 0;
    let line_x1 = if board_open { 0xaf } else { 0xec };
    let (mx, my) = host.mouse_xy();
    if let Some(fr) = form_rects
        && p1 == 0
        && host.button == LEFT_DOWN
    {
        let k = Ui::hit_test_buttons(fr, 3, mx, my, None);
        if k != 0 {
            let g = group(state);
            let f = state.lineup_state[g][0] as i8;
            if f > 1 {
                let x = 0x3c * i16::from(f) - 0x74;
                host.ui
                    .draw_button(&mut host.g, &mut host.rp, x, 0xc0, 0, 0x2000, false, 8);
            }
            state.lineup_state[g][0] = (k + 1) as u8;
            host.ui.draw_button(
                &mut host.g,
                &mut host.rp,
                (k - 1) * 0x3c + 4,
                0xc0,
                0,
                0xa000u16 as i16,
                false,
                8,
            );
            auto_lineup(state, rules);
            lineup_renumber_draw(host, state, true);
            for i in 0..8 {
                if rec(state, i).player_id != 0 {
                    squad_draw_status_cell(host, state, sc, i as i16);
                }
            }
            board_draw(host, state, sc);
            board_reset(sc);
            return Ok(1);
        }
    }
    if sc.cursor_x != -1 {
        let row = (sc.cursor_y - 0x6a) / 0xe;
        let col = (sc.cursor_x - 0xbc) / 16;
        let r = board_find_starter_at(state, col as u8, row as u8).map_or(0x7f, i16::from);
        if r == 0x7f && shown {
            fill(host, 6, 0xb1, line_x1, 0xb9, 8);
            shown = false;
        }
        if i16::from(sc.board_hl_row) != r {
            if sc.board_hl_row != -1 {
                let h = i16::from(sc.board_hl_row);
                xor_fill(host, 4, (h + 5) * 6, hl_x1, h * 6 + 0x22);
            }
            fill(host, 6, 0xb1, line_x1, 0xb9, 8);
            if r != 0x7f {
                squad_draw_strength_age_line(host, state, r as usize, board_open);
                shown = true;
            }
            sc.board_hl_row = r as i8;
            xor_fill(host, 4, (r + 5) * 6, hl_x1, r * 6 + 0x22);
        }
        if host.button == LEFT_DOWN {
            auto_lineup_disable(host, state, board_open)?;
            if sc.nothing_held {
                let Some(held) = board_find_starter_at(state, col as u8, row as u8) else {
                    sc.held_idx = 0x7f;
                    return Ok(1);
                };
                sc.held_idx = held;
                let screen = host.screen();
                host.g.blit_bitmap_transparent(
                    sc.pic6,
                    0x1a,
                    0x79,
                    screen,
                    sc.cursor_x - 7,
                    sc.cursor_y - 7,
                    0xd,
                    0xd,
                );
                board_save_under_cursor(host, sc);
                sc.held_from = (col as u8, row as u8);
                sc.nothing_held = false;
                return Ok(1);
            }
            sc.nothing_held = true;
            let held = sc.held_idx as usize;
            if r == 0x7f {
                let h = rec_mut(state, held);
                h.board_col = col as i8;
                h.board_row = row as i8;
                board_draw_player_badge(host, state, sc, held);
                let (fc, fr) = (i16::from(sc.held_from.0), i16::from(sc.held_from.1));
                let screen = host.screen();
                host.g.blit_bitmap(
                    sc.pic6,
                    fc * 16 + 6,
                    fr * 0xe + 6,
                    screen,
                    fc * 16 + 0xc2,
                    fr * 0xe + 0x63,
                    0xd,
                    0xd,
                );
            } else {
                let a = rec(state, held).clone();
                let b = rec(state, r as usize).clone();
                let ra = rec_mut(state, held);
                ra.board_col = b.board_col;
                ra.board_row = b.board_row;
                let rb = rec_mut(state, r as usize);
                rb.board_col = a.board_col;
                rb.board_row = a.board_row;
                board_draw_player_badge(host, state, sc, r as usize);
                board_draw_player_badge(host, state, sc, held);
            }
            board_save_under_cursor(host, sc);
            ret = 1;
        }
    }
    let (x, y) = host.mouse_xy();
    if x > 0xc2 && x < 0x134 && y > 0x63 && y < 0xd5 {
        let cx = ((x - 0xc2) / 16).min(6) * 16 + 0xc9;
        let cy = ((y - 0x63) / 0xe).min(7) * 0xe + 0x6a;
        if sc.cursor_x == cx && sc.cursor_y == cy {
            return Ok(ret);
        }
        if sc.cursor_x != -1 {
            board_restore_under_cursor(host, sc);
        }
        sc.cursor_y = cy;
        sc.cursor_x = cx;
        board_save_under_cursor(host, sc);
        let screen = host.screen();
        host.g
            .blit_bitmap_transparent(sc.pic6, 0x34, 0x79, screen, cx - 7, cy - 7, 0xd, 0xd);
        return Ok(ret);
    }
    if shown {
        fill(host, 6, 0xb1, line_x1, 0xb9, 8);
    }
    if sc.cursor_x != -1 {
        board_restore_under_cursor(host, sc);
    }
    sc.cursor_x = -1;
    if sc.board_hl_row != -1 {
        let h = i16::from(sc.board_hl_row);
        xor_fill(host, 4, (h + 5) * 6, hl_x1, h * 6 + 0x22);
        sc.board_hl_row = -1;
    }
    Ok(ret)
}

/// Keeps bench numbers within `rules.max_lineup_slot`: a substitute numbered
/// above it gets the first free number from 12 (or none). Returns the first
/// free substitute number found by the consecutive scan, starting at
/// `next_res`. The original repeats the pass until the scan finds nothing.
fn renumber_pass(
    host: &mut Host,
    state: &mut GameState,
    sc: &Screen,
    rules: LineupRules,
    p1: u8,
    board_open: bool,
    mut next_res: u8,
) -> u8 {
    host.set_font(2);
    loop {
        let mut unchanged = true;
        let cnt = i16::from(squad_count_listed(state, group(state)));
        for di in 0..cnt {
            if i16::from(rec(state, di as usize).lineup_position) > i16::from(rules.max_lineup_slot)
                && p1 == 0
            {
                let t = free_bench_number(state, cnt, rules.max_lineup_slot);
                rec_mut(state, di as usize).lineup_position = t;
                fill(host, 6, (di + 5) * 6, 16, di * 6 + 0x23, 8);
                lineup_renumber_draw(host, state, true);
                if di < 9 || !board_open {
                    squad_draw_status_cell(host, state, sc, di);
                }
            }
            if rec(state, di as usize).lineup_position == next_res {
                unchanged = false;
                next_res = next_res.wrapping_add(1);
            }
        }
        if unchanged {
            return next_res;
        }
    }
}

/// The squad / lineup screen. `p1` = 0 (office), 0x63 (contracts tab) or the
/// number of players allowed on the field (match substitution); `comp` = 0xff
/// outside a match, else the competition (0 league, 1 DFB-Pokal, 2 Europapokal).
///
/// On exit the team strength is recomputed (`manager_ai_weekly` 1f9d:0002, the full
/// calculation with its random draws during a match).
///
/// Origin: BMMAIN.EXE 2ecd:1560 (screen_squad_lineup)
pub fn screen_squad_lineup(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    rules: LineupRules,
    mut p1: u8,
    comp: u8,
    mut subs: Option<&mut MatchSubs>,
) -> Run<()> {
    let mut maxp: u8 = 11;
    let mut tab: i16 = 0;
    let mut board_is_open = false;
    let mut first = true;
    let mut saved_col: u8 = 0xff;
    let mut saved_row: u8 = 0;
    let mut next_res: u8 = 12;
    if p1 == 0x63 {
        tab = 2;
        p1 = 0;
    }
    if p1 != 0 {
        maxp = p1;
    }
    // The pictures are part of the game data; the original exits on a failure.
    let pic37 = host.picture(0x25)?;
    let save13 = host.blank_bitmap(0xd, 0xd, 0);
    let pic6 = host.picture(6)?;
    let mut sc = Screen {
        pic6,
        pic6_rp: RastPort::new(pic6),
        save13,
        list: ListHover::new(rules.suspended_allowed),
        nothing_held: true,
        board_hl_row: -1,
        cursor_x: -1,
        cursor_y: 0,
        held_idx: 0,
        held_from: (0, 0),
    };
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    let mut btn = [[0i16; 4]; 4];
    btn[0] = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 2, 7, 0xee, 0xb5, false, 8)
        .1;
    if tab == 0 {
        squad_list_draw(
            host,
            state,
            &mut session.rng,
            6,
            0x12,
            0,
            24,
            0xec,
            0xb0,
            SQUAD_COLUMNS,
            0,
            rules.suspended_allowed,
        );
    }
    for i in 0..3i16 {
        let pressed = if tab == i { 0x8000u16 } else { 0 };
        let code = (SCREEN_ICONS[i as usize] as u16 | pressed) as i16;
        btn[i as usize] = host
            .ui
            .draw_button(
                &mut host.g,
                &mut host.rp,
                0x10c,
                0x29 + 0x2d * i,
                0,
                code,
                false,
                8,
            )
            .1;
    }
    let exit_icon = 0x22 + i16::from(comp != 0xff);
    btn[3] = host
        .ui
        .draw_button(
            &mut host.g,
            &mut host.rp,
            0x10c,
            0x29 + 0x2d * 3,
            0,
            exit_icon,
            false,
            8,
        )
        .1;
    let cur = group(state);
    squad_draw_tactic_slider(host, pic37, i16::from(state.managers[cur].value_131));
    let nmax = (squad_count_listed(state, cur) as i8).wrapping_sub(1);
    squad_list_hover_info(host, state, &mut sc.list, -1, 0, 0, 0, 0, 0, 0, &btn)?;
    board_reset(&mut sc);
    host.input_wait_release()?;
    host.button = 0;
    if comp != 0xff {
        host.set_font(3);
        let team = state.managers[cur].team as usize;
        let name = state.clubs[team].name().to_vec();
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &name, 2, 0xf0, 0xe4, 0xb, 0);
    }
    if tab == 0 {
        host.screen_fade(1)?;
    }
    let mut frame_id: u8 = 0;
    let mut form_rects: Option<[[i16; 4]; 3]> = None;
    let mut si: i16;
    let mut di: i16;
    loop {
        di = 0;
        if first {
            // The original starts with an uninitialised count here; only the
            // number of repeated passes depends on it.
            renumber_pass(host, state, &sc, rules, p1, board_is_open, next_res);
            first = false;
        }
        // Poll until a list row or a button is hit (2ecd:1865).
        loop {
            si = 0x7f;
            if tab == 0 {
                let x1 = if board_is_open { 0xaf } else { 0xec };
                si = squad_list_hover_info(
                    host,
                    state,
                    &mut sc.list,
                    6,
                    0x1e,
                    0,
                    nmax,
                    x1,
                    0xb8,
                    SQUAD_COLUMNS,
                    &btn,
                )?;
            }
            if board_is_open {
                di = board_handle_input(
                    host,
                    state,
                    rules,
                    &mut sc,
                    form_rects.as_ref(),
                    0xb3,
                    board_is_open,
                    p1,
                )?;
                if di != 0 {
                    saved_col = 0xff;
                }
            }
            let (mx, my) = host.mouse_xy();
            if host.button == LEFT_DOWN && mx > 0x10a && mx < 0x13f && my > 4 && my < 0x24 {
                let v = (mx - 0x113).clamp(0, 0x22);
                state.managers[cur].value_131 = v as u8;
                squad_draw_tactic_slider(host, pic37, v);
            } else if host.button == LEFT_DOWN && si == 0x7f && di == 0 {
                auto_lineup_disable(host, state, board_is_open)?;
            }
            if !(tab == 0 && si == 0x7f) {
                break;
            }
        }
        next_res = 12;
        if tab != 0 {
            si = 0x67;
        }
        next_res = renumber_pass(host, state, &sc, rules, p1, board_is_open, next_res);
        let mut msg = false;
        Gfx::rastport_set_draw_mode(&mut host.rp, 0);
        let mut skip_redraw = false;
        if si < 0x63 {
            let i = si as usize;
            let g = group(state);
            if state.lineup_state[g][0] as i8 > 1 {
                ui_dialog_box(host, AUTO_LINEUP, b"ist aktiviert !")?;
                skip_redraw = true;
            } else {
                let r = rec(state, i).clone();
                let st = r.status & 3;
                let outfield = usize::from(state.players[r.player_id as usize].position_value != 0);
                if (st == 1 && !rules.suspended_allowed) || st == 2 {
                    msg = ui_dialog_box(host, b"Spieler ist", ui_string(133 - i16::from(st)))?;
                }
                if p1 != 0 && r.lineup_position == 0 {
                    msg = ui_dialog_box(host, b"Spieler ist", b"nicht aufgestellt")?;
                }
                if p1 != 0 && subs.as_ref().is_some_and(|s| s.remaining[outfield] == 0) {
                    msg = ui_dialog_box(host, b"Keine Auswechslung", b"mehr m|glich")?;
                }
                if !msg {
                    let slot = r.lineup_position;
                    let starters = state.squads[g][..24]
                        .iter()
                        .filter(|r| is_starter(r))
                        .count() as i16;
                    if ((slot != 0 && p1 == 0) || (p1 != 0 && slot < 12))
                        && host.button == LEFT_DOWN
                    {
                        // Take the player out of the eleven (2ecd:1d2d).
                        if i16::from(maxp) > starters && slot != 0 && slot < 12 {
                            let mut l1 = b"Nicht weniger als ".to_vec();
                            l1.extend(num(host, i32::from(maxp) - 1));
                            msg = ui_dialog_box(host, &l1, b"Spieler einsetzen !")?;
                        } else {
                            if slot < 12 {
                                saved_col = r.board_col as u8;
                                saved_row = r.board_row as u8;
                            }
                            rec_mut(state, i).lineup_position = if p1 != 0 { next_res } else { 0 };
                            // The original clears row 24 here (its loop counter), not row si.
                            fill(host, 6, (24 + 5) * 6, 16, 24 * 6 + 0x23, 8);
                            lineup_renumber_draw(host, state, true);
                            if si < 9 || !board_is_open {
                                squad_draw_status_cell(host, state, &sc, si);
                            }
                        }
                    } else if p1 == 0 && host.button == RIGHT_DOWN && comp == 0xff && slot == 0 {
                        // Put the player on the bench (2ecd:1ec3).
                        if rules.max_lineup_slot < next_res as i8 {
                            let mut l1 = b"Maximal ".to_vec();
                            l1.extend(num(host, i32::from(rules.max_lineup_slot) - 11));
                            ui_dialog_box(host, &l1, b"Reservespieler !")?;
                            skip_redraw = true;
                        } else {
                            let rm = rec_mut(state, i);
                            rm.lineup_position = next_res;
                            rm.board_col = BOARD_UNPLACED;
                            fill(host, 6, (si + 5) * 6, 16, si * 6 + 0x23, 8);
                            let t = num(host, i32::from(next_res));
                            text_at(host, &t, 6, si * 6 + 0x22, 4);
                            if si < 9 || !board_is_open {
                                squad_draw_status_cell(host, state, &sc, si);
                            }
                        }
                    } else if ((slot == 0 && p1 == 0) || slot > 11) && host.button == LEFT_DOWN {
                        // Bring the player into the eleven (2ecd:2042).
                        if i16::from(maxp) - 1 < starters {
                            let mut l1 = ui_string(309).to_vec();
                            l1.extend(num(host, i32::from(maxp)));
                            msg = ui_dialog_box(host, &l1, b"Spieler einsetzen !")?;
                        } else {
                            if p1 != 0 {
                                if let Some(s) = subs.as_deref_mut() {
                                    s.remaining[outfield] = s.remaining[outfield].wrapping_sub(1);
                                    s.made = s.made.wrapping_add(1);
                                }
                                let pid = r.player_id as usize;
                                if comp == 0 {
                                    state.players[pid].counters[1] =
                                        state.players[pid].counters[1].wrapping_add(1);
                                }
                                let k = usize::from(comp >= 1) + usize::from(comp > 1);
                                let bump = session.rng.random_range(2, 4) as u8 + 4;
                                let rm = rec_mut(state, i);
                                rm.appearances[k] = rm.appearances[k].wrapping_add(1);
                                rm.exhaustion = rm.exhaustion.wrapping_add(bump);
                            }
                            let rm = rec_mut(state, i);
                            if saved_col != 0xff {
                                rm.board_col = saved_col as i8;
                                rm.board_row = saved_row as i8;
                                saved_col = 0xff;
                            } else {
                                rm.board_col = BOARD_UNPLACED;
                            }
                            rm.lineup_position = 11;
                            board_auto_place_player(state, i as u8);
                            lineup_renumber_draw(host, state, true);
                            if si < 9 || !board_is_open {
                                squad_draw_status_cell(host, state, &sc, si);
                            }
                        }
                    }
                }
            }
        }
        if !skip_redraw {
            if !msg && si < 100 {
                if board_is_open {
                    board_draw(host, state, &mut sc);
                }
                if si < 8 || !board_is_open {
                    squad_draw_status_cell(host, state, &sc, si);
                }
            }
            if si == 0x66 && !board_is_open {
                let (id, rects) = board_open(host, state, &mut sc, comp == 0xff);
                frame_id = id;
                if rects.is_some() {
                    form_rects = rects;
                }
                board_is_open = true;
                board_reset(&mut sc);
                host.input_wait_release()?;
            }
            if si == 0x67 && !board_is_open && p1 == 0 {
                host.ui.draw_button(
                    &mut host.g,
                    &mut host.rp,
                    0x10c,
                    0x83,
                    0,
                    0xa000u16 as i16,
                    false,
                    8,
                );
                host.ui
                    .draw_button(&mut host.g, &mut host.rp, 0x10c, 0x29, 0, 0x2000, false, 8);
                let back = contract_negotiation_browse(
                    host,
                    session,
                    state,
                    Browse {
                        sc: &mut sc,
                        btn: &btn,
                        rules,
                    },
                )?;
                if !back {
                    si = 0x68;
                } else {
                    fill(host, 3, 8, 0xef, 0xb8, 8);
                    squad_list_draw(
                        host,
                        state,
                        &mut session.rng,
                        6,
                        0x12,
                        0,
                        24,
                        0xec,
                        0xb0,
                        SQUAD_COLUMNS,
                        0,
                        rules.suspended_allowed,
                    );
                }
                tab = 0;
            }
            if si == 0x65 && board_is_open {
                board_is_open = false;
                host.ui
                    .redraw_buttons(&mut host.g, &mut host.rp, frame_id, 0);
                if comp == 0xff
                    && let Some(work) = host.ui.work
                {
                    let screen = host.screen();
                    host.g
                        .blit_bitmap_or_restore_page2(work, 0, 0xc0, screen, 0, 0xc0, 0xbb, 0x2a);
                }
                for i in 8..24i16 {
                    if rec(state, i as usize).player_id != 0 {
                        squad_draw_status_cell(host, state, &sc, i);
                    }
                }
                si = 24;
            }
        }
        if si == 0x68 && !board_is_open {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xb0, session.auto_play)?;
    host.g.free(pic6);
    host.g.free(pic37);
    host.g.free(save13);
    // manager_ai_weekly(m, p1 != 0): during a match (p1 != 0) with the random draws.
    let m = group(state) as u8;
    squad_recalc_team_strength(state, &session.rt, &mut session.rng, m, p1 != 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn squad_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s
    }

    #[test]
    fn list_columns_follow_the_mask_widths() {
        // NR 0..10, ART 11..28, NAME 29..85, SP 86..99, STÄRKEN 100..135.
        assert_eq!(list_column_at(SQUAD_COLUMNS, 0), 0);
        assert_eq!(list_column_at(SQUAD_COLUMNS, 10), 0);
        assert_eq!(list_column_at(SQUAD_COLUMNS, 11), 1);
        assert_eq!(list_column_at(SQUAD_COLUMNS, 44), 2);
        assert_eq!(list_column_at(SQUAD_COLUMNS, 86), 6);
        assert_eq!(list_column_at(SQUAD_COLUMNS, 101), 7);
        // Right of TD (last column ends at 0xd9 + 0x0d).
        assert_eq!(list_column_at(SQUAD_COLUMNS, 0xe6), 18);
        assert_eq!(list_column_at(0, 0), 18);
    }

    #[test]
    fn foot_text_lists_left_right_or_both() {
        let mut s = squad_state();
        s.squads[0][0].player_id = 5;
        let text = |s: &GameState| {
            let mut b = Vec::new();
            player_append_foot_text(&mut b, s, 0);
            b
        };
        s.players[5].attr_20 = 0;
        assert_eq!(text(&s), b" (L)");
        s.players[5].attr_20 = 3;
        assert_eq!(text(&s), b" (L+R)");
        s.players[5].attr_20 = 6;
        assert_eq!(text(&s), b" (R)");
    }

    #[test]
    fn bench_number_takes_the_first_free_one() {
        let mut s = squad_state();
        for (i, slot) in [12u8, 1, 13, 0, 2].iter().enumerate() {
            s.squads[0][i].player_id = i as u8 + 1;
            s.squads[0][i].lineup_position = *slot;
        }
        assert_eq!(free_bench_number(&s, 5, 15), 14);
        // With a bench of two (13) the next number is past the limit.
        assert_eq!(free_bench_number(&s, 5, 13), 0);
        // A holder at index cnt or beyond does not block the number.
        s.squads[0][5].lineup_position = 14;
        assert_eq!(free_bench_number(&s, 5, 15), 14);
        s.squads[0][4].lineup_position = 14;
        assert_eq!(free_bench_number(&s, 5, 15), 15);
    }

    #[test]
    fn ui_strings_cover_the_tendency_and_status_texts() {
        let class = |tendency: i16| (tendency - 0x1e) / 0xd;
        assert_eq!(ui_string(13 + class(35)), b"-");
        assert_eq!(ui_string(13 + class(50)), b"O");
        assert_eq!(ui_string(13 + class(65)), b"+");
        assert_eq!(ui_string(133 - 1), b"GESPERRT.");
        assert_eq!(ui_string(133 - 2), b"VERLETZT ");
        assert_eq!(ui_string(999), b"");
    }
}
