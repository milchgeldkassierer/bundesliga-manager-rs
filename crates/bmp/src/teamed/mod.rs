//! TEAMED.EXE: the team editor shipped with the game (see `docs/re/teamed/overview.md`).
//!
//! The editor edits `MANA.DAT` (club names, crests, league placement, last season's
//! table, player names, the strengths of the foreign clubs) on one 320x242 screen:
//! an information panel and the three buttons at the top left, the player list at the
//! top right and a grid of 8 x 4 crests below, which the bar at the right edge pages
//! ("LIGA") or swaps for the UEFA strength list ("UEFA").
//!
//! It links the same graphics library as `BMMAIN.EXE` (segments `16b7` = `45ac`,
//! `1a46` = `4930`, `1000` UI helpers = `16c7`/`176b`, `1b1f` = CRT `4a01`), so the port
//! reuses [`crate::gfx`] and [`crate::host`]. The differences are a 242-line CRTC table
//! ([`Gfx::vga_set_mode_x_teamed`]) and bitmap descriptors with an origin
//! ([`crate::gfx::Bitmap::origin`]): the crest grid draws through a view of the screen
//! that starts at row 75.
//!
//! Ported: the start-up, the main loop's hover display, the "LIGA" paging,
//! "PROGRAMM-ENDE" with the save question and the strength recomputation ([`save`]), the
//! `MANA.DAT` reader and writer, the editing of a selected club ([`club`]) and the UEFA
//! list ([`uefa`]) and the crest editor with its picture import ([`crest`],
//! [`crest_import`]).

pub mod club;
pub mod crest;
pub mod crest_import;
pub mod dialog;
pub mod save;
pub mod uefa;

use std::borrow::Cow;

use bmp_formats::mana::{self, ManaDat};
use bmp_game::league_layout::{self, MOD2026, ORIGINAL, is_mod2026};
use bmp_game::rng::{NumberFormat, Rng, format_long_thousands};

use crate::game::tables::league_names;
use crate::game::text_to_upper_de;
use crate::gfx::{self, Bitmap, BitmapId, Gfx, RastPort, TEAMED_SCREEN_H};
use crate::host::{Host, LEFT_DOWN, Run};

/// First screen row of the crest grid view (`DS:005A`, the origin of the bitmap
/// `DS:1DF6`): every crest-area coordinate of the original is relative to it.
pub const GRID_ORIGIN_Y: i16 = 0x4b;
/// Size of a crest picture.
pub const CREST_SIZE: i16 = 0x26;
/// Crest grid pitch (crest plus a one-pixel gap).
pub const CREST_PITCH: i16 = 0x27;
/// Crests per page (8 columns x 4 rows).
pub const CRESTS_PER_PAGE: u8 = 0x20;
/// Pictures the editor loads at start-up (`PIC\<n>.VGA`).
pub const PIC_BAR: u16 = 0x3f;
/// Second picture loaded at start-up (`DS:011A`); never drawn (see `Editor::crest_import_pic`).
pub const PIC_CREST_IMPORT: u16 = 0x40;
/// Background of the crest editor (`DS:011E`, blitted by `1000:1d73`).
pub const PIC_CREST_EDITOR: u16 = 0x3d;
/// Top part of the screen: panel labels, buttons, player list frame.
pub const PIC_BACKGROUND: u16 = 0x3c;

/// The 32-entry `0x0RGB` palette at `DS:00C0` (entry 0x20 is set to black afterwards).
pub const PALETTE: [u16; 32] = [
    0x0000, 0x0aac, 0x088a, 0x0779, 0x0668, 0x0557, 0x0446, 0x0335, 0x0007, 0x0975, 0x0ba8, 0x0dcb,
    0x0561, 0x0672, 0x0683, 0x0794, 0x0601, 0x0901, 0x0b02, 0x0c72, 0x0a62, 0x0741, 0x034c, 0x0ff0,
    0x0ba7, 0x0985, 0x0864, 0x0643, 0x0532, 0x0fff, 0x0c3c, 0x0000,
];

