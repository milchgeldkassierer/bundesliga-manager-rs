//! "Spielstand Laden": the file selector and loading a `.MAN` save game.
//! Spec: `docs/re/logic/stats.md` §5.2 and the call traces of 422b:120c / 422b:2bbe.

use std::path::{Path, PathBuf};

use bmp_formats::savegame::{self, SaveGame};
use bmp_game::rng::format_long_thousands;
use bmp_game::state::{GameState, StateError};

use super::start_menu::Pictures;
use super::start_menu::text_input;
use super::widgets::{TextButtons, dialog_okay, ui_wait_right_click_button};
use super::{Session, text_to_upper_de};
use crate::gfx::{
    Gfx, Ui, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// Entries per page of the file list (two columns of 18).
const PAGE: usize = 0x24;
/// Most directories (`FAR:2f40`, 13-byte names) and files (`FAR:04a4`) listed.
const MAX_DIRS: usize = 0x13;
const MAX_FILES: usize = 199;
/// Drive letters of the drive buttons (`DS:93ec`) and the delete button (`DS:2994`).
const DRIVES: [&[u8]; 7] = [b"A:", b"B:", b"C:", b"D:", b"E:", b"F:", b"G:"];
const DELETE_LABEL: &[u8] = b"L\x99SCHEN";
/// Number of logical drives DOS reports (`crt_dos_setdrive`); DOSBox has 26.
const LOGICAL_DRIVES: usize = 26;
/// Most save ids remembered per session (`FAR:a6d8`).
const SAVE_ID_HISTORY: usize = 50;
/// The message for [`LoadError::Corrupt`]; the original has none (it hangs later).
const CORRUPT_SAVE: [&[u8]; 4] = [
    b"Achtung !",
    b"Spielstand",
    b"ist fehlerhaft !",
    b"Vorgang abgebrochen.",
];

/// File-selector and loader state kept between calls.
#[derive(Debug, Clone, Default)]
pub struct SaveSession {
    /// `FAR:4af6`: directory of the last selection, as components below the game
    /// directory (the DOS drive root `C:\`).
    pub dir: Vec<String>,
    /// `FAR:54a2`: name of the last loaded file, offered again by the selector.
    pub last_name: Vec<u8>,
    /// `FAR:a6d8` / `DS:05ae`: ids of the saves loaded in this session.
    pub loaded_ids: Vec<u32>,
}

impl SaveSession {
    /// `C:\` plus the directory, as `getcwd` reports it.
    fn cwd_text(&self) -> Vec<u8> {
        let mut s = b"C:\\".to_vec();
        s.extend_from_slice(self.dir.join("\\").as_bytes());
        s
    }

    pub(crate) fn path(&self, data: &Path) -> PathBuf {
        self.dir.iter().fold(data.to_path_buf(), |p, c| p.join(c))
    }

    /// `chdir(name)` relative to the current directory; unknown names are ignored
    /// like a failing `chdir`.
    fn change_dir(&mut self, data: &Path, name: &[u8]) {
        let name = String::from_utf8_lossy(name).to_string();
        match name.as_str() {
            "." | "" => {}
            ".." => {
                self.dir.pop();
            }
            _ => {
                let parts: Vec<&str> = name.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
                let mut dir = if name.starts_with('\\') || name.get(1..2) == Some(":") {
                    Vec::new()
                } else {
                    self.dir.clone()
                };
                for p in parts {
                    if p.ends_with(':') {
                        continue;
                    }
                    if p == ".." {
                        dir.pop();
                    } else if p != "." {
                        dir.push(p.to_string());
                    }
                }
                let mut path = data.to_path_buf();
                let mut real = Vec::new();
                for c in &dir {
                    // DOS names are case-insensitive: find the host spelling.
                    let found = std::fs::read_dir(&path).ok().and_then(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .find(|n| n.eq_ignore_ascii_case(c))
                    });
                    let Some(found) = found else {
                        return;
                    };
                    path = path.join(&found);
                    real.push(found);
                }
                if path.is_dir() {
                    self.dir = real;
                }
            }
        }
    }

    /// Appends a save id to the history (`FAR:a6d8`, `DS:05ae`), dropping the
    /// oldest one when 50 are stored.
    ///
    /// Origin: BMMAIN.EXE 422b:07fe (screen_save_game, 422b:0a1c..0a79)
    pub fn record_id(&mut self, id: u32) {
        if self.loaded_ids.len() >= SAVE_ID_HISTORY {
            self.loaded_ids.remove(0);
        }
        self.loaded_ids.push(id);
    }

    /// Remembers a loaded save id; false when it was loaded before in this session.
    fn remember_id(&mut self, id: u32) -> bool {
        if self.loaded_ids.contains(&id) {
            return false;
        }
        self.record_id(id);
        true
    }
}

