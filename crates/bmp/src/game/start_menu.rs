//! Start menu: club carousel, four manager slots with portraits and names, then the
//! new-game options dialog. Spec: `docs/re/logic/flow.md` §4.

use bmp_game::newgame::{
    GameOptions, MODE_ENDLESS, MODE_HISTORICAL, MODE_LENGTHS, MODE_ONE_YEAR, MODE_THREE_YEARS,
    ManagerChoice, MenuChoices,
};
use bmp_game::rng::format_long_thousands;
use bmp_game::state::{Club, GameState, NO_TEAM};

use super::disk::copy_protection;
use super::{Session, load_game, office_show_cached_background};
use crate::gfx::{
    BitmapId, Gfx, RastPort, ui_draw_text_at, ui_draw_text_centered_shadow, ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, LEFT_UP, RIGHT_DOWN, Run};

/// Pictures `main` loads before the start menu.
pub struct Pictures {
    /// `PIC\0.VGA`, manager portrait sheet (`FAR:2f24`).
    pub portraits: BitmapId,
    /// Blank 164x38 crest cache of the four managers (`DS:05b0`).
    pub crest_cache: BitmapId,
}

impl Pictures {
    pub fn load(host: &mut Host) -> Result<Self, String> {
        let crest_cache = host.blank_bitmap(0xa4, 0x26, 0);
        let portraits = host.load_picture(0)?;
        Ok(Self {
            portraits,
            crest_cache,
        })
    }
}

/// Result of the start menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NewGame(MenuChoices),
    /// The header bar was clicked and a saved game was loaded.
    LoadGame(Box<GameState>),
}

struct Screen<'a> {
    host: &'a mut Host,
    pics: &'a Pictures,
    clubs: &'a [Club],
    managers: [ManagerChoice; 4],
    /// `DS:07ab`.
    count: u8,
    hdr: BitmapId,
    strip: BitmapId,
    frame: BitmapId,
    slot_frame: BitmapId,
}

fn rp_for(bm: BitmapId) -> RastPort {
    RastPort::new(bm)
}

