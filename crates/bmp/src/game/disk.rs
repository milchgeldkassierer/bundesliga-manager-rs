//! The screens behind the office main icon "Disk/Optionen": saving a game
//! (segment 422b), the high-score table, the settings (segment 362f) and the
//! quit question of the main icon "Spiel beenden".
//! Spec: `docs/re/logic/stats.md` §4.1, §5.2, §5.3 and `flow.md` §6.7.

use std::path::{Path, PathBuf};

use bmp_formats::savegame::{
    self, HIGHSCORE_ENTRIES, HIGHSCORE_FILE_LEN, HighScoreEntry, HighScoreTable,
    highscore_file_name,
};
use bmp_game::highscore::highscore_build_entry;
use bmp_game::rng::{Rng, format_long_thousands};
use bmp_game::state::GameState;

use super::load_game::{SaveSession, file_selector, find_case_insensitive};
use super::widgets::{
    dialog_okay, dialog_two_choice, input_wait_click, ui_wait_right_click_button,
};
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, text_to_upper_de};
use crate::gfx::{
    Gfx, Ui, ui_draw_frame, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow,
    ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, Run};

/// `LBL[106]`: title of the save screen.
const SAVE_TITLE: &[u8] = b"Spielstand Speichern";
/// `LBL[208..210]`, `LBL[218..219]`: the overwrite question.
const FILE_EXISTS: [&[u8]; 5] = [
    b"FILE EXISTIERT",
    b"BEREITS.",
    b"]BERSCHREIBEN ?",
    b"ACH BITTE !",
    b"BLO~ NICHT !",
];
/// `LBL[139..142]`: the disk error message.
const DISK_ERROR: [&[u8]; 4] = [
    b"Achtung !",
    b"Fehler beim",
    b"Diskettenzugriff !",
    b"Vorgang abgebrochen.",
];
/// `DS:4d94..4dce`: the quit question.
const QUIT_QUESTION: [&[u8]; 5] = [
    b"M|CHTEN SIE",
    b"DAS SPIEL WIRKLICH",
    b"BEENDEN ?",
    b"LEIDER JA",
    b"KEIN GEDANKE",
];

/// Values of the save header that the original draws from the RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveHeader {
    pub save_id: u32,
    pub pad: u8,
    pub initial_key: u8,
}

/// Draws the random part of a save header in the original's order: the
/// save id from two `random_range(0, 0x8fff_u16 as i16)` (low word + 1, high word), then
/// the cipher key `random_range(0, 255) + 0x78` and the unused pad byte
/// `random_range(0, 255)`.
///
/// Origin: BMMAIN.EXE 422b:07fe (screen_save_game, 422b:09e4..0ac2)
pub fn draw_save_header(rng: &mut Rng) -> SaveHeader {
    let lo = rng.random_range(0, 0x8fff_u16 as i16);
    let hi = rng.random_range(0, 0x8fff_u16 as i16);
    // `CWD` + `ADD/ADC` of the long `(long)lo + ((long)hi << 16) + 1`.
    let save_id = (i32::from(lo))
        .wrapping_add(i32::from(hi) << 16)
        .wrapping_add(1) as u32;
    let initial_key = (rng.random_range(0, 0xff) as u8).wrapping_add(0x78);
    let pad = rng.random_range(0, 0xff) as u8;
    SaveHeader {
        save_id,
        pad,
        initial_key,
    }
}

/// The `.MAN` bytes of `state` with `header`: signature and version as the
/// running program writes them, `DS:05fa[0]` set from the copy-protection
/// flag `DS:05ac`, then every block, the message strings and the tail.
///
/// Origin: BMMAIN.EXE 422b:07fe (screen_save_game, write part 422b:09ab..11cf)
pub fn savegame_bytes(
    state: &mut GameState,
    header: SaveHeader,
    copy_protection_ok: bool,
) -> Vec<u8> {
    state.signature = *savegame::SIGNATURE;
    state.version = savegame::VERSION_V2;
    state.save_id = header.save_id;
    state.pad = header.pad;
    state.initial_key = header.initial_key;
    state.ds_05fa[0] = u8::from(copy_protection_ok);
    state
        .to_savegame()
        .to_bytes()
        .expect("a GameState always encodes")
}

