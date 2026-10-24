//! The training screen `screen_training` and the training camp
//! `screen_training_camp` (office "Mannschaft" -> fourth and fifth sub icon).
//! Spec: `docs/re/logic/finance.md` §2.3, §2.4, §2.10 and
//! `docs/re/logic/transfer.md` §4.2-4.4.

use bmp_game::rng::{Rng, lmul, lshr};
use bmp_game::squad::{player_init_random_attrs, squad_count_listed};
use bmp_game::state::GameState;
use bmp_game::transfer::training_free_slots;

use super::stadium::{StatusText, status_line_show};
use super::widgets::{ui_dialog_box, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{
    BitmapId, Ui, ui_draw_frame, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow,
    ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, Run};

/// `DS:255c`: the training kinds of group A.
const GROUP_A: [&[u8]; 4] = [b"Kondition", b"Spiel", b"Schuss", b"Taktik"];
/// `DS:256c`: the training kinds of group B.
const GROUP_B: [&[u8]; 4] = [b"Tor", b"Abwehr", b"Mittelfeld", b"Angriff"];
/// `DS:2580`: labels of the effect box.
const EFFECT_LABELS: [&[u8]; 3] = [b"K", b"T", b"E"];

/// `DS:5258`: base prices of the eight camps per week; multiplied once per
/// program run by `2000 * squad size` (see [`Session::camp_prices`]).
pub const CAMP_BASE_PRICES: [i32; 8] = [5, 6, 7, 10, 11, 10, 13, 15];
/// `DS:2394`: the camp names (CP437 umlauts).
const CAMP_NAMES: [&[u8]; 8] = [
    b"Kickfelder M\x81hle",
    b"Trlg. Hackstedt",
    b"Hof Schwei\xe1perle",
    b"Fitnesscenter Krone",
    b"Scola Ludum Pilae",
    b"Sportschule Tr\x84ne",
    b"Hotel Medium",
    b"Luxushotel La Preux",
];
/// `DS:29a4`: the legend letters next to each camp picture.
const LEGEND: &[u8; 5] = b"EFPRV";
/// `DS:5278`: colours of the legend letters.
const LEGEND_COLORS: [i8; 5] = [24, 1, 15, 19, 18];
/// `DS:527e`: the ratings of the camps (only the first four of five are used).
const CAMP_RATINGS: [[i8; 5]; 8] = [
    [1, 2, 1, 1, 1],
    [1, 2, 3, 1, 1],
    [1, 3, 2, 1, 1],
    [2, 4, 3, 2, 2],
    [2, 1, 2, 4, 4],
    [2, 4, 3, 2, 2],
    [3, 2, 2, 3, 4],
    [5, 3, 2, 4, 5],
];
/// `DS:52a6`: weight of rating `q` for skill `k`.
const RATING_WEIGHTS: [[i8; 3]; 4] = [[20, 20, 40], [60, 10, 20], [20, 50, 20], [10, 40, 30]];

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn centered(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16, color: u8, shadow: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

/// Sums skills 0 and 1 and the exhaustion of the starting eleven of manager
/// `FAR:304a`; the fourth value is the average exhaustion.
///
/// Origin: BMMAIN.EXE 1f9d:1122 (training_effects)
pub fn training_effects(state: &GameState) -> [i16; 4] {
    let m = state.current_manager as usize;
    let mut e = [0i16; 4];
    let mut cnt: i16 = 0;
    let n = usize::from(squad_count_listed(state, m));
    for p in &state.squads[m][..n] {
        if p.lineup_position != 0 && p.lineup_position < 12 {
            cnt += 1;
            e[0] = e[0].wrapping_add(i16::from(p.skills[0]));
            e[1] = e[1].wrapping_add(i16::from(p.skills[1]));
            e[2] = e[2].wrapping_add(i16::from(p.exhaustion));
        }
    }
    if cnt == 0 {
        cnt = 1;
    }
    e[3] = e[2] / cnt;
    e
}

/// The three skill gains of `camp`: its ratings weighted per skill, / 5.
fn camp_weights(camp: usize) -> [i16; 3] {
    let mut s = [0i16; 3];
    for (q, w) in RATING_WEIGHTS.iter().enumerate() {
        for k in 0..3 {
            s[k] += i16::from(CAMP_RATINGS[camp][q]) * i16::from(w[k]);
        }
    }
    s.map(|v| v / 5)
}

/// The training effect of a week in `camp` on every player of manager
/// `FAR:304a`: exhaustion, the three skills, possibly the tendency, then new
/// random traits (which overwrite the tendency again). `level` is `DS:4a28`.
///
/// Origin: BMMAIN.EXE 212a:072d (training_camp_apply, player loop)
pub fn training_camp_train(state: &mut GameState, rng: &mut Rng, camp: usize) {
    let m = state.current_manager as usize;
    let s = camp_weights(camp);
    let diff = i16::from(state.level as i8);
    let mut i = 0;
    while i < usize::from(squad_count_listed(state, m)) {
        let p = &mut state.squads[m][i];
        let d = ((s[0] + s[1] + s[2]) / -3 + (s[0] - s[2] + 10) * 2 + s[1]).max(80);
        p.exhaustion = p
            .exhaustion
            .wrapping_add(rng.random_range(d / 9, d / 6) as u8);
        p.exhaustion = p.exhaustion.clamp(60, 150);
        let mut t: i16 = 0;
        for (k, &sk) in s.iter().enumerate() {
            let r = rng.random_range(2 * diff + 87, 2 * diff + 117);
            t = (r + sk - 100) / 10;
            let a = rng.random_range(0, t.wrapping_mul(20) / 30);
            let b = rng.random_range(0, t);
            t = t.wrapping_add(b).wrapping_sub(a);
            p.skills[k] = p.skills[k].wrapping_add((t >> 2) as u8);
            if p.skills[k] > 99 {
                p.skills[k] = 99;
            }
        }
        t = t.wrapping_add(t.wrapping_mul(s[2]) / 200);
        if rng.random_range(0, 4) == 0 {
            let r = i32::from(rng.random_range(155, 300));
            p.tendency = p.tendency.wrapping_add((r * i32::from(t) / 800) as u8);
        }
        player_init_random_attrs(rng, p);
        i += 1;
    }
}

/// Books `camp`: the camp picture is shown while the squad trains, then the
/// camp cool-down of 7..18 is drawn. Returns 1 (booked).
///
/// Origin: BMMAIN.EXE 212a:072d (training_camp_apply)
fn training_camp_apply(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    camp: usize,
) -> Run<bool> {
    let pic = host.load_picture(0x1c).expect("PIC\\28.VGA");
    let (slot, _) = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x84, 0x4e, 0x39, 0x3a, true, 7);
    // The saved background comes from vga_load_picture, which resets the
    // field width DS:079c to 1.
    host.number_pad = 1;
    let screen = host.screen();
    host.g
        .blit_bitmap(pic, 0, 0, screen, 0x85, 0x4f, 0x37, 0x38);
    host.g.free(pic);
    training_camp_train(state, &mut session.rng, camp);
    host.delay_ticks(80)?;
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, slot, 0);
    let m = state.current_manager as usize;
    state.managers[m].camp_cooldown = session.rng.random_range(7, 18) as u8;
    host.delay_ticks(20)?;
    Ok(true)
}