impl Screen<'_> {
    fn g(&mut self) -> &mut Gfx {
        &mut self.host.g
    }

    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        src: BitmapId,
        sx: i16,
        sy: i16,
        dst: BitmapId,
        dx: i16,
        dy: i16,
        w: i16,
        h: i16,
    ) {
        self.host.g.blit_bitmap(src, sx, sy, dst, dx, dy, w, h);
    }

    fn fill(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, c: u8) {
        ui_fill_rect(&mut self.host.g, &mut self.host.rp, x0, y0, x1, y1, c);
    }

    fn centered(&mut self, text: &[u8], x0: i16, x1: i16, y: i16, c: u8, shadow: u8) {
        ui_draw_text_centered_shadow(
            &mut self.host.g,
            &mut self.host.rp,
            text,
            x0,
            x1,
            y,
            c,
            shadow,
        );
    }

    /// Draws crest `code` (`PIC\<code + 0x50>.VGA`) into `dst` at `(x, y)` and clears
    /// the two columns right of it. The "blank crest" list `DS:05dc` is inactive
    /// (`DS:05da == 0`).
    ///
    /// Origin: BMMAIN.EXE 18bc:1cdd (draw_club_crest)
    fn draw_club_crest(&mut self, code: u8, dst: BitmapId, x: i16, y: i16) -> Run<()> {
        let spr = self.host.picture(u16::from(code) + 0x50)?;
        let mut rp = rp_for(dst);
        ui_fill_rect(
            &mut self.host.g,
            &mut rp,
            x + 0x26,
            y,
            x + 0x27,
            y + 0x25,
            0,
        );
        self.blit(spr, 0, 0, dst, x, y, 0x26, 0x26);
        self.g().free(spr);
        Ok(())
    }

    /// §4.2, label 18bc:22ca.
    fn full_redraw(&mut self, n0: u8, session: &mut Session) -> Run<()> {
        office_show_cached_background(self.host)?;
        let screen = self.host.screen();
        self.blit(self.frame, 0, 0, screen, 5, 0x28, 0x135, 0x41);
        self.blit(self.hdr, 0, 0, screen, 0, -3, 0x140, 0x2c);
        // copy_protection_penalty: one RNG call after loading a save whose protection
        // flag is clear.
        copy_protection(self.host, session)?;
        for i in 0..4i16 {
            self.blit(
                self.slot_frame,
                0,
                0,
                screen,
                i * 0x50 + 0x0c,
                0x69,
                0x3a,
                0x89,
            );
            self.blit(
                self.pics.portraits,
                i * 0x29 + 1,
                0,
                screen,
                i * 0x50 + 0x15,
                0xaa,
                0x27,
                0x28,
            );
            if i + 1 > i16::from(n0) {
                self.managers[i as usize].team = NO_TEAM;
            } else {
                self.blit(
                    self.pics.crest_cache,
                    i * 0x29,
                    0,
                    screen,
                    i * 0x50 + 0x16,
                    0x72,
                    0x26,
                    0x26,
                );
                let name = self.managers[i as usize].name.clone();
                ui_draw_text_at(
                    &mut self.host.g,
                    &mut self.host.rp,
                    &name,
                    i * 0x50 + 0x14,
                    0xe6,
                    0x13,
                );
            }
        }
        self.blit(screen, 0x11, 0x30, self.strip, 0, 0, 0x11f, 0x26);
        for i in 0..7usize {
            let code = self.clubs[i].crest & 0x7f;
            self.draw_club_crest(code, self.strip, i as i16 * 0x29 + 1, 0)?;
        }
        self.blit(self.strip, 0, 0, screen, 0x11, 0x30, 0x11f, 0x26);
        self.host.set_font(1);
        Gfx::rastport_set_draw_mode(&mut self.host.rp, 0);
        let clubs = self.clubs;
        self.centered(clubs[3].name(), 0x55, 0xeb, 0x62, 0, 0xff);
        self.host.screen_fade(1)
    }

    /// §4.4: scroll while the left button is held.
    fn scroll(&mut self, dir: i8, pos: &mut i16, center: &mut i16) -> Run<()> {
        let screen = self.host.screen();
        let (sx, dx) = if dir < 0 { (0x0d, 0x0a) } else { (0x13, 0x131) };
        self.host
            .g
            .blit_bitmap_transparent(self.frame, sx, 0x41, screen, dx, 0x32, 6, 0x22);
        self.host.delay_ticks(8)?;
        loop {
            if dir < 0 {
                *pos -= 1;
                if *pos < 0 {
                    *pos = 0x3f;
                }
            } else {
                *pos += 1;
                if *pos > 0x3f {
                    *pos = 0;
                }
            }
            *center = *pos + 3;
            if *center > 0x3f {
                *center -= 0x40;
            }
            if dir < 0 {
                for k in (1..=6i16).rev() {
                    self.blit(
                        self.strip,
                        k * 0x29 - 0x28,
                        0,
                        self.strip,
                        k * 0x29 + 1,
                        0,
                        0x28,
                        0x26,
                    );
                }
                let code = self.clubs[*pos as usize].crest & 0x7f;
                self.draw_club_crest(code, self.strip, 1, 0)?;
            } else {
                for k in 0..6i16 {
                    self.blit(
                        self.strip,
                        k * 0x29 + 0x2a,
                        0,
                        self.strip,
                        k * 0x29 + 1,
                        0,
                        0x28,
                        0x26,
                    );
                }
                let mut n = *pos + 6;
                if n > 0x3f {
                    n -= 0x40;
                }
                let code = self.clubs[n as usize].crest & 0x7f;
                self.draw_club_crest(code, self.strip, 0xf7, 0)?;
            }
            self.blit(self.strip, 0, 0, screen, 0x11, 0x30, 0x11f, 0x26);
            self.fill(0x55, 0x59, 0xeb, 0x63, 5);
            let clubs = self.clubs;
            self.centered(clubs[*center as usize].name(), 0x55, 0xeb, 0x62, 0, 0xff);
            self.host.input_poll_mouse()?;
            self.host.wait_5_ticks()?;
            if self.host.button == LEFT_UP {
                break;
            }
        }
        let sx = if dir < 0 { 0 } else { 6 };
        self.host
            .g
            .blit_bitmap_transparent(self.frame, sx, 0x41, screen, dx, 0x32, 6, 0x22);
        Ok(())
    }

    /// Origin: BMMAIN.EXE 18bc:1e96 (start_menu_swap_portraits)
    fn swap_portraits(&mut self, a: i16, b: i16) {
        let screen = self.host.screen();
        let p = self.pics.portraits;
        self.blit(
            p,
            a * 0x29 + 1,
            0,
            screen,
            b * 0x50 + 0x15,
            0xaa,
            0x27,
            0x28,
        );
        self.blit(
            p,
            b * 0x29 + 1,
            0,
            screen,
            a * 0x50 + 0x15,
            0xaa,
            0x27,
            0x28,
        );
        self.blit(
            screen,
            b * 0x50 + 0x15,
            0xaa,
            p,
            b * 0x29 + 1,
            0,
            0x27,
            0x28,
        );
        self.blit(
            screen,
            a * 0x50 + 0x15,
            0xaa,
            p,
            a * 0x29 + 1,
            0,
            0x27,
            0x28,
        );
    }

    /// §4.5: clicks on the manager slots `n0..4`.
    fn slot_clicks(
        &mut self,
        n0: u8,
        mx: i16,
        my: i16,
        center: i16,
        pending_swap: &mut i16,
    ) -> Run<()> {
        let screen = self.host.screen();
        for m in n0..4 {
            let mi = m as usize;
            let xl = i16::from(m) * 0x50 + 0x16;
            // (a) crest box: assign the centre club.
            if self.count > m && xl < mx && my > 0x72 && mx < xl + 0x29 && my < 0x99 {
                let used = self.managers[..self.count as usize]
                    .iter()
                    .any(|mg| i16::from(mg.team) == center);
                if !used {
                    self.managers[mi].team = center as u8;
                    self.blit(screen, 0x8d, 0x30, screen, xl, 0x72, 0x26, 0x26);
                }
            }
            // (b) portrait: two clicks swap two portraits.
            if xl < mx && my > 0x97 && my < 0xd2 && mx < xl + 0x29 {
                if *pending_swap != 0 {
                    *pending_swap -= 1;
                    let other = *pending_swap;
                    self.swap_portraits(other, i16::from(m));
                    let (a, b) = (self.managers[other as usize].slot, self.managers[mi].slot);
                    self.managers[other as usize].slot = b;
                    self.managers[mi].slot = a;
                    *pending_swap = 0;
                } else {
                    *pending_swap = i16::from(m) + 1;
                }
            }
            // (c) name field.
            if (self.count >= m || !self.managers[mi].name.is_empty())
                && xl < mx
                && my > 0xd2
                && mx < xl + 0x29
            {
                self.fill(xl - 3, 0xe1, xl + 0x27, 0xe7, 0);
                self.host.set_font(2);
                let name = text_input(self.host, xl - 2, 0xe6, 0x28, 0x13, 0)?;
                self.managers[mi].name = name;
                if self.managers[mi].name.is_empty() {
                    for k in i16::from(m)..4 {
                        // NB: index m, not k (original bug).
                        self.managers[mi].team = NO_TEAM;
                        self.fill(k * 0x50 + 0x16, 0x72, k * 0x50 + 0x3d, 0x97, 0);
                        self.fill(k * 0x50 + 0x13, 0xe1, k * 0x50 + 0x3b, 0xe7, 0);
                    }
                    self.count = m;
                } else if self.count < m + 1 {
                    self.count = m + 1;
                }
            }
        }
        Ok(())
    }
}

