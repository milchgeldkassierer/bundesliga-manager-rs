//! Package "club": selecting a club and editing its data on the main screen (spec:
//! `docs/re/teamed/club.md`).
//!
//! The panel and the player list edit the copies ([`Editor::copy`]); "AENDERUNG
//! SICHERN" copies them back into the `MANA.DAT` buffers ([`Editor::mana`]), "NICHT
//! UEBERNEHMEN" only ends the selection. The league swap exchanges two clubs in both.

use bmp_formats::mana::{self, CLUB_NAME_LEN, GERMAN_CLUB_COUNT, ManaDat, PLAYER_NAME_LEN};
use bmp_game::league_layout;

use crate::gfx::{self, Gfx};
use crate::host::{Host, LEFT_DOWN, Run};

use super::dialog::{self, text_input};
use super::{
    CREST_PITCH, CREST_SIZE, Editor, GRID_ORIGIN_Y, draw_hover_panel, draw_page, draw_panel,
    league_group, screen_set_visible, set_font,
};

/// Highest rank the panel accepts for the pool (`DS:033C` entry 3).
const POOL_RANKS: u8 = 20;

/// Highest rank the panel accepts in league group `group` (`DS:033C`: 18, 20, 20, 20): the
/// tier's club count, [`POOL_RANKS`] for the pool.
///
/// MOD2026: the club counts come from the selected league layout.
pub fn group_ranks(group: usize) -> u8 {
    league_layout::layout()
        .team_count
        .get(group)
        .copied()
        .unwrap_or(POOL_RANKS)
}

/// The two 38x38 boxes beside the buttons that show the selected crest.
const BOX_LEFT_X: i16 = 2;
const BOX_RIGHT_X: i16 = 0x73;
const BOX_Y: i16 = 0x2e;

/// Click on the top-left pixel: toggles the debug switch `DS:0340` and shows it as a
/// 3x3 square at the top right in colour `3 * (flag + 5)` (15 off, 18 on).
///
/// Origin: TEAMED.EXE 12a5:04eb (teamed_main_loop, debug switch)
pub fn toggle_debug(g: &mut Gfx, ed: &mut Editor) {
    ed.debug_flag ^= 1;
    gfx::ui_fill_rect(
        g,
        &mut ed.screen,
        0x13d,
        0,
        0x13f,
        2,
        debug_color(ed.debug_flag),
    );
}

/// Colour of the debug square: `3 * (flag + 5)`.
fn debug_color(flag: u8) -> u8 {
    (flag + 5) * 3
}

/// Click on a crest with no club selected: the crest goes into both boxes beside the
/// buttons and into the two scratch pictures, `DS:0336` = club, the club's player names
/// are copied into the edit buffer, and the lower half becomes the crest editor.
///
/// Origin: TEAMED.EXE 12a5:11b7 (teamed_main_loop, select club)
pub fn select(host: &mut Host, ed: &mut Editor) -> Run<()> {
    screen_set_visible(&mut host.g, false);
    let club = ed.hovered as u8;
    // The source is DS:1E48[club], the bitmap of the crest picture DS:5698[club].
    let crest = ed.crests[usize::from(club)];
    let g = &mut host.g;
    let screen = ed.screen.bitmap;
    g.blit_bitmap(
        crest, 0, 0, screen, BOX_LEFT_X, BOX_Y, CREST_SIZE, CREST_SIZE,
    );
    g.blit_bitmap(
        crest,
        0,
        0,
        screen,
        BOX_RIGHT_X,
        BOX_Y,
        CREST_SIZE,
        CREST_SIZE,
    );
    g.blit_bitmap(crest, 0, 0, ed.scratch[1], 0, 0, CREST_SIZE, CREST_SIZE);
    g.blit_bitmap(crest, 0, 0, ed.scratch[0], 0, 0, CREST_SIZE, CREST_SIZE);
    ed.selected = club;
    let c = usize::from(club);
    ed.copy.clubs[c].players = ed.mana.clubs[c].players.clone();
    super::crest::draw_editor(host, ed, crest, false)
}