/// The directory listing as `findfirst("*.*", 0x10)` returns it under DOSBox: `.`
/// and `..`, then the sub-directories and the files, each sorted by name.
/// Directories are capped at 19 entries and files at 199 (the loop stops at the
/// first cap that is reached).
pub fn list_directory(path: &Path) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut dirs: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut sub: Vec<(Vec<u8>, bool)> = std::fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        return None;
                    }
                    let mut n = text_to_upper_de(name.as_bytes());
                    n.truncate(12);
                    Some((n, e.path().is_dir()))
                })
                .collect()
        })
        .unwrap_or_default();
    sub.sort();
    for (name, is_dir) in sub {
        if dirs.len() >= MAX_DIRS || files.len() >= MAX_FILES {
            break;
        }
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    (dirs, files)
}

/// `true` for names ending in `.MAN` (drawn in colour 0x13 instead of 1).
fn is_save_name(name: &[u8]) -> bool {
    name.len() >= 4 && &name[name.len() - 4..] == b".MAN"
}

/// The file selector: lists the current directory in two columns, lets the
/// player pick or type a file name, change directory or drive, and returns the
/// chosen name (empty when cancelled with the icon button). `icon` selects the
/// icon of the cancel button (`0x22 + icon`).
///
/// TODO(#15): the delete button needs `dialog_two_choice` 4091:146c; the drive
/// buttons only re-read the current directory (the game directory is drive C:).
///
/// Origin: BMMAIN.EXE 422b:2bbe (file_selector)
pub fn file_selector(host: &mut Host, saves: &mut SaveSession, icon: u8) -> Run<Vec<u8>> {
    host.set_font(2);
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 6, 0x21, 0xf1, 0xb5, false, 8);
    let (_, cancel_rect) = host.ui.draw_button(
        &mut host.g,
        &mut host.rp,
        0x10c,
        0xc1,
        0,
        0x22 + i16::from(icon),
        false,
        8,
    );
    let mut y = 0xa3i16;
    while y < 0xb8 {
        ui_draw_line(&mut host.g, &mut host.rp, 7, y as u8, 0xf6, y as u8, 0xb);
        y += 10;
    }
    let mut labels: Vec<&[u8]> = (0..7)
        .map(|i| if i < LOGICAL_DRIVES { DRIVES[i] } else { b"" })
        .collect();
    labels.push(DELETE_LABEL);
    let mut buttons = TextButtons::default();
    for row in 0..2i16 {
        for col in 0..4i16 {
            let label = labels[(row * 4 + col) as usize];
            buttons.register(host, col * 0x3b + 0xa, 0xd * row + 0xbb, label, 0x1a);
        }
    }
    let mut name = saves.last_name.clone();
    let data = host.data.clone();
    'list: loop {
        let mut first = 0usize;
        Gfx::rastport_set_draw_mode(&mut host.rp, 0);
        ui_fill_rect(&mut host.g, &mut host.rp, 7, 0x22, 0xf6, 0xa1, 8);
        let (dirs, files) = list_directory(&saves.path(&data));
        let mut text = b"VERZEICHNIS: ".to_vec();
        text.extend(text_to_upper_de(&saves.cwd_text()));
        text.truncate(0x2c);
        let (g, rp) = (&mut host.g, &mut host.rp);
        ui_fill_rect(g, rp, 0xa, 0xa5, 0xf0, 0xab, 8);
        ui_draw_text_at(g, rp, &text, 0xa, 0xaa, 1);
        ui_fill_rect(g, rp, 0xa, 0xaf, 0xf2, 0xb5, 8);
        ui_draw_text_at(g, rp, b"FILENAME      :", 0xa, 0xb4, 1);
        if !name.is_empty() {
            ui_draw_text_at(g, rp, &name, 0x3f, 0xb4, 1);
        }
        'page: loop {
            let mut hl_row: i16 = -1;
            let mut hl_col: i16 = -1;
            ui_fill_rect(&mut host.g, &mut host.rp, 0xd2, 0xaf, 0xf2, 0xb5, 8);
            let mut text = b"SEITE: ".to_vec();
            text.extend(format_long_thousands(
                (first / PAGE + 1) as i32,
                host.number_format(),
            ));
            ui_draw_text_at(&mut host.g, &mut host.rp, &text, 0xd2, 0xb4, 2);
            let (mut x, mut y) = (0xai16, 0x29i16);
            let advance = |x: &mut i16, y: &mut i16| {
                *y += 7;
                if *y > 0xa0 {
                    *y = 0x29;
                    *x = 0x82;
                }
            };
            for d in dirs.iter().take(first + PAGE).skip(first) {
                ui_draw_text_at(&mut host.g, &mut host.rp, d, x, y, 3);
                advance(&mut x, &mut y);
            }
            let mut i = first.saturating_sub(dirs.len());
            while i < files.len() && dirs.len() + i < first + PAGE {
                let color = if is_save_name(&files[i]) { 0x13 } else { 1 };
                ui_draw_text_at(&mut host.g, &mut host.rp, &files[i], x, y, color);
                advance(&mut x, &mut y);
                i += 1;
            }
            host.screen_fade(1)?;
            loop {
                let ev = host.input_poll_mouse()?;
                let (mut mx, mut v) = host.mouse_xy();
                Gfx::rastport_set_draw_mode(&mut host.rp, 2);
                let xor_row = |host: &mut Host, row: i16, col: i16| {
                    ui_fill_rect(
                        &mut host.g,
                        &mut host.rp,
                        col * 0x78 + 0xa,
                        row * 7 + 0x25,
                        col * 0x78 + 0x7d,
                        row * 7 + 0x29,
                        1,
                    );
                };
                if mx > 0xa && mx < 0xf6 && v > 0x24 && v < 0xa0 {
                    // Inside the list: `mx` becomes the column, `v` the row (the
                    // original reuses both variables, the checks below see them).
                    mx = i16::from(mx > 0x82);
                    v = (v - 0x25) / 7;
                    if v != hl_row || hl_col != mx {
                        if hl_row != -1 {
                            xor_row(host, hl_row, hl_col);
                        }
                        xor_row(host, v, mx);
                        hl_row = v;
                        hl_col = mx;
                    }
                } else if hl_row != -1 {
                    xor_row(host, hl_row, hl_col);
                    hl_row = -1;
                }
                Gfx::rastport_set_draw_mode(&mut host.rp, 0);
                let (px, py) = host.mouse_xy();
                let cancel = Ui::hit_test_buttons(&[cancel_rect], 1, px, py, None);
                if v < 0xa5 && ev == RIGHT_DOWN && dirs.len() + files.len() > PAGE {
                    first += PAGE;
                    if dirs.len() + files.len() < first {
                        first = 0;
                    }
                    ui_fill_rect(&mut host.g, &mut host.rp, 7, 0x22, 0xf6, 0xa1, 8);
                    continue 'page;
                }
                if (ev == LEFT_DOWN || ev == RIGHT_DOWN)
                    && mx > 10
                    && mx < 0xf6
                    && v > 0xa3
                    && v < 0xb7
                {
                    let field = i16::from(v > 0xad);
                    if ev == LEFT_DOWN || field == 0 {
                        let x1 = if field == 0 { 0xf0 } else { 0xcd };
                        ui_fill_rect(
                            &mut host.g,
                            &mut host.rp,
                            0x3c,
                            field * 10 + 0xa5,
                            x1,
                            field * 10 + 0xab,
                            8,
                        );
                    }
                    if field == 0 {
                        let dir = text_input(host, 0x3f, 0xaa, 0xaf, 1, 8)?;
                        saves.change_dir(&data, &dir);
                        continue 'list;
                    }
                    if ev == LEFT_DOWN {
                        name = text_input(host, 0x3f, 0xb4, 0x8c, 1, 8)?;
                    }
                    if !name.is_empty() {
                        for c in name.iter_mut() {
                            if *c == b' ' {
                                *c = b'_';
                            }
                        }
                        if !(name.len() >= 5 && is_save_name(&name)) {
                            name.extend_from_slice(b".MAN");
                        }
                        return Ok(name);
                    }
                }
                if ev == LEFT_DOWN && cancel != 0 {
                    ui_wait_right_click_button(host, 0x10c, 0xc1, false)?;
                    return Ok(Vec::new());
                }
                if ev == LEFT_DOWN && hl_row != -1 {
                    let idx = (hl_col * 0x12) as usize + first + hl_row as usize;
                    if idx < dirs.len() {
                        saves.change_dir(&data, &dirs[idx]);
                        continue 'list;
                    }
                    if let Some(file) = files.get(idx - dirs.len()) {
                        if *file == name {
                            return Ok(name);
                        }
                        name = file.clone();
                        ui_fill_rect(&mut host.g, &mut host.rp, 0x3f, 0xaf, 0xf2, 0xb5, 8);
                        ui_draw_text_at(&mut host.g, &mut host.rp, &name, 0x3f, 0xb4, 1);
                    }
                }
                let slot = buttons.hit(px, py);
                if ev == LEFT_DOWN && slot != 0 {
                    if slot == 8 && !name.is_empty() {
                        // TODO(#15): "FILE <name> WIRKLICH L|SCHEN ?" dialog, then delete.
                        eprintln!("file_selector: deleting files is not ported");
                    }
                    if slot <= LOGICAL_DRIVES {
                        continue 'list;
                    }
                }
            }
        }
    }
}