/// Writes `state` as a save game to `path`: draws the header (after the file
/// was created, like the original), remembers the save id as loaded in this
/// session and writes the file. `Err` is the original's `disk_error_dialog`
/// case.
///
/// Origin: BMMAIN.EXE 422b:07fe (screen_save_game, 422b:0973..11cf)
pub fn savegame_write_man(
    path: &Path,
    state: &mut GameState,
    rng: &mut Rng,
    saves: &mut SaveSession,
    copy_protection_ok: bool,
) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let header = draw_save_header(rng);
    saves.record_id(header.save_id);
    let bytes = savegame_bytes(state, header, copy_protection_ok);
    std::io::Write::write_all(&mut &file, &bytes)
}

/// `DS:9440 = 1` and the four-line disk error message.
///
/// Origin: BMMAIN.EXE 422b:0006 (disk_error_dialog)
fn disk_error_dialog(host: &mut Host) -> Run<()> {
    let [a, b, c, d] = DISK_ERROR;
    dialog_okay(host, a, b, c, d)?;
    Ok(())
}

/// The copy-protection trap every screen starts with.
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

/// The file the original opens for `name` in the selector's directory:
/// an existing file whatever its case, else the DOS (upper-case) spelling.
fn save_path(host: &Host, saves: &SaveSession, name: &[u8]) -> PathBuf {
    let path = saves
        .path(&host.data)
        .join(String::from_utf8_lossy(&text_to_upper_de(name)).to_string());
    find_case_insensitive(&path).unwrap_or(path)
}

/// "Spielstand Speichern": picks a file name with the file selector (or
/// takes `fixed_name`, the autosave and crash-save case), asks before
/// overwriting, and writes the save. After a save from the menu the original
/// sets `DS:05ad`, which blocks saving again and ending the turn with a click
/// (both game options are 1 in the shipped data).
///
/// Origin: BMMAIN.EXE 422b:07fe (screen_save_game)
pub fn screen_save_game(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    fixed_name: &[u8],
) -> Run<()> {
    let path = if fixed_name.is_empty() {
        copy_protection(host, session)?;
        host.set_font(3);
        let title = text_to_upper_de(SAVE_TITLE);
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &title, 0, 0x140, 0x14, 0xb, 0);
        let name = file_selector(host, &mut session.saves, 0)?;
        if name.is_empty() {
            return Ok(());
        }
        let path = save_path(host, &session.saves, &name);
        if path.is_file() {
            let cur = (state.current_manager as usize).min(3);
            let who = state.managers[cur].name().to_vec();
            let [l1, l2, l3, yes, no] = FILE_EXISTS;
            if dialog_two_choice(host, session.auto_play, &who, l1, l2, l3, yes, no)? == 0 {
                return Ok(());
            }
        }
        session.saves.last_name = name;
        path
    } else {
        host.data
            .join(String::from_utf8_lossy(fixed_name).to_string())
    };
    if let Err(e) = savegame_write_man(
        &path,
        state,
        &mut session.rng,
        &mut session.saves,
        session.copy_protection_ok,
    ) {
        eprintln!("screen_save_game: {}: {e}", path.display());
        disk_error_dialog(host)?;
        return Ok(());
    }
    if fixed_name.is_empty() && GAME_OPTION_A == GAME_OPTION_B {
        session.save_disabled = true;
    }
    Ok(())
}

/// Shows the pressed quit icon and asks whether to end the game; "LEIDER JA"
/// ends the program (`fatal_error_exit(99)`, which skips the crash save),
/// otherwise the icon is redrawn.
///
/// Origin: BMMAIN.EXE 18bc:1c1d (quit_game_confirm)
pub fn quit_game_confirm(host: &mut Host, session: &Session, state: &GameState) -> Run<()> {
    // `ui_draw_button` loads PIC\37.VGA itself for icon 0xc8.
    let pic = host
        .load_picture(37)
        .expect("PIC\\37.VGA (fatal_error_exit 2 in the original)");
    host.ui.picture_37 = Some(pic);
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x10a, 0xc4, 0, 0x40c8, false, 8);
    host.ui.picture_37 = None;
    host.g.free(pic);
    let cur = (state.current_manager as usize).min(3);
    let who = state.managers[cur].name().to_vec();
    let [l1, l2, l3, yes, no] = QUIT_QUESTION;
    if dialog_two_choice(host, session.auto_play, &who, l1, l2, l3, yes, no)? != 0 {
        return Err(crate::host::Quit);
    }
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x10a, 0xc4, 0, 0x4005, false, 8);
    Ok(())
}