/// Clicks on the information panel while a club is selected (x 0x35..0x97, y 3..0x27,
/// row `(y - 3) / 8`): name, league swap, rank, points, goals. Returns the pointer
/// position `(x, y - 75)` the league-swap prompt ended with, which replaces the main
/// loop's pointer copies for the rest of its iteration.
///
/// Origin: TEAMED.EXE 12a5:056b (teamed_main_loop, panel editing)
pub fn edit_panel(
    host: &mut Host,
    ed: &mut Editor,
    x: i16,
    y: i16,
    ev: u16,
) -> Run<Option<(i16, i16)>> {
    if !(x > 0x34 && x < 0x98 && y > 2 && y < 0x28 && ed.selected != 0xff && ev == LEFT_DOWN) {
        return Ok(None);
    }
    let sel = ed.selected;
    let group = league_group(sel);
    let mut row = (y - 3) / 8;
    let mut value = 0;
    if row != 1 {
        gfx::ui_fill_rect(
            &mut host.g,
            &mut ed.screen,
            0x33,
            row * 8 + 3,
            0x95,
            (row + 1) * 8,
            0xc,
        );
        value = text_input(
            host,
            &mut ed.screen,
            &mut ed.input_buf,
            0x35,
            row * 8 + 7,
            0x4d,
            0xb,
            0xc,
            row != 2,
        )?;
        if ed.input_buf[0] == 0 {
            row = 9;
        }
    }
    let input = mana::c_str(&ed.input_buf).to_vec();
    let mut pos = None;
    match row {
        0 => {
            if name_exists(&ed.mana, &input) && ed.debug_flag != 0 {
                dialog::message(host, ed, b"DIESER TEAMNAME", b"EXISTIERT BEREITS.")?;
            } else {
                strncpy(&mut ed.copy.clubs[usize::from(sel)].name, &input);
            }
        }
        1 => pos = Some(swap_prompt(host, ed)?),
        2 => {
            if !set_rank(&mut ed.copy, sel, group, value as i16) {
                dialog::message(host, ed, b"DIESER TABELLENPLATZ", b"EXISTIERT NICHT.")?;
            }
        }
        // The 16-bit sum wraps like the original's ADD (parse_pair accepts wrapped,
        // negative values such as 40000 -> -25536).
        3 => match parse_pair(&input) {
            Some((a, b)) if a.wrapping_add(b) % 2 == 0 => {
                ed.copy.points[usize::from(sel)] = [a as u8, b as u8];
            }
            _ => dialog::message(host, ed, b"FALSCHE", b"EINGABE")?,
        },
        4 => match parse_pair(&input) {
            Some((a, b)) => ed.copy.goals[usize::from(sel)] = [a as u8, b as u8],
            None => dialog::message(host, ed, b"FALSCHE", b"EINGABE")?,
        },
        _ => {}
    }
    // 12a5:0790
    if ed.selected != 0xff {
        draw_panel(&mut host.g, &mut ed.screen, &ed.copy, ed.selected);
    }
    Ok(pos)
}

/// Whether one of the 200 club names in the `MANA.DAT` buffer (not the edit copy, and
/// including the selected club itself) equals `name` (`strcmp`).
fn name_exists(m: &ManaDat, name: &[u8]) -> bool {
    m.clubs.iter().any(|c| mana::c_str(&c.name) == name)
}

/// C `strncpy(dst, src, dst.len())`: copies up to the field length and pads with NULs.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    for (i, d) in dst.iter_mut().enumerate() {
        *d = src.get(i).copied().unwrap_or(0);
    }
}

/// Sets the rank of club `sel` (league group `group`) in the copy. In the two
/// Bundesligas the club that had that rank gets `sel`'s old one. Returns `false` for a
/// rank the group does not have: below 1 (0 is allowed in groups 2 and 3) or above
/// [`group_ranks`].
///
/// Origin: TEAMED.EXE 12a5:0e7a (teamed_main_loop, rank input)
pub fn set_rank(copy: &mut ManaDat, sel: u8, group: usize, value: i16) -> bool {
    if value < 0 || (value == 0 && group <= 1) || i16::from(group_ranks(group)) < value {
        return false;
    }
    let sel = usize::from(sel);
    if group < 2 {
        // The original scans `DS:033C` clubs from `DS:0338` (0, 18): the tier's club ids.
        let mut ids = league_layout::layout().team_ids(group).map(usize::from);
        if let Some(j) = ids.find(|&j| i16::from(copy.rank[j]) == value) {
            copy.rank[j] = copy.rank[sel];
        }
    }
    copy.rank[sel] = value as u8;
    true
}