/// League names by group (`DS:0326` -> `DS:02F0..`), shown in the information panel.
pub const LEAGUE_NAMES: [&[u8]; 4] = [
    b"BUNDESLIGA",
    b"2.BUNDESLI.",
    b"REGIONALLIGA    ",
    b"OBERLIGA    ",
];

/// The name of league group `group` in the information panel.
fn league_name(group: usize) -> Cow<'static, [u8]> {
    // MOD2026: a mod game names the third group as the game names its third tier, upper-cased.
    if group == 2 && is_mod2026() {
        Cow::Owned(text_to_upper_de(league_names()[2]))
    } else {
        Cow::Borrowed(LEAGUE_NAMES[group])
    }
}

/// TEAMED's game font numbers for `ui_set_font` (`1000:0e47`, name table at `DS:0060`
/// with a 0x14 stride: 1 newbold, 2 flow, 3 micro4) mapped to library indices
/// ([`gfx::FONT_NAMES`]).
const FONT_TO_LIBRARY: [u16; 4] = [0, 3, 2, 1];

/// Selects game font `n` for a rastport.
///
/// Origin: TEAMED.EXE 1000:0e47 (ui_set_font)
pub fn set_font(rp: &mut RastPort, n: usize) {
    rp.font = Some(FONT_TO_LIBRARY[n]);
}

/// League group of a German club: `(i > 17) + (i > 37) + (i > 57)`, the last club ids of
/// the three tiers; group 3 is the pool.
///
/// MOD2026: the last club ids come from the selected [`LeagueLayout`](league_layout::LeagueLayout).
pub fn league_group(club: u8) -> usize {
    let l = league_layout::layout();
    (0..3).filter(|&tier| club > l.last_team(tier)).count()
}

/// Size of the main loop's input buffer `[BP-AA]..[BP-58]`.
pub const INPUT_BUF_LEN: usize = 0x52;

/// Exit state of the main loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// "PROGRAMM-ENDE" (after the save question).
    Ended,
    /// A file could not be opened at start-up: the "DISK" screen was shown.
    Fatal,
}

/// The editor's state: the `MANA.DAT` buffers, the pictures and the screen.
pub struct Editor {
    /// The buffers read from `MANA.DAT`: crests `DS:5956`, strength `DS:33CA`, points
    /// `DS:467C`, goals `DS:364E`, ranks `DS:1E04`, club names `DS:0042` and the player
    /// names in the far buffer `[5A02]`.
    pub mana: ManaDat,
    /// The copies made at start-up (`DS:21CA` names, `DS:57E4` points, `DS:2058` goals,
    /// `DS:123E` ranks, `[213E]` players): the edit buffer. The panel and player inputs
    /// write here, `1000:26f1` shows the selected club from here, "AENDERUNG SICHERN"
    /// copies all of it back into [`Editor::mana`] (see `docs/re/teamed/club.md`). Only
    /// those fields of this copy are used; its crests and strengths are never read.
    pub copy: ManaDat,
    /// Screen rastport (`DS:1E44`, the mouse library's rastport on the screen).
    pub screen: RastPort,
    /// Rastport of the crest view (`DS:364A` on bitmap `DS:1DF6`: the screen, origin
    /// `(0, 75)`).
    pub grid: RastPort,
    /// "LIGA"/"UEFA" bar (`DS:0116`, `PIC\63.VGA`).
    pub bar: BitmapId,
    /// `DS:011A` (`PIC\64.VGA`): loaded at start-up and only freed again by
    /// `fatal_error_exit` (`1000:1039`); nothing draws it (not even the crest import).
    #[allow(dead_code)]
    pub crest_import_pic: BitmapId,
    /// `DS:011E` (`PIC\61.VGA`), background of the crest editor.
    pub crest_editor_pic: BitmapId,
    /// Crest pictures of the 64 German clubs (`DS:5698`).
    pub crests: Vec<BitmapId>,
    /// Two blank 38x38 pictures the main loop allocates (`[BP-58]`, `[BP-C8]`).
    pub scratch: [BitmapId; 2],
    /// Club under the pointer whose data the panel shows (`[BP-F4]`, -1 = none).
    pub hovered: i16,
    /// Club being edited (`DS:0336`, 0xFF = none).
    pub selected: u8,
    /// Crest page: clubs `page * 32 ..` (`[BP-D8]`).
    pub page: u8,
    /// `DS:0340` (initially 1): enables the duplicate checks of club and player
    /// names; toggled by a click on the top-left pixel ([`club::toggle_debug`]).
    pub debug_flag: u8,
    /// Page of the UEFA list (`DS:04CA`): clubs `64 + page * 68 ..`; kept between visits.
    pub uefa_page: u8,
    /// `[BP-DA]` of the main loop: the crest of the selected club was edited, so
    /// "AENDERUNG SICHERN" writes it (set by package crest, cleared on deselect).
    pub crest_changed: bool,
    /// The main loop's input buffer `[BP-AA]` (0x52 bytes) that every panel and player
    /// input reuses. It is never cleared: `text_input` measures one stale byte past the
    /// text (see [`dialog::text_input`]).
    pub input_buf: [u8; INPUT_BUF_LEN],
    /// `[BP-04]` of the main loop: the colour the crest editor paints with (-1 = none
    /// chosen yet; it stays chosen across clubs).
    pub crest_color: i16,
    /// The crest import's browser directory and last picture header.
    pub import: crest_import::ImportState,
    /// `random_range` of the CRT (`rand()` is never seeded: the MSC start state).
    pub rng: Rng,
}