// ----- settings -------------------------------------------------------------------

/// `DS:5454` / `DS:5464`: position of the 15 option toggles.
const TOGGLE_X: [i16; 15] = [
    122, 122, 122, 180, 180, 180, 238, 238, 238, 122, 122, 122, 122, 238, 238,
];
const TOGGLE_Y: [i16; 15] = [
    35, 54, 73, 35, 54, 73, 35, 54, 73, 100, 119, 138, 157, 100, 119,
];
/// `FAR:4a20`, `UI[416]`, `FAR:4a24`, `DS:5452`: the version line.
const VERSION_LINE: &[u8] = b"V2.0 - ENDE: ";
const NEVER: &[u8] = b" NIE.";
const LEVEL: &[u8] = b" (LEVEL ";
/// `UI[417..424]`, `UI[127]`: row labels left (x 0x16..0x79) and right
/// (x 0xac..0xed) with their y.
const ROW_LABELS: [(&[u8], i16, i16, i16); 9] = [
    (b"Halbzeitst{nde", 0x16, 0x79, 0x2e),
    (b"Ergebnisse", 0x16, 0x79, 0x40),
    (b"Tabelle", 0x16, 0x79, 0x53),
    (b"Torszenen", 0x16, 0x79, 0x6f),
    (b"DfB-Pokal", 0x16, 0x79, 0x82),
    (b"Europapokale", 0x16, 0x79, 0x94),
    (b"Nachholspiele", 0x16, 0x79, 0xa8),
    (b"Zeitung", 0xac, 0xed, 0x6f),
    (b"Blenden", 0xac, 0xed, 0x82),
];
/// `UI[425..427]`: the league column headers.
const LEAGUE_COLUMNS: [(&[u8], i16); 3] = [(b"1.Liga", 0x7d), (b"2.Liga", 0xb8), (b"3.Liga", 0xf2)];
/// `UI[428]`.
const SPEED: &[u8] = b"Spielgeschwindigkeit";

/// Draws option toggle "AN"/"AUS" at `(x, y)`: two 24x17 cuts of the toggle
/// picture, the lit half on the side of the state.
///
/// Origin: BMMAIN.EXE 362f:0004 (settings_draw_toggle)
fn settings_draw_toggle(host: &mut Host, pic: crate::gfx::BitmapId, x: i16, y: i16, on: i16) {
    let screen = host.screen();
    host.g
        .blit_bitmap(pic, on * 0x30 + 0x64, 0, screen, x, y, 0x18, 0x11);
    host.g
        .blit_bitmap(pic, 0xac - on * 0x30, 0, screen, x + 0x1a, y, 0x18, 0x11);
}

/// Draws the knob of the game-speed slider at `0xca + pos`.
///
/// Origin: BMMAIN.EXE 362f:0086 (settings_draw_speed_slider)
fn settings_draw_speed_slider(host: &mut Host, pos: i16) {
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_fill_rect(g, rp, 0xca, 0xa1, 0x115, 0xa8, 5);
    let p = pos.wrapping_add(0xca);
    Gfx::rastport_set_color(rp, 0x10);
    g.rastport_put_pixel(rp, p, 0xa1);
    ui_draw_line(g, rp, p, 0xa2, p, 0xa7, 0x11);
    ui_draw_line(g, rp, p + 1, 0xa1, p + 5, 0xa1, 0x11);
    ui_draw_line(g, rp, p + 1, 0xa7, p + 5, 0xa7, 0x13);
    ui_draw_line(g, rp, p + 5, 0xa2, p + 5, 0xa7, 0x13);
    ui_fill_rect(g, rp, p + 1, 0xa2, p + 4, 0xa6, 0x12);
}