/// Parses `"<a>:<b>"` from the right: the digits after the last `:` give `b`, the ones
/// before it `a`. Any other character, or a value of 255 or more, makes it invalid.
/// An input without a colon reads the bytes before the buffer in the original (stack
/// contents); the port treats it as invalid, which those bytes (a far pointer's
/// segment) give in practice.
///
/// Origin: TEAMED.EXE 1000:207d (parse_pair)
pub fn parse_pair(s: &[u8]) -> Option<(i16, i16)> {
    let colon = s.iter().rposition(|&c| c == b':')?;
    let number = |digits: &[u8]| {
        let mut v = 0i16;
        let mut mult = 1i16;
        for &c in digits.iter().rev() {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v.wrapping_add(i16::from(c - b'0').wrapping_mul(mult));
            mult = mult.wrapping_mul(10);
        }
        Some(v)
    };
    let b = number(&s[colon + 1..])?;
    let a = number(&s[..colon])?;
    (a < 0xff && b < 0xff).then_some((a, b))
}

/// The league-swap prompt: "MIT WEM TAUSCHEN ?" over the cleared league row, the
/// crest grid with the hover panel, the left and right edge flip the page; a click on
/// a crest swaps the selected club with that one (nothing when it is the selected club
/// itself) and ends the selection. Returns the last pointer position `(x, y - 75)`.
///
/// Origin: TEAMED.EXE 12a5:0869 (teamed_main_loop, league swap)
fn swap_prompt(host: &mut Host, ed: &mut Editor) -> Run<(i16, i16)> {
    let mut target: i16 = -1;
    screen_set_visible(&mut host.g, false);
    let page = ed.page;
    draw_page(&mut host.g, ed, page);
    gfx::ui_fill_rect(&mut host.g, &mut ed.screen, 0x35, 0xb, 0x96, 0xf, 0xc);
    gfx::ui_draw_text_at(
        &mut host.g,
        &mut ed.screen,
        b"MIT WEM TAUSCHEN ?",
        0x35,
        0xf,
        0x1d,
    );
    screen_set_visible(&mut host.g, true);
    let (x, y_rel) = loop {
        // This loop reads the pointer before polling the button event.
        let (x, y) = host.mouse_xy();
        let y_rel = y.wrapping_sub(GRID_ORIGIN_Y);
        let ev = host.input_poll_mouse()?;
        if ev == LEFT_DOWN && (x == 0 || x == 0x13f) {
            screen_set_visible(&mut host.g, false);
            ed.page ^= 1;
            let page = ed.page;
            draw_page(&mut host.g, ed, page);
            screen_set_visible(&mut host.g, true);
        }
        if x < 0x137 && y_rel > 0xb && y_rel < 0xa8 {
            let club =
                x / CREST_PITCH + ((y_rel - 0xc) / CREST_PITCH) * 8 + i16::from(ed.page) * 32;
            if club != target {
                target = club;
                draw_hover_panel(&mut host.g, ed, club as u8);
                continue;
            }
        } else if target != -1 {
            gfx::ui_fill_rect(&mut host.g, &mut ed.screen, 0x33, 3, 0x95, 0x29, 0xc);
            target = -1;
        }
        if ev == LEFT_DOWN && target != -1 {
            break (x, y_rel);
        }
    };
    let sel = ed.selected;
    if target != i16::from(sel) {
        let t = target as usize;
        swap_clubs(&mut ed.mana, &mut ed.copy, usize::from(sel), t);
        ed.crests.swap(usize::from(sel), t);
        // DS:5996[sel] and DS:5996[target] are set to 1 here; nothing reads them.
    }
    deselect(host, ed);
    Ok((x, y_rel))
}