/// Single-line text input as used for the manager names and the two fields of the
/// file selector: letters are upper-cased, Enter accepts, Esc clears, Backspace
/// deletes; characters are accepted while the text stays within `max_width` pixels
/// (there is no character limit: the callers' buffers hold more than the pixel
/// limit lets through, e.g. 58 narrow glyphs in the 0xaf-pixel directory field).
///
/// DEVIATION: Backspace on an empty text falls through to the "add a character"
/// path in the original (176b:0911 jumps to 176b:0a0e) and appends the control
/// character 8 to the text; the port ignores it.
///
/// TODO(re): the cursor and erase rectangles are read from a damaged decompile of
/// 176b:07d7 and may be off by a pixel. The numeric mode is
/// [`super::widgets::ui_input_number`].
///
/// Origin: BMMAIN.EXE 176b:07d7 (ui_scroll_text_box, text mode)
pub fn text_input(
    host: &mut Host,
    mut x: i16,
    y: i16,
    max_width: i16,
    color: u8,
    bg: u8,
) -> Run<Vec<u8>> {
    let color = if color == 0 { 0xff } else { color };
    let mut text: Vec<u8> = Vec::new();
    let height = host.rp.font.map_or(8, |f| {
        host.g.fonts[f as usize]
            .as_ref()
            .map_or(8, |f| f.height as i16)
    });
    loop {
        ui_fill_rect(&mut host.g, &mut host.rp, x + 2, y - 4, x + 4, y, color);
        let key = host.getch()?;
        ui_fill_rect(&mut host.g, &mut host.rp, x + 2, y - 4, x + 4, y, bg);
        match key {
            b'\r' => return Ok(text),
            0x1b => return Ok(Vec::new()),
            8 => {
                if let Some(c) = text.pop() {
                    let w = host.g.rastport_text_width(&host.rp, &[c]);
                    x -= w;
                    ui_fill_rect(
                        &mut host.g,
                        &mut host.rp,
                        x,
                        y - height + 1,
                        x + w,
                        y + 1,
                        bg,
                    );
                }
            }
            _ => {
                let up = key & 0xdf;
                let c = if up.is_ascii_uppercase() { up } else { key };
                let w = host.g.rastport_text_width(&host.rp, &[c]);
                if w + host.g.rastport_text_width(&host.rp, &text) <= max_width {
                    Gfx::rastport_move_to(&mut host.rp, x, y);
                    Gfx::rastport_set_color(&mut host.rp, color);
                    host.g.rastport_draw_text(&host.rp, &[c]);
                    text.push(c);
                    x += w;
                }
            }
        }
    }
}