/// Why a save could not be loaded (each case but [`LoadError::Corrupt`] is a
/// dialog in the original).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// `disk_error_dialog` 422b:0006.
    Disk,
    /// "Kein Bundesliga Manager Prof. Spielstand. !!!"
    NotASave,
    /// "Spielstand aus falscher Version !"
    WrongVersion,
    /// "Keinen heutigen Spielstand laden !": the save id was loaded before.
    AlreadyLoaded,
    /// A checksum does not match: "DER GROSSE MOGELGURU" screen.
    Checksum,
    /// A player's position code would hang the lineup logic
    /// ([`StateError::PositionCode`]); the original loads such a save.
    Corrupt,
}

/// Loads `path` and checks it like the original: signature, version, session
/// save-id history (the id is remembered before the blocks are read).
pub fn read_save(saves: &mut SaveSession, path: &Path) -> Result<GameState, LoadError> {
    let bytes = std::fs::read(path).map_err(|_| LoadError::Disk)?;
    let parsed = SaveGame::parse(&bytes);
    let save = match parsed {
        Err(savegame::Error::BadSignature) => return Err(LoadError::NotASave),
        Err(savegame::Error::BadVersion(_)) => return Err(LoadError::WrongVersion),
        other => other,
    };
    let id = bytes
        .get(29..33)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or(LoadError::Disk)?;
    if !saves.remember_id(id) {
        return Err(LoadError::AlreadyLoaded);
    }
    let save = match save {
        Ok(s) => s,
        Err(savegame::Error::Checksum { .. }) => return Err(LoadError::Checksum),
        Err(_) => return Err(LoadError::Disk),
    };
    GameState::from_savegame(&save).map_err(|e| match e {
        StateError::PositionCode { .. } => LoadError::Corrupt,
        StateError::BlockSize { .. } | StateError::BlockCount => LoadError::Disk,
    })
}