/// Loads `MANA.DAT` from the data directory (`open("mana.dat")` in the current
/// directory of the original). A file that cannot be opened is TEAMED's "DISK" error
/// (`1000:1791`): it is recorded in [`Host::fatal`] for [`run`].
pub fn load_mana(host: &mut Host) -> Result<ManaDat, String> {
    let path = host.data.join("MANA.DAT");
    let bytes = std::fs::read(&path).map_err(|e| {
        let detail = format!("{}: {e}", path.display());
        host.fatal_error(crate::game::fatal::DISK_ERROR, &detail);
        detail
    })?;
    ManaDat::parse(&bytes).map_err(|e| e.to_string())
}

/// Message of `fatal_error_exit` for a file that cannot be opened (`DS:0293` from
/// `picture_load`, `DS:02D2` for `mana.dat`).
pub const FATAL_DISK: &[u8] = b"DISK";
/// Second line of the error screen (`DS:0270`).
pub const FATAL_PRESS_KEY: &[u8] = b"<TASTE>";

/// TEAMED's error screen: unless the message is "OK." (the normal end, `main`), the
/// screen is cleared and shows the message (font 1, centred at y 0x78, colour 0x11, no
/// shadow) and, 10 ticks later, "<TASTE>" (font 3, y 0x8C); then it waits for a left
/// click. The caller ends the program (the original frees the pictures and buffers,
/// closes mouse and screen and exits to DOS).
///
/// The other messages ("SPEICHER", "FONT", "GRAFIK-ERROR": allocation, font and screen
/// failures) cannot occur in the port.
///
/// Origin: TEAMED.EXE 1000:0ee4 (fatal_error_exit)
pub fn fatal_error_exit(host: &mut Host, msg: &[u8]) -> Run<()> {
    screen_set_visible(&mut host.g, true);
    // The mouse library's screen rastport DS:1E44 (no origin).
    let bm = host
        .g
        .alloc(Bitmap::vram(gfx::SCREEN_W, TEAMED_SCREEN_H, 0));
    let mut rp = RastPort::new(bm);
    host.g.rastport_clear(&rp, 0);
    Gfx::rastport_set_draw_mode(&mut rp, 0);
    set_font(&mut rp, 1);
    gfx::ui_draw_text_centered_shadow(&mut host.g, &mut rp, msg, 0, 0x140, 0x78, 0x11, 0xff);
    screen_set_visible(&mut host.g, true);
    host.delay_ticks(10)?;
    set_font(&mut rp, 3);
    gfx::ui_draw_text_centered_shadow(
        &mut host.g,
        &mut rp,
        FATAL_PRESS_KEY,
        0,
        0x140,
        0x8c,
        0x11,
        0xff,
    );
    // input_wait_click (1000:088e): polls until a left-button event, without first
    // waiting for the button to be released.
    while host.input_poll_mouse()? != LEFT_DOWN {}
    Ok(())
}