/// Origin: BMMAIN.EXE 18bc:2061 (start_menu_draw_level_box)
fn draw_level_box(host: &mut Host, k: i16, selected: bool) {
    let y = (k & 0xfe) * 9;
    let x = (k % 2) * 0x1a;
    let (c1, c2) = if selected { (0, 1) } else { (1, 3) };
    let label = format_long_thousands(i32::from(k), host.number_format());
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_fill_rect(g, rp, x + 0xb7, y + 0x4d, x + 0xca, y + 0x59, c1);
    ui_fill_rect(g, rp, x + 0xb8, y + 0x4e, x + 0xca, y + 0x59, c2);
    ui_fill_rect(g, rp, x + 0xb8, y + 0x4e, x + 0xc9, y + 0x58, 6);
    ui_draw_text_centered_shadow(
        g,
        rp,
        &label,
        x + 0xb8,
        x + 0xce,
        y + 0x56,
        u8::from(selected) * 0xf + 4,
        0,
    );
}

/// Draws the on/off toggle pair of `PIC\33.VGA`.
///
/// Origin: BMMAIN.EXE 362f:0004 (FUN_362f_0004)
fn draw_toggle(host: &mut Host, pic: BitmapId, x: i16, y: i16, on: bool) {
    let on = i16::from(on);
    let screen = host.screen();
    host.g
        .blit_bitmap(pic, 0x64 + 0x30 * on, 0, screen, x, y, 0x18, 0x11);
    host.g
        .blit_bitmap(pic, 0xac - 0x30 * on, 0, screen, x + 0x1a, y, 0x18, 0x11);
}