/// The training camp screen: eight camps with their legend, the price on
/// hover, and booking one with a click.
///
/// TODO(#24): `manager_ai_weekly(m, 0)` 1f9d:0002 is not called on exit.
///
/// Origin: BMMAIN.EXE 212a:0145 (screen_training_camp)
pub fn screen_training_camp(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
) -> Run<()> {
    let m = state.current_manager as usize;
    let any_closed = state.ds_0620.iter().any(|&c| c > 0);
    let closed = any_closed.then(|| host.load_picture(0x2b).expect("PIC\\43.VGA"));
    // copy_protection_penalty("Trainingslager"): the protection counts as passed.
    if session.camp_prices[0] < 10 {
        for i in 0..8 {
            let n = u32::from(squad_count_listed(state, m)) * 2000;
            session.camp_prices[i] = lmul(session.camp_prices[i], n as i32);
        }
    }
    let tiles = host.load_picture(5).expect("PIC\\5.VGA");
    host.set_font(2);
    let screen = host.screen();
    for (i, camp_name) in CAMP_NAMES.iter().enumerate() {
        let (col, row) = (i16::from(i > 3), (i % 4) as i16);
        host.g.blit_bitmap(
            tiles,
            col * 0x82,
            row * 0x27,
            screen,
            col * 0x84 + 4,
            row * 0x34 + 7,
            0x82,
            0x27,
        );
        if let Some(c) = closed
            && state.ds_0620[i] > 0
        {
            host.g.blit_bitmap_transparent(
                c,
                0,
                0,
                screen,
                col * 0x56 + 0x34,
                row * 0x34 + 9,
                0x50,
                0x21,
            );
        }
        let name = text_to_upper_de(camp_name);
        centered(
            host,
            &name,
            col * 0x56 + 0x2f,
            col * 0x56 + 0x8d,
            row * 0x34 + 0x33,
            0x1d,
            0,
        );
        for q in 0..5i16 {
            let letter = [LEGEND[q as usize]];
            let y = row * 0x34 + 0x33;
            ui_draw_text_at(
                &mut host.g,
                &mut host.rp,
                &letter,
                col * 0xd9 + 8 * q + 9,
                y,
                0,
            );
            let c = i16::from(LEGEND_COLORS[q as usize]);
            ui_draw_text_at(
                &mut host.g,
                &mut host.rp,
                &letter,
                col * 0xd9 + 8 * (q + 1),
                y,
                c,
            );
        }
    }
    host.g.free(tiles);
    if let Some(c) = closed {
        host.g.free(c);
    }
    let exit = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x10c, 0xc1, 0, 0x22, false, 8)
        .1;
    host.set_font(1);
    host.screen_fade(1)?;
    let mut hover: i8 = -1;
    let mut booked = false;
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx0, my) = host.mouse_xy();
        if Ui::hit_test_buttons(&[exit], 1, mx0, my, Some(ev as i16)) == 1 {
            break;
        }
        let mut mx = mx0;
        let mut sel: i8 = -1;
        // Uninitialised in the original until a camp or the legend is hit.
        let mut price: i32 = 0;
        for r in 0..4i16 {
            if !(r * 0x34 + 8 < my && my < r * 0x34 + 0x2a) {
                continue;
            }
            if (5 < mx && mx < 0x2d) || (0xde < mx && mx < 0x106) {
                mx = ((mx - 6) % 0xd9) >> 3;
                sel = mx as i8;
                price = 0;
            }
            if (0x33 < mx && mx < 0x83) || (0x88 < mx && mx < 0xd9) {
                mx = i16::from(mx > 0x88) * 4 + r + 5;
                sel = mx as i8;
                price = session.camp_prices[(mx - 5) as usize];
            }
            if sel == -1 {
                continue;
            }
            if i16::from(hover) != mx {
                let shown = lmul(price, GAME_OPTION_B) / GAME_OPTION_A;
                let text = if mx < 5 {
                    StatusText::CampLegend(mx as usize)
                } else {
                    StatusText::CampPrice(CAMP_NAMES[(mx - 5) as usize], shown)
                };
                status_line_show(host, session, text);
                hover = mx as i8;
            }
            if host.button != LEFT_DOWN || price == 0 {
                continue;
            }
            let camp = (mx - 5) as usize;
            if state.ds_0620[camp] > 0 {
                ui_dialog_box(host, CAMP_NAMES[camp], b"hat nicht ge|ffnet...")?;
                continue;
            }
            if state.managers[m].balance < price {
                ui_dialog_box(host, b"Keine finanzielle", b"M|glichkeit gegeben...")?;
                host.set_font(1);
                continue;
            }
            state.managers[m].balance = state.managers[m].balance.wrapping_sub(price);
            booked = training_camp_apply(host, session, state, camp)?;
            host.input_wait_release()?;
            break;
        }
        if hover != -1 && sel == -1 {
            status_line_show(host, session, StatusText::None);
            hover = -1;
        }
        if booked {
            break;
        }
    }
    if !booked {
        ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    }
    // TODO(#24): manager_ai_weekly(m, 0) 1f9d:0002.
    Ok(())
}