/// The endless "DER GROSSE MOGELGURU" screen shown for a manipulated save.
fn cheat_screen(host: &mut Host) -> Run<()> {
    const LINES: [&[u8]; 2] = [
        b"SOLLTE ES IHNEN ETWA PASSIERT SEIN, DA~ SIE OHNE JEGLICHE",
        b"B|SE ABSICHT MIT EINEM MONITOR MANIPULIEREN WOLLTEN ?",
    ];
    host.screen_fade(0)?;
    ui_fill_rect(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xef, 0);
    host.set_font(3);
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_text_centered_shadow(g, rp, b"DER GROSSE MOGELGURU", 0, 0x13f, 0x19, 0x12, 0xff);
    ui_draw_text_centered_shadow(g, rp, b"VERABSCHEUT IHRE TATEN", 0, 0x13f, 0x2d, 0x12, 0xff);
    host.set_font(2);
    for (i, line) in LINES.iter().enumerate() {
        let y = (i as i16 + 9) * 10;
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, line, 0, 0x13f, y, 1, 0xff);
    }
    host.screen_fade(1)?;
    let mut color = 0u8;
    loop {
        color ^= 0x12;
        for i in 0..3i16 {
            crate::gfx::ui_draw_frame(&mut host.g, &mut host.rp, i, i, 0x13f - i, 0x35 - i, color);
        }
        host.delay_ticks(20)?;
    }
}

