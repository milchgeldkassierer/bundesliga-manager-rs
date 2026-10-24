//! The top scorers list `screen_top_scorers` (office "Mannschaft" -> sixth
//! sub icon) and the player info window `screen_player_info` (segment 24a4).
//! Spec: `docs/re/logic/stats.md` §3.8, §3.9.

use bmp_game::rng::NumberFormat;
use bmp_game::squad::squad_count_listed;
use bmp_game::state::{GameState, SQUAD_COUNT, TRANSFER_LIST};

use super::squad_screen::ui_string;
use super::stats_screens::format_long_or_blank;
use super::widgets::{TextButtons, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{
    Gfx, Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// `DS:5324..`: colours of the bar scales; the exhaustion marker of a value
/// below 50 reads past the eight entries into the following data.
const BAR: [u8; 22] = [
    0x0c, 0x0d, 0x0e, 0x0f, 0x13, 0x12, 0x11, 0x10, 0x89, 0x00, 0xc4, 0x00, 0xff, 0x00, 0x02, 0x03,
    0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
];
/// `DS:532c`: left edges of the three columns of the info window.
const COLX: [i16; 3] = [0x89, 0xc4, 0xff];
/// `UI[190..=192]`: the competition columns.
const COMPETITIONS: [&[u8]; 3] = [b"LIGA", b"DFB-POKAL", b"EUROPACUP"];
/// `UI[193..=201]`: the labels of the "DATEN" block.
const DATA_LABELS: [&[u8]; 9] = [
    b"KONDITION",
    b"TECHNIK",
    b"FORM",
    b"ROTE KARTEN",
    b"GELBE KARTEN",
    b"GELB-ROTE K.",
    b"SEITE(N)",
    b"GEHALT/M.",
    b"VERTRAGSDAUER",
];
/// `UI[202..=205]`: the age categories.
const AGE_TEXT: [&[u8]; 4] = [
    b"(GR}NSCHNABEL)",
    b"(NOCH GANZ FRISCH)",
    b"(MITTELALTER)",
    b"(FAST AM ENDE)",
];
/// `UI[206..=208]`: the preferred side.
const SIDE_TEXT: [&[u8]; 3] = [b"LINKS", b"MITTE", b"RECHTS"];
/// `UI[225..=226]`: the list modes.
const MODE_TEXT: [&[u8]; 2] = [b"Liga", b"Spieler"];
/// `DS:5340`: colours of the LIGA / SPIELER icons (`[mode]` and `[1 - mode]`).
const MODE_COLORS: [u8; 2] = [0x11, 0x0a];
/// `DS:2272`: first and last club of each tier.
const TIER_RANGE: [(i8, i8); 3] = [(0, 17), (18, 37), (38, 57)];

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: u8) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, i16::from(color));
}