/// `format_long_thousands` with TEAMED's globals: no separators (`DS:0123 = 1`), minimum
/// width `DS:0122` (1, which `picture_load` sets again on every call).
///
/// Origin: TEAMED.EXE 1000:072e (format_long_thousands)
pub fn format_number(v: i32) -> Vec<u8> {
    format_long_thousands(
        v,
        NumberFormat {
            no_separators: true,
            pad_width: 1,
            ..Default::default()
        },
    )
}

/// Screen on or off after waiting for the vertical retrace (`1579:0002(screen, on)`).
///
/// Origin: TEAMED.EXE 1579:0002 (screen_set_visible)
pub fn screen_set_visible(g: &mut Gfx, on: bool) {
    g.vga_wait_vretrace();
    if on {
        g.video_screen_on();
    } else {
        g.video_screen_off();
    }
}

/// Runs the editor until "PROGRAMM-ENDE". `Ok(Err(Quit))` when the window is closed
/// or the script ends.
///
/// Origin: TEAMED.EXE 1000:111a (main)
pub fn run(host: &mut Host) -> Result<Run<Exit>, String> {
    // MOD2026: the league structure of the mod data, selected before the editor starts.
    league_layout::set_layout(if host.mod2026 { &MOD2026 } else { &ORIGINAL });
    let mut ed = match init(host) {
        Ok(ed) => ed,
        // A picture or MANA.DAT that cannot be opened: fatal_error_exit("DISK").
        Err(_) if host.fatal.take().is_some() => {
            return Ok(fatal_error_exit(host, FATAL_DISK).map(|()| Exit::Fatal));
        }
        Err(e) => return Err(e),
    };
    Ok(main_loop(host, &mut ed))
    // main then calls fatal_error_exit("OK.") (1000:0ee4): text mode, message, exit.
}