const LEVEL_LABELS: [&[u8]; 3] = [b"Spiel-Level", b"", b""];
/// The mode rows of the dialog, top to bottom, with the bit each row sets.
const MODE_ROWS: [(&[u8], u8); 4] = [
    (b"Endlosspiel", MODE_ENDLESS),
    (b"1-Jahres-Spiel", MODE_ONE_YEAR),
    (b"3-Jahres-Spiel", MODE_THREE_YEARS),
    (b"Histor. Start", MODE_HISTORICAL),
];

/// §4.7: level and game-mode dialog of a new game. Returns `(level, flags)`.
fn options_dialog(host: &mut Host) -> Result<Run<(u8, u8)>, String> {
    let dlg = host.load_picture(0x21)?;
    // DS:2252/2254 == DS:224e/2250 (both 1 in the shipped data): four mode rows.
    let four: i16 = 4;
    let mut level: i16 = 2; // DS:4a28 initial value
    let mut flags = MODE_ENDLESS;
    let h = 0x8e - 0x14 * i16::from(four == 3);
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x4e, 0x45, 0xa4, h, true, 8);
    for i in 0..3i16 {
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            LEVEL_LABELS[i as usize],
            0x4f,
            0xb9,
            i * 0x12 + 0x57,
            1,
            0,
        );
        draw_toggle(host, dlg, 0xb5, i * 0x12 + 0x4b, false);
        draw_level_box(host, 2 * i, i == 1);
        draw_level_box(host, 2 * i + 1, false);
    }
    for i in 0..four {
        ui_draw_text_centered_shadow(
            &mut host.g,
            &mut host.rp,
            MODE_ROWS[i as usize].0,
            0x4f,
            0xb9,
            i * 0x12 + 0x92,
            1,
            0,
        );
        draw_toggle(
            host,
            dlg,
            0xb5,
            i * 0x12 + 0x86,
            flags & MODE_ROWS[i as usize].1 != 0,
        );
    }
    loop {
        let ev = match host.input_poll_mouse() {
            Ok(ev) => ev,
            Err(q) => return Ok(Err(q)),
        };
        let (mut mx, my) = host.mouse_xy();
        if ev == LEFT_DOWN {
            for _ in 0..4 {
                if !(mx > 0xb4 && mx < 0xe8) {
                    continue;
                }
                if my > 0x85 && my < 0xcd - 0x12 * i16::from(four == 3) {
                    let i = (my - 0x86) / 0x12;
                    let bit = MODE_ROWS[i as usize].1;
                    if bit == MODE_HISTORICAL {
                        flags &= MODE_LENGTHS;
                    } else {
                        flags &= MODE_HISTORICAL;
                    }
                    if mx < 0xcd {
                        flags |= bit;
                    }
                    // The original reuses mx as the loop counter here.
                    mx = 0;
                    while mx < four {
                        draw_toggle(
                            host,
                            dlg,
                            0xb5,
                            mx * 0x12 + 0x86,
                            flags & MODE_ROWS[mx as usize].1 != 0,
                        );
                        mx += 1;
                    }
                }
                if my > 0x4a && my < 0x80 {
                    draw_level_box(host, level, false);
                    level = ((my - 0x4b) / 0x12) * 2 + i16::from(mx > 0xcd);
                    draw_level_box(host, level, true);
                }
            }
        }
        if ev == RIGHT_DOWN && flags & MODE_LENGTHS != 0 {
            break;
        }
    }
    if let Err(q) = host.screen_fade(0) {
        return Ok(Err(q));
    }
    host.g.free(dlg);
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
    Ok(Ok((level as u8, flags)))
}

