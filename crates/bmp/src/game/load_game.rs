//! "Spielstand Laden": the file selector and loading a `.MAN` save game.
//! Spec: `docs/re/logic/stats.md` §5.2 and the call traces of 422b:120c / 422b:2bbe.

use std::path::{Path, PathBuf};

use bmp_formats::savegame::{self, SaveGame};
use bmp_game::league_layout::{layout, saved_layout};
use bmp_game::rng::format_long_thousands;
use bmp_game::state::{GameState, StateError};

use super::disk::disk_error_dialog;
use super::start_menu::Pictures;
use super::start_menu::text_input;
use super::widgets::{
    TextButtons, dialog_okay, dialog_two_choice, ui_dialog_box, ui_value_spinner,
    ui_wait_right_click_button,
};
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
/// `DS:94bb`, `LBL[211..213]`: the delete question (`"FILE"`, the name, then these).
const DELETE_FILE: &[u8] = b"FILE";
const DELETE_QUESTION: [&[u8]; 3] = [b"WIRKLICH L|SCHEN ?", b"MACH SCHON!", b"OH NEIN!"];
/// `LBL[285..288]`: the message for a file without the save signature.
const NOT_A_SAVE: [&[u8]; 4] = [b"Kein", b"Bundesliga Manager Prof.", b"Spielstand.", b"!!!"];
/// `UI[18..19]`: the version tag does not start with `">V"`.
const WRONG_VERSION: [&[u8]; 2] = [b"Spielstand aus", b"falscher Version !"];
/// `LBL[192..193]`: the save id was loaded (or saved) before in this session.
const ALREADY_LOADED: [&[u8]; 2] = [b"Keinen heutigen", b"Spielstand laden !"];
/// `DS:9448`, `DS:9471`: the level question after loading a V1.x save.
const V1_LOADED: [&[u8]; 2] = [
    b"Sie haben einen V1.x-Spielstand geladen.",
    b"Bitte geben Sie den Spiel-Level (0-5) ein:",
];
/// The message for [`LoadError::Corrupt`]; the original has none (it hangs later).
const CORRUPT_SAVE: [&[u8]; 4] = [
    b"Achtung !",
    b"Spielstand",
    b"ist fehlerhaft !",
    b"Vorgang abgebrochen.",
];
/// MOD2026: the message for [`LoadError::OtherLayout`], port-only text.
const OTHER_LAYOUT_SAVE: [&[u8]; 4] = [
    b"Achtung !",
    b"Spielstand passt nicht",
    b"zu diesen Spieldaten !",
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
    /// `DS:942c`: the drive the selector was left on, 1 = A: or 2 = B:; 0 is the
    /// game drive C:.
    pub drive: u8,
}

impl SaveSession {
    /// `C:\` plus the directory, as `getcwd` reports it.
    pub(crate) fn cwd_text(&self) -> Vec<u8> {
        let mut s = b"C:\\".to_vec();
        s.extend_from_slice(self.dir.join("\\").as_bytes());
        s
    }

    /// `_chdrive(n)` (1 = A:) as DOSBox answers it: the floppy drives A: and B:
    /// can be selected without a disk, C: is the game directory, and any other
    /// drive does not exist (the call fails and nothing changes).
    pub(crate) fn change_drive(&mut self, n: usize) {
        match n {
            1 | 2 => self.drive = n as u8,
            3 => self.drive = 0,
            _ => {}
        }
    }

    /// The current drive is an empty floppy drive: `findfirst` finds nothing,
    /// `getcwd` fails (the old directory text stays) and nothing can be deleted.
    pub(crate) fn on_floppy(&self) -> bool {
        self.drive != 0
    }

    pub(crate) fn path(&self, data: &Path) -> PathBuf {
        self.dir.iter().fold(data.to_path_buf(), |p, c| p.join(c))
    }

    /// `chdir(name)` relative to the current directory; unknown names are ignored
    /// like a failing `chdir`.
    pub(crate) fn change_dir(&mut self, data: &Path, name: &[u8]) {
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
/// icon of the cancel button (`0x22 + icon`). `who` is the title of the delete
/// question: the name of the current manager (`FAR:304a`).
///
/// The "L|SCHEN" button asks `dialog_two_choice(FAR:304a, "FILE", name,
/// LBL[211..213])` and deletes the selected file in the current directory unless
/// the answer is "OH NEIN!". A drive button (and "L|SCHEN" when nothing was
/// deleted, slot 8 is a drive number too) calls `_chdrive(slot)` and lists the
/// directory again; see [`SaveSession::change_drive`] for the drives the port
/// knows. The drive is kept for the next selector (`DS:942c`), while the
/// directory of drive C: (`FAR:4af6`) stays the one loads and saves use.
///
/// Origin: BMMAIN.EXE 422b:2bbe (file_selector)
pub fn file_selector(
    host: &mut Host,
    saves: &mut SaveSession,
    icon: u8,
    who: &[u8],
) -> Run<Vec<u8>> {
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
        let (dirs, files) = if saves.on_floppy() {
            (Vec::new(), Vec::new())
        } else {
            list_directory(&saves.path(&data))
        };
        let mut text = b"VERZEICHNIS: ".to_vec();
        // DEVIATION: on a floppy drive `getcwd` fails and the original shows what
        // the caller's uninitialised stack buffer holds (often the last directory,
        // also "" or garbage such as "U.07"); the port shows the C: directory.
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
                        let [l3, yes, no] = DELETE_QUESTION;
                        // Automatic play (`DS:05d4`) cannot be on here: the
                        // selector is only reached from menus the player opens.
                        let answer =
                            dialog_two_choice(host, false, who, DELETE_FILE, &name, l3, yes, no)?;
                        if answer != 0 {
                            if !saves.on_floppy() {
                                delete_file(&saves.path(&data), &name);
                            }
                            name.clear();
                            continue 'list;
                        }
                    }
                    if slot <= LOGICAL_DRIVES {
                        saves.change_drive(slot);
                        continue 'list;
                    }
                }
            }
        }
    }
}