/// The league swap of clubs `a` and `b` (`mem_swap` 1000:012b on each buffer): name,
/// points, goals, rank and players in the `MANA.DAT` buffers and in the copies, and the
/// crest number. The strengths stay (the save recomputes them for all German clubs).
///
/// Origin: TEAMED.EXE 12a5:0aba (teamed_main_loop, league swap), 1000:012b (mem_swap)
pub fn swap_clubs(m: &mut ManaDat, copy: &mut ManaDat, a: usize, b: usize) {
    for buf in [&mut *m, &mut *copy] {
        buf.clubs.swap(a, b);
        buf.points.swap(a, b);
        buf.goals.swap(a, b);
        buf.rank.swap(a, b);
    }
    m.crest.swap(a, b);
}

/// Ends the selection: the crest page again, both boxes and the player list cleared.
///
/// Origin: TEAMED.EXE 12a5:1544 (teamed_main_loop, deselect; also 12a5:0d4a)
fn deselect(host: &mut Host, ed: &mut Editor) {
    ed.selected = 0xff;
    screen_set_visible(&mut host.g, false);
    let page = ed.page;
    draw_page(&mut host.g, ed, page);
    let (g, rp) = (&mut host.g, &mut ed.screen);
    gfx::ui_fill_rect(g, rp, BOX_LEFT_X, BOX_Y, 0x27, 0x53, 0x1d);
    gfx::ui_fill_rect(g, rp, BOX_RIGHT_X, BOX_Y, 0x98, 0x53, 0x1d);
    ed.crest_changed = false;
    for i in 0..10i16 {
        gfx::ui_fill_rect(g, rp, 0xa3, (i + 2) * 7, 0xea, i * 7 + 0x13, 0xd);
        gfx::ui_fill_rect(g, rp, 0xf6, (i + 2) * 7, 0x13d, i * 7 + 0x13, 0xd);
    }
    screen_set_visible(g, true);
}

/// "NICHT UEBERNEHMEN" (row 0) / "AENDERUNG SICHERN" (row 1) while a club is selected;
/// row 2 (the "PROGRAMM-ENDE" button) only deselects as well. "AENDERUNG SICHERN" copies
/// all copies back (every club, not just the selected one) and writes an edited crest.
/// "NICHT UEBERNEHMEN" leaves the copies as they are: the discarded panel edits stay in
/// them and are committed by the next "AENDERUNG SICHERN" of any club (only the player
/// names of a club are refreshed when it is selected).
///
/// Origin: TEAMED.EXE 12a5:1361 (teamed_main_loop, club buttons)
pub fn buttons(host: &mut Host, ed: &mut Editor, y: i16) -> Run<()> {
    if (y - 0x2e) / 0xd == 1 {
        commit(&mut ed.mana, &ed.copy);
        if ed.crest_changed {
            // "pic\<crest>.vga" from the scratch picture [BP-58], which then replaces
            // the club's crest picture.
            super::crest::save_crest(host, ed)?;
        }
    }
    deselect(host, ed);
    Ok(())
}

/// "AENDERUNG SICHERN": `far_memcpy` of the copies back into the `MANA.DAT` buffers
/// (all 200 names, points, goals, ranks and the player names of the 64 German clubs;
/// also `DS:371E -> DS:4742`, a dead buffer).
///
/// Origin: TEAMED.EXE 12a5:13e2 (teamed_main_loop, commit)
pub fn commit(m: &mut ManaDat, copy: &ManaDat) {
    for (dst, src) in m.clubs.iter_mut().zip(&copy.clubs) {
        dst.name = src.name;
    }
    m.points = copy.points;
    m.goals = copy.goals;
    m.rank = copy.rank;
    for i in 0..GERMAN_CLUB_COUNT {
        m.clubs[i].players = copy.clubs[i].players.clone();
    }
}