/// Start-up: screen, mouse, palette, pictures, `MANA.DAT`, the first crest page.
///
/// Origin: TEAMED.EXE 1000:1439 (teamed_init_and_load_mana)
pub fn init(host: &mut Host) -> Result<Editor, String> {
    // install_int_handler(0, 0) and the EMS check (1000:0d8d) have no effect here.
    // screen_open(DS:20D8 origin (0, 0)): the 320x329 screen descriptor, mode X with
    // TEAMED's CRTC table.
    let g = &mut host.g;
    let screen_bm = g.alloc(Bitmap::vram(gfx::SCREEN_W, TEAMED_SCREEN_H, 0));
    g.vga_set_mode_x_teamed();
    g.mouse_open(true, true);
    let mut screen = RastPort::new(screen_bm);
    screen.fg = 0;
    screen.mode = 0;
    // The crest view: a second descriptor of the screen memory with origin (0, 0x4B).
    let mut view = Bitmap::vram(gfx::SCREEN_W, TEAMED_SCREEN_H, 0);
    view.origin = (0, GRID_ORIGIN_Y);
    let view = g.alloc(view);
    let grid = RastPort::new(view);
    screen_set_visible(g, false);
    g.palette_load_rgb4(&PALETTE, 0x20);
    g.palette_set_entry(0x20, 0, 0, 0);
    let bar = host.load_picture(PIC_BAR)?;
    let crest_import_pic = host.load_picture(PIC_CREST_IMPORT)?;
    let crest_editor_pic = host.load_picture(PIC_CREST_EDITOR)?;
    let background = host.load_picture(PIC_BACKGROUND)?;
    host.g
        .blit_bitmap(background, 0, 0, screen.bitmap, 0, 0, 0x140, 0x56);
    host.g.free(background);
    screen_set_visible(&mut host.g, false);
    let mana = load_mana(host)?;
    let mut ed = Editor {
        copy: mana.clone(),
        mana,
        screen,
        grid,
        bar,
        crest_import_pic,
        crest_editor_pic,
        crests: Vec::with_capacity(mana::GERMAN_CLUB_COUNT),
        scratch: [BitmapId(0); 2],
        hovered: -1,
        selected: 0xff,
        page: 0,
        debug_flag: 1,
        crest_changed: false,
        input_buf: [0; INPUT_BUF_LEN],
        uefa_page: 0,
        crest_color: -1,
        import: crest_import::ImportState::default(),
        rng: Rng::default(),
    };
    clear_grid(&mut host.g, &mut ed.grid);
    for i in 0..mana::GERMAN_CLUB_COUNT {
        let pic = host.load_picture(u16::from(ed.mana.crest[i]))?;
        ed.crests.push(pic);
    }
    for i in 0..i16::from(CRESTS_PER_PAGE) {
        let (x, y) = crest_cell(i);
        host.g.blit_bitmap(
            ed.crests[i as usize],
            0,
            0,
            ed.grid.bitmap,
            x,
            y,
            CREST_SIZE,
            CREST_SIZE,
        );
    }
    // far_memcpy of the buffers into their copies (1000:1921..19cd). DS:4742 -> DS:371E
    // (0xF56 bytes, BSS) is copied here and back by "AENDERUNG SICHERN" but never read
    // or written otherwise: a dead buffer (docs/re/teamed/club.md), not modelled.
    ed.copy = ed.mana.clone();
    host.g
        .blit_bitmap(ed.bar, 0, 0, ed.grid.bitmap, 0x138, 0xc, 8, 0x9b);
    host.g.mouse_pointer_show();
    screen_set_visible(&mut host.g, true);
    Ok(ed)
}

/// Grid position of crest `i` of a page (crest view coordinates).
pub fn crest_cell(i: i16) -> (i16, i16) {
    ((i % 8) * CREST_PITCH, (i / 8) * CREST_PITCH + 0xc)
}

/// Clears the crest area to colour 0 (`(0, 0xC)-(0x13F, 0xFE)` of the view), then
/// leaves the pen at 0xFE.
///
/// Origin: TEAMED.EXE 1000:0000 (grid_clear)
pub fn clear_grid(g: &mut Gfx, grid: &mut RastPort) {
    Gfx::rastport_set_color(grid, 0);
    g.rastport_fill_rect(grid, 0, 0xc, 0x13f, 0xfe);
    Gfx::rastport_set_color(grid, 0xfe);
}

/// Redraws the crest page `page` (clubs `page * 32 .. + 32`) and the bar.
///
/// Origin: TEAMED.EXE 1000:2924 (grid_draw_page)
pub fn draw_page(g: &mut Gfx, ed: &mut Editor, page: u8) {
    clear_grid(g, &mut ed.grid);
    g.blit_bitmap(ed.bar, 0, 0, ed.grid.bitmap, 0x138, 0xc, 8, 0x9b);
    let first = page.wrapping_shl(5);
    let mut i = first;
    while u16::from(first) + 0x20 > u16::from(i) {
        let (x, y) = crest_cell(i16::from(i - first));
        g.blit_bitmap(
            ed.crests[usize::from(i)],
            0,
            0,
            ed.grid.bitmap,
            x,
            y,
            CREST_SIZE,
            CREST_SIZE,
        );
        i = i.wrapping_add(1);
    }
}