fn centered(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16, color: u8, shadow: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

fn num(host: &Host, v: i32) -> Vec<u8> {
    bmp_game::rng::format_long_thousands(v, host.number_format())
}

fn num_or_blank(host: &Host, v: i32) -> Vec<u8> {
    let fmt: NumberFormat = host.number_format();
    format_long_or_blank(fmt, v)
}

/// The age category of a player: `(age - 18) / 5`, 3 for the very young
/// (an unsigned compare of a negative quotient) and the old.
fn age_category(age: u8) -> usize {
    let k = ((i16::from(age) - 18) / 5) as u8;
    usize::from(k.min(3))
}

/// The x offset of the TENDENZ marker (0..=0x5e).
fn tendency_marker(tendency: u8) -> u8 {
    let t = ((i32::from(i16::from(tendency) - 30) * 100) / 38) as u8;
    t.min(0x5e)
}

/// The x offset of the ERSCHÖPFUNG marker (not clamped).
fn exhaustion_marker(exhaustion: u8) -> u8 {
    ((i32::from(i16::from(exhaustion) - 50) * 0x5e) / 100) as u8
}

/// The info window of record `slot` of squad `FAR:304a`: age, status, goals
/// and games per competition, skills and contract, tendency and exhaustion.
/// Waits for a click on "AHA !", then restores the background.
///
/// Origin: BMMAIN.EXE 24a4:0906 (screen_player_info)
pub fn screen_player_info(
    host: &mut Host,
    _session: &mut Session,
    state: &GameState,
    slot: u8,
) -> Run<()> {
    let g = (state.current_manager as usize).min(TRANSFER_LIST);
    let r = state.squads[g][slot as usize].clone();
    let pl = state.players[r.player_id as usize].clone();
    let (id, _) = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x59, 0x37, 0xe0, 0xae, true, 8);
    // The saved background comes from vga_load_picture, which resets the
    // field width DS:079c to 1.
    host.number_pad = 1;
    host.set_font(1);
    let mut buf = b"Info ]ber ".to_vec();
    buf.extend_from_slice(pl.name());
    centered(host, &buf, 0x59, 0x139, 0x3f, 0x0b, 0);
    ui_draw_line(&mut host.g, &mut host.rp, 0x5a, 0x41, 0x138, 0x41, 0x0b);
    host.set_font(2);
    let mut icons = TextButtons::default();
    icons.register(host, 0xac, 0xd6, b"AHA !", 0x0a);
    let mut buf = pl.name().to_vec();
    buf.extend_from_slice(b" IST ");
    buf.extend(num(host, i32::from(pl.age)));
    buf.extend_from_slice(b" JAHRE ALT, ");
    buf.extend_from_slice(AGE_TEXT[age_category(pl.age)]);
    text_at(host, &buf, 0x5c, 0x49, 1);
    let mut buf = b"STATUS: ".to_vec();
    let st = r.status & 3;
    let lp = r.lineup_position;
    if lp != 0 && lp < 12 {
        buf.extend_from_slice(b"IST F]R'S N[CHSTE SPIEL EINGEPLANT (NR.");
        buf.extend(num(host, i32::from(lp)));
        buf.push(b')');
    } else if lp > 11 {
        buf.extend_from_slice(b"H}TET DIE ERSATZBANK");
    } else if st == 0 {
        buf.extend_from_slice(b"KANN SPIELEN, DARF ABER SCHEINBAR NICHT...");
    } else {
        host.number_pad = 1;
        if st == 1 {
            buf.extend_from_slice(b"NOCH ");
            buf.extend(num(host, i32::from(r.suspended)));
            buf.extend_from_slice(b" SPIELE ");
        }
        // Status 3 shows UI[130], "UEFA-Pokal" (a quirk of the original).
        buf.extend_from_slice(ui_string(133 - i16::from(st)));
        if st == 2 {
            buf.push(b'(');
            buf.extend(text_to_upper_de(ui_string(53 + i16::from(r.injury))));
            buf.push(b')');
        }
    }
    text_at(host, &buf, 0x5c, 0x51, 1);
    for (k, c) in COMPETITIONS.iter().enumerate() {
        centered(host, c, COLX[k], COLX[k] + 0x3a, 0x5c, 1, 0);
    }
    host.number_pad = 2;
    text_at(host, b"TORE:", 0x5c, 0x64, 1);
    for (k, &x) in COLX.iter().enumerate() {
        let total = r.club_goals[k];
        let mut buf = num_or_blank(host, i32::from(r.goals[k]));
        buf.push(b'(');
        buf.extend(num_or_blank(host, i32::from(total)));
        buf.push(b')');
        text_at(host, &buf, x + 0x0c, 0x64, 1);
    }
    text_at(host, b"SPIELE:", 0x5c, 0x6c, 1);
    for (k, &x) in COLX.iter().enumerate() {
        let mut buf = num_or_blank(host, i32::from(r.appearances[k]));
        buf.push(b'(');
        buf.extend(num_or_blank(host, i32::from(r.value_terms[k])));
        buf.push(b')');
        text_at(host, &buf, x + 0x0c, 0x6c, 1);
    }
    text_at(host, b"DATEN:", 0x5c, 0x77, 1);
    host.number_no_separators = false;
    for i in 0..9usize {
        let ry = (i / 3) as i16 * 16 + 0x77;
        let x0 = COLX[i % 3];
        centered(host, DATA_LABELS[i], x0, x0 + 0x3a, ry, 1, 0);
        let v: i32 = match i {
            0..=2 => i32::from(r.skills[i]),
            3..=5 => i32::from(r.cards[i - 3]),
            6 => i32::from(pl.attr_20),
            7 => {
                ((r.market_value.wrapping_mul(GAME_OPTION_B) as u32) / GAME_OPTION_A as u32) as i32
            }
            _ => i32::from(r.contract_years),
        };
        let mut buf = num_or_blank(host, v);
        match i {
            6 => buf = SIDE_TEXT[usize::from(v > 1) + usize::from(v > 4)].to_vec(),
            7 => buf.extend_from_slice(b" DM"),
            8 => {
                buf.extend_from_slice(b" JAHR");
                if v > 1 {
                    buf.push(b'E');
                }
            }
            _ => {}
        }
        centered(host, &buf, x0, x0 + 0x3a, ry + 7, 1, 0xff);
    }
    text_at(host, b"TENDENZ", 0x7f, 0xb1, 1);
    fill(host, 0x5f, 0xb4, 0xc1, 0xbe, 1);
    fill(host, 0x60, 0xb5, 0xc0, 0xbd, 0);
    for i in 0..8i16 {
        fill(
            host,
            (i + 8) * 12,
            0xb7,
            (i + 9) * 12,
            0xbb,
            BAR[(7 - i) as usize],
        );
    }
    let t = tendency_marker(r.tendency);
    let x = 0x5f + i16::from(t);
    ui_draw_line(
        &mut host.g,
        &mut host.rp,
        x,
        0xb5,
        x,
        0xbd,
        BAR[7 - usize::from(t / 12)],
    );
    text_at(host, b"ERSCH|PFUNG", 0xea, 0xb1, 1);
    fill(host, 0xd0, 0xb4, 0x132, 0xbe, 1);
    fill(host, 0xd1, 0xb5, 0x131, 0xbd, 0);
    for i in 0..8i16 {
        fill(
            host,
            12 * i + 0xd1,
            0xb7,
            12 * i + 0xdd,
            0xbb,
            BAR[i as usize],
        );
    }
    let e = exhaustion_marker(r.exhaustion);
    let x = 0xd0 + i16::from(e);
    ui_draw_line(
        &mut host.g,
        &mut host.rp,
        x,
        0xb5,
        x,
        0xbd,
        BAR[usize::from(e / 12)],
    );
    loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        if host.button == LEFT_DOWN && icons.hit(mx, my) == 1 {
            break;
        }
    }
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, id, 0);
    host.number_no_separators = true;
    Ok(())
}