/// A settings cheat key typed while the pointer is on a pixel of colour 0x0b.
/// Returns `true` when the key is one of the cheats.
///
/// Origin: BMMAIN.EXE 362f:01b8 (screen_settings, 362f:07c7..0956)
fn settings_cheat(session: &mut Session, state: &mut GameState, key: u8) -> bool {
    let cur = (state.current_manager as usize).min(3);
    match key {
        b'0'..=b'5' => state.level = b'5' - key,
        b's' | b'a' => {
            for p in state.squads[cur][..24].iter_mut() {
                if p.player_id != 0 {
                    p.tendency = 0x44;
                    p.exhaustion = 0x32;
                    if key == b'a' {
                        p.skills = [99; 3];
                        state.managers[cur].balance = 99_999_999;
                    }
                }
            }
        }
        b'd' => {
            session.save_disabled = false;
            session.saves.loaded_ids.clear();
        }
        b'g' => {
            let m = &mut state.managers[cur];
            m.balance = m.balance.wrapping_add(200_000);
        }
        b'p' => session.cheat_05f5 = true,
        _ => return false,
    }
    true
}

/// "Einstellungen": the fifteen on/off options (half-time scores, results
/// and tables per league, goal scenes, cups, rescheduled games, newspaper,
/// fades), the game-speed slider and the cheat keys. The exit icon returns.
///
/// Origin: BMMAIN.EXE 362f:01b8 (screen_settings)
pub fn screen_settings(host: &mut Host, session: &mut Session, state: &mut GameState) -> Run<()> {
    let pic = host
        .load_picture(0x21)
        .expect("PIC\\33.VGA (fatal_error_exit 2 in the original)");
    copy_protection(host, session)?;
    let panel = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0xd, 0xd, 0x125, 0xa8, false, 8)
        .1;
    let exit = host
        .ui
        .draw_button(&mut host.g, &mut host.rp, 0x10c, 0xc1, 0, 0x22, false, 8)
        .1;
    let rects = [panel, exit];
    host.set_font(2);
    let mut line = VERSION_LINE.to_vec();
    let fmt = host.number_format();
    if state.scenario < 2000 {
        line.extend(format_long_thousands(i32::from(state.scenario), fmt));
    } else {
        line.extend_from_slice(NEVER);
    }
    line.extend_from_slice(LEVEL);
    line.extend(format_long_thousands(5 - i32::from(state.level as i8), fmt));
    line.push(b')');
    ui_draw_text_at(&mut host.g, &mut host.rp, &line, 0x10, 0x17, 3);
    host.set_font(1);
    for (text, x0, x1, y) in ROW_LABELS {
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, 0x1d, 0);
    }
    for (text, x) in LEAGUE_COLUMNS {
        ui_draw_text_at(&mut host.g, &mut host.rp, text, x, 0x1d, 1);
    }
    let (g, rp) = (&mut host.g, &mut host.rp);
    Gfx::rastport_move_to(rp, 0x10, 0x11);
    g.rastport_line_to(rp, 0x130, 0x11);
    ui_draw_text_centered_shadow(g, rp, SPEED, 0xb6, 0x12c, 0x96, 0x1d, 0);
    Gfx::rastport_move_to(rp, 0x10, 0x62);
    g.rastport_line_to(rp, 0x130, 0x62);
    Gfx::rastport_move_to(rp, 0x10, 0xb4);
    g.rastport_line_to(rp, 0x130, 0xb4);
    for i in 0..15 {
        let on = i16::from(state.options[i] as i8);
        settings_draw_toggle(host, pic, TOGGLE_X[i], TOGGLE_Y[i], on);
    }
    let screen = host.screen();
    host.g
        .blit_bitmap(pic, 0, 0, screen, 0xbe, 0x9b, 0x64, 0x13);
    settings_draw_speed_slider(host, state.match_options as i16);
    host.screen_fade(1)?;
    let mut hit: i16 = 0;
    loop {
        host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let key = host.read_key_nowait();
        let pixel = host.g.rastport_get_pixel(&host.rp, mx, my);
        if pixel == 0x0b && key != 0 {
            let cheat = settings_cheat(session, state, key);
            if cheat {
                ui_draw_frame(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xff, 1);
                // The original drains the keyboard buffer while the frame shows.
                while host.read_key_nowait() != 0 {}
                ui_draw_frame(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xff, 0);
            }
        }
        if host.button == LEFT_DOWN {
            hit = Ui::hit_test_buttons(&rects, 2, mx, my, None);
            for i in 0..15 {
                let (x, y) = (TOGGLE_X[i], TOGGLE_Y[i]);
                // The y compares are unsigned in the original.
                if x <= mx
                    && (y as u16) <= my as u16
                    && mx < x + 0x31
                    && (my as u16) < (y + 0x12) as u16
                {
                    state.options[i] = u8::from(mx < x + 0x1a);
                    settings_draw_toggle(host, pic, x, y, i16::from(state.options[i]));
                }
            }
            if 0xbe < mx && mx < 0x122 && 0x97 < my && my < 0xab {
                let x = mx.clamp(0xca, 0x110);
                state.match_options = (x - 0xca) as u16;
                settings_draw_speed_slider(host, x - 0xca);
            }
        }
        if hit == 2 {
            ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
            host.g.free(pic);
            return Ok(());
        }
    }
}

