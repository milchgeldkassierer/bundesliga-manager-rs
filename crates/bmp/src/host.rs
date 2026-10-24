//! The machine the game code runs on: the emulated VGA/mouse library ([`Gfx`]), the
//! BIOS tick counter, keyboard input and the game's picture loader.
//!
//! The original game is written as blocking code (busy-wait loops around
//! `input_poll_mouse` and the timer). The port keeps that structure: every waiting
//! primitive calls [`Host::pump`], which feeds input into the library, advances the
//! tick counter and presents frames. Two backends exist: an SDL window running in
//! real time, and a scripted headless backend with a virtual clock for reproducible
//! screenshots and tests.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bmp_formats::{cp, font::FontSet, pal, vga};

use crate::gfx::{Bitmap, BitmapId, Gfx, PALETTE_SLOT_OFFSET, RastPort, Ui};
use crate::video::{Vga, Window};

/// BIOS timer frequency (PIT 1193182 Hz / 65536).
const TICKS_PER_SECOND: f64 = 1_193_182.0 / 65_536.0;

/// Virtual milliseconds that pass per [`Host::pump`] call in the scripted backend.
const SCRIPT_MS_PER_PUMP: u64 = 1;

/// The game's font numbers (`ui_set_font` 176b:0c91) map to library indices: the
/// name table at `DS:012a` lists `flow`, `micro4`, `newbold` for game fonts 1..3,
/// while the library ([`crate::gfx::FONT_NAMES`]) orders them micro4, flow, newbold.
const GAME_FONT_TO_LIBRARY: [u16; 4] = [0, 2, 1, 3];

/// `FAR:2ea2` values written by `input_poll_mouse`.
pub const LEFT_DOWN: u16 = 1;
pub const LEFT_UP: u16 = 2;
pub const RIGHT_DOWN: u16 = 3;

/// One scripted input step.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptStep {
    /// Let `ms` virtual milliseconds pass.
    Wait(u64),
    /// Move the pointer and press/release a button (`right` selects the right one).
    Click { x: i16, y: i16, right: bool },
    /// Press and hold the left button at a position (released by `Release`).
    Press { x: i16, y: i16 },
    /// Release the left (or right) button.
    Release { right: bool },
    /// Queue keyboard characters (`\r` = Enter, `\x08` = Backspace, `\x1b` = Esc).
    Keys(Vec<u8>),
    /// Save the visible screen as `<name>.png` in the screenshot directory.
    Shot(String),
    /// End the program.
    Quit,
}

/// Parses a script: one step per line, `#` comments. Commands: `wait <ms>`,
/// `click <x> <y>`, `rclick <x> <y>`, `press <x> <y>`, `release`, `type <text>`,
/// `key enter|backspace|esc`, `shot <name>`, `quit`.
pub fn parse_script(text: &str) -> Result<Vec<ScriptStep>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let err = |m: &str| format!("script line {}: {m}", n + 1);
        let nums = || -> Result<Vec<i64>, String> {
            rest.split_whitespace()
                .map(|v| v.parse::<i64>().map_err(|e| err(&e.to_string())))
                .collect()
        };
        let xy = || -> Result<(i16, i16), String> {
            match nums()?.as_slice() {
                [x, y] => Ok((*x as i16, *y as i16)),
                _ => Err(err("expected <x> <y>")),
            }
        };
        steps.push(match cmd {
            "wait" => match nums()?.as_slice() {
                [ms] if *ms >= 0 => ScriptStep::Wait(*ms as u64),
                _ => return Err(err("expected wait <ms>")),
            },
            "click" | "rclick" => {
                let (x, y) = xy()?;
                ScriptStep::Click {
                    x,
                    y,
                    right: cmd == "rclick",
                }
            }
            "press" => {
                let (x, y) = xy()?;
                ScriptStep::Press { x, y }
            }
            "release" => ScriptStep::Release { right: false },
            "type" => ScriptStep::Keys(bmp_formats::font::encode_cp437(rest)),
            "key" => ScriptStep::Keys(vec![match rest {
                "enter" => b'\r',
                "backspace" => 8,
                "esc" => 0x1b,
                _ => return Err(err("unknown key")),
            }]),
            "shot" if !rest.is_empty() => ScriptStep::Shot(rest.to_string()),
            "quit" => ScriptStep::Quit,
            _ => return Err(err("unknown command")),
        });
    }
    Ok(steps)
}