/// The intensity bar of the training screen and its monthly cost.
///
/// Origin: BMMAIN.EXE 3277:0081 (training_draw_cost_bar)
fn training_draw_cost_bar(host: &mut Host, bmp: BitmapId, n: i16) {
    let screen = host.screen();
    host.g
        .blit_bitmap(bmp, 0, 0x36, screen, 0xd1, 0xc1, 0x32, 0x29);
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_line(g, rp, 0xd9, 0xd4, 0xd9 + n, 0xd4, 0x1d);
    ui_draw_line(g, rp, 0xd9, 0xd5, 0xd9 + n, 0xd5, 0x1d);
    host.set_font(2);
    let cost = lshr(lmul(i32::from(n) + 1, 10000), 4);
    let cost = lmul(cost, GAME_OPTION_B) / GAME_OPTION_A;
    let t = bmp_game::rng::format_long_thousands(cost, host.number_format());
    centered(host, &t, 0xd3, 0x105, 0xdf, 0x1d, 0xff);
}

/// Draws (`mode` 1) or erases (`mode` 0) the balls `a..b` of training bar
/// `col`: 0/1 = free points of group A/B, 2..5 group A, 6..9 group B, 10 the
/// fifth A row.
///
/// Origin: BMMAIN.EXE 3277:0225 (training_draw_slot_bar)
fn training_draw_slot_bar(host: &mut Host, bmp: BitmapId, col: i16, mut a: i16, b: i16, mode: i16) {
    if b == 0 {
        return;
    }
    let screen = host.screen();
    if col < 2 {
        let x = col * 0x127 + 5;
        let t = b * 16 - 0xc4;
        let y0 = -t;
        if a == 0 && mode == 0 {
            for i in 0..(b * 16 - 1) / 15 {
                let y = i * 16 + y0;
                if y + 1 > 0x23 {
                    host.g.blit_bitmap(bmp, 2, 2, screen, x, y - 0x13, 15, 15);
                }
            }
        } else {
            if a == 0 {
                a = 1;
            }
            a = 0xc4 - a * 16;
            if mode == 0 {
                if a < 0x23 {
                    a = 0x23;
                }
                for i in 0..(a + t - 1) / 15 {
                    let y = i * 16 + y0;
                    if y + 1 > 0x23 {
                        host.g.blit_bitmap(bmp, 2, 2, screen, x, y - 0x13, 15, 15);
                    }
                }
            } else {
                while y0 - 5 < a && a > 0x22 {
                    host.g
                        .blit_bitmap_transparent(bmp, 0x15, 0x15, screen, x, a - 0x13, 15, 15);
                    a -= 16;
                }
            }
        }
    } else {
        let col = col - 2;
        let mut x0 = if col > 3 { 0xd2 } else { 0x1f };
        let mut y = (col % 4) * 0x25 + 0x23;
        if col == 8 {
            x0 = 0x1f;
            y = 0xbd;
        }
        a = (a - 1) * 16 + x0;
        let e = b * 16 + x0 - 6;
        if mode == 0 {
            if a < 0x23 {
                a = 0x23;
            }
            for i in 0..((e - a) - 0xb) / 15 {
                host.g
                    .blit_bitmap(bmp, 0x17, 2, screen, i * 16 + a, y + 1, 15, 15);
            }
        } else {
            while a < e {
                host.g
                    .blit_bitmap_transparent(bmp, 0x15, 0x15, screen, a, y + 1, 15, 15);
                a += 16;
            }
        }
    }
}