/// Information panel for the club under the pointer: name, league, rank, points and
/// goals of the previous season from the `MANA.DAT` buffers.
///
/// Origin: TEAMED.EXE 1000:24be (panel_draw_hovered)
pub fn draw_hover_panel(g: &mut Gfx, ed: &mut Editor, club: u8) {
    draw_panel(g, &mut ed.screen, &ed.mana, club);
}

/// The panel of [`draw_hover_panel`] drawn from the given buffers: `1000:24be` reads the
/// `MANA.DAT` buffers, `panel_draw_selected` (`1000:26f1`, the same code) their copies.
///
/// Origin: TEAMED.EXE 1000:24be (panel_draw_hovered), 1000:26f1 (panel_draw_selected)
pub fn draw_panel(g: &mut Gfx, rp: &mut RastPort, m: &ManaDat, club: u8) {
    gfx::ui_fill_rect(g, rp, 0x33, 3, 0x92, 0x27, 0xc);
    set_font(rp, 3);
    let c = usize::from(club);
    let name = mana::c_str(&m.clubs[c].name).to_vec();
    gfx::ui_draw_text_at(g, rp, &name, 0x35, 7, 0x1d);
    gfx::ui_draw_text_at(g, rp, &league_name(league_group(club)), 0x35, 0xf, 0x1d);
    let rank = format_number(i32::from(m.rank[c]));
    gfx::ui_draw_text_at(g, rp, &rank, 0x35, 0x17, 0x1d);
    let pair = |v: [u8; 2]| {
        let mut s = format_number(i32::from(v[0]));
        s.push(b':');
        s.extend(format_number(i32::from(v[1])));
        s
    };
    gfx::ui_draw_text_at(g, rp, &pair(m.points[c]), 0x35, 0x1f, 0x1d);
    gfx::ui_draw_text_at(g, rp, &pair(m.goals[c]), 0x35, 0x27, 0x1d);
}

/// The player list of the club under the pointer: slots 0..9 at the left, 10..19 at
/// the right, each on a cleared row.
///
/// Origin: TEAMED.EXE 12a5:0fd3 (teamed_main_loop, hover player list)
pub fn draw_hover_players(g: &mut Gfx, ed: &mut Editor, club: u8) {
    let rp = &mut ed.screen;
    let players = &ed.mana.clubs[usize::from(club)].players;
    for i in 0..10i16 {
        let y0 = (i + 2) * 7;
        let y1 = i * 7 + 0x13;
        gfx::ui_fill_rect(g, rp, 0xa3, y0, 0xea, y1, 0xd);
        gfx::ui_fill_rect(g, rp, 0xf6, y0, 0x13d, y1, 0xd);
        let left = mana::c_str(&players[i as usize]).to_vec();
        gfx::ui_draw_text_at(g, rp, &left, 0xa3, i * 7 + 0x12, 0xb);
        let right = mana::c_str(&players[i as usize + 10]).to_vec();
        gfx::ui_draw_text_at(g, rp, &right, 0xf6, i * 7 + 0x12, 0xb);
    }
}

/// Upper-cases the umlauts of every player name the way the start of the main loop
/// does: `|` -> `\`, `{` -> `[`, `}` -> `]` (the German ISO 646 umlaut codes).
///
/// Origin: TEAMED.EXE 12a5:02ca (teamed_main_loop, name fix-up)
pub fn fix_player_names(m: &mut ManaDat) {
    for club in m.clubs.iter_mut().take(mana::GERMAN_CLUB_COUNT) {
        for name in club.players.iter_mut() {
            let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            for b in &mut name[..len] {
                *b = match *b {
                    b'|' => b'\\',
                    b'{' => b'[',
                    b'}' => b']',
                    other => other,
                };
            }
        }
    }
}