/// Click on a player row of the selected club (x > 0xA1, y 0xE..0x52): the name is
/// entered in place (row `(y - 0xE) / 7`, the right column from x 0xF0); with the debug
/// switch on, a name that any of the 1280 player slots of the copy already has is
/// refused with "DIESER NAME / EXISTIERT BEREITS.".
///
/// Origin: TEAMED.EXE 12a5:2028 (teamed_main_loop, player names)
pub fn edit_player(host: &mut Host, ed: &mut Editor, x: i16, y: i16) -> Run<()> {
    if !(ed.selected != 0xff && x > 0xa1 && y > 0xd && y < 0x53) {
        return Ok(());
    }
    set_font(&mut ed.screen, 3);
    let row = (y - 0xe) / 7;
    let col = i16::from(x > 0xef);
    let slot = (col * 10 + row) as usize;
    let sel = usize::from(ed.selected);
    let x0 = col * 0x53 + 0xa3;
    let x1 = col * 0x53 + 0xea;
    gfx::ui_fill_rect(
        &mut host.g,
        &mut ed.screen,
        x0,
        (row + 2) * 7,
        x1,
        row * 7 + 0x13,
        0xd,
    );
    text_input(
        host,
        &mut ed.screen,
        &mut ed.input_buf,
        x0,
        row * 7 + 0x12,
        0x34,
        0xb,
        0xd,
        true,
    )?;
    if ed.input_buf[0] != 0 {
        let input = mana::c_str(&ed.input_buf).to_vec();
        if ed.debug_flag != 0 && player_exists(&ed.copy, &input) {
            dialog::message(host, ed, b"DIESER NAME", b"EXISTIERT BEREITS.")?;
        } else {
            strncpy(&mut ed.copy.clubs[sel].players[slot], &input);
        }
    }
    gfx::ui_fill_rect(
        &mut host.g,
        &mut ed.screen,
        x0,
        (row + 2) * 7,
        x1,
        row * 7 + 0x13,
        0xd,
    );
    let name = mana::c_str(&ed.copy.clubs[sel].players[slot]).to_vec();
    gfx::ui_draw_text_at(&mut host.g, &mut ed.screen, &name, x0, row * 7 + 0x12, 0xb);
    Ok(())
}

/// Whether one of the 1280 player slots of the copy (`[213E]`) equals `name`.
fn player_exists(copy: &ManaDat, name: &[u8]) -> bool {
    copy.clubs
        .iter()
        .take(GERMAN_CLUB_COUNT)
        .flat_map(|c| c.players.iter())
        .any(|p| mana::c_str(p) == name)
}