// ----- high scores ----------------------------------------------------------------

/// `UI[140]`, `DS:94a4`.
const POINTS: &[u8] = b"PUNKTE:";
const TITLES_TEMPLATE: &[u8] = b"(X/X/X)";
/// `DS:05da`: disables the high-score icon; 0 in the shipped data.
pub const HIGHSCORES_DISABLED: bool = false;

/// What `highscore_screen` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighscoreMode {
    /// 0: show the table only (game over).
    Show,
    /// 1: show the table with the current manager's entry (office menu).
    Preview,
    /// 2: insert the current manager silently and save the table.
    Insert,
}

/// Reads `HIGH.XY` (or creates it zeroed when missing) and returns the table
/// with the variant digit (0 normal, 1 or 3 for the one-year scenarios).
///
/// Origin: BMMAIN.EXE 422b:21c4 (highscore_load_save, read)
fn highscore_load(data: &Path, state: &GameState) -> (HighScoreTable, u8) {
    let name = highscore_file_name(state.scenario, state.level);
    let variant = name.as_bytes()[5] - b'0';
    let path = find_case_insensitive(&data.join(&name)).unwrap_or(data.join(&name));
    let zero = HighScoreTable::parse(&[0; HIGHSCORE_FILE_LEN]).expect("a zeroed table");
    let table = match std::fs::read(&path) {
        Ok(bytes) => {
            let mut buf = vec![0u8; HIGHSCORE_FILE_LEN];
            let n = bytes.len().min(HIGHSCORE_FILE_LEN);
            buf[..n].copy_from_slice(&bytes[..n]);
            HighScoreTable::parse(&buf).expect("a full-size table")
        }
        Err(_) => {
            // TODO(re): the original fades out when MANA.DAT is missing too.
            let _ = std::fs::write(&path, zero.to_bytes().expect("20 entries"));
            zero
        }
    };
    (table, variant)
}

/// Writes the table back to `HIGH.XY`.
///
/// Origin: BMMAIN.EXE 422b:21c4 (highscore_load_save, write)
fn highscore_save(data: &Path, state: &GameState, table: &HighScoreTable) {
    let name = highscore_file_name(state.scenario, state.level);
    let path = find_case_insensitive(&data.join(&name)).unwrap_or(data.join(&name));
    if let Err(e) = std::fs::write(&path, table.to_bytes().expect("20 entries")) {
        eprintln!("highscore_save: {}: {e}", path.display());
    }
}