/// `dos_delete_file(name)` in the selector's directory (DOS names ignore case);
/// a failure is ignored like the original's.
///
/// Origin: BMMAIN.EXE 45ac:3690 (dos_delete_file)
fn delete_file(dir: &Path, name: &[u8]) {
    let path = dir.join(String::from_utf8_lossy(name).to_string());
    if let Some(path) = find_case_insensitive(&path) {
        let _ = std::fs::remove_file(path);
    }
}

/// Why a save could not be loaded (each case but [`LoadError::Corrupt`] and
/// [`LoadError::OtherLayout`] is a dialog in the original).
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
    /// ([`StateError::PositionCode`]), or a squad record or another saved
    /// index is past its table ([`StateError::SquadPlayerId`],
    /// [`StateError::SavedIndex`]), or a saved value would divide by zero,
    /// overflow or name a missing crest picture ([`StateError::SavedValue`]);
    /// the original loads such a save.
    Corrupt,
    /// MOD2026: the save's tier rows are another known league layout's
    /// ([`saved_layout`]) than the one of this run's data; the original has no
    /// such check.
    OtherLayout,
}

/// Loads `path` and checks it like the original: signature, version, session
/// save-id history (the id is remembered before the blocks are read, and
/// forgotten again when the save is refused as [`LoadError::Corrupt`] or
/// [`LoadError::OtherLayout`]). A save played under another league layout
/// than this run's is refused.
///
/// A file too short for the header is judged like the original judges it: the
/// short `file_read` leaves the rest of its stack buffer stale, so a file shorter
/// than the 24 compared signature bytes is "Kein ... Spielstand", and a file with
/// the signature but without the two version bytes is a wrong version (the
/// buffer still starts with the signature's "We").
pub fn read_save(saves: &mut SaveSession, path: &Path) -> Result<GameState, LoadError> {
    let bytes = std::fs::read(path).map_err(|_| LoadError::Disk)?;
    let check = savegame::SIGNATURE_CHECK_LEN;
    if bytes.get(..check) != Some(&savegame::SIGNATURE[..check]) {
        return Err(LoadError::NotASave);
    }
    if bytes.get(25..27) != Some(&b">V"[..]) {
        return Err(LoadError::WrongVersion);
    }
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
    let history = saves.loaded_ids.clone();
    if !saves.remember_id(id) {
        return Err(LoadError::AlreadyLoaded);
    }
    let save = match save {
        Ok(s) => s,
        Err(savegame::Error::Checksum { .. }) => return Err(LoadError::Checksum),
        Err(_) => return Err(LoadError::Disk),
    };
    let tail_short = save.tail_short();
    let result = GameState::from_savegame(&save).map_err(|e| match e {
        StateError::PositionCode { .. }
        | StateError::SquadPlayerId { .. }
        | StateError::SavedIndex { .. }
        | StateError::SavedValue { .. } => LoadError::Corrupt,
        StateError::BlockSize { .. } | StateError::BlockCount => LoadError::Disk,
    });
    // MOD2026: the save format has no layout marker, so the tier rows tell which
    // layout a save was played under; a save that fits none loads as in v1.0.0.
    // DEVIATION: on the original data this refuses a Mod 2026 save, whose tier
    // rows the original never writes (club 0 twice in league tier 1, clubs 36
    // and 37 in tier 2); the original loads it (stats.md §7a).
    let result = result.and_then(|state| match saved_layout(&state.league_teams) {
        Some(saved) if saved != layout() => Err(LoadError::OtherLayout),
        _ => Ok(state),
    });
    // DEVIATION: the original has no corrupt-save or layout refusal (it loads the
    // save and hangs or reads past its tables later). The port forgets the id of
    // a refused save again, the dropped oldest id included, so picking the same
    // file again shows the refusal message instead of "already loaded".
    if matches!(result, Err(LoadError::Corrupt | LoadError::OtherLayout)) {
        saves.loaded_ids = history;
    }
    // A file that ends before the first tail long counts as having passed the
    // copy protection: the loader calls 1cb5:0000 (`DS:05ac = 1`) after taking
    // `DS:05ac` from block 54. The caller copies `ds_05fa[0]` into the flag,
    // and the next save writes `ds_05fa[0]` from the flag again, so setting it
    // here is the same.
    // Origin: BMMAIN.EXE 422b:1d28 (screen_load_game), 1cb5:0000
    result.map(|mut state| {
        if tail_short {
            state.ds_05fa[0] = 1;
        }
        state
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
/// Errors: a file that cannot be opened or read gets `disk_error_dialog`, a
/// missing signature the `dialog_okay` box "Kein Bundesliga Manager Prof.
/// Spielstand. !!!", a wrong version and a save id loaded before the timed
/// `ui_dialog_box`, a checksum mismatch the endless "MOGELGURU" screen, and a
/// corrupt save ([`LoadError::Corrupt`], port only) and a save of the other
/// league layout ([`LoadError::OtherLayout`], Mod 2026) a `dialog_okay` box. A `>V1`
/// save asks for the level (0-5, `ui_value_spinner`, default 3) and stores
/// `5 - level`. `who` titles the delete question of the selector.
///
/// The original reads the remaining blocks after a disk error (`DS:9440` makes
/// them no-ops) and then leaves the half-loaded data in place; the port loads
/// nothing in that case, a failed load never touches the running game.
///
/// Origin: BMMAIN.EXE 422b:120c (screen_load_game)
pub fn screen_load_game(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    arg: u8,
    who: &[u8],
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
    let name = file_selector(host, &mut session.saves, arg, who)?;
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
        Err(LoadError::OtherLayout) => {
            let [a, b, c, d] = OTHER_LAYOUT_SAVE;
            dialog_okay(host, a, b, c, d)?;
            return Ok(None);
        }
        // DEVIATION: a read error after the header leaves the original with a
        // half-loaded game (the blocks read so far stay in memory); the port
        // loads nothing.
        Err(LoadError::Disk) => {
            disk_error_dialog(host)?;
            return Ok(None);
        }
        Err(LoadError::NotASave) => {
            let [a, b, c, d] = NOT_A_SAVE;
            dialog_okay(host, a, b, c, d)?;
            return Ok(None);
        }
        // DEVIATION: after the signature check the original frees the message list
        // (`messages_free_all` 4091:0532) before it compares the version and the
        // save id, so a refused load from the office loses the running game's
        // messages. The port keeps them.
        Err(LoadError::WrongVersion) => {
            ui_dialog_box(host, WRONG_VERSION[0], WRONG_VERSION[1])?;
            return Ok(None);
        }
        Err(LoadError::AlreadyLoaded) => {
            ui_dialog_box(host, ALREADY_LOADED[0], ALREADY_LOADED[1])?;
            return Ok(None);
        }
    };
    session.saves.last_name = name;
    session.copy_protection_ok = state.ds_05fa[0] != 0;
    // The save sets `DS:060c` ("Blenden") with the other options.
    host.sync_options(&state);
    let screen_crest = pics.crest_cache;
    for m in 0..usize::from(state.manager_count).min(4) {
        let team = state.managers[m].team as usize;
        let code = state.clubs.get(team).map_or(0, |c| c.crest) & 0x7f;
        // `GameState::from_savegame` refuses a crest code without a shipped picture.
        let spr = host.picture(u16::from(code) + 0x50)?;
        host.g
            .blit_bitmap(spr, 0, 0, screen_crest, 0x29 * m as i16, 0, 0x28, 0x26);
        host.g.free(spr);
    }
    host.screen_fade(0)?;
    let sheet = host.picture(0)?;
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
    // `DS:9440 = 0; DS:05ad = 0`: a loaded game may be saved again.
    session.save_disabled = false;
    if state.version[2] == b'1' {
        state.level = v1_level_question(host)?;
    }
    Ok(Some(Box::new(state)))
}

/// The level question after loading a V1.x save: the two lines on a cleared
/// screen and a value spinner 0..5 starting at 3. Returns the stored level
/// `5 - input` (`DS:4a28`; input 3 when the spinner was left unchanged).
///
/// Origin: BMMAIN.EXE 422b:120c (screen_load_game, 422b:20bf..21b4)
fn v1_level_question(host: &mut Host) -> Run<u8> {
    host.screen_fade(0)?;
    host.g.rastport_clear(&host.rp, 8);
    host.set_font(1);
    for (line, y) in V1_LOADED.iter().zip([0x14, 0x1e]) {
        ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, line, 0, 0x13f, y, 1, 0);
    }
    host.screen_fade(1)?;
    let input = match ui_value_spinner(host, 3, 0x7d, 0x32, 0, 5, 1)? {
        -1 => 3,
        v => v,
    };
    Ok(5u8.wrapping_sub(input as u8))
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
pub(crate) mod tests {
    use super::*;
    use bmp_formats::mana::ManaDat;
    use bmp_game::league_layout::{MOD2026, ORIGINAL, set_layout};
    use bmp_game::state::{CREST_CODES, DAY_COUNTER_MAX};

    /// A save written by the port on Mod 2026 data: a new game on `mana` at its
    /// first office turn. Leaves the selected layout as it was.
    pub(crate) fn mod_save_bytes(mana: &ManaDat) -> Vec<u8> {
        use bmp_game::newgame::{
            CupDraws, GameOptions, MODE_ENDLESS, ManagerChoice, MenuChoices, new_game, startup,
        };
        let previous = layout();
        set_layout(&MOD2026);
        let mut rt = bmp_game::runtime::Runtime::default();
        let mut rng = bmp_game::rng::Rng::default();
        let mut draws = CupDraws(|_| false);
        let mut s = startup(mana, &mut rt, &mut rng, 35372, &mut draws);
        let choices = MenuChoices {
            managers: std::array::from_fn(|m| ManagerChoice {
                name: if m == 0 {
                    b"OLIVER".to_vec()
                } else {
                    Vec::new()
                },
                team: if m == 0 { 20 } else { bmp_game::state::NO_TEAM },
                slot: m as u8 + 1,
            }),
            count: 1,
            carousel_pos: 2,
            options: GameOptions {
                level: 2,
                mode_flags: MODE_ENDLESS,
            },
        };
        new_game(&mut s, &mut rt, &mut rng, mana, &choices, 35400, &mut draws);
        set_layout(previous);
        s.to_savegame().to_bytes().unwrap()
    }

    /// `MOD.MAN` holding [`mod_save_bytes`] of `BMP_DATA`'s `MANA.DAT` in a
    /// temporary directory named after `tag`. `None` without `BMP_DATA`.
    fn write_mod_save(tag: &str) -> Option<(PathBuf, PathBuf)> {
        let data = bmp_formats::data_dir()?;
        let mana = ManaDat::parse(&std::fs::read(data.join("MANA.DAT")).unwrap()).unwrap();
        let dir = std::env::temp_dir().join(format!("bmp-mod-save-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("MOD.MAN");
        std::fs::write(&file, mod_save_bytes(&mana)).unwrap();
        Some((dir, file))
    }

    /// Every shipped (original) save is refused on Mod 2026 data, again on a
    /// second pick, and leaves even a full save-id history unchanged; the same
    /// session then loads it on the original data.
    #[test]
    fn an_original_save_is_refused_in_a_mod_run() {
        let files = bmp_formats::save_files();
        let Some(first) = files.first() else {
            return;
        };
        let mut saves = SaveSession {
            loaded_ids: (100..150).collect(),
            ..SaveSession::default()
        };
        let full = saves.loaded_ids.clone();
        set_layout(&MOD2026);
        let refused: Vec<_> = files
            .iter()
            .chain(files.iter())
            .map(|f| (f, read_save(&mut saves, f).err(), saves.loaded_ids.clone()))
            .collect();
        set_layout(&ORIGINAL);
        let loaded = read_save(&mut saves, first);
        for (f, err, ids) in refused {
            let what = f.display();
            assert_eq!(err, Some(LoadError::OtherLayout), "{what}");
            assert_eq!(ids, full, "{what}: history unchanged");
        }
        assert!(loaded.is_ok(), "{}: {:?}", first.display(), loaded.err());
    }

    /// A save the port wrote on Mod 2026 data is refused on the original data,
    /// and its id is not remembered.
    #[test]
    fn a_mod_save_is_refused_in_an_original_run() {
        let Some((dir, file)) = write_mod_save("refused") else {
            return;
        };
        let mut saves = SaveSession::default();
        let result = read_save(&mut saves, &file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result.err(), Some(LoadError::OtherLayout));
        assert!(saves.loaded_ids.is_empty(), "no id was remembered");
    }

    /// A save the port wrote on Mod 2026 data loads on Mod 2026 data, with the
    /// 18 / 18 / 20 club tier rows of the mod layout.
    #[test]
    fn a_mod_save_loads_in_a_mod_run() {
        let Some((dir, file)) = write_mod_save("loaded") else {
            return;
        };
        set_layout(&MOD2026);
        let result = read_save(&mut SaveSession::default(), &file);
        set_layout(&ORIGINAL);
        std::fs::remove_dir_all(&dir).unwrap();
        let state = result.expect("the mod save loads");
        assert_eq!(saved_layout(&state.league_teams), Some(&MOD2026));
    }

    /// A save whose tier rows fit no known layout (a club twice in league
    /// tier 2) loads as in v1.0.0 on both data sets, and its id is remembered.
    #[test]
    fn a_save_of_no_known_layout_loads_in_both_runs() {
        let dir = std::env::temp_dir().join(format!("bmp-no-layout-{}", std::process::id()));
        let unknown = |s: &mut GameState| s.league_teams[2][0] = s.league_teams[2][1];
        let Some(file) = write_corrupt_save(&dir, unknown) else {
            return;
        };
        let mut original = SaveSession::default();
        let on_original = read_save(&mut original, &file);
        set_layout(&MOD2026);
        let mut modded = SaveSession::default();
        let on_mod = read_save(&mut modded, &file);
        set_layout(&ORIGINAL);
        std::fs::remove_dir_all(&dir).unwrap();
        for (run, result, saves) in [("original", on_original, original), ("mod", on_mod, modded)] {
            let state = result.unwrap_or_else(|e| panic!("{run}: {e:?}"));
            assert_eq!(saved_layout(&state.league_teams), None, "{run}");
            assert_eq!(saves.loaded_ids.len(), 1, "{run}: the id is remembered");
        }
    }

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

    /// Gives the first squad player of squad 0 an out-of-range position code (100).
    fn corrupt_position_code(state: &mut GameState) {
        let id = state.squads[0]
            .iter()
            .find(|r| r.player_id != 0)
            .unwrap()
            .player_id;
        state.players[usize::from(id)].position_value = 100;
    }

    /// [`read_save`] of a `CORRUPT.MAN` written by [`write_corrupt_save`] into a
    /// temporary directory named after `tag`. `None` without `BMP_DATA`.
    fn read_corrupt_save(
        tag: &str,
        corrupt: fn(&mut GameState),
    ) -> Option<Result<GameState, LoadError>> {
        let dir =
            std::env::temp_dir().join(format!("bmp-corrupt-save-{tag}-{}", std::process::id()));
        let file = write_corrupt_save(&dir, corrupt)?;
        let result = read_save(&mut SaveSession::default(), &file);
        std::fs::remove_dir_all(&dir).unwrap();
        Some(result)
    }

    #[test]
    fn save_with_a_squad_record_past_the_player_table_is_corrupt() {
        let corrupt = |s: &mut GameState| s.squads[0][0].player_id = 151;
        let Some(result) = read_corrupt_save("squad", corrupt) else {
            return;
        };
        assert_eq!(result, Err(LoadError::Corrupt));
    }

    /// A corrupt save is refused again on every pick and leaves the save-id
    /// history unchanged, even a full one; a checksum error and a valid save
    /// keep their id, so the second pick is "already loaded".
    #[test]
    fn a_refused_corrupt_save_is_not_remembered() {
        let dir = std::env::temp_dir().join(format!("bmp-corrupt-again-{}", std::process::id()));
        let Some(corrupt) = write_corrupt_save(&dir, corrupt_position_code) else {
            return;
        };
        let checksum = dir.join("CHECKSUM.MAN");
        std::fs::write(
            &checksum,
            patched_save(|b| b[savegame::ENCRYPTED_START + 100] ^= 1).unwrap(),
        )
        .unwrap();
        let valid = dir.join("VALID.MAN");
        std::fs::write(&valid, patched_save(|_| {}).unwrap()).unwrap();

        let mut saves = SaveSession {
            loaded_ids: (100..150).collect(),
            ..SaveSession::default()
        };
        let full = saves.loaded_ids.clone();
        let mut corrupt_results = Vec::new();
        for _ in 0..2 {
            corrupt_results.push((
                read_save(&mut saves, &corrupt).err(),
                saves.loaded_ids.clone(),
            ));
        }
        let mut saves_2 = SaveSession::default();
        let checksum_results = [
            read_save(&mut saves_2, &checksum).err(),
            read_save(&mut saves_2, &checksum).err(),
        ];
        let mut saves_3 = SaveSession::default();
        let valid_results = [
            read_save(&mut saves_3, &valid).err(),
            read_save(&mut saves_3, &valid).err(),
        ];
        std::fs::remove_dir_all(&dir).unwrap();

        for (i, (err, ids)) in corrupt_results.into_iter().enumerate() {
            assert_eq!(err, Some(LoadError::Corrupt), "corrupt pick {i}");
            assert_eq!(ids, full, "corrupt pick {i}: history unchanged");
        }
        assert_eq!(
            checksum_results,
            [Some(LoadError::Checksum), Some(LoadError::AlreadyLoaded)]
        );
        assert_eq!(valid_results, [None, Some(LoadError::AlreadyLoaded)]);
    }

    /// Writes `CORRUPT.MAN` into `dir`: the first real save changed by `corrupt`
    /// and with the copy-protection flag `ds_05fa[0]` cleared, so a successful
    /// load would change `copy_protection_ok`. `None` without `BMP_DATA`.
    fn write_corrupt_save(dir: &Path, corrupt: fn(&mut GameState)) -> Option<PathBuf> {
        let path = bmp_formats::save_files().into_iter().next()?;
        let save = SaveGame::parse(&std::fs::read(&path).unwrap()).unwrap();
        let mut state = GameState::from_savegame(&save).unwrap();
        corrupt(&mut state);
        state.ds_05fa[0] = 0;
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join("CORRUPT.MAN");
        std::fs::write(&file, state.to_savegame().to_bytes().unwrap()).unwrap();
        Some(file)
    }

    #[test]
    fn only_the_floppy_drives_and_c_can_be_selected() {
        let mut s = SaveSession::default();
        assert!(!s.on_floppy());
        for n in [4, 5, 7, 8] {
            s.change_drive(n);
            assert!(!s.on_floppy(), "drive {n} does not exist");
        }
        s.change_drive(1);
        assert!(s.on_floppy());
        s.change_drive(5);
        assert_eq!(s.drive, 1, "a failing _chdrive keeps the drive");
        s.change_drive(2);
        assert_eq!(s.drive, 2);
        s.change_drive(3);
        assert!(!s.on_floppy());
    }

    /// A temporary game directory holding `files` (name, bytes) plus the original
    /// data (linked). `None` without `BMP_DATA`.
    #[cfg(unix)]
    fn game_dir(tag: &str, files: &[(&str, Vec<u8>)]) -> Option<PathBuf> {
        let data = bmp_formats::data_dir()?;
        let dir = std::env::temp_dir().join(format!("bmp-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        for e in std::fs::read_dir(&data).unwrap().flatten() {
            let link = dir.join(e.file_name());
            if !link.exists() {
                std::os::unix::fs::symlink(e.path(), link).unwrap();
            }
        }
        Some(dir)
    }

    /// The bytes of the first shipped save with `edit` applied.
    fn patched_save(edit: impl FnOnce(&mut Vec<u8>)) -> Option<Vec<u8>> {
        let path = bmp_formats::save_files().into_iter().next()?;
        let mut bytes = std::fs::read(path).unwrap();
        edit(&mut bytes);
        Some(bytes)
    }

    /// Runs `screen_load_game` with the preset file name `name` (taken by a right
    /// click on the name field) and `script` after it.
    #[cfg(unix)]
    fn load_with_script(
        dir: &Path,
        session: &mut Session,
        name: &[u8],
        script: &str,
    ) -> (Run<Option<Box<GameState>>>, usize) {
        use crate::host::{Host, parse_script};
        let script =
            parse_script(&format!("wait 1000\nrclick 100 178\n{script}wait 5000\n")).unwrap();
        let mut host = Host::with_script(dir, script, dir, false).unwrap();
        let pics = Pictures::load(&mut host).unwrap();
        session.saves.last_name = name.to_vec();
        let result = screen_load_game(&mut host, session, &pics, 0, b"OLIVER");
        (result, host.script_clicks_left())
    }

    /// Wrong version, a save id loaded before and a missing signature: each shows
    /// its dialog (the timed boxes need no click, the "Kein ... !!!" box takes
    /// the OKAY click), nothing is loaded and the session is left as it was. The
    /// running game is not touched either (the original frees its messages,
    /// stats.md §7a).
    #[cfg(unix)]
    #[test]
    fn refused_loads_show_their_dialogs_and_load_nothing() {
        use bmp_game::rng::Rng;
        let Some(old) = patched_save(|b| b[0x1a] = b'X') else {
            return;
        };
        let good = patched_save(|_| {}).unwrap();
        let nosave = patched_save(|b| b[0] = b'X').unwrap();
        let id = u32::from_le_bytes(good[29..33].try_into().unwrap());
        let files = [
            ("OLDVER.MAN", old),
            ("AGAIN.MAN", good),
            ("NOSAVE.MAN", nosave),
        ];
        let dir = game_dir("refused-load", &files).unwrap();
        let mut session = Session::new(Rng::new(1));
        session.saves.loaded_ids = vec![id];
        let cases: [(&[u8], &str); 3] = [
            (b"OLDVER", ""),
            (b"AGAIN", ""),
            (b"NOSAVE", "wait 1000\nclick 191 165\n"),
        ];
        let mut results = Vec::new();
        for (name, script) in cases {
            let (result, clicks_left) = load_with_script(&dir, &mut session, name, script);
            results.push((
                matches!(result, Ok(None)),
                clicks_left,
                session.saves.last_name.clone(),
            ));
        }
        std::fs::remove_dir_all(&dir).unwrap();
        for ((ok, clicks_left, last), (name, _)) in results.into_iter().zip(cases) {
            assert!(ok, "{name:?}: nothing is loaded");
            assert_eq!(clicks_left, 0, "{name:?}: every click was taken");
            assert_eq!(last, name, "{name:?}: last name unchanged");
        }
        assert_eq!(session.saves.loaded_ids, vec![id], "no id was remembered");
    }

    /// A `>V1` save asks for the level: one click into the upper half of the
    /// spinner raises 3 to 4, the right click ends it, and the level is 5 - 4.
    #[cfg(unix)]
    #[test]
    fn a_v1_save_asks_for_the_level() {
        use bmp_game::rng::Rng;
        let Some(v1) = patched_save(|b| b[0x1b] = b'1') else {
            return;
        };
        let dir = game_dir("v1-load", &[("V1.MAN", v1)]).unwrap();
        let mut session = Session::new(Rng::new(1));
        session.save_disabled = true;
        let script = "wait 2000\nclick 160 58\nwait 500\nrclick 160 120\n";
        let (result, clicks_left) = load_with_script(&dir, &mut session, b"V1", script);
        std::fs::remove_dir_all(&dir).unwrap();
        let state = result.ok().flatten().expect("the V1 save is loaded");
        assert_eq!(clicks_left, 0);
        assert_eq!(state.level, 1);
        assert!(!session.save_disabled, "DS:05ad is cleared by a load");
    }

    /// The delete button: "OH NEIN!" keeps the selected file, "MACH SCHON!"
    /// deletes it; the cancel icon then leaves the selector.
    #[cfg(unix)]
    #[test]
    fn the_delete_button_asks_and_deletes() {
        use crate::host::{Host, parse_script};
        let Some(save) = patched_save(|_| {}) else {
            return;
        };
        let dir = game_dir("delete-file", &[("DELME.MAN", save)]).unwrap();
        let run = |answer: &str| {
            // The preset name is the selected file; L|SCHEN, the answer, cancel.
            let script = format!(
                "wait 1000\nclick 210 205\nwait 500\nclick {answer}\nwait 500\nclick 290 210\nwait 1000\n"
            );
            let mut host =
                Host::with_script(&dir, parse_script(&script).unwrap(), &dir, false).unwrap();
            let mut saves = SaveSession {
                last_name: b"DELME.MAN".to_vec(),
                ..SaveSession::default()
            };
            let name = file_selector(&mut host, &mut saves, 0, b"OLIVER");
            (name, dir.join("DELME.MAN").exists())
        };
        let (kept_name, kept) = run("187 166");
        let (deleted_name, still_there) = run("117 166");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(kept_name, Ok(ref n) if n.is_empty()), "cancelled");
        assert!(kept, "OH NEIN! keeps the file");
        assert!(
            matches!(deleted_name, Ok(ref n) if n.is_empty()),
            "cancelled"
        );
        assert!(!still_there, "MACH SCHON! deletes the file");
    }

    /// Gives the club of manager 0 crest code [`CREST_CODES`], the first
    /// without a `PIC` picture file.
    fn corrupt_crest(state: &mut GameState) {
        let team = usize::from(state.managers[0].team);
        state.clubs[team].crest = CREST_CODES;
    }

    /// The saved values that are no index (`state.md` §1.1): a ticket price of
    /// 0, a day counter that overflows or gives day 0 of a month, a crest code
    /// without a picture.
    #[test]
    fn save_with_an_out_of_range_value_is_corrupt() {
        type Corrupt = fn(&mut GameState);
        let cases: [(&str, Corrupt); 5] = [
            ("popularity", |s| s.managers[0].value_10a = 0),
            ("day-max", |s| s.day_counter = i32::MAX - 0x10),
            ("day-zero", |s| s.day_counter = -0xd2),
            ("day-past", |s| s.day_counter = DAY_COUNTER_MAX + 1),
            ("crest", corrupt_crest),
        ];
        for (tag, corrupt) in cases {
            let Some(result) = read_corrupt_save(tag, corrupt) else {
                return;
            };
            assert_eq!(result, Err(LoadError::Corrupt), "{tag}");
        }
    }

    #[test]
    fn save_with_an_out_of_range_position_code_is_corrupt() {
        let Some(result) = read_corrupt_save("position", corrupt_position_code) else {
            return;
        };
        assert_eq!(result, Err(LoadError::Corrupt));
    }

    /// Files too short for the header get the original's header dialogs, not the
    /// disk error: no signature (empty, 23 bytes) is "Kein ... Spielstand", the
    /// signature without the version bytes a wrong version; the id is not
    /// remembered.
    #[test]
    fn short_files_fail_the_header_checks() {
        use bmp_formats::savegame::SIGNATURE;
        let dir = std::env::temp_dir().join(format!("bmp-short-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sig_v = SIGNATURE.to_vec();
        sig_v.extend_from_slice(b">V");
        let cases: [(&[u8], LoadError); 5] = [
            (b"", LoadError::NotASave),
            (&SIGNATURE[..23], LoadError::NotASave),
            (&SIGNATURE[..], LoadError::WrongVersion),
            (&sig_v[..26], LoadError::WrongVersion),
            (&sig_v[..], LoadError::Disk),
        ];
        let mut saves = SaveSession::default();
        let mut results = Vec::new();
        for (i, (bytes, _)) in cases.iter().enumerate() {
            let file = dir.join(format!("SHORT{i}.MAN"));
            std::fs::write(&file, bytes).unwrap();
            results.push(read_save(&mut saves, &file));
        }
        std::fs::remove_dir_all(&dir).unwrap();
        for (result, (bytes, want)) in results.into_iter().zip(cases) {
            assert_eq!(result.err(), Some(want), "{} bytes", bytes.len());
        }
        assert!(saves.loaded_ids.is_empty());
    }

    /// The tail is read without end-of-file checks: a save cut inside the first
    /// tail long still loads and counts as having passed the copy protection
    /// (1cb5:0000), one cut after it keeps the saved flag, and bytes after the
    /// tail are ignored.
    #[test]
    fn short_tail_loads_and_passes_the_copy_protection() {
        let dir = std::env::temp_dir().join(format!("bmp-short-tail-{}", std::process::id()));
        // write_corrupt_save stores the protection flag as failed (0).
        let Some(file) = write_corrupt_save(&dir, |_| {}) else {
            return;
        };
        let full = std::fs::read(&file).unwrap();
        let tail_start = full.len() - 12;
        let cases: [(usize, bool, u8); 4] =
            [(0, false, 1), (3, false, 1), (4, false, 0), (12, true, 0)];
        for (kept, extra, want) in cases {
            let mut bytes = full[..tail_start + kept].to_vec();
            if extra {
                bytes.extend_from_slice(b"JUNK");
            }
            let path = dir.join(format!("TAIL{kept}.MAN"));
            std::fs::write(&path, &bytes).unwrap();
            let state = read_save(&mut SaveSession::default(), &path)
                .unwrap_or_else(|e| panic!("{kept} tail bytes: {e:?}"));
            assert_eq!(state.ds_05fa[0], want, "{kept} tail bytes");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The [`LoadError::Corrupt`] arm end to end: the file selector returns
    /// `CORRUPT.MAN`, the message box is shown and dismissed with its OKAY
    /// button, and nothing is loaded or remembered. A second pick of the same
    /// file in the same session shows the message box again (the timed
    /// "already loaded" box would leave the OKAY click unused).
    #[cfg(unix)]
    #[test]
    fn loading_a_corrupt_save_shows_the_message_and_loads_nothing() {
        corrupt_save_end_to_end("position", corrupt_position_code);
    }

    /// The crest code of the loaded managers' clubs is drawn right after the
    /// load: a code without a picture is refused before that `expect`.
    #[cfg(unix)]
    #[test]
    fn loading_a_save_with_a_missing_crest_shows_the_message() {
        corrupt_save_end_to_end("crest", corrupt_crest);
    }

    /// The body of the end-to-end corrupt-save tests, in a directory named after `tag`.
    #[cfg(unix)]
    fn corrupt_save_end_to_end(tag: &str, corrupt: fn(&mut GameState)) {
        use crate::host::{Host, parse_script};
        use bmp_game::rng::Rng;

        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        // A game directory holding the original data (linked) plus the corrupt save.
        let dir =
            std::env::temp_dir().join(format!("bmp-corrupt-load-{tag}-{}", std::process::id()));
        if write_corrupt_save(&dir, corrupt).is_none() {
            return;
        }
        for e in std::fs::read_dir(&data).unwrap().flatten() {
            let link = dir.join(e.file_name());
            if !link.exists() {
                std::os::unix::fs::symlink(e.path(), link).unwrap();
            }
        }
        // Right-click on the file name field takes the preset name ("CORRUPT"
        // plus ".MAN"); the left click hits the OKAY button of the message box.
        let script = parse_script("wait 1000\nrclick 100 178\nwait 1000\nclick 191 165\n").unwrap();
        let mut host = Host::with_script(&dir, script, &dir, false).unwrap();
        let pics = Pictures::load(&mut host).unwrap();
        let mut session = Session::new(Rng::new(1));
        session.saves.last_name = b"CORRUPT".to_vec();
        let result = screen_load_game(&mut host, &mut session, &pics, 0, b"");
        let clicks_left = host.script_clicks_left();
        let ids_after_first = session.saves.loaded_ids.clone();
        let (again, again_clicks_left) =
            load_with_script(&dir, &mut session, b"CORRUPT", "wait 1000\nclick 191 165\n");
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(matches!(result, Ok(None)), "nothing is loaded");
        assert_eq!(clicks_left, 0, "the message box took the OKAY click");
        assert!(ids_after_first.is_empty(), "no id was remembered");
        assert!(matches!(again, Ok(None)), "second pick: nothing is loaded");
        assert_eq!(again_clicks_left, 0, "second pick: the message box again");
        assert!(
            session.saves.loaded_ids.is_empty(),
            "still no id remembered"
        );
        assert_eq!(session.saves.last_name, b"CORRUPT", "last name unchanged");
        assert!(session.copy_protection_ok, "copy-protection flag unchanged");
    }

    /// The [`LoadError::OtherLayout`] arm end to end: a shipped save picked on
    /// Mod 2026 data shows the message box, which takes the OKAY click, and
    /// nothing is loaded or remembered; a second pick shows the box again.
    #[cfg(unix)]
    #[test]
    fn loading_a_save_of_the_other_layout_shows_the_message_and_loads_nothing() {
        use bmp_game::rng::Rng;
        let Some(save) = patched_save(|_| {}) else {
            return;
        };
        let dir = game_dir("other-layout-load", &[("ORIGINAL.MAN", save)]).unwrap();
        let mut session = Session::new(Rng::new(1));
        session.copy_protection_ok = false;
        session.saves.last_name = b"BEFORE".to_vec();
        set_layout(&MOD2026);
        let picks: Vec<_> = (0..2)
            .map(|_| {
                let (result, clicks_left) = load_with_script(
                    &dir,
                    &mut session,
                    b"ORIGINAL",
                    "wait 1000\nclick 191 165\n",
                );
                (matches!(result, Ok(None)), clicks_left)
            })
            .collect();
        set_layout(&ORIGINAL);
        std::fs::remove_dir_all(&dir).unwrap();
        for (i, (nothing_loaded, clicks_left)) in picks.into_iter().enumerate() {
            assert!(nothing_loaded, "pick {i}: nothing is loaded");
            assert_eq!(
                clicks_left, 0,
                "pick {i}: the message box took the OKAY click"
            );
        }
        assert!(session.saves.loaded_ids.is_empty(), "no id was remembered");
        assert_eq!(session.saves.last_name, b"ORIGINAL", "only the preset name");
        assert!(
            !session.copy_protection_ok,
            "copy-protection flag unchanged"
        );
    }
}