// The field sizes the strncpy calls of the original use (0x17 and 0x1A).
const _: () = assert!(CLUB_NAME_LEN == 0x17 && PLAYER_NAME_LEN == 0x1a);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ManaDat {
        let mut bytes = vec![0u8; mana::FILE_SIZE];
        for i in 0..GERMAN_CLUB_COUNT {
            let at = mana::CLUBS_OFFSET + i * mana::GERMAN_CLUB_RECORD_LEN;
            bytes[at..at + 4].copy_from_slice(format!("C{i:03}").as_bytes());
            let p = at + CLUB_NAME_LEN;
            bytes[p..p + 4].copy_from_slice(format!("P{i:03}").as_bytes());
            bytes[mana::RANK_OFFSET + i] = (i % 18 + 1) as u8;
            bytes[mana::POINTS_OFFSET + 2 * i] = i as u8;
            bytes[i] = 0x50 + i as u8;
        }
        ManaDat::parse(&bytes).unwrap()
    }

    #[test]
    fn debug_square_colours() {
        assert_eq!((debug_color(0), debug_color(1)), (15, 18));
    }

    #[test]
    fn parse_pair_reads_both_sides_of_the_colon() {
        assert_eq!(parse_pair(b"40:28"), Some((40, 28)));
        assert_eq!(parse_pair(b":7"), Some((0, 7)));
        assert_eq!(parse_pair(b"7:"), Some((7, 0)));
        assert_eq!(parse_pair(b"1:2:3"), None);
        assert_eq!(parse_pair(b"4A:2"), None);
        assert_eq!(parse_pair(b"255:0"), None);
        assert_eq!(parse_pair(b"254:254"), Some((254, 254)));
        assert_eq!(parse_pair(b"12"), None);
        // 16-bit wrap-around as in the original: 65537 -> 1, 40000 -> -25536 (accepted,
        // since only values >= 255 are refused).
        assert_eq!(parse_pair(b"65537:3"), Some((1, 3)));
        assert_eq!(parse_pair(b"40000:40000"), Some((-25536, -25536)));
    }

    #[test]
    fn rank_swaps_within_the_bundesligas() {
        let mut m = sample();
        // Club 0 has rank 1, club 4 rank 5.
        assert!(set_rank(&mut m, 0, 0, 5));
        assert_eq!((m.rank[0], m.rank[4]), (5, 1));
        assert!(!set_rank(&mut m, 0, 0, 0));
        assert!(!set_rank(&mut m, 0, 0, 19));
        assert!(!set_rank(&mut m, 0, 0, -1));
        // Group 2: 0 is allowed and nothing is swapped.
        let before = m.rank;
        assert!(set_rank(&mut m, 40, 2, 0));
        assert_eq!(m.rank[40], 0);
        assert!(set_rank(&mut m, 40, 2, 3));
        assert_eq!(m.rank[..40], before[..40]);
        assert!(!set_rank(&mut m, 40, 2, 21));
    }

    /// MOD2026: the rank limits and the swap follow the mod's 18 / 18 / 20 tiers.
    #[test]
    fn mod_ranks_follow_the_league_layout() {
        use bmp_game::league_layout::{MOD2026, ORIGINAL, set_layout};
        set_layout(&MOD2026);
        let mut m = sample();
        let group = super::league_group;
        assert_eq!((group(20), group(40)), (1, 2));
        assert_eq!([0, 1, 2, 3].map(group_ranks), [18, 18, 20, 20]);
        assert!(!set_rank(&mut m, 20, 1, 19));
        // No 2. Liga club has rank 18, so club 36 (now 3. Liga) keeps its 18.
        m.rank[35] = 0;
        m.rank[36] = 18;
        assert!(set_rank(&mut m, 20, 1, 18));
        assert_eq!((m.rank[20], m.rank[36]), (18, 18));
        assert!(set_rank(&mut m, 40, 2, 20));
        assert!(!set_rank(&mut m, 40, 2, 21));
        set_layout(&ORIGINAL);
        assert_eq!([0, 1, 2, 3].map(group_ranks), [18, 20, 20, 20]);
    }

    #[test]
    fn swap_exchanges_both_buffers_but_not_strengths() {
        let mut m = sample();
        m.strength[2] = [1, 2, 3];
        let mut copy = m.clone();
        copy.clubs[2].name[0] = b'X';
        swap_clubs(&mut m, &mut copy, 2, 30);
        assert_eq!(&m.clubs[30].name[..4], b"C002");
        assert_eq!(&m.clubs[2].players[0][..4], b"P030");
        assert_eq!(copy.clubs[30].name[0], b'X');
        assert_eq!((m.points[2][0], m.points[30][0]), (30, 2));
        assert_eq!((m.crest[2], m.crest[30]), (0x50 + 30, 0x50 + 2));
        assert_eq!(m.strength[2], [1, 2, 3]);
    }

    #[test]
    fn commit_copies_every_club_back() {
        let mut m = sample();
        let mut copy = m.clone();
        copy.clubs[1].name[0] = b'Y';
        copy.clubs[63].players[19][0] = b'Z';
        copy.goals[5] = [9, 8];
        copy.crest[0] = 0;
        commit(&mut m, &copy);
        assert_eq!(m.clubs[1].name[0], b'Y');
        assert_eq!(m.clubs[63].players[19][0], b'Z');
        assert_eq!(m.goals[5], [9, 8]);
        // The crest numbers are not part of the copies.
        assert_eq!(m.crest[0], 0x50);
    }

    #[test]
    fn strncpy_pads_with_nuls() {
        let mut f = [b'#'; 6];
        strncpy(&mut f, b"AB");
        assert_eq!(&f, b"AB\0\0\0\0");
        strncpy(&mut f, b"ABCDEFGH");
        assert_eq!(&f, b"ABCDEF");
    }

    #[test]
    fn duplicate_checks() {
        let m = sample();
        assert!(name_exists(&m, b"C005"));
        assert!(!name_exists(&m, b"C5"));
        assert!(player_exists(&m, b"P063"));
        assert!(!player_exists(&m, b"P064"));
    }
}