/// The editor's event loop: hover display, paging, the buttons and the save question.
///
/// Origin: TEAMED.EXE 12a5:02bd (teamed_main_loop)
pub fn main_loop(host: &mut Host, ed: &mut Editor) -> Run<Exit> {
    fix_player_names(&mut ed.mana);
    ed.scratch = [
        host.blank_bitmap(CREST_SIZE, CREST_SIZE, 0),
        host.blank_bitmap(CREST_SIZE, CREST_SIZE, 0),
    ];
    set_font(&mut ed.screen, 3);
    set_font(&mut ed.grid, 2);
    Gfx::rastport_set_draw_mode(&mut ed.screen, 0);
    Gfx::rastport_set_draw_mode(&mut ed.grid, 0);
    screen_set_visible(&mut host.g, true);
    let mut quit = false;
    loop {
        let ev = host.input_poll_mouse()?;
        let (x, y) = host.mouse_xy();
        let y_rel = y.wrapping_sub(GRID_ORIGIN_Y);
        if x == 0 && y == 0 && ev == LEFT_DOWN {
            club::toggle_debug(&mut host.g, ed);
        }
        // 12a5:0521..056a: the scroll-view switch (12a5:2d39), an empty function.
        // The hover and bar checks read the pointer copies [BP-E2]/[BP-EA], which the
        // league-swap prompt of the panel overwrites with its last pointer position.
        let (mut hx, mut hy) = (x, y_rel);
        if let Some(pos) = club::edit_panel(host, ed, x, y, ev)? {
            (hx, hy) = pos;
        }
        let in_grid = hx < 0x137 && hy > 0xb && hy < 0xa8;
        if in_grid {
            let club = hx / CREST_PITCH + ((hy - 0xc) / CREST_PITCH) * 8 + i16::from(ed.page) * 32;
            // With a club selected the hover is ignored (the grid shows the crest editor).
            if club != ed.hovered && ed.selected == 0xff {
                ed.hovered = club;
                draw_hover_panel(&mut host.g, ed, club as u8);
                draw_hover_players(&mut host.g, ed, club as u8);
                // Straight back to the end of the loop (12a5:22ae).
                if quit && let Some(exit) = end_program(host, ed, &mut quit)? {
                    return Ok(exit);
                }
                continue;
            }
        } else if ed.hovered != -1 && ed.selected == 0xff {
            gfx::ui_fill_rect(&mut host.g, &mut ed.screen, 0x33, 3, 0x95, 0x29, 0xc);
            ed.hovered = -1;
        }
        if ev == LEFT_DOWN && hy > 0xb && hx > 0x136 && ed.selected == 0xff {
            if hy > 0x59 {
                // "UEFA": the strength list of the foreign clubs.
                uefa::run(host, ed)?;
            } else {
                // "LIGA": the other crest page.
                ed.page ^= 1;
            }
            screen_set_visible(&mut host.g, false);
            let page = ed.page;
            draw_page(&mut host.g, ed, page);
            screen_set_visible(&mut host.g, true);
        }
        let in_buttons_x = x > 0x2a && x < 0x70;
        if ev == LEFT_DOWN && ed.hovered != -1 && ed.selected == 0xff {
            // 12a5:11b7: a click on the crest under the pointer selects that club.
            club::select(host, ed)?;
        } else if ev == LEFT_DOWN && in_buttons_x && y > 0x46 && y < 0x55 && ed.selected == 0xff {
            // "PROGRAMM-ENDE"
            quit = true;
        } else if ev == LEFT_DOWN && in_buttons_x && y > 0x2d && y < 0x55 && ed.selected != 0xff {
            // 12a5:1361: "NICHT UEBERNEHMEN" / "AENDERUNG SICHERN" (the third row only
            // deselects). They end the selection, so the crest editor and player checks
            // that follow in the original (with y replaced by the row) never match.
            club::buttons(host, ed, y)?;
        } else if !crest::click(host, ed, ev, hx, hy)? && ev == LEFT_DOWN {
            // 12a5:1671: the crest editor (both buttons; it reads the pointer copies
            // [BP-E2]/[BP-EA]); unless it ended the iteration, 12a5:2028: editing a
            // player name of the selected club in the list.
            club::edit_player(host, ed, x, y)?;
        }
        if quit && let Some(exit) = end_program(host, ed, &mut quit)? {
            return Ok(exit);
        }
    }
}