/// Puts `cand` into the table: over the entry of the same manager and club,
/// or over the last entry when it scores more; then the exchange sort, whose
/// swaps go through the candidate buffer. Returns the index, the sort's
/// final outer counter (19) and the buffer, or `None`.
///
/// Origin: BMMAIN.EXE 422b:2366 (highscore_screen, 422b:24ea..25b2)
fn highscore_insert(
    table: &mut HighScoreTable,
    cand: &mut HighScoreEntry,
    tag: i32,
) -> Option<usize> {
    for i in 0..HIGHSCORE_ENTRIES {
        let e = &table.entries[i];
        let same = e.manager_name() == cand.manager_name() && e.club_name() == cand.club_name();
        if !same && !(i == HIGHSCORE_ENTRIES - 1 && e.score < cand.score) {
            continue;
        }
        table.entries[i] = *cand;
        table.entries[i].extra = tag as u32;
        for a in 0..HIGHSCORE_ENTRIES - 1 {
            for b in a + 1..HIGHSCORE_ENTRIES {
                if table.entries[b].score > table.entries[a].score {
                    *cand = table.entries[a];
                    table.entries[a] = table.entries[b];
                    table.entries[b] = *cand;
                }
            }
        }
        return Some(i);
    }
    None
}

/// The high-score table (`PIC\45.CP`): the 20 best managers with club,
/// score and the title counters. `Preview` shows the current manager's
/// entry in place; in the normal game it then inserts and saves it with
/// `Insert`. Highlighted rows (colour 3) are the new entry, or in the normal
/// game every entry tagged with `DS:0668`.
///
/// Origin: BMMAIN.EXE 422b:2366 (highscore_screen)
pub fn highscore_screen(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    mode: HighscoreMode,
) -> Run<()> {
    if mode != HighscoreMode::Insert {
        host.screen_fade(0)?;
    }
    let data = host.data.clone();
    let (mut table, variant) = highscore_load(&data, state);
    let cur = (state.current_manager as usize).min(3);
    let mut cand = HighScoreEntry {
        manager: [0; 26],
        club: [0; 26],
        score: 0,
        extra: 0,
    };
    if mode != HighscoreMode::Show {
        cand = highscore_build_entry(state, &mut session.rng, cur, variant);
    }
    if mode != HighscoreMode::Insert {
        host.g.mouse_pointer_hide(0);
        host.load_screen_picture(0x2d)
            .expect("PIC\\45.CP (fatal_error_exit 2 in the original)");
        host.g.mouse_pointer_show();
        host.set_font(2);
    }
    // `[bp-0x70]`: the sort's outer counter, later the preview row + 1.
    let mut mark = 0usize;
    if mode != HighscoreMode::Show
        && highscore_insert(&mut table, &mut cand, session.highscore_tag).is_some()
    {
        if mode == HighscoreMode::Insert {
            highscore_save(&data, state, &table);
            return Ok(());
        }
        mark = HIGHSCORE_ENTRIES - 1;
    }
    if mode == HighscoreMode::Insert {
        return Ok(());
    }
    for i in 0..HIGHSCORE_ENTRIES {
        if mode != HighscoreMode::Show && mark == 0 && table.entries[i].score <= cand.score {
            for j in (i + 1..HIGHSCORE_ENTRIES).rev() {
                table.entries[j] = table.entries[j - 1];
            }
            table.entries[i] = cand;
            mark = i + 1;
        }
        // TODO(#31): mode Show (game over) overwrites the fixture
        // permutation strings DS:06fc[i] with 'a'.
        let e = table.entries[i];
        if e.score == 0 {
            continue;
        }
        let y = 10 * i as i16 + 0x20;
        let hl = if variant != 0 {
            i + 1 == mark
        } else {
            e.extra as i32 == session.highscore_tag
        };
        let color = i16::from(hl) * 2 + 1;
        let (g, rp) = (&mut host.g, &mut host.rp);
        ui_draw_text_at(g, rp, e.manager_name(), 0x38, y, color);
        ui_draw_text_at(g, rp, e.club_name(), 0x68, y, color);
        let mut text = POINTS.to_vec();
        host.number_pad = 5;
        text.extend(format_long_thousands(
            i32::from(e.score),
            host.number_format(),
        ));
        host.number_pad = 1;
        ui_draw_text_at(&mut host.g, &mut host.rp, &text, 0xbb, y, color);
        let mut titles = TITLES_TEMPLATE.to_vec();
        for k in 0..3 {
            titles[2 * k + 1] = e.club[23 + k].wrapping_add(b'0');
        }
        ui_draw_text_at(&mut host.g, &mut host.rp, &titles, 0xf3, y, color);
    }
    host.screen_fade(1)?;
    if variant == 0 {
        highscore_screen(host, session, state, HighscoreMode::Insert)?;
    }
    input_wait_click(host, session.auto_play)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::savegame::SaveGame;

    /// The RNG state right before the header draws of each shipped save,
    /// found by searching all 2^32 states for the save id, key and pad byte
    /// (bit 31 does not influence the draws). `2021-AUG.MAN` was saved four
    /// draws after `AUTOSAVE.MAN`.
    const SHIPPED_SAVES: [(&str, u32); 6] = [
        ("07072019.MAN", 0x0bcf_d005),
        ("2020-JUN.MAN", 0x611f_24c1),
        ("2021-AUG.MAN", 0x20cb_c2a0),
        ("AUTOSAVE.MAN", 0x6a57_091c),
        ("BMP/AUTOSAVE.MAN", 0x7d49_31ca),
        ("BMP/MICHEL1.MAN", 0x4701_6591),
    ];

    fn shipped(name: &str) -> Option<Vec<u8>> {
        let dir = bmp_formats::data_dir()?;
        std::fs::read(dir.join(name)).ok()
    }

    #[test]
    fn header_draws_follow_the_original_order() {
        // Two draws for the id, one for the key, one for the pad byte.
        let mut rng = Rng::from_state(0x1234_5678);
        let h = draw_save_header(&mut rng);
        let mut r = Rng::from_state(0x1234_5678);
        let lo = r.random_range(0, 0x8fff_u16 as i16) as u32;
        let hi = r.random_range(0, 0x8fff_u16 as i16) as u32;
        let key = (r.random_range(0, 255) as u8).wrapping_add(0x78);
        let pad = r.random_range(0, 255) as u8;
        assert_eq!(h.save_id, (hi << 16) + lo + 1);
        assert_eq!((h.initial_key, h.pad), (key, pad));
        assert_eq!(rng, r);
    }

    #[test]
    fn saving_a_loaded_game_reproduces_the_file() {
        let dir = std::env::temp_dir().join(format!("bmp-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, rng_state) in SHIPPED_SAVES {
            let Some(bytes) = shipped(name) else {
                continue;
            };
            let save = SaveGame::parse(&bytes).unwrap();
            let mut state = GameState::from_savegame(&save).unwrap();
            let path = dir.join("TEST.MAN");
            let mut rng = Rng::from_state(rng_state);
            let mut saves = SaveSession::default();
            let protection_ok = state.ds_05fa[0] != 0;
            savegame_write_man(&path, &mut state, &mut rng, &mut saves, protection_ok).unwrap();
            let written = std::fs::read(&path).unwrap();
            assert_eq!(saves.loaded_ids, vec![save.save_id], "{name}");
            assert_eq!(written.len(), bytes.len(), "{name}");
            assert!(written == bytes, "the save differs from {name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn highscore_entry_of_the_shipped_save_replaces_the_old_one() {
        let (Some(bytes), Some(high)) = (shipped("07072019.MAN"), shipped("HIGH.00")) else {
            return;
        };
        let state = GameState::from_savegame(&SaveGame::parse(&bytes).unwrap()).unwrap();
        let mut rng = Rng::default();
        let mut cand = highscore_build_entry(&state, &mut rng, 0, 0);
        // The original writes 96 for this save (DOSBox, office menu "Highscore").
        assert_eq!(cand.score, 96);
        assert_eq!(cand.manager_name(), b"OLIVER");
        assert_eq!(cand.club_name(), b"BAYER 05 UERDINGEN");
        let mut table = HighScoreTable::parse(&high).unwrap();
        assert_eq!(table.entries[1].score, 131);
        assert_eq!(highscore_insert(&mut table, &mut cand, 0), Some(1));
        assert_eq!(table.entries[1].score, 96);
        assert_eq!(table.entries[0].score, 207);
    }
}