/// The start menu with `allow_load = 1` and no existing managers (new game).
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load)
pub fn start_menu_new_or_load(
    host: &mut Host,
    pics: &Pictures,
    clubs: &[Club],
    session: &mut Session,
) -> Result<Run<Outcome>, String> {
    let empty = std::array::from_fn(|m| ManagerChoice {
        name: Vec::new(),
        team: NO_TEAM,
        slot: m as u8 + 1,
    });
    let (outcome, s) = match run_menu(host, pics, clubs, session, true, empty, 0)? {
        Ok(v) => v,
        Err(q) => return Ok(Err(q)),
    };
    s.free(host);
    let outcome = match outcome {
        MenuEnd::Loaded(state) => Outcome::LoadGame(state),
        MenuEnd::Finished {
            managers,
            count,
            pos,
        } => {
            host.set_font(1);
            let (level, mode_flags) = match options_dialog(host)? {
                Ok(v) => v,
                Err(q) => return Ok(Err(q)),
            };
            Outcome::NewGame(MenuChoices {
                managers,
                count,
                carousel_pos: pos,
                options: GameOptions { level, mode_flags },
            })
        }
    };
    Ok(Ok(outcome))
}

/// The start menu opened from the office (`allow_load = 0`) with the managers of the running
/// game in slots `0..n0`: returns the managers the player added in the free slots (possibly
/// none). The caller hands them to [`bmp_game::newgame::add_managers`].
///
/// Origin: BMMAIN.EXE 18bc:2184 (start_menu_new_or_load(0), office sub icon 5/5)
pub fn start_menu_add_managers(
    host: &mut Host,
    pics: &Pictures,
    state: &GameState,
    session: &mut Session,
) -> Result<Run<Vec<ManagerChoice>>, String> {
    let n0 = state.manager_count.min(4);
    let current = std::array::from_fn(|m| {
        let mgr = &state.managers[m];
        ManagerChoice {
            name: mgr.name().to_vec(),
            team: mgr.team,
            slot: mgr.slot,
        }
    });
    let clubs = &state.clubs[..CAROUSEL_CLUBS];
    let (outcome, s) = match run_menu(host, pics, clubs, session, false, current, n0)? {
        Ok(v) => v,
        Err(q) => return Ok(Err(q)),
    };
    s.free(host);
    let added = match outcome {
        MenuEnd::Finished {
            managers, count, ..
        } => managers[usize::from(n0)..usize::from(count.max(n0))].to_vec(),
        // `allow_load = 0`: the header bar does not load.
        MenuEnd::Loaded(_) => unreachable!("the office start menu cannot load a game"),
    };
    // §4.7b starts with screen_fade(0).
    host.set_font(1);
    if let Err(q) = host.screen_fade(0) {
        return Ok(Err(q));
    }
    Ok(Ok(added))
}

/// The clubs of the carousel: indices 0..63.
const CAROUSEL_CLUBS: usize = 0x40;

/// How [`run_menu`] ended.
enum MenuEnd {
    Loaded(Box<GameState>),
    /// Right click with every active slot holding a club (§4.6). The chosen crests are
    /// already cached.
    Finished {
        managers: [ManagerChoice; 4],
        count: u8,
        /// `[bp-4]`, the carousel position.
        pos: i16,
    },
}

/// The pictures [`run_menu`] loaded; [`MenuPictures::free`] releases them.
struct MenuPictures {
    hdr: BitmapId,
    strip: BitmapId,
    frame: BitmapId,
    slot_frame: BitmapId,
}

impl MenuPictures {
    fn free(self, host: &mut Host) {
        for bm in [self.slot_frame, self.frame, self.hdr, self.strip] {
            host.g.free(bm);
        }
    }
}