enum Backend {
    Window {
        win: Box<Window>,
        start: Instant,
        last_present: Instant,
    },
    Script {
        now_ms: u64,
        /// Virtual time at which the next step runs.
        next_ms: u64,
        steps: VecDeque<ScriptStep>,
        shots: PathBuf,
    },
}

/// The program ends (window closed or script `quit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quit;

pub type Run<T> = Result<T, Quit>;

pub struct Host {
    pub g: Gfx,
    /// The main rastport `FAR:2f28` on the screen bitmap `FAR:a6ca`.
    pub rp: RastPort,
    /// Button slots and pictures of the 16c7 UI primitives.
    pub ui: Ui,
    pub data: PathBuf,
    /// `FAR:2ea2`: last event read by [`Host::input_poll_mouse`].
    pub button: u16,
    /// `DS:079c`: minimum field width of `format_long_thousands`; every
    /// `vga_load_picture` call sets it to 1.
    pub number_pad: i8,
    /// `DS:014a`: last `screen_fade` state.
    fade_state: u8,
    keys: VecDeque<u8>,
    backend: Backend,
    vga: Vga,
}

impl Host {
    fn new(data: &Path, backend: Backend) -> Result<Self, String> {
        let mut g = Gfx::new();
        let fonts = FontSet::load(data).map_err(|e| e.to_string())?;
        g.set_fonts(&fonts);
        let rp = g.screen_open();
        g.mouse_open(true, true);
        let mut ui = Ui::new(rp.bitmap);
        // `main` turns a blank 1x1 picture into the 320x240 page-2 token `FAR:56fa`.
        ui.work = Some(g.alloc(Bitmap::new_mem(320, 240)));
        Ok(Self {
            g,
            rp,
            ui,
            data: data.to_path_buf(),
            button: 0,
            number_pad: 0,
            fade_state: 0xff,
            keys: VecDeque::new(),
            backend,
            vga: Vga::new(),
        })
    }

    pub fn with_window(data: &Path, scale: u32) -> Result<Self, String> {
        let win = Window::open(scale)?;
        win.hide_cursor();
        let now = Instant::now();
        Self::new(
            data,
            Backend::Window {
                win: Box::new(win),
                start: now,
                last_present: now,
            },
        )
    }

    pub fn with_script(data: &Path, steps: Vec<ScriptStep>, shots: &Path) -> Result<Self, String> {
        Self::new(
            data,
            Backend::Script {
                now_ms: 0,
                next_ms: 0,
                steps: steps.into(),
                shots: shots.to_path_buf(),
            },
        )
    }

    /// Processes pending input, advances the clock and presents the screen.
    pub fn pump(&mut self) -> Run<()> {
        match &mut self.backend {
            Backend::Window {
                win,
                start,
                last_present,
            } => {
                use sdl2::event::Event;
                use sdl2::keyboard::Keycode;
                use sdl2::mouse::MouseButton;
                let events: Vec<Event> = win.events.poll_iter().collect();
                for ev in events {
                    match ev {
                        Event::Quit { .. } => return Err(Quit),
                        Event::MouseMotion { x, y, .. } => {
                            self.g
                                .mouse_event_handler(1, (x.max(0) * 2) as u16, y.max(0) as u16)
                        }
                        Event::MouseButtonDown {
                            mouse_btn, x, y, ..
                        }
                        | Event::MouseButtonUp {
                            mouse_btn, x, y, ..
                        } => {
                            let down = matches!(ev, Event::MouseButtonDown { .. });
                            let mask = match (mouse_btn, down) {
                                (MouseButton::Left, true) => 2,
                                (MouseButton::Left, false) => 4,
                                (MouseButton::Right, true) => 8,
                                (MouseButton::Right, false) => 0x10,
                                _ => 0,
                            };
                            self.g.mouse_event_handler(
                                mask | 1,
                                (x.max(0) * 2) as u16,
                                y.max(0) as u16,
                            );
                        }
                        Event::KeyDown {
                            keycode: Some(k), ..
                        } => match k {
                            Keycode::Return | Keycode::KpEnter => self.keys.push_back(b'\r'),
                            Keycode::Backspace => self.keys.push_back(8),
                            Keycode::Escape => self.keys.push_back(0x1b),
                            _ => {}
                        },
                        Event::TextInput { text, .. } => self
                            .keys
                            .extend(bmp_formats::font::encode_cp437(&text).iter()),
                        _ => {}
                    }
                }
                let elapsed = start.elapsed().as_secs_f64();
                self.g.bios_ticks = (elapsed * TICKS_PER_SECOND) as u32;
                if last_present.elapsed() >= Duration::from_millis(14) {
                    *last_present = Instant::now();
                    self.g.to_vga(&mut self.vga);
                    win.present(&self.vga).map_err(|_| Quit)?;
                } else {
                    std::thread::sleep(Duration::from_micros(500));
                }
            }
            Backend::Script { .. } => {
                self.advance_script()?;
            }
        }
        Ok(())
    }