/// Lets the player pick a save game and loads it. Returns the loaded state, or
/// `None` when nothing was loaded. On success the crest cache and the portrait
/// sheet of [`Pictures`] are rebuilt for the loaded managers.
///
/// A corrupt save ([`LoadError::Corrupt`]) gets a `dialog_okay` message box.
///
/// TODO(#15): the original's error dialogs (`dialog_okay`, `ui_dialog_box`) are
/// not ported; those errors are reported on stderr. The V1.x level question
/// (`ui_value_spinner` 4091:0ba5) keeps its default level 3.
///
/// Origin: BMMAIN.EXE 422b:120c (screen_load_game)
pub fn screen_load_game(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    arg: u8,
) -> Run<Option<Box<GameState>>> {
    // copy_protection_penalty: one RNG call when the protection was not passed.
    if !session.copy_protection_ok && session.rng.random_range(0, 100) == 0 {
        loop {
            let on = session.rng.random_range(0, 1) as u8;
            host.screen_fade(on)?;
        }
    }
    host.set_font(3);
    let title = text_to_upper_de(b"Spielstand Laden");
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &title, 0, 0x140, 0x14, 0xb, 0);
    let name = file_selector(host, &mut session.saves, arg)?;
    if name.is_empty() {
        return Ok(None);
    }
    let path = session
        .saves
        .path(&host.data)
        .join(String::from_utf8_lossy(&name).to_string());
    let path = find_case_insensitive(&path).unwrap_or(path);
    let mut state = match read_save(&mut session.saves, &path) {
        Ok(s) => s,
        Err(LoadError::Checksum) => {
            cheat_screen(host)?;
            return Ok(None);
        }
        Err(LoadError::Corrupt) => {
            let [a, b, c, d] = CORRUPT_SAVE;
            dialog_okay(host, a, b, c, d)?;
            return Ok(None);
        }
        Err(e) => {
            eprintln!("screen_load_game: {}: {e:?}", path.display());
            return Ok(None);
        }
    };
    session.saves.last_name = name;
    session.copy_protection_ok = state.ds_05fa[0] != 0;
    let screen_crest = pics.crest_cache;
    for m in 0..usize::from(state.manager_count).min(4) {
        let team = state.managers[m].team as usize;
        let code = state.clubs.get(team).map_or(0, |c| c.crest) & 0x7f;
        let spr = host
            .load_picture(u16::from(code) + 0x50)
            .expect("crest picture (fatal_error_exit 2 in the original)");
        host.g
            .blit_bitmap(spr, 0, 0, screen_crest, 0x29 * m as i16, 0, 0x28, 0x26);
        host.g.free(spr);
    }
    host.screen_fade(0)?;
    let sheet = host
        .load_picture(0)
        .expect("PIC\\0.VGA (fatal_error_exit 2 in the original)");
    for i in 0..4usize {
        // FAR:4bec[i] = 0 (per-manager flag, not modelled yet).
        if state.managers[i].slot == 0 {
            state.managers[i].slot = i as u8 + 1;
        }
        let slot = i16::from(state.managers[i].slot);
        host.g.blit_bitmap(
            sheet,
            slot * 0x29 - 0x28,
            0,
            pics.portraits,
            0x29 * i as i16 + 1,
            0,
            0x27,
            0x28,
        );
    }
    host.g.free(sheet);
    if state.version[2] == b'1' {
        // TODO(#15): "Sie haben einen V1.x-Spielstand geladen." level question.
        state.level = 5 - 3;
    }
    Ok(Some(Box::new(state)))
}