/// §4.1..4.6 up to the options dialog / adding the managers: slots `0..n0` hold the
/// existing managers (`managers`, `DS:07ab = n0`) and cannot be edited.
#[allow(clippy::type_complexity)]
fn run_menu(
    host: &mut Host,
    pics: &Pictures,
    clubs: &[Club],
    session: &mut Session,
    allow_load: bool,
    managers: [ManagerChoice; 4],
    n0: u8,
) -> Result<Run<(MenuEnd, MenuPictures)>, String> {
    host.set_font(2);
    let hdr = host.load_picture(4)?;
    let strip = host.blank_bitmap(0x135, 0x26, 0);
    let frame = host.load_picture(0x22)?;
    let slot_frame = host.load_picture(0x23)?;
    let loaded = MenuPictures {
        hdr,
        strip,
        frame,
        slot_frame,
    };
    let mut s = Screen {
        host,
        pics,
        clubs,
        managers,
        count: n0,
        hdr,
        strip,
        frame,
        slot_frame,
    };
    // `[bp-4]` and `[bp-0xa]`: the carousel position and the club under the centre crest.
    let (mut pos, mut center) = (0i16, 3i16);
    let mut result = || -> Run<Option<Box<GameState>>> {
        s.host.screen_fade(0)?;
        s.host.g.palette_load_slot(0, 0x100);
        s.full_redraw(n0, session)?;
        let mut pending_swap = 0i16;
        loop {
            let ev = s.host.input_poll_mouse()?;
            if ev == RIGHT_DOWN
                && s.count != 0
                && s.managers[..s.count as usize]
                    .iter()
                    .all(|m| m.team != NO_TEAM)
            {
                return Ok(None);
            }
            if ev != LEFT_DOWN {
                continue;
            }
            let (mx, my) = s.host.mouse_xy();
            if my < 0x24 && allow_load {
                office_show_cached_background(s.host)?;
                // FAR:304a is 0 in the start menu: the dialogs address the first slot.
                let who = s.managers[0].name.clone();
                if let Some(state) = load_game::screen_load_game(s.host, session, s.pics, 1, &who)?
                {
                    return Ok(Some(state));
                }
                s.full_redraw(n0, session)?;
                continue;
            }
            if my > 0x2e && my < 0x69 {
                s.host.set_font(1);
                if mx > 5 && mx < 0xa0 {
                    s.scroll(-1, &mut pos, &mut center)?;
                } else if mx > 0x9f && mx < 0x13a {
                    s.scroll(1, &mut pos, &mut center)?;
                }
            }
            s.slot_clicks(n0, mx, my, center, &mut pending_swap)?;
        }
    };
    let result = result();
    let end = match result {
        Err(q) => {
            loaded.free(s.host);
            return Ok(Err(q));
        }
        Ok(Some(state)) => MenuEnd::Loaded(state),
        Ok(None) => {
            // §4.6: cache the chosen crests of the new managers.
            let screen = s.host.screen();
            for i in n0..s.count {
                let i = i16::from(i);
                s.blit(
                    screen,
                    i * 0x50 + 0x16,
                    0x72,
                    s.pics.crest_cache,
                    i * 0x29,
                    0,
                    0x28,
                    0x26,
                );
            }
            MenuEnd::Finished {
                managers: s.managers.clone(),
                count: s.count,
                pos,
            }
        }
    };
    Ok(Ok((end, loaded)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::parse_script;

    /// The directory field of the file selector (0xaf pixels, font 2) takes
    /// characters up to its pixel width only: narrow glyphs fill it with more
    /// than 28 characters, and Backspace on the empty text adds nothing.
    #[test]
    fn text_input_is_limited_by_pixels_only() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let typed = "1".repeat(80);
        let script = parse_script(&format!(
            "wait 100\nkey backspace\ntype {typed}\nkey enter\nwait 1000\n"
        ))
        .unwrap();
        let shots = std::env::temp_dir();
        let mut host = Host::with_script(&data, script, &shots, false).unwrap();
        host.set_font(2);
        let w = host.g.rastport_text_width(&host.rp, b"1");
        assert!(w > 0);
        let expected = (0xaf / w) as usize;
        assert!(expected > 28, "the glyph is {w} pixels wide");
        let text = text_input(&mut host, 0x3f, 0xaa, 0xaf, 1, 8).unwrap();
        assert_eq!(text, vec![b'1'; expected]);
    }
}