/// The current value of training row `row` of group B (`group_b`) or A
/// (rows 0..3, row 4 = the fifth A slot).
fn training_value(state: &GameState, m: usize, group_b: bool, row: usize) -> u8 {
    let mgr = &state.managers[m];
    if group_b {
        mgr.training_146[row]
    } else {
        mgr.training_141[row]
    }
}

fn set_training_value(state: &mut GameState, m: usize, group_b: bool, row: usize, v: u8) {
    let mgr = &mut state.managers[m];
    if group_b {
        mgr.training_146[row] = v;
    } else {
        mgr.training_141[row] = v;
    }
}

/// The training screen: four rows of group A (Kondition, Spiel, Schuss,
/// Taktik) with 20 points, four of group B (Tor, Abwehr, Mittelfeld,
/// Angriff) with 10, the fifth A row, the intensity with its cost and the
/// K/T/E box of the starting eleven.
///
/// Origin: BMMAIN.EXE 212a:274c (screen_training)
pub fn screen_training(host: &mut Host, session: &mut Session, state: &mut GameState) -> Run<()> {
    let m = state.current_manager as usize;
    // copy_protection_penalty("Training"): the protection counts as passed.
    let p1 = host.load_picture(1).expect("PIC\\1.VGA");
    let p25 = host.load_picture(0x25).expect("PIC\\37.VGA");
    let exit = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x10c, 0xc1, 0, 0x22, false, 8)
        .1;
    training_draw_cost_bar(host, p25, i16::from(state.managers[m].training_intensity));
    let screen = host.screen();
    host.g.blit_bitmap(p1, 0, 0, screen, 3, 0xe, 0x15, 0xa5);
    host.g.blit_bitmap(p1, 0, 0, screen, 0x12a, 0xe, 0x15, 0xa5);
    host.set_font(1);
    for k in 0..4i16 {
        let y = k * 0x25 + 0x22;
        host.g.blit_bitmap(p1, 0x15, 0, screen, 0x1e, y, 0x54, 0x15);
        host.g.blit_bitmap(p1, 0x17, 0, screen, 0x70, y, 0x52, 0x15);
        centered(
            host,
            GROUP_A[k as usize],
            0x1d,
            0xbf,
            k * 0x25 + 0x3e,
            0xb,
            0,
        );
        host.g.blit_bitmap(p1, 0x15, 0, screen, 0xd1, y, 0x54, 0x15);
        centered(
            host,
            GROUP_B[k as usize],
            0xd0,
            0x122,
            k * 0x25 + 0x3e,
            0xb,
            0,
        );
    }
    host.g
        .blit_bitmap(p1, 0x15, 0, screen, 0x1e, 0xbc, 0x54, 0x15);
    host.g
        .blit_bitmap(p1, 0x17, 0, screen, 0x70, 0xbc, 0x52, 0x15);
    centered(host, b"Intensit{t", 0x1d, 0xc1, 0xd7, 0xb, 0);
    for k in 0..4 {
        let v = i16::from(state.managers[m].training_141[k]);
        training_draw_slot_bar(host, p1, k as i16 + 2, 1, v, 1);
    }
    let mut free_a = i16::from(training_free_slots(state, 0));
    training_draw_slot_bar(host, p1, 0, 1, free_a, 1);
    for k in 0..4 {
        let v = i16::from(state.managers[m].training_146[k]);
        training_draw_slot_bar(host, p1, k as i16 + 6, 1, v, 1);
    }
    let mut free_b = i16::from(training_free_slots(state, 1));
    training_draw_slot_bar(host, p1, 1, 1, free_b, 1);
    let v = i16::from(state.managers[m].training_141[4]);
    training_draw_slot_bar(host, p1, 10, 1, v, 1);
    fill(host, 3, 0xb7, 0x16, 0xe8, 7);
    ui_draw_frame(&mut host.g, &mut host.rp, 3, 0xb7, 0x16, 0xe8, 1);
    let e = training_effects(state);
    let mut sum = e[1].wrapping_add(e[0]);
    if sum == 0 {
        sum = 1;
    }
    host.set_font(2);
    for s in 0..3i16 {
        let c = (s + 0xd) as u8;
        ui_draw_text_at(
            &mut host.g,
            &mut host.rp,
            EFFECT_LABELS[s as usize],
            6 * s + 5,
            0xe6,
            i16::from(c),
        );
        let v: i32 = if s < 2 {
            100 * i32::from(e[s as usize]) / i32::from(sum)
        } else {
            i32::from(e[3]) - 50
        };
        let h = (0x26 - (v * 38 / 100) as i16).min(0x26);
        fill(host, 6 * s + 5, 0xb9 + h, 6 * s + 8, 0xdf, c);
        ui_draw_frame(
            &mut host.g,
            &mut host.rp,
            6 * s + 5,
            0xb9 + h,
            6 * s + 8,
            0xdf,
            0,
        );
    }
    host.screen_fade(1)?;
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx, my0) = host.mouse_xy();
        if Ui::hit_test_buttons(&[exit], 1, mx, my0, Some(ev as i16)) == 1 {
            break;
        }
        if host.button != LEFT_DOWN {
            continue;
        }
        let mut my = my0;
        if 0xd0 < mx && mx < 0x105 && 0xc0 < my && my < 0xe9 {
            let v = (mx - 0xd9).clamp(0, 0x22);
            state.managers[m].training_intensity = v as u8;
            training_draw_cost_bar(host, p25, v);
        }
        for k in 0..4i16 {
            let mut bar: i16 = -1;
            if (k * 0x25 + 0x21 < my && my < k * 0x25 + 0x33)
                || (0xbc < my && my < 0xce && mx < 0xbe)
            {
                let mut row = (my - 0x21) / 0x25;
                my = row;
                let mut rejected = false;
                if 0x1e < mx && mx < 0xbe {
                    bar = row + 2;
                    if row > 3 {
                        bar = 10;
                        row = 4;
                    }
                    let v = ((mx - 0x1e) >> 4) as i8 + 1;
                    let cur = training_value(state, m, false, row as usize);
                    if i16::from(cur) - i16::from(v) + free_a >= 0 || row == 4 {
                        let (from, to, add) = if cur as i8 > v {
                            (i16::from(v) + 1, i16::from(cur) + 1, 0)
                        } else {
                            (i16::from(cur), i16::from(v), 1)
                        };
                        set_training_value(state, m, false, row as usize, v as u8);
                        training_draw_slot_bar(host, p1, bar, from, to, add);
                    } else {
                        rejected = true;
                    }
                }
                if !rejected && 0xd1 < mx && mx < 0x121 {
                    bar = row + 6;
                    let v = ((mx - 0xd1) / 16) as i8 + 1;
                    let cur = training_value(state, m, true, row as usize);
                    if i16::from(cur) - i16::from(v) + free_b < 0 {
                        rejected = true;
                    } else {
                        let (from, to, add) = if cur as i8 > v {
                            (i16::from(v) + 1, i16::from(cur) + 1, 0)
                        } else {
                            (i16::from(cur), i16::from(v), 1)
                        };
                        set_training_value(state, m, true, row as usize, v as u8);
                        training_draw_slot_bar(host, p1, bar, from, to, add);
                    }
                }
                if rejected {
                    continue;
                }
            }
            if bar != -1 {
                let (d, free) = if bar < 6 {
                    (i16::from(training_free_slots(state, 0)), &mut free_a)
                } else {
                    (i16::from(training_free_slots(state, 1)), &mut free_b)
                };
                let (from, to, add) = if d < *free {
                    (d, *free, 0)
                } else {
                    (*free, d, 1)
                };
                *free = d;
                training_draw_slot_bar(host, p1, i16::from(bar > 5), from, to, add);
            }
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    host.g.free(p25);
    host.g.free(p1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s
    }

    #[test]
    fn camp_weights_follow_the_ratings() {
        // Kickfelder Mühle: (1*20 + 2*60 + 1*20 + 1*10) / 5 = 34, ...
        assert_eq!(camp_weights(0), [34, 26, 26]);
        // Luxushotel La Preux: (100+180+40+40)/5, (100+30+100+160)/5, (200+60+40+120)/5.
        assert_eq!(camp_weights(7), [72, 78, 84]);
    }

    #[test]
    fn effects_sum_the_starting_eleven_only() {
        let mut s = state();
        for (i, (slot, sk, ex)) in [(1u8, 50u8, 100u8), (12, 90, 90), (0, 70, 70), (11, 40, 80)]
            .into_iter()
            .enumerate()
        {
            let p = &mut s.squads[0][i];
            p.player_id = i as u8 + 1;
            p.lineup_position = slot;
            p.skills = [sk, sk + 1, 0];
            p.exhaustion = ex;
        }
        assert_eq!(training_effects(&s), [90, 92, 180, 90]);
        assert_eq!(training_effects(&state()), [0, 0, 0, 0]);
    }

    #[test]
    fn camp_training_clamps_exhaustion_and_skills() {
        let mut s = state();
        s.squads[0][0].player_id = 1;
        s.squads[0][0].skills = [99, 99, 99];
        s.squads[0][0].exhaustion = 10;
        let mut rng = Rng::new(1);
        training_camp_train(&mut s, &mut rng, 7);
        let p = &s.squads[0][0];
        assert!((60..=150).contains(&p.exhaustion));
        assert!(p.skills.iter().all(|&k| k <= 99));
        // New random traits: tendency 35..65.
        assert!((0x23..=0x41).contains(&p.tendency));
    }
}
