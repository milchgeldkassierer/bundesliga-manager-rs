//! The stadium screen (segment 1000): the stadium picture, the expansion
//! panel and the status table of the seven stadium items and the ticket
//! price. Spec: `docs/re/logic/finance.md` §3.1-§3.4 and §3.8.
//!
//! The money and construction state lives in [`bmp_game::state::Manager`]
//! (`stadium`, `build_days`, `value_10a` = ticket price, `balance`); the daily
//! construction progress is [`bmp_game::finance::stadium_construction_daily`].

use bmp_game::rng::{format_long_thousands, ldiv, lmul};
use bmp_game::state::{GameState, Manager, STADIUM_QUALITY_LEVELS, STADIUM_SIZE_LEVELS};

use super::widgets::{TextButtons, ui_dialog_box, ui_value_spinner, ui_wait_right_click_button};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, currency};
use crate::gfx::{
    Gfx, Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// `DS:2818`: help text of the stadium rows 0..9 (and status line ids 0..9).
const ROW_HELP: [&[u8]; 10] = [
    b"",
    b"SITZPL[TZE AUSBAUEN",
    b"STEHPL[TZE AUSBAUEN",
    b"PL[TZE ]BERDACHEN",
    b"FLUTLICHTMASTEN AUSBAUEN",
    b"ANZEIGETAFEL ERRICHTEN",
    b"STADION RENOVIEREN",
    b"KOMFORT VERBESSERN",
    b"EINTRITTSPREIS [NDERN",
    b"",
];
/// `DS:283c`: condition / comfort levels 0..6.
const QUALITY_NAMES: [&[u8]; STADIUM_QUALITY_LEVELS] = [
    b"",
    b"UNGENUEGEND",
    b"MANGELHAFT",
    b"AUSREICHEND",
    b"BEFRIEDIGEND",
    b"GUT",
    b"SEHR GUT",
];
/// `DS:2858`: floodlight / scoreboard sizes 0..3.
const SIZE_NAMES: [&[u8]; STADIUM_SIZE_LEVELS] = [b"IS NICH", b"KLEIN", b"MITTEL", b"GRO~"];
/// `DS:2868`: row labels of the status table.
const ROW_LABELS: [&[u8]; 9] = [
    b"GESAMTKAPAZIT[T",
    b"SITZPL[TZE",
    b"STEHPL[TZE",
    b"]BERDACHTE PL[TZE",
    b"FLUTLICHT",
    b"ANZEIGETAFEL",
    b"ZUSTAND",
    b"KOMFORT",
    b"EINTRITTSPREIS",
];
/// `DS:28ac..28d4`; `PER_1000` and `PER_POINT` without their leading `" DM"`, the
/// [`currency`].
const YES: &[u8] = b"NA KLAR !";
const NO: &[u8] = b"ACH NEE...";
const COSTS: &[u8] = b"KOSTET ";
const PER_1000: &[u8] = b" PRO 1000";
const PER_POINT: &[u8] = b" PRO PUNKT";
const PER_WEEK: &[u8] = b"/Woche.";
const MAX_SIZE: &[u8] = b"MAX. GR|~E: ";
const COSTS_2: &[u8] = b" KOSTEN: ";
const MAX_STATUS: &[u8] = b"MAX. STATUS: ";
const BALANCE: &[u8] = b"IHR KONTOSTAND:";
const NOW: &[u8] = b"MOMENTAN";
const AFTER: &[u8] = b"NACH AUSBAU";
/// `DS:2890..289c`.
const EXPAND_BY: &[u8] = b"AUSBAU UM ";
const EXPAND_TO: &[u8] = b" AUF ";
const NEW_SIZE: &[u8] = b"NEUE GR|~E: ";
const NEW_STATUS: &[u8] = b"NEUER STATUS: ";
const COSTS_OPEN: &[u8] = b"(KOSTEN: ";
const BUILD_TIME: &[u8] = b"BAUZEIT: CIRKA";
const BUILD_TIME_TOTAL: &[u8] = b"BAUZEIT: INSGESAMT CA.";
const WEEKS: &[u8] = b" WOCHEN";
/// `DS:4a18`, `DS:4f64..4f70` (`" DM"`, `" DM."` and `" DM)"` are the [`currency`] and their
/// tails).
const WEEKS_SHORT: &[u8] = b" WOCHE(N)";

/// `DS:0044 + 4 * row`: price per 1000 places (rows 1..3) or per level
/// (rows 4..7); row 0 is never used as a price (the word pair doubles as the
/// status-line strip, see [`StatusLine`]).
const PRICES: [i32; 8] = [
    -1, 150_000, 100_000, 160_000, 380_000, 440_000, 550_000, 650_000,
];
/// `DS:0063 + row`: base construction time in weeks.
const BASE_WEEKS: [i32; 8] = [0, 15, 15, 20, 10, 10, 8, 4];
/// Most places of seats + standing (`0x1FBD0`).
const MAX_CAPACITY: i32 = 130_000;

/// Overlay positions of the floodlights and the scoreboard per stadium size
/// (`DS:006c`, `DS:0070`, `DS:0078`, `DS:007c`, `DS:0080`).
const LIGHT_HEIGHT: [i16; 4] = [23, 23, 36, 36];
const LIGHT_X: [i16; 8] = [9, 111, 9, 111, 9, 115, 9, 115];
const LIGHT_Y: [i16; 4] = [1, 0, 1, 1];
const BOARD_X: [i16; 4] = [53, 53, 55, 55];
const BOARD_Y: [i16; 4] = [3, 1, 0, 0];

/// `display(x)` of finance.md §0.2: an internal DM amount as shown on screen.
fn display(x: i32) -> i32 {
    ldiv(lmul(x, GAME_OPTION_B), GAME_OPTION_A)
}

fn num(host: &Host, v: i32) -> Vec<u8> {
    format_long_thousands(v, host.number_format())
}

fn text_at(host: &mut Host, text: &[u8], x: i16, y: i16, color: u8) {
    ui_draw_text_at(&mut host.g, &mut host.rp, text, x, y, i16::from(color));
}

fn centered(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16, color: u8, shadow: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn line(host: &mut Host, x0: i16, y0: u8, x1: i16, y1: u8, color: u8) {
    ui_draw_line(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn panel(host: &mut Host, x: i16, y: i16, w: i16, h: i16, color: u8) -> [i16; 4] {
    host.ui
        .draw_button(&mut host.g, &mut host.rp, x, y, w, h, false, color)
        .1
}

fn current(state: &GameState) -> usize {
    (state.current_manager as usize).min(3)
}

/// Picture number of the stadium: three sizes by capacity, two looks by the
/// condition.
///
/// Origin: BMMAIN.EXE 1000:16e3 (stadium_draw_picture, picture choice)
pub fn stadium_picture(m: &Manager) -> (u16, usize, i16) {
    let cap = m.stadium[0][0].wrapping_add(m.stadium[1][0]);
    let size = usize::from(cap > 34_000) + usize::from(cap > 14_000) + usize::from(cap > 54_000);
    let cond = (m.stadium[5][0].wrapping_sub(1) >> 1) as u8 as i16;
    let pic = (3 * size as i16 - cond + 2 + 9) as u8;
    (u16::from(pic), size, cond)
}

/// Draws the stadium picture with floodlights and scoreboard into a frame at
/// `(x, y)`.
///
/// Origin: BMMAIN.EXE 1000:16e3 (stadium_draw_picture)
fn stadium_draw_picture(host: &mut Host, m: &Manager, x: i16, y: i16) {
    let (pic, size, cond) = stadium_picture(m);
    let screen = host.screen();
    if let Ok(b) = host.load_picture(pic) {
        panel(host, x, y, 0x8b, 0x52, 7);
        host.g
            .blit_bitmap(b, 0, 0, screen, x + 1, y + 1, 0x8a, 0x51);
        host.g.free(b);
    }
    let lights = m.stadium[3][0] as u16 as i16;
    let board = m.stadium[4][0] as u16 as i16;
    if lights == 0 && board == 0 {
        return;
    }
    let Ok(ov) = host.load_picture(0x28) else {
        return;
    };
    if lights != 0 {
        let v = 3 - lights;
        for j in 0..2 {
            host.g.blit_bitmap_transparent(
                ov,
                0x67 + 0x11 * (v - 3 * cond),
                1,
                screen,
                LIGHT_X[2 * size + j] + x,
                LIGHT_Y[size] + y - 1,
                0x11,
                LIGHT_HEIGHT[size],
            );
        }
    }
    if board != 0 {
        // The original copies the screen rectangle into a 0x28x0x28 scratch
        // bitmap, blits the board transparently onto it and copies it back:
        // the same as a transparent blit onto the screen.
        let sx = (board - 1) * 0x16 + 0x29;
        let sy = 0x43 - 0x21 * cond;
        let dx = BOARD_X[size] + x;
        let dy = BOARD_Y[size] + y;
        host.g
            .blit_bitmap_transparent(ov, sx, sy, screen, dx, dy, 0x1f, 0x14);
    }
    host.g.free(ov);
}

/// The texts of one row of the status table: current value, value after the
/// construction (`None` when nothing is being built) and the weeks left.
///
/// Origin: BMMAIN.EXE 1000:1a49 (stadium_draw_status_panel, row texts)
pub fn status_row(
    m: &Manager,
    r: usize,
    fmt: bmp_game::rng::NumberFormat,
    mod2026: bool,
) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let f = |v: i32| format_long_thousands(v, fmt);
    let s = &m.stadium;
    let (now, after) = match r {
        0 => {
            let now = s[0][0].wrapping_add(s[1][0]);
            let after = (m.build_days[0] != 0 || m.build_days[1] != 0)
                .then(|| f(s[0][1].wrapping_add(s[1][1]).wrapping_add(now)));
            (f(now), after)
        }
        1..=3 => {
            let [cur, pend] = s[r - 1];
            (
                f(cur),
                (m.build_days[r - 1] != 0).then(|| f(cur.wrapping_add(pend))),
            )
        }
        4..=7 => {
            let names: &[&[u8]] = if Manager::stadium_item_is_quality(r - 1) {
                &QUALITY_NAMES
            } else {
                &SIZE_NAMES
            };
            let name = |v: i32| {
                names
                    .get(v as u16 as usize)
                    .copied()
                    .unwrap_or(b"")
                    .to_vec()
            };
            let [cur, pend] = s[r - 1];
            (name(cur), (pend != 0).then(|| name(cur.wrapping_add(pend))))
        }
        _ => {
            let k = if GAME_OPTION_A > 1 { 10 } else { 1 };
            let mut t = f(ldiv(
                lmul(lmul(i32::from(m.value_10a), GAME_OPTION_B), k),
                GAME_OPTION_A,
            ));
            if GAME_OPTION_A > 1 {
                // buf[4] = buf[3]; buf[3] = '.'; buf[5] = '0'; buf[6] = 0.
                t.resize(t.len().max(7), 0);
                t[4] = t[3];
                t[3] = b'.';
                t[5] = b'0';
                t[6] = 0;
                let n = t.iter().position(|&c| c == 0).unwrap_or(t.len());
                t.truncate(n);
            }
            // MOD2026: the mod's ticket price is in euros.
            t.extend_from_slice(currency(mod2026));
            (t, None)
        }
    };
    let weeks = match (&after, r) {
        (Some(_), 1..=7) => {
            let mut fw = fmt;
            fw.pad_width = 2;
            let mut w = format_long_thousands(i32::from(m.build_days[r - 1] / 7 + 1), fw);
            w.extend_from_slice(WEEKS_SHORT);
            Some(w)
        }
        _ => None,
    };
    (now, after, weeks)
}

/// The balance line and, unless `balance_only`, the table of the stadium
/// items now and after the construction.
///
/// Origin: BMMAIN.EXE 1000:1a49 (stadium_draw_status_panel)
fn stadium_draw_status_panel(host: &mut Host, state: &GameState, balance_only: bool) {
    let m = &state.managers[current(state)];
    host.set_font(2);
    host.number_no_separators = false;
    fill(host, 0x9a, 0x59, 0x120, 0x5e, 8);
    let t = money_text(
        BALANCE,
        display(m.balance),
        host.number_format(),
        b"",
        host.mod2026,
    );
    text_at(host, &t, 0x9a, 0x5e, 1);
    if balance_only {
        return;
    }
    fill(host, 7, 0x69, 0x11f, 0xb8, 8);
    text_at(host, NOW, 0x76, 0x6f, 1);
    text_at(host, AFTER, 0xc0, 0x6f, 1);
    for (r, label) in ROW_LABELS.iter().enumerate() {
        let y = 0x77 + 8 * r as i16;
        text_at(host, label, 0xa, y, 2);
        let (now, after, weeks) = status_row(m, r, host.number_format(), host.mod2026);
        text_at(host, &now, 0x76, y, 2);
        let (after, color) = match after {
            Some(a) => (a, 2),
            None => (now, 4),
        };
        if let Some(w) = weeks {
            text_at(host, &w, 0xf3, y, 2);
        }
        text_at(host, &after, 0xc0, y, color);
    }
}

/// The help line at the bottom of the screen: `DS:0044` is its x position (-1
/// = nothing shown), `DS:0046` its width; the strip is restored from the
/// office background in VRAM page 2 before a new text is drawn.
#[derive(Debug, Clone, Copy)]
pub struct StatusLine {
    pub x: i16,
    pub w: i16,
}

impl Default for StatusLine {
    fn default() -> Self {
        Self { x: -1, w: -1 }
    }
}

/// `DS:28d0[10..=14]`: the training-camp legend texts.
const CAMP_LEGEND: [&[u8]; 5] = [
    b"Erholungswert",
    b"Fitnessausr]stung",
    b"Platz-Qualit[t",
    b"Raumausstattung",
    b"Verpflegung",
];

/// What the status line shows (`id` of the original).
pub enum StatusText<'a> {
    /// `0..=9`: the stadium row help.
    Stadium(usize),
    /// `23`, value below 50: "Kredit zu <rate>% aufnehmen."
    BankCredit(i32),
    /// `23`, value a name pointer: "Kredit von <name> aufnehmen."
    ManagerCredit(&'a [u8]),
    /// `24..=26`: the loan prompts.
    Prompt(&'static [u8]),
    /// `10..=14`: the training-camp legend letter `i` (E, F, P, R, V).
    CampLegend(usize),
    /// `15..=22`: a training camp's name and, unless 0, its weekly price.
    CampPrice(&'a [u8], i32),
    /// `-1`: only restore the strip.
    None,
}

/// Shows a help text centred on the status line (prompts left-aligned at x 2).
///
/// Origin: BMMAIN.EXE 1000:243f (status_line_show)
pub fn status_line_show(host: &mut Host, session: &mut Session, text: StatusText) {
    host.set_font(1);
    let sl = session.status_line;
    if sl.x != -1 {
        let screen = host.screen();
        if let Some(work) = host.ui.work {
            host.g
                .blit_bitmap_or_restore_page2(work, sl.x, 0xd9, screen, sl.x, 0xd9, sl.w, 0xa);
        }
    }
    let (t, prompt) = match text {
        StatusText::None => return,
        StatusText::Stadium(i) => (ROW_HELP[i.min(9)].to_vec(), false),
        StatusText::BankCredit(v) => {
            let mut t = b"Kredit zu ".to_vec();
            t.extend_from_slice(&num(host, v));
            t.extend_from_slice(b"% aufnehmen.");
            (t, false)
        }
        StatusText::ManagerCredit(name) => {
            let mut t = b"Kredit von ".to_vec();
            t.extend_from_slice(name);
            t.extend_from_slice(b" aufnehmen.");
            (t, false)
        }
        StatusText::Prompt(p) => (p.to_vec(), true),
        StatusText::CampLegend(i) => (CAMP_LEGEND[i.min(4)].to_vec(), false),
        StatusText::CampPrice(name, price) => {
            let t = if price != 0 {
                camp_price_text(name, price, host.number_format(), host.mod2026)
            } else {
                name.to_vec()
            };
            (t, false)
        }
    };
    let w = host.g.rastport_text_width(&host.rp, &t);
    session.status_line.w = w;
    if prompt {
        session.status_line.x = 2;
        text_at(host, &t, 2, 0xe1, 1);
    } else {
        session.status_line.x = 0x85 - w / 2;
        centered(host, &t, 0, 0x10a, 0xe1, 1, 0);
    }
}

/// Asks to confirm an expansion of row `aa` by `amount` (places or the new
/// level) for `cost`, with a random construction time. Returns 1 when ordered,
/// -1 when declined or unaffordable.
///
/// Origin: BMMAIN.EXE 1000:0000 (stadium_expansion_order)
fn stadium_expansion_order(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    aa: usize,
    amount: i32,
    cost: i32,
) -> Run<i32> {
    let cur = current(state);
    let m = &state.managers[cur];
    if m.balance.wrapping_sub(cost) < 0 {
        ui_dialog_box(host, b"Zu wenig", b"Kleingeld vorhanden...")?;
        return Ok(-1);
    }
    let [it_cur, it_pend] = m.stadium[aa - 1];
    let total = it_pend.wrapping_add(it_cur);
    let r1 = i32::from(session.rng.random_range(20, 80));
    let rest = ldiv(lmul(i32::from(m.build_days[aa - 1]), r1), 100);
    let r2 = i32::from(session.rng.random_range(80, 120));
    let days = ldiv(lmul(7 * BASE_WEEKS[aa] + rest, r2), 100);
    fill(host, 0x97, 0x1b, 0x120, 0x5f, 8);
    host.set_font(2);
    host.number_no_separators = false;
    let (l1, l2) = if aa < 4 {
        let mut l1 = EXPAND_BY.to_vec();
        l1.extend_from_slice(&num(host, amount));
        l1.extend_from_slice(EXPAND_TO);
        let mut l2 = num(host, amount.wrapping_add(total));
        l2.push(b' ');
        l2.extend_from_slice(ROW_LABELS[aa]);
        (l1, l2)
    } else if aa < 6 {
        (NEW_SIZE.to_vec(), SIZE_NAMES[amount as usize & 3].to_vec())
    } else {
        (
            NEW_STATUS.to_vec(),
            QUALITY_NAMES[(amount as usize).min(QUALITY_NAMES.len() - 1)].to_vec(),
        )
    };
    centered(host, &l1, 0x97, 0x120, 0x25, 2, 0xff);
    centered(
        host,
        &l2,
        0x97,
        0x120,
        0x2e,
        if aa < 4 { 2 } else { 1 },
        0xff,
    );
    let l3 = money_text(
        COSTS_2,
        display(cost),
        host.number_format(),
        b"",
        host.mod2026,
    );
    centered(host, &l3, 0x97, 0x120, 0x37, 2, 0xff);
    let l4 = if rest == 0 {
        BUILD_TIME
    } else {
        BUILD_TIME_TOTAL
    };
    centered(host, l4, 0x97, 0x120, 0x40, 2, 0xff);
    let mut l5 = num(host, days / 7 + 1);
    l5.extend_from_slice(WEEKS);
    centered(host, &l5, 0x97, 0x120, 0x49, 1, 0);
    let mut buttons = TextButtons::default();
    buttons.register(host, 0x9e, 0x51, YES, 0x12);
    buttons.register(host, 0xe3, 0x51, NO, 1);
    let r = loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let r = buttons.hit(mx, my);
        if host.button == LEFT_DOWN && r != 0 {
            break r;
        }
    };
    if r == 2 {
        state.construction[aa - 1] = session.rng.random_range(15, 55) as u8;
        return Ok(-1);
    }
    let m = &mut state.managers[cur];
    m.balance = m.balance.wrapping_sub(cost);
    let item = &mut m.stadium[aa - 1];
    item[1] = item[1].wrapping_add(amount);
    if aa > 3 {
        item[1] = item[1].wrapping_sub(total);
    }
    m.build_days[aa - 1] = days as u16;
    Ok(1)
}

/// Places that can still be added in row `aa` (1..3): seats and standing up
/// to 130 000 places in all, roofs up to the seats and standing.
///
/// Origin: BMMAIN.EXE 1000:0602 (screen_stadium, 1000:0820)
pub fn capacity_left(m: &Manager, aa: usize) -> i32 {
    let s = &m.stadium;
    if aa == 3 {
        s[1][0]
            .wrapping_sub(s[2][1])
            .wrapping_sub(s[2][0])
            .wrapping_add(s[0][0])
    } else {
        MAX_CAPACITY
            .wrapping_sub(s[1][0])
            .wrapping_sub(s[0][1])
            .wrapping_sub(s[1][1])
            .wrapping_sub(s[0][0])
    }
}

/// `head`, the shown `amount`, the currency and `tail`: every money text of the stadium screen.
fn money_text(
    head: &[u8],
    amount: i32,
    fmt: bmp_game::rng::NumberFormat,
    tail: &[u8],
    mod2026: bool,
) -> Vec<u8> {
    // MOD2026: the mod's amounts are in euros.
    [
        head,
        &format_long_thousands(amount, fmt),
        currency(mod2026),
        tail,
    ]
    .concat()
}

/// `"KOSTET <amount> DM PRO 1000"` (places) or `"... PRO PUNKT"` (levels) of the expansion
/// panel.
fn price_text(
    amount: i32,
    fmt: bmp_game::rng::NumberFormat,
    per_1000: bool,
    mod2026: bool,
) -> Vec<u8> {
    let per = if per_1000 { PER_1000 } else { PER_POINT };
    money_text(COSTS, amount, fmt, per, mod2026)
}

/// `"<camp>, <price> DM/Woche."` of the status line.
fn camp_price_text(
    name: &[u8],
    price: i32,
    fmt: bmp_game::rng::NumberFormat,
    mod2026: bool,
) -> Vec<u8> {
    money_text(&[name, b", "].concat(), price, fmt, PER_WEEK, mod2026)
}

/// Handles a click on row `aa` (1..8) of the status table. Returns the result
/// of the expansion (-1: nothing changed), or `None` when the dialog "Im
/// Moment keine Baufirma" ended it early.
fn handle_row(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    aa: usize,
    probe: &mut i16,
) -> Run<Option<i32>> {
    // For the ticket price (aa == 8) the original reads FAR:5787, the byte
    // after the seven construction locks, which nothing in the port models.
    if aa < 8 && state.construction[aa - 1] != 0 {
        ui_dialog_box(host, b"Im Moment keine", b"Baufirma aufzutreiben.")?;
        return Ok(None);
    }
    let cur = current(state);
    let cap_left = capacity_left(&state.managers[cur], aa);
    fill(host, 0x97, 0x10, 0x120, 0x5f, 8);
    host.set_font(2);
    centered(host, ROW_HELP[aa], 0x97, 0x120, 0x16, 0xb, 0);
    line(host, 0x97, 0x18, 0x120, 0x18, 0xb);
    let price = PRICES.get(aa).copied().unwrap_or(0);
    let t = if aa < 8 {
        price_text(display(price), host.number_format(), aa < 4, host.mod2026)
    } else {
        Vec::new()
    };
    text_at(host, &t, 0x9a, 0x49, 1);
    let mut res: i32 = -1;
    let mut max: i32 = 0;
    'body: {
        if aa < 4 {
            let afford = lmul(ldiv(state.managers[cur].balance, price), 1000).max(0);
            if afford == 0 {
                ui_dialog_box(host, b"Kein Geld f}r", b"neue Pl{tze da...")?;
                break 'body;
            }
            max = cap_left.min(afford);
            let mut t = MAX_SIZE.to_vec();
            t.extend_from_slice(&num(host, max));
            text_at(host, &t, 0x9a, 0x50, 1);
            let t = money_text(
                COSTS_2,
                display(lmul(price, max / 1000)),
                host.number_format(),
                b".",
                host.mod2026,
            );
            text_at(host, &t, 0x9a, 0x57, 1);
        }
        stadium_draw_status_panel(host, state, true);
        line(host, 0x97, 0x42, 0x120, 0x42, 0xb);
        match aa {
            1..=3 => {
                let amt = ui_value_spinner(host, 0, 0xb8, 0x1b, 0, max, 1000)?;
                if amt != -1 {
                    let cost = lmul(ldiv(amt, 1000), price);
                    res = stadium_expansion_order(host, session, state, aa, amt, cost)?;
                }
            }
            8 => {
                let (step, k) = if GAME_OPTION_A > 1 { (5, 10) } else { (1, 1) };
                let t = lmul(GAME_OPTION_B, k);
                let ticket = i32::from(state.managers[cur].value_10a);
                let v = ui_value_spinner(
                    host,
                    ldiv(lmul(lmul(ticket, GAME_OPTION_B), k), GAME_OPTION_A),
                    0xb8,
                    0x1b,
                    ldiv(lmul(t, 5), GAME_OPTION_A),
                    ldiv(lmul(t, 25), GAME_OPTION_A),
                    step,
                )?;
                if v != -1 {
                    let price = ldiv(ldiv(lmul(v, GAME_OPTION_A), GAME_OPTION_B), k);
                    state.managers[cur].value_10a = price as u8;
                }
                res = v;
            }
            _ => {
                res = choose_level(host, session, state, aa, price, probe)?;
            }
        }
    }
    fill(host, 0x97, 0x10, 0x120, 0x5f, 8);
    stadium_draw_status_panel(host, state, res == -1);
    line(host, 0x97, 0x54, 0x120, 0x54, 0xb);
    Ok(Some(res))
}

/// The level options of rows 4..7 (floodlights, scoreboard, condition,
/// comfort) and the XOR-highlighted selection loop.
///
/// Origin: BMMAIN.EXE 1000:0602 (screen_stadium, 1000:0eb8..1000:149b)
fn choose_level(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    aa: usize,
    price: i32,
    probe: &mut i16,
) -> Run<i32> {
    let cur_m = current(state);
    let m = &state.managers[cur_m];
    let cur = m.stadium_level(aa - 1);
    let quality = Manager::stadium_item_is_quality(aa - 1);
    let names: &[&[u8]] = if quality { &QUALITY_NAMES } else { &SIZE_NAMES };
    let top = names.len() as i16 - 1;
    if top == cur {
        ui_dialog_box(host, b"Was denn...?", b"NOCH besser ???")?;
        return Ok(-1);
    }
    let afford = ldiv(m.balance, price);
    if afford <= 0 {
        ui_dialog_box(host, b"Da m}ssen Sie erst", b"noch sparen...")?;
        return Ok(-1);
    }
    let mut lim = i32::from(top).min(afford) + i32::from(cur);
    if lim > i32::from(top) {
        lim = i32::from(top);
    }
    host.number_pad = 9;
    let (first, base_y) = if quality { (2, 0x12) } else { (1, 0x19) };
    for l in first..=top {
        let y = 7 * l + base_y;
        let color = if l > cur && i32::from(l) <= lim { 1 } else { 3 };
        text_at(host, names[l as usize], 0x9a, y, color);
        if l > cur || cur == 0 {
            let t = money_text(
                b"",
                display(lmul(i32::from(l - cur), price)),
                host.number_format(),
                b"",
                host.mod2026,
            );
            text_at(host, &t, 0xd8, y, color);
        }
    }
    let lim_name = names[lim as usize];
    let max_row: i16 = if quality { 4 } else { 2 };
    host.number_pad = 1;
    let mut t = MAX_STATUS.to_vec();
    t.extend_from_slice(lim_name);
    text_at(host, &t, 0x9a, 0x50, 1);
    let t = money_text(
        COSTS_OPEN,
        display(lmul(lim - i32::from(cur), price)),
        host.number_format(),
        b")",
        host.mod2026,
    );
    text_at(host, &t, 0x9a, 0x57, 1);
    Gfx::rastport_set_draw_mode(&mut host.rp, 2);
    let mut sel: i16 = -1;
    let highlight = |host: &mut Host, row: i16| {
        fill(host, 0x97, (row + 4) * 7, 0x11f, 7 * row + 0x20, 1);
    };
    loop {
        host.input_poll_mouse()?;
        if host.button == RIGHT_DOWN {
            sel = -1;
            break;
        }
        let level = sel + i16::from(quality) + 1;
        if host.button == LEFT_DOWN && sel != -1 && i32::from(level) <= lim && level > cur {
            break;
        }
        let (mx, my) = host.mouse_xy();
        *probe = mx;
        if mx > 0x96 && mx < 0x120 && my > 0x19 && my < 0x42 {
            let row = (my - 0x1c) / 7;
            if row > max_row || sel == row {
                continue;
            }
            if sel != -1 {
                highlight(host, sel);
            }
            sel = row;
            highlight(host, row);
        } else if sel != -1 {
            highlight(host, sel);
            sel = -1;
        }
    }
    Gfx::rastport_set_draw_mode(&mut host.rp, 0);
    if sel == -1 {
        return Ok(-1);
    }
    let level = sel + i16::from(quality) + 1;
    let cost = lmul(i32::from(level - cur), price);
    stadium_expansion_order(host, session, state, aa, i32::from(level), cost)
}

/// The stadium: picture, expansion panel with the balance and the table of
/// the stadium items. Hovering a row shows its help on the status line, a
/// click starts the expansion of that item (or changes the ticket price).
///
/// Origin: BMMAIN.EXE 1000:0602 (screen_stadium)
pub fn screen_stadium(host: &mut Host, session: &mut Session, state: &mut GameState) -> Run<()> {
    // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
    let m = state.managers[current(state)].clone();
    stadium_draw_picture(host, &m, 6, 0xf);
    panel(host, 0x96, 0xf, 0x8b, 0x52, 8);
    panel(host, 6, 0x68, 0x11b, 0x52, 8);
    let exit = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x10c, 0xc1, 0, 0x22, false, 8)
        .1;
    host.set_font(2);
    stadium_draw_status_panel(host, state, false);
    line(host, 0x97, 0x54, 0x120, 0x54, 0xb);
    host.screen_fade(1)?;
    let mut hover: i16 = -1;
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let mut si = Ui::hit_test_buttons(&[exit], 1, mx, my, Some(ev as i16));
        // [bp-0xae]: the row in the table, else the x coordinate (compared
        // with the row below, a quirk of the original).
        let mut probe = mx;
        let mut early = false;
        if mx > 6 && mx < 0x121 && my > 0x72 && my < 0xb9 {
            let aa = (my - 0x72) >> 3;
            probe = aa;
            if aa != hover {
                status_line_show(host, session, StatusText::Stadium(aa as usize));
                hover = aa;
            }
            if host.button == LEFT_DOWN && hover != 0 {
                // The level selection loop leaves the pointer x in [bp-0xae].
                match handle_row(host, session, state, hover as usize, &mut probe)? {
                    None => early = true,
                    Some(_) => si = 0,
                }
            }
        }
        if !early && hover != probe && hover != -1 {
            status_line_show(host, session, StatusText::None);
            hover = -1;
            host.set_font(2);
        }
        if si == 1 || session.auto_play {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::assert_mod_texts_narrower;
    use bmp_game::rng::NumberFormat;

    fn fmt() -> NumberFormat {
        NumberFormat {
            no_separators: false,
            comma_separator: false,
            pad_width: 1,
        }
    }

    fn manager() -> Manager {
        let mut m = GameState::zeroed().managers[0].clone();
        m.stadium = [[0, 0], [15_000, 0], [0, 0], [0, 0], [0, 0], [4, 0], [4, 0]];
        m.value_10a = 13;
        m
    }

    #[test]
    fn picture_by_capacity_and_condition() {
        let mut m = manager();
        assert_eq!(stadium_picture(&m), (13, 1, 1));
        m.stadium[1][0] = 10_000;
        m.stadium[5][0] = 6;
        assert_eq!(stadium_picture(&m).0, 9);
        m.stadium[0][0] = 60_000;
        m.stadium[5][0] = 1;
        assert_eq!(stadium_picture(&m).0, 20);
    }

    #[test]
    fn capacity_left_of_places_and_roofs() {
        let mut m = manager();
        m.stadium[0] = [2000, 500];
        m.stadium[1] = [15_000, 1000];
        m.stadium[2] = [3000, 400];
        assert_eq!(capacity_left(&m, 1), 130_000 - 15_000 - 500 - 1000 - 2000);
        assert_eq!(capacity_left(&m, 3), 15_000 - 400 - 3000 + 2000);
    }

    #[test]
    fn status_rows_now_and_after() {
        let mut m = manager();
        let row = |m: &Manager, r| status_row(m, r, fmt(), false);
        assert_eq!(row(&m, 0), (b"15.000".to_vec(), None, None));
        assert_eq!(row(&m, 4), (b"IS NICH".to_vec(), None, None));
        assert_eq!(row(&m, 6), (b"BEFRIEDIGEND".to_vec(), None, None));
        assert_eq!(row(&m, 8), (b"13 DM".to_vec(), None, None));
        m.stadium[1][1] = 2000;
        m.build_days[1] = 20;
        assert_eq!(
            row(&m, 2),
            (
                b"15.000".to_vec(),
                Some(b"17.000".to_vec()),
                Some(b"^3 WOCHE(N)".to_vec())
            )
        );
        assert_eq!(row(&m, 0).1, Some(b"17.000".to_vec()));
        m.stadium[6][1] = 2;
        m.build_days[6] = 6;
        assert_eq!(
            row(&m, 7),
            (
                b"BEFRIEDIGEND".to_vec(),
                Some(b"SEHR GUT".to_vec()),
                Some(b"^1 WOCHE(N)".to_vec())
            )
        );
    }

    /// MOD2026: the ticket price and the expansion panel texts in euros.
    #[test]
    fn amounts_in_euros() {
        let m = manager();
        let ticket = |mod2026| status_row(&m, 8, fmt(), mod2026).0;
        assert_eq!(ticket(true), b"13 \xd5");
        assert_eq!(ticket(false), b"13 DM");
        assert_eq!(
            price_text(150_000, fmt(), true, true),
            b"KOSTET 150.000 \xd5 PRO 1000"
        );
        assert_eq!(
            price_text(150_000, fmt(), true, false),
            b"KOSTET 150.000 DM PRO 1000"
        );
        assert_eq!(
            price_text(380_000, fmt(), false, false),
            b"KOSTET 380.000 DM PRO PUNKT"
        );
        assert_eq!(
            camp_price_text(b"Trlg. Hackstedt", 25_000, fmt(), true),
            b"Trlg. Hackstedt, 25.000 \xd5/Woche."
        );
        assert_eq!(
            camp_price_text(b"Trlg. Hackstedt", 25_000, fmt(), false),
            b"Trlg. Hackstedt, 25.000 DM/Woche."
        );
        let texts = |mod2026| {
            vec![
                price_text(150_000, fmt(), true, mod2026),
                price_text(380_000, fmt(), false, mod2026),
                ticket(mod2026),
                camp_price_text(b"Trlg. Hackstedt", 25_000, fmt(), mod2026),
                money_text(BALANCE, -1_234_567, fmt(), b"", mod2026),
                money_text(COSTS_2, 4_500_000, fmt(), b"", mod2026),
                money_text(COSTS_2, 4_500_000, fmt(), b".", mod2026),
                money_text(COSTS_OPEN, 1_140_000, fmt(), b")", mod2026),
            ]
        };
        assert_eq!(texts(true)[6], b" KOSTEN: 4.500.000 \xd5.");
        assert_eq!(texts(false)[7], b"(KOSTEN: 1.140.000 DM)");
        let pairs: Vec<_> = texts(true).into_iter().zip(texts(false)).collect();
        assert_mod_texts_narrower(&pairs);
    }

    /// MOD2026: in the 9-padded level cost column (font 2 at x 0xd8) every row keeps the
    /// right edge of its amount, and its text ends no further right than with "DM".
    #[test]
    fn level_cost_column_keeps_its_amount_edge() {
        let mut fmt9 = fmt();
        fmt9.pad_width = 9;
        let row = |v, mod2026| money_text(b"", v, fmt9, b"", mod2026);
        let Some(g) = crate::game::euro_gfx() else {
            eprintln!("skipped: no BMP_DATA");
            return;
        };
        for v in [38_000, 380_000, 1_140_000, 15_200_000] {
            let (modded, original) = (row(v, true), row(v, false));
            let amount_edge = |t: &[u8], unit: &[u8]| {
                assert!(t.ends_with(unit), "{v}");
                g.text_width(2, &t[..t.len() - unit.len()])
            };
            assert_eq!(
                amount_edge(&modded, currency(true)),
                amount_edge(&original, currency(false)),
                "{v}"
            );
            assert!(
                g.text_width(2, &modded) <= g.text_width(2, &original),
                "{v}"
            );
        }
        assert_eq!(row(380_000, false), b"^^380.000 DM");
    }
}