/// "PROGRAMM-ENDE": the save question; a click outside it (9) keeps the editor running.
///
/// Origin: TEAMED.EXE 12a5:22ae (teamed_main_loop, end of the loop)
fn end_program(host: &mut Host, ed: &mut Editor, quit: &mut bool) -> Run<Option<Exit>> {
    if save::teamed_save_mana(host, ed)? == save::SaveResult::Cancelled {
        let page = ed.page;
        draw_page(&mut host.g, ed, page);
        *quit = false;
        return Ok(None);
    }
    for id in ed.scratch {
        host.g.free(id);
    }
    for &id in &ed.crests {
        host.g.free(id);
    }
    Ok(Some(Exit::Ended))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn league_groups() {
        assert_eq!(league_group(0), 0);
        assert_eq!(league_group(17), 0);
        assert_eq!(league_group(18), 1);
        assert_eq!(league_group(37), 1);
        assert_eq!(league_group(38), 2);
        assert_eq!(league_group(57), 2);
        assert_eq!(league_group(58), 3);
        assert_eq!(league_group(63), 3);
    }

    /// MOD2026: the groups and the third group's name follow the selected league layout.
    #[test]
    fn mod_league_groups_follow_the_league_layout() {
        let ids = [17, 18, 35, 36, 55, 56, 57];
        assert_eq!(ids.map(league_group), [0, 1, 1, 1, 2, 2, 2]);
        assert_eq!(*league_name(2), *b"REGIONALLIGA    ");
        league_layout::set_layout(&MOD2026);
        assert_eq!(ids.map(league_group), [0, 1, 1, 2, 2, 3, 3]);
        assert_eq!(*league_name(2), *b"3. LIGA");
        assert_eq!(*league_name(3), *b"OBERLIGA    ");
        league_layout::set_layout(&ORIGINAL);
    }

    /// MOD2026: `run` selects the league layout from the host's data. Needs `BMP_DATA`.
    #[test]
    fn run_selects_the_league_layout_of_the_data() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let shots = std::env::temp_dir().join(format!("bmp-teamed-layout-{}", std::process::id()));
        let mut host = Host::with_script(&data, Vec::new(), &shots, false).unwrap();
        host.mod2026 = true;
        let _ = run(&mut host).unwrap();
        assert_eq!(*league_layout::layout(), MOD2026);
        host.mod2026 = false;
        let _ = run(&mut host).unwrap();
        assert_eq!(*league_layout::layout(), ORIGINAL);
        let _ = std::fs::remove_dir_all(&shots);
    }

    #[test]
    fn crest_cells() {
        assert_eq!(crest_cell(0), (0, 12));
        assert_eq!(crest_cell(7), (7 * 39, 12));
        assert_eq!(crest_cell(8), (0, 51));
        assert_eq!(crest_cell(31), (7 * 39, 12 + 3 * 39));
    }

    #[test]
    fn numbers_have_no_separators() {
        assert_eq!(format_number(0), b"0");
        assert_eq!(format_number(18), b"18");
        assert_eq!(format_number(1234), b"1234");
    }

    #[test]
    fn player_names_get_upper_case_umlauts() {
        let mut bytes = vec![0u8; mana::FILE_SIZE];
        let at = mana::CLUBS_OFFSET + mana::CLUB_NAME_LEN;
        bytes[at..at + 6].copy_from_slice(b"M|{}LL");
        // A stale byte after the NUL stays untouched.
        bytes[at + 7] = b'|';
        let mut m = ManaDat::parse(&bytes).unwrap();
        fix_player_names(&mut m);
        assert_eq!(&m.clubs[0].players[0][..8], b"M\\[]LL\0|");
    }
}