    fn advance_script(&mut self) -> Run<()> {
        let Backend::Script {
            now_ms,
            next_ms,
            steps,
            shots,
        } = &mut self.backend
        else {
            return Ok(());
        };
        *now_ms += SCRIPT_MS_PER_PUMP;
        self.g.bios_ticks = (*now_ms as f64 / 1000.0 * TICKS_PER_SECOND) as u32;
        while *now_ms >= *next_ms {
            let Some(step) = steps.pop_front() else {
                return Err(Quit);
            };
            match step {
                ScriptStep::Wait(ms) => *next_ms = *now_ms + ms,
                ScriptStep::Click { x, y, right } => {
                    let down = if right { 8 } else { 2 };
                    self.g.mouse_event_handler(1, (x * 2) as u16, y as u16);
                    self.g.mouse_event_handler(down, (x * 2) as u16, y as u16);
                    // The release follows after a short hold, like a real click.
                    steps.push_front(ScriptStep::Release { right });
                    steps.push_front(ScriptStep::Wait(60));
                }
                ScriptStep::Press { x, y } => {
                    self.g.mouse_event_handler(1, (x * 2) as u16, y as u16);
                    self.g.mouse_event_handler(2, (x * 2) as u16, y as u16);
                }
                ScriptStep::Release { right } => {
                    self.g
                        .mouse_event_handler(if right { 0x10 } else { 4 }, 0, 0)
                }
                ScriptStep::Keys(k) => self.keys.extend(k),
                ScriptStep::Shot(name) => {
                    self.g.to_vga(&mut self.vga);
                    let path = shots.join(format!("{name}.png"));
                    self.vga.save_png(&path).map_err(|_| Quit)?;
                    eprintln!("screenshot {}", path.display());
                }
                ScriptStep::Quit => return Err(Quit),
            }
        }
        Ok(())
    }

    // ----- input (176b) ---------------------------------------------------------------

    /// Reads the next latched mouse event into `FAR:2ea2` (0 = none).
    ///
    /// Origin: BMMAIN.EXE 176b:0319 (input_poll_mouse)
    pub fn input_poll_mouse(&mut self) -> Run<u16> {
        self.pump()?;
        self.button = self.g.mouse_pop_button_event();
        Ok(self.button)
    }

    /// Pointer position as the game reads it (`mouse_state[1]`, `mouse_state[0]`).
    pub fn mouse_xy(&self) -> (i16, i16) {
        (self.g.pointer.client_x, self.g.pointer.client_y)
    }

    /// Blocks until a key is available (`dos_getch`, INT21 AH=07).
    pub fn getch(&mut self) -> Run<u8> {
        loop {
            if let Some(k) = self.keys.pop_front() {
                return Ok(k);
            }
            self.pump()?;
        }
    }

    // ----- timer ----------------------------------------------------------------------

    /// `delay_ticks(n)` followed by the busy wait its callers do.
    ///
    /// Origin: BMMAIN.EXE 4930:01e7 (delay_ticks)
    pub fn delay_ticks(&mut self, n: i32) -> Run<()> {
        self.g.delay_ticks(n);
        self.wait_deadline()
    }

    /// Origin: BMMAIN.EXE 18bc:1e75 (wait_5_ticks)
    pub fn wait_5_ticks(&mut self) -> Run<()> {
        self.g.timer_set_deadline(5);
        self.wait_deadline()
    }

    fn wait_deadline(&mut self) -> Run<()> {
        while !self.g.timer_deadline_reached() {
            self.pump()?;
        }
        Ok(())
    }

    // ----- screen -----------------------------------------------------------------------

    /// Turns the screen off (0) or on (1) when the state changes. With the palette
    /// fade option `DS:060c` set (default 0, not modelled yet) the original fades
    /// palette entries 0..31 instead.
    ///
    /// Origin: BMMAIN.EXE 176b:1416 (screen_fade)
    pub fn screen_fade(&mut self, on: u8) -> Run<()> {
        if on != self.fade_state {
            self.fade_state = on;
            if on == 0 {
                self.g.video_screen_off();
            } else {
                self.g.video_screen_on();
            }
            self.pump()?;
        }
        Ok(())
    }