/// Copies the league goals of the human squads into the player table and
/// returns the player ids 1..=150 ordered by goals (descending), ties by
/// games (ascending), with the original's exchange sort.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, sort)
pub fn top_scorers_order(state: &mut GameState) -> [u8; 151] {
    for m in 0..usize::from(state.manager_count) {
        for i in 0..usize::from(squad_count_listed(state, m)) {
            let q = &state.squads[m][i];
            let (pid, goals) = (q.player_id as usize, q.goals[0]);
            state.players[pid].counters[0] = goals;
        }
    }
    let mut ord = [0u8; 151];
    for (i, o) in ord.iter_mut().enumerate() {
        *o = i as u8;
    }
    let goals = |p: u8| state.players[p as usize].counters[0];
    let games = |p: u8| state.players[p as usize].counters[1];
    for i in 1..151 {
        let mut cur = ord[i];
        for j in i + 1..151 {
            let o = ord[j];
            if goals(o) > goals(cur) || (goals(o) == goals(cur) && games(o) < games(cur)) {
                ord[i] = o;
                ord[j] = cur;
                cur = o;
            }
        }
    }
    ord
}

/// The top scorers of the current manager's league: mode 0 the whole
/// league, mode 1 only the players of the human managers' clubs (with their
/// rank in the league). A click on a row opens the player info of a player in
/// a human squad or on the market. With `no_display` nothing is drawn and the
/// result tells whether the league's top scorer plays for the current
/// manager.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers)
pub fn screen_top_scorers(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    no_display: bool,
) -> Run<bool> {
    let cur = state.current_manager as usize;
    let tier = usize::from(state.managers[cur].league).min(2);
    let mut mode: usize = 0;
    let mut ret = false;
    let ord = top_scorers_order(state);
    let mut rects = [[0i16; 4]; 2];
    if !no_display {
        // copy_protection_penalty("Torsch}tzenliste"): counts as passed.
        let (g, rp) = (&mut host.g, &mut host.rp);
        rects[0] = host
            .ui
            .draw_button(g, rp, 0x23, 0x08, 0xfa, 0xb5, false, 8)
            .1;
        rects[1] = host.ui.draw_button(g, rp, 0x10c, 0xc1, 0, 0x22, false, 8).1;
    }
    // TODO(re): uninitialised in the original until a row is listed.
    let mut last_y: u8 = 0;
    'redraw: loop {
        let mut ids = [0u8; 20];
        let mut cnt = [0u8; 4];
        if !no_display {
            ui_draw_line(&mut host.g, &mut host.rp, 0x24, 0x1a, 0x11c, 0x1a, 1);
            host.set_font(1);
            let mut buf = b"Die Besten der ".to_vec();
            buf.extend_from_slice(MODE_TEXT[mode]);
            centered(host, &buf, 0x23, 0x11d, 0x10, 1, 7);
            host.set_font(2);
            text_at(host, b"PL. SPIELER                 VEREIN", 0x26, 0x18, 1);
            text_at(host, b"TORE       TORE/SPIEL", 0xd2, 0x18, 1);
        }
        host.number_pad = 2;
        let mut y: i16 = 0x22;
        let mut rank: i32 = 0;
        let mut rcap: i8 = 0;
        let mut rows = 0usize;
        let mut si: i16 = 1;
        while si < 0x97 {
            let p = ord[si as usize];
            let pl = &state.players[p as usize];
            let team = pl.team as i8;
            let (lo, hi) = TIER_RANGE[tier];
            if team < lo || team > hi {
                si += 1;
                continue;
            }
            if pl.counters[0] < 2 {
                break;
            }
            let mut human = false;
            let mut col: u8 = 1;
            let managers = &state.managers[..usize::from(state.manager_count)];
            for (mgr, n) in managers.iter().zip(cnt.iter_mut()) {
                if mgr.team == team as u8 {
                    if *n != 0 && mode == 0 {
                        continue;
                    }
                    human = true;
                    *n += 1;
                    break;
                }
            }
            if state.manager_index_of_team(team as u8, 0xff) != 0 {
                col = 0x0b;
            }
            rank += 1;
            if rcap < 0x13 {
                rcap += 1;
            }
            if rcap == 1 && col == 0x0b && no_display && state.managers[cur].team == team as u8 {
                ret = true;
                break;
            }
            if !no_display && rows < 20 && (mode == 0 || human) {
                let (goals, games) = (pl.counters[0], pl.counters[1]);
                let name = pl.name().to_vec();
                let club = state.clubs[team as u8 as usize].name().to_vec();
                let mut buf = num(host, rank);
                buf.extend_from_slice(b". ");
                buf.extend_from_slice(&name);
                text_at(host, &buf, 0x26, y, col);
                text_at(host, &club, 0x72, y, col);
                let t = num(host, i32::from(goals));
                text_at(host, &t, 0xd4, y, col);
                ids[rows] = p;
                host.number_pad = 1;
                let mut q = 10 * i16::from(goals);
                if games != 0 {
                    q /= i16::from(games);
                }
                let mut buf = num(host, i32::from(q / 10));
                buf.push(b'.');
                buf.extend(num(host, i32::from(q % 10)));
                host.number_pad = 2;
                text_at(host, &buf, 0xfc, y, col);
                rows += 1;
                y += 7;
                if mode == 0 && si == 12 {
                    // The gap after the twelfth: the dotted line reuses the
                    // list index, so the scan resumes further down (original).
                    y += 0x0e;
                    Gfx::rastport_set_color(&mut host.rp, 1);
                    si = y - 0x12;
                    while si < y - 7 {
                        host.g.rastport_put_pixel(&host.rp, 0x2d, si);
                        si += 2;
                    }
                }
            }
            last_y = y as u8;
            si += 1;
        }
        if no_display {
            return Ok(ret);
        }
        if y == 0x22 {
            centered(
                host,
                b"NOCH KEINE EINTR[GE VORHANDEN...",
                0x23,
                0x11d,
                0x28,
                1,
                0,
            );
        }
        'icons: loop {
            let mut icons = TextButtons::default();
            icons.register(host, 0x36, 0xaf, ui_string(16), MODE_COLORS[mode]);
            icons.register(host, 0xd4, 0xaf, ui_string(17), MODE_COLORS[1 - mode]);
            host.screen_fade(1)?;
            loop {
                let (hx, hy) = host.mouse_xy();
                let h = Ui::hit_test_buttons(&rects, 2, hx, hy, None);
                host.input_poll_mouse()?;
                let (mx, my) = host.mouse_xy();
                if (host.button == LEFT_DOWN || host.button == RIGHT_DOWN)
                    && h == 1
                    && my > 0x1d
                    && i16::from(last_y) > my
                {
                    let k = ((my - 0x1e) / 7) as usize;
                    if k < 20 {
                        let pid = ids[k];
                        for g in 0..SQUAD_COUNT {
                            if !(g < usize::from(state.manager_count) || g == TRANSFER_LIST) {
                                continue;
                            }
                            let n = usize::from(squad_count_listed(state, g));
                            if let Some(j) = (0..n).find(|&j| state.squads[g][j].player_id == pid) {
                                state.current_manager = g as i8;
                                let r = screen_player_info(host, session, state, j as u8);
                                state.current_manager = state.saved_manager;
                                r?;
                                continue 'icons;
                            }
                        }
                    }
                    host.button = 0;
                }
                let r = icons.hit(mx, my);
                if host.button == LEFT_DOWN && r != 0 {
                    mode ^= 1;
                    fill(host, 0x24, 9, 0x11b, 0xbb, 8);
                    continue 'redraw;
                }
                if host.button == LEFT_DOWN && h == 2 {
                    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
                    return Ok(ret);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_categories() {
        assert_eq!(age_category(18), 0);
        assert_eq!(age_category(16), 0);
        assert_eq!(age_category(13), 3);
        assert_eq!(age_category(25), 1);
        assert_eq!(age_category(28), 2);
        assert_eq!(age_category(33), 3);
    }

    #[test]
    fn bar_markers() {
        assert_eq!(tendency_marker(30), 0);
        assert_eq!(tendency_marker(68), 0x5e);
        // Below 30 the quotient wraps and is clamped to the right end.
        assert_eq!(tendency_marker(20), 0x5e);
        assert_eq!(exhaustion_marker(150), 94);
        assert_eq!(exhaustion_marker(100), 47);
        assert_eq!(exhaustion_marker(40), (-9i32) as u8);
    }

    #[test]
    fn scorers_sort_by_goals_then_fewer_games() {
        let mut s = GameState::zeroed();
        s.manager_count = 0;
        s.players[3].counters = [5, 10];
        s.players[7].counters = [5, 8];
        s.players[9].counters = [6, 20];
        let ord = top_scorers_order(&mut s);
        assert_eq!(&ord[1..4], &[9, 7, 3]);
    }
}