/// The host file whose name matches `path`'s file name ignoring case (DOS names).
pub(crate) fn find_case_insensitive(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let want = path.file_name()?.to_string_lossy().to_string();
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        e.file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(&want)
            .then(|| e.path())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_id_history_rejects_repeats_and_keeps_fifty() {
        let mut s = SaveSession::default();
        assert!(s.remember_id(7));
        assert!(!s.remember_id(7));
        for id in 100..149 {
            assert!(s.remember_id(id));
        }
        assert_eq!(s.loaded_ids.len(), 50);
        assert!(s.remember_id(1000));
        assert_eq!(s.loaded_ids.len(), 50);
        assert!(s.remember_id(7), "the oldest id was dropped");
    }

    #[test]
    fn listing_matches_dosbox_order() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let (dirs, files) = list_directory(&dir);
        assert_eq!(&dirs[..2], &[b".".to_vec(), b"..".to_vec()]);
        assert!(dirs.contains(&b"PIC".to_vec()));
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
        assert!(files.iter().any(|f| is_save_name(f)));
    }

    #[test]
    fn cwd_text_and_change_dir() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let mut s = SaveSession::default();
        assert_eq!(s.cwd_text(), b"C:\\");
        s.change_dir(&data, b"PIC");
        assert_eq!(s.cwd_text(), b"C:\\PIC");
        s.change_dir(&data, b"..");
        assert_eq!(s.cwd_text(), b"C:\\");
        s.change_dir(&data, b"NOSUCHDIR");
        assert_eq!(s.cwd_text(), b"C:\\");
    }

    #[test]
    fn save_with_an_out_of_range_position_code_is_corrupt() {
        let Some(path) = bmp_formats::save_files().into_iter().next() else {
            return;
        };
        let save = SaveGame::parse(&std::fs::read(&path).unwrap()).unwrap();
        let mut state = GameState::from_savegame(&save).unwrap();
        let id = state.squads[0]
            .iter()
            .find(|r| r.player_id != 0)
            .unwrap()
            .player_id;
        state.players[usize::from(id)].position_value = 100;
        let dir = std::env::temp_dir().join(format!("bmp-corrupt-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("CORRUPT.MAN");
        std::fs::write(&file, state.to_savegame().to_bytes().unwrap()).unwrap();
        let result = read_save(&mut SaveSession::default(), &file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result, Err(LoadError::Corrupt));
    }
}