    /// Stores the three palettes of `PIC\1.PAL` in off-screen VRAM (plane `i` at
    /// [`PALETTE_SLOT_OFFSET`]), colour 255 forced to black.
    ///
    /// Origin: BMMAIN.EXE 18bc:0004 (startup_init_screen_mouse_palette)
    pub fn load_palette_slots(&mut self) -> Result<(), String> {
        let pals = pal::PalFile::load(&self.data).map_err(|e| e.to_string())?;
        for i in 0..3 {
            let bytes = pals.vram_palette(i).to_dac_bytes();
            let at = PALETTE_SLOT_OFFSET as usize;
            self.g.planes[i][at..at + bytes.len()].copy_from_slice(&bytes);
        }
        Ok(())
    }

    /// Selects game font `n` (1 flow, 2 micro4, 3 newbold) for the main rastport.
    ///
    /// Origin: BMMAIN.EXE 176b:0c91 (ui_set_font)
    pub fn set_font(&mut self, n: usize) {
        self.rp.font = Some(GAME_FONT_TO_LIBRARY[n]);
    }

    fn read_pic(&self, name: &str) -> Result<Vec<u8>, String> {
        let path = self.data.join("PIC").join(name);
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Loads `PIC\<n>.VGA` into a new bitmap.
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture, from-file path)
    pub fn load_picture(&mut self, n: u16) -> Result<BitmapId, String> {
        self.number_pad = 1;
        let img = vga::parse(&self.read_pic(&format!("{n}.VGA"))?).map_err(|e| e.to_string())?;
        Ok(self.g.alloc(Bitmap::from_image(&img)))
    }

    /// A blank `w x h` bitmap filled with `color`.
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture with picture 0 and no file)
    pub fn blank_bitmap(&mut self, w: i16, h: i16, color: u8) -> BitmapId {
        self.number_pad = 1;
        let id = self.g.alloc(Bitmap::new_mem(w, h));
        self.g.clear_bitmap(id, color);
        id
    }

    /// Decodes `PIC\<n>.CP` straight into the draw page (`vga_load_picture(n, 30000, ..)`).
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture, 30000 = full screen)
    pub fn load_screen_picture(&mut self, n: u16) -> Result<(), String> {
        self.number_pad = 1;
        let img = cp::decode(&self.read_pic(&format!("{n}.CP"))?).map_err(|e| e.to_string())?;
        let base = if self.g.draw_page != 0 {
            crate::gfx::PAGE_SIZE as usize
        } else {
            0
        };
        for (i, &px) in img.pixels.iter().enumerate() {
            let (x, y) = (i % 320, i / 320);
            self.g.planes[x & 3][base + y * 80 + x / 4] = px;
        }
        Ok(())
    }

    /// Number format of `format_long_thousands` as set by the current globals.
    pub fn number_format(&self) -> bmp_game::rng::NumberFormat {
        bmp_game::rng::NumberFormat {
            pad_width: self.number_pad,
            ..Default::default()
        }
    }

    /// The bitmap the main rastport draws to (`FAR:a6ca`).
    pub fn screen(&self) -> BitmapId {
        self.rp.bitmap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_parses_all_commands() {
        let steps = parse_script(
            "# comment\nwait 10\nclick 1 2\nrclick 3 4\npress 5 6\nrelease\ntype ab\nkey enter\nshot x\nquit\n",
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                ScriptStep::Wait(10),
                ScriptStep::Click {
                    x: 1,
                    y: 2,
                    right: false
                },
                ScriptStep::Click {
                    x: 3,
                    y: 4,
                    right: true
                },
                ScriptStep::Press { x: 5, y: 6 },
                ScriptStep::Release { right: false },
                ScriptStep::Keys(b"ab".to_vec()),
                ScriptStep::Keys(vec![b'\r']),
                ScriptStep::Shot("x".into()),
                ScriptStep::Quit,
            ]
        );
    }

    #[test]
    fn script_errors_name_the_line() {
        let err = parse_script("wait 1\nclick 1\n").unwrap_err();
        assert!(err.starts_with("script line 2"), "{err}");
        assert!(parse_script("jump 1 2").is_err());
    }
}
