//! The machine the game code runs on: the emulated VGA/mouse library ([`Gfx`]), the
//! BIOS tick counter, keyboard input and the game's picture loader.
//!
//! The original game is written as blocking code (busy-wait loops around
//! `input_poll_mouse` and the timer). The port keeps that structure: every waiting
//! primitive calls [`Host::pump`], which feeds input into the library, advances the
//! tick counter and presents frames. Two backends exist: an SDL window running in
//! real time, and a scripted headless backend with a virtual clock for reproducible
//! screenshots and tests.
//!
//! The Sound Blaster is emulated by [`SoundCard`]: with a window it plays through
//! SDL audio, headless (or without an audio device) it runs silently on the same
//! clock, so the music still ends the title screen at the same time.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bmp_formats::{cp, font::FontSet, pal, vga};

use crate::audio::SoundCard;
use crate::display::DisplayFrame;
use crate::gfx::{Bitmap, BitmapId, Gfx, PALETTE_SLOT_OFFSET, RastPort, Ui};
use crate::video::{Vga, Window};

/// BIOS timer frequency (PIT 1193182 Hz / 65536).
const TICKS_PER_SECOND: f64 = 1_193_182.0 / 65_536.0;

/// Virtual milliseconds that pass per [`Host::pump`] call in the scripted backend.
const SCRIPT_MS_PER_PUMP: u64 = 1;
/// The refresh rate of VGA mode X: one DAC upload of a palette fade per retrace.
const VRETRACE_HZ: f64 = 70.0;
/// `GameState::options` index of "Blenden" (`DS:060c` = `DS:05fe + 14`).
pub const PALETTE_FADE_OPTION: usize = 14;

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
    /// Move the pointer without pressing a button.
    Move { x: i16, y: i16 },
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
/// `click <x> <y>`, `rclick <x> <y>`, `move <x> <y>`, `press <x> <y>`, `release`, `type <text>`,
/// `key enter|space|backspace|esc`, `shot <name>`, `quit`.
/// Pointer coordinates are mouse driver coordinates; where a program sets a driver y range
/// (TEAMED.EXE's scrolled UEFA list: 87..328) they are clamped to it.
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
            "move" => {
                let (x, y) = xy()?;
                ScriptStep::Move { x, y }
            }
            "press" => {
                let (x, y) = xy()?;
                ScriptStep::Press { x, y }
            }
            "release" => ScriptStep::Release { right: false },
            "type" => ScriptStep::Keys(bmp_formats::font::encode_cp437(rest)),
            "key" => ScriptStep::Keys(vec![match rest {
                "enter" => b'\r',
                "space" => b' ',
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
        /// MOD2026: the presented 2x display frame (unused while the layer is off).
        display_frame: Box<DisplayFrame>,
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
    /// `DS:07b2`: suppresses the thousands separators of `format_long_thousands`.
    pub number_no_separators: bool,
    /// `DS:0210`: the office background (`PIC\22.CP`) is cached in VRAM page 2.
    pub office_bg_cached: bool,
    /// `DS:05d4` (automatic play) as seen by the widgets that get no session, e.g.
    /// `dialog_okay`. The office keeps it equal to `Session::auto_play`.
    pub auto_play: bool,
    /// `DS:014a`: last `screen_fade` state.
    fade_state: u8,
    /// `DS:060c`: the settings option "Blenden" (`GameState::options[14]`), mirrored here
    /// because `screen_fade` reads it. [`Host::sync_options`] keeps it equal to the state.
    pub palette_fade: bool,
    keys: VecDeque<u8>,
    /// A key went down that produced no character (e.g. a cursor key); read by
    /// [`Host::key_hit`].
    key_down: bool,
    /// The program draws `vga` itself (BMINTRO's mode 13h) instead of the library
    /// screen being converted on every present.
    raw_video: bool,
    backend: Backend,
    vga: Vga,
    /// The sound card; `None` when sound is disabled (`DS:018c` = 0).
    card: Option<SoundCard>,
    /// The code of a pending `fatal_error_exit` (the first failure wins): set where the
    /// original calls it, handled by `game::run`, where the game state is reachable.
    pub fatal: Option<u8>,
    /// MOD2026: the data directory holds Mod 2026 data, read once at start-up; switches
    /// the 2x display layer on in a window.
    pub mod2026: bool,
    /// MOD2026: loaded mod crest pictures get their 2x version as display side; set with
    /// the display layer by [`Host::enable_display_layer`].
    hires_crests: bool,
    /// Test support: per screenshot of the script, the logical frame and the 2x display
    /// frame (only recorded while the layer is on).
    #[cfg(test)]
    pub(crate) display_shots: Vec<(String, Vga, DisplayFrame)>,
}

/// MOD2026: whether the 2x display layer is used: only with Mod 2026 data, in a window
/// and without `--lowres`.
pub fn display_layer_on(mod2026: bool, window: bool, lowres: bool) -> bool {
    mod2026 && window && !lowres
}

impl Host {
    fn new(
        data: &Path,
        backend: Backend,
        card: Option<SoundCard>,
        lowres: bool,
        hires_fonts: bool,
    ) -> Result<Self, String> {
        let mut g = Gfx::new();
        // `font_load_all() == 0` is `fatal_error_exit(3)`; without fonts there is nothing to draw
        // the message with, so the port only reports it.
        let fonts = FontSet::load(data).map_err(|e| {
            let kind = crate::game::fatal::error_text(crate::game::fatal::FONT_ERROR);
            format!("{}: {e}", String::from_utf8_lossy(kind))
        })?;
        g.set_fonts(&fonts);
        let mod2026 = bmp_game::mod2026::is_active(data);
        if mod2026 {
            // MOD2026: the fonts get the euro glyph.
            g.set_euro_glyphs();
        }
        let rp = g.screen_open();
        g.mouse_open(true, true);
        let mut ui = Ui::new(rp.bitmap);
        // `main` turns a blank 1x1 picture into the 320x240 page-2 token `FAR:56fa`.
        ui.work = Some(g.alloc(Bitmap::new_mem(320, 240)));
        let window = matches!(backend, Backend::Window { .. });
        let mut host = Self {
            g,
            rp,
            ui,
            data: data.to_path_buf(),
            button: 0,
            number_pad: 0,
            number_no_separators: false,
            office_bg_cached: false,
            auto_play: false,
            fade_state: 0xff,
            palette_fade: false,
            keys: VecDeque::new(),
            key_down: false,
            raw_video: false,
            backend,
            vga: Vga::new(),
            card,
            fatal: None,
            mod2026,
            hires_crests: false,
            #[cfg(test)]
            display_shots: Vec::new(),
        };
        if display_layer_on(host.mod2026, window, lowres) {
            host.enable_display_layer_with(hires_fonts);
        }
        Ok(host)
    }

    /// MOD2026: switches the 2x display layer on, with the 2x fonts and crests that belong
    /// to it (`--hires-fonts`), as the display tests use it.
    #[cfg(test)]
    pub(crate) fn enable_display_layer(&mut self) {
        self.enable_display_layer_with(true);
    }

    /// MOD2026: switches the 2x display layer on with the 2x crests; the 2x fonts only with
    /// `hires_fonts` (`--hires-fonts`), else the original glyphs show pixel-doubled.
    pub(crate) fn enable_display_layer_with(&mut self, hires_fonts: bool) {
        self.g.display_on();
        if hires_fonts {
            self.g.set_hires_fonts();
        }
        self.hires_crests = true;
    }

    /// A window; `sound` = the sound flag `DS:018c`. Without an audio device
    /// the card runs silently. `lowres` keeps the Mod 2026 2x display off; `hires_fonts`
    /// gives it the 2x fonts.
    pub fn with_window(
        data: &Path,
        scale: u32,
        sound: bool,
        lowres: bool,
        hires_fonts: bool,
    ) -> Result<Self, String> {
        // A failed `screen_open` is `fatal_error_exit(0)`; without a screen the port only
        // reports it.
        let win = Window::open(scale).map_err(|e| {
            let kind = crate::game::fatal::error_text(crate::game::fatal::GRAPHICS_ERROR);
            format!("{}: {e}", String::from_utf8_lossy(kind))
        })?;
        win.hide_cursor();
        let card = sound.then(|| {
            win.audio()
                .and_then(|a| SoundCard::open(&a))
                .unwrap_or_else(|e| {
                    eprintln!("no audio output ({e}); sound runs silently");
                    SoundCard::silent()
                })
        });
        let now = Instant::now();
        Self::new(
            data,
            Backend::Window {
                win: Box::new(win),
                start: now,
                last_present: now,
                display_frame: Box::default(),
            },
            card,
            lowres,
            hires_fonts,
        )
    }

    /// The headless backend; with `sound` a silent card runs on the virtual clock.
    pub fn with_script(
        data: &Path,
        steps: Vec<ScriptStep>,
        shots: &Path,
        sound: bool,
    ) -> Result<Self, String> {
        Self::new(
            data,
            Backend::Script {
                now_ms: 0,
                next_ms: 0,
                steps: steps.into(),
                shots: shots.to_path_buf(),
            },
            sound.then(SoundCard::silent),
            false,
            false,
        )
    }

    /// Records `fatal_error_exit(code)` (the first failure wins) with `detail` on stderr and
    /// returns the [`Quit`] that unwinds to `game::run`, which shows the error screen.
    pub fn fatal_error(&mut self, code: u8, detail: &str) -> Quit {
        let kind = crate::game::fatal::error_text(code);
        eprintln!("{}: {detail}", String::from_utf8_lossy(kind));
        self.fatal.get_or_insert(code);
        Quit
    }

    /// Processes pending input, advances the clock and presents the screen. A pending
    /// fatal error ends the run here, so a failure that a caller does not propagate
    /// itself still reaches `fatal_error_exit`.
    pub fn pump(&mut self) -> Run<()> {
        if self.fatal.is_some() {
            return Err(Quit);
        }
        match &mut self.backend {
            Backend::Window {
                win,
                start,
                last_present,
                display_frame,
            } => {
                use sdl2::event::Event;
                use sdl2::keyboard::Keycode;
                use sdl2::mouse::MouseButton;
                let events: Vec<Event> = win.events.poll_iter().collect();
                for ev in events {
                    match ev {
                        Event::Quit { .. } => return Err(Quit),
                        Event::MouseMotion { x, y, .. } => {
                            let y = self.g.mouse_window_y(y.max(0) as i16);
                            self.g
                                .mouse_event_handler(1, (x.max(0) * 2) as u16, y as u16)
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
                            let y = self.g.mouse_window_y(y.max(0) as i16);
                            self.g
                                .mouse_event_handler(mask | 1, (x.max(0) * 2) as u16, y as u16);
                        }
                        Event::KeyDown {
                            keycode: Some(k), ..
                        } => match k {
                            Keycode::Return | Keycode::KpEnter => self.keys.push_back(b'\r'),
                            Keycode::Backspace => self.keys.push_back(8),
                            Keycode::Escape => self.keys.push_back(0x1b),
                            Keycode::LShift
                            | Keycode::RShift
                            | Keycode::LCtrl
                            | Keycode::RCtrl
                            | Keycode::LAlt
                            | Keycode::RAlt
                            | Keycode::LGui
                            | Keycode::RGui
                            | Keycode::CapsLock
                            | Keycode::NumLockClear
                            | Keycode::ScrollLock => {}
                            _ => self.key_down = true,
                        },
                        Event::TextInput { text, .. } => self
                            .keys
                            .extend(bmp_formats::font::encode_cp437(&text).iter()),
                        _ => {}
                    }
                }
                let elapsed = start.elapsed().as_secs_f64();
                self.g.bios_ticks = (elapsed * TICKS_PER_SECOND) as u32;
                if let Some(card) = &mut self.card {
                    card.sync(elapsed);
                }
                if last_present.elapsed() >= Duration::from_millis(14) {
                    *last_present = Instant::now();
                    if !self.raw_video && self.g.to_display(display_frame) {
                        win.present_display(display_frame).map_err(|_| Quit)?;
                    } else {
                        if !self.raw_video {
                            self.g.to_vga(&mut self.vga);
                        }
                        win.present(&self.vga).map_err(|_| Quit)?;
                    }
                } else {
                    std::thread::sleep(Duration::from_micros(500));
                }
            }
            Backend::Script { .. } => {
                self.advance_script()?;
                if let (Some(card), Backend::Script { now_ms, .. }) =
                    (&mut self.card, &self.backend)
                {
                    card.sync(*now_ms as f64 / 1000.0);
                }
            }
        }
        Ok(())
    }

    /// Test support: the clicks of the script that have not run yet (0 for a window).
    #[cfg(test)]
    pub(crate) fn script_clicks_left(&self) -> usize {
        match &self.backend {
            Backend::Script { steps, .. } => steps
                .iter()
                .filter(|s| matches!(s, ScriptStep::Click { .. }))
                .count(),
            Backend::Window { .. } => 0,
        }
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
                    let y = self.g.mouse_clamp_y(y);
                    let down = if right { 8 } else { 2 };
                    self.g.mouse_event_handler(1, (x * 2) as u16, y as u16);
                    self.g.mouse_event_handler(down, (x * 2) as u16, y as u16);
                    // The release follows after a short hold, like a real click.
                    steps.push_front(ScriptStep::Release { right });
                    steps.push_front(ScriptStep::Wait(60));
                }
                ScriptStep::Move { x, y } => {
                    let y = self.g.mouse_clamp_y(y);
                    self.g.mouse_event_handler(1, (x * 2) as u16, y as u16);
                }
                ScriptStep::Press { x, y } => {
                    let y = self.g.mouse_clamp_y(y);
                    self.g.mouse_event_handler(1, (x * 2) as u16, y as u16);
                    self.g.mouse_event_handler(2, (x * 2) as u16, y as u16);
                }
                ScriptStep::Release { right } => {
                    self.g
                        .mouse_event_handler(if right { 0x10 } else { 4 }, 0, 0)
                }
                ScriptStep::Keys(k) => self.keys.extend(k),
                ScriptStep::Shot(name) => {
                    if !self.raw_video {
                        self.g.to_vga(&mut self.vga);
                        #[cfg(test)]
                        {
                            let mut frame = DisplayFrame::new();
                            if self.g.to_display(&mut frame) {
                                // Evidence aid: the 2x frame as a PNG to
                                // `$BMP_DISPLAY_SHOTS_OUT` when that is set.
                                if let Some(dir) = std::env::var_os("BMP_DISPLAY_SHOTS_OUT") {
                                    let path = Path::new(&dir).join(format!("{name}.png"));
                                    crate::video::write_rgb_png(
                                        &path,
                                        crate::display::DISPLAY_W as u32,
                                        frame.height as u32,
                                        &frame.to_rgb24(),
                                    )
                                    .map_err(|_| Quit)?;
                                }
                                self.display_shots
                                    .push((name.clone(), self.vga.clone(), frame));
                            }
                        }
                    }
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

    /// Polls until no mouse event is pending (`FAR:3046` stays 0).
    ///
    /// Origin: BMMAIN.EXE 176b:0e27 (input_wait_release)
    pub fn input_wait_release(&mut self) -> Run<()> {
        while self.input_poll_mouse()? != 0 {}
        Ok(())
    }

    /// Pointer position as the game reads it (`mouse_state[1]`, `mouse_state[0]`).
    pub fn mouse_xy(&self) -> (i16, i16) {
        (self.g.pointer.client_x, self.g.pointer.client_y)
    }

    /// Blocks until a key is available (`dos_getch`, INT21 AH=07).
    ///
    /// Origin: BMMAIN.EXE 45ac:326b (dos_getch)
    pub fn getch(&mut self) -> Run<u8> {
        loop {
            if let Some(k) = self.keys.pop_front() {
                return Ok(k);
            }
            self.pump()?;
        }
    }

    /// Whether a key was pressed (INT 21h AH=06h DL=FFh returns ZF clear): takes
    /// one pending character, or the press of a key without one.
    pub fn key_hit(&mut self) -> bool {
        let hit = self.keys.pop_front().is_some() || self.key_down;
        self.key_down = false;
        hit
    }

    /// Seconds since the program started (virtual time when scripted).
    pub fn now_secs(&self) -> f64 {
        match &self.backend {
            Backend::Window { start, .. } => start.elapsed().as_secs_f64(),
            Backend::Script { now_ms, .. } => *now_ms as f64 / 1000.0,
        }
    }

    /// Hands the video output to the caller (`true`): from now on `pump` presents
    /// [`Host::vga_mut`] as it is. `false` returns to the library screen.
    pub fn set_raw_video(&mut self, on: bool) {
        self.raw_video = on;
    }

    /// The emulated VGA output (see [`Host::set_raw_video`]).
    pub fn vga_mut(&mut self) -> &mut Vga {
        &mut self.vga
    }

    /// Reads a pending key without waiting (`dos_direct_console_io`, INT 21h
    /// AH=06h DL=FFh); 0 when none is pending.
    ///
    /// Origin: BMMAIN.EXE 45ac:3272 (dos_direct_console_io)
    pub fn read_key_nowait(&mut self) -> u8 {
        self.keys.pop_front().unwrap_or(0)
    }

    // ----- sound (4d6c, 176b) ------------------------------------------------------------

    /// The sound checks and driver calls of `main` for the title music: with sound
    /// enabled (`DS:018c`; the card always passes `sb_detect_port` and
    /// `fmdrv_init`) it reads `SOUND\BM2TITLE.CMF` and starts it. Returns whether
    /// the music plays (`main`'s `music` flag).
    ///
    /// Origin: BMMAIN.EXE 18bc:0418 (main, 18bc:04a0..05d8)
    pub fn music_start(&mut self) -> Result<bool, String> {
        let Some(card) = &self.card else {
            return Ok(false);
        };
        let path = self.data.join("SOUND").join("BM2TITLE.CMF");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cmf = bmp_formats::cmf::Cmf::parse(&bytes).map_err(|e| e.to_string())?;
        card.mixer().music = Some(bmp_sound::music::Music::start(&cmf));
        Ok(true)
    }

    /// Non-zero while the music plays.
    ///
    /// Origin: BMMAIN.EXE 4d6c:0580 (fmdrv_poll_status)
    pub fn music_poll_status(&mut self) -> u8 {
        let Some(card) = &self.card else {
            return 0;
        };
        card.mixer().music.as_mut().map_or(0, |m| m.poll_status())
    }

    /// `fmdrv_stop_music` followed by `fmdrv_shutdown`.
    ///
    /// Origin: BMMAIN.EXE 4d6c:0544 (fmdrv_stop_music), 4d6c:059a (fmdrv_shutdown)
    pub fn music_stop(&mut self) {
        if let Some(card) = &self.card {
            let mut mixer = card.mixer();
            if let Some(m) = mixer.music.as_mut() {
                m.stop();
            }
        }
    }

    /// `sound_voice_init` + `sound_load_digi_voc`: with sound enabled the voices
    /// of `SOUND\DIGI.VOC` become available (`DS:018d`).
    ///
    /// Origin: BMMAIN.EXE 176b:0006 (sound_voice_init), 176b:005f (sound_load_digi_voc)
    pub fn voice_init(&mut self) -> Result<(), String> {
        let Some(card) = &self.card else {
            return Ok(());
        };
        let path = self.data.join("SOUND").join("DIGI.VOC");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let voices = bmp_sound::voice::Voices::from_digi(&bytes).map_err(|e| e.to_string())?;
        card.mixer().voices = Some(voices);
        Ok(())
    }

    fn with_voices(&mut self, f: impl FnOnce(&mut bmp_sound::voice::Voices)) {
        if let Some(card) = &self.card
            && let Some(v) = card.mixer().voices.as_mut()
        {
            f(v);
        }
    }

    /// Origin: BMMAIN.EXE 176b:0182 (sound_play_voice)
    pub fn voice_play(&mut self, n: u8) {
        self.with_voices(|v| v.play(n));
    }

    /// Origin: BMMAIN.EXE 176b:020f (sound_stop_voice)
    pub fn voice_stop(&mut self) {
        self.with_voices(|v| v.stop());
    }

    /// Busy-waits until the voice output has ended (the original polls no input).
    ///
    /// Origin: BMMAIN.EXE 176b:024b (sound_wait_voice_end)
    pub fn voice_wait_end(&mut self) -> Run<()> {
        loop {
            let mut busy = false;
            self.with_voices(|v| {
                busy = v.busy();
                if !busy {
                    v.ended();
                }
            });
            if !busy {
                return Ok(());
            }
            self.pump()?;
        }
    }

    /// `DS:018e`: the voice last started (0 = none, also without sound).
    pub fn voice_current(&mut self) -> u8 {
        let mut n = 0;
        self.with_voices(|v| n = v.current);
        n
    }

    /// Origin: BMMAIN.EXE 176b:027d (sound_restart_loop_voice)
    pub fn voice_restart_loop(&mut self) {
        self.with_voices(|v| v.restart_loop());
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

    /// Turns the screen off (0) or on (1) when the state changes. With the option
    /// "Blenden" (`DS:060c`, [`Host::palette_fade`]) set, palette entries 0..31 are faded
    /// out to black before the screen goes off, or faded in from black after it went on;
    /// the screen palette keeps its colours either way.
    ///
    /// Origin: BMMAIN.EXE 176b:1416 (screen_fade)
    pub fn screen_fade(&mut self, on: u8) -> Run<()> {
        if on == self.fade_state {
            return Ok(());
        }
        self.fade_state = on;
        if !self.palette_fade {
            self.g.vga_wait_vretrace();
            if on == 0 {
                self.g.video_screen_off();
            } else {
                self.g.video_screen_on();
            }
            return self.pump();
        }
        let mut words = [0u16; 32];
        for (i, w) in words.iter_mut().enumerate() {
            *w = self.g.palette_get_rgb4(i as u8);
            if on != 0 {
                self.g.palette_set_entry(i as u8, 0, 0, 0);
            }
        }
        if on == 0 {
            self.palette_fade_frames(&words, false)?;
            self.g.video_screen_off();
        } else {
            self.g.video_screen_on();
            self.palette_fade_frames(&words, true)?;
        }
        self.g.palette_load_rgb4(&words, 32);
        self.pump()
    }

    /// Runs `vga_set_palette_rgb4`'s fade and shows each of its 30 DAC uploads for one
    /// vertical retrace (1/70 s): the routine waits for the retrace before every upload.
    ///
    /// Origin: BMMAIN.EXE 45ac:33ca (vga_set_palette_rgb4)
    fn palette_fade_frames(&mut self, words: &[u16; 32], fade_in: bool) -> Run<()> {
        let frames = self.g.vga_set_palette_rgb4(words, fade_in);
        let last = self.g.dac;
        let start = self.now_secs();
        for (k, f) in frames.iter().enumerate() {
            for (e, c) in f.iter().enumerate() {
                self.g.dac[e] = [c[0] & 0x3f, c[1] & 0x3f, c[2] & 0x3f];
            }
            let until = start + (k + 1) as f64 / VRETRACE_HZ;
            while self.now_secs() < until {
                self.pump()?;
            }
        }
        self.g.dac = last;
        Ok(())
    }

    /// Mirrors the options of `state` that the host itself reads (`DS:060c`).
    pub fn sync_options(&mut self, state: &bmp_game::state::GameState) {
        self.palette_fade = state.options[PALETTE_FADE_OPTION] != 0;
    }

    /// Stores the three palettes of `PIC\1.PAL` in off-screen VRAM (plane `i` at
    /// [`PALETTE_SLOT_OFFSET`]), colour 255 forced to black.
    ///
    /// Origin: BMMAIN.EXE 18bc:0004 (startup_init_screen_mouse_palette)
    pub fn load_palette_slots(&mut self) -> Result<(), String> {
        let pals = pal::PalFile::load(&self.data).map_err(|e| e.to_string())?;
        for i in 0..3 {
            let bytes = pals.vram_palette(i).to_dac_bytes();
            for (k, &b) in bytes.iter().enumerate() {
                self.g.vram_write(i, PALETTE_SLOT_OFFSET + k as u16, b);
            }
        }
        Ok(())
    }

    /// Selects game font `n` (1 flow, 2 micro4, 3 newbold) for the main rastport.
    ///
    /// Origin: BMMAIN.EXE 176b:0c91 (ui_set_font)
    pub fn set_font(&mut self, n: usize) {
        Self::set_rastport_font(&mut self.rp, n);
    }

    /// Selects game font `n` for any rastport (`ui_set_font` on a picture).
    ///
    /// Origin: BMMAIN.EXE 176b:0c91 (ui_set_font)
    pub fn set_rastport_font(rp: &mut RastPort, n: usize) {
        rp.font = Some(GAME_FONT_TO_LIBRARY[n]);
    }

    fn read_pic(&self, name: &str) -> Result<Vec<u8>, String> {
        let path = self.data.join("PIC").join(name);
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Loads `PIC\<n>.VGA` into a new bitmap. A missing file is `fatal_error_exit(2)`
    /// (176b:042f), recorded in [`Host::fatal`]; the error is also returned.
    ///
    /// DEVIATION: the original reads whatever a damaged file holds; a file the port cannot
    /// decode is treated like a missing one.
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture, from-file path)
    pub fn load_picture(&mut self, n: u16) -> Result<BitmapId, String> {
        self.number_pad = 1;
        let img = self
            .read_pic(&format!("{n}.VGA"))
            .and_then(|b| vga::parse(&b).map_err(|e| format!("PIC\\{n}.VGA: {e}")))
            .inspect_err(|e| {
                self.fatal_error(crate::game::fatal::DISK_ERROR, e);
            })?;
        let id = self.g.alloc(Bitmap::from_image(&img));
        // MOD2026: a mod crest is drawn into the display layer as its 2x version.
        if self.hires_crests
            && let Some(side) = crate::display::hires_crest_side(n, &img)
        {
            self.g.set_display_side(id, side);
        }
        Ok(id)
    }

    /// [`Host::load_picture`] for callers that end the run on a failure.
    pub fn picture(&mut self, n: u16) -> Run<BitmapId> {
        self.load_picture(n).map_err(|_| Quit)
    }

    /// [`Host::load_screen_picture`] for callers that end the run on a failure.
    pub fn screen_picture(&mut self, n: u16) -> Run<()> {
        self.load_screen_picture(n).map_err(|_| Quit)
    }

    /// A blank `w x h` bitmap filled with `color`. The original's blank pictures
    /// (`vga_load_picture(0, w, h, depth, 0)`) are cleared with `rastport_clear(rp, 0)`;
    /// the fourth argument is the depth, not a colour.
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture with picture 0 and no file)
    pub fn blank_bitmap(&mut self, w: i16, h: i16, color: u8) -> BitmapId {
        self.number_pad = 1;
        let id = self.g.alloc(Bitmap::new_mem(w, h));
        self.g.clear_bitmap(id, color);
        id
    }

    /// Decodes `PIC\<n>.CP` straight into the draw page (`vga_load_picture(n, 30000, ..)`).
    /// Failures are recorded like [`Host::load_picture`]'s.
    ///
    /// Origin: BMMAIN.EXE 176b:0356 (vga_load_picture, 30000 = full screen)
    pub fn load_screen_picture(&mut self, n: u16) -> Result<(), String> {
        self.number_pad = 1;
        let img = self
            .read_pic(&format!("{n}.CP"))
            .and_then(|b| cp::decode(&b).map_err(|e| format!("PIC\\{n}.CP: {e}")))
            .inspect_err(|e| {
                self.fatal_error(crate::game::fatal::DISK_ERROR, e);
            })?;
        let base = if self.g.draw_page != 0 {
            crate::gfx::PAGE_SIZE as usize
        } else {
            0
        };
        for (i, &px) in img.pixels.iter().enumerate() {
            let (x, y) = (i % 320, i / 320);
            self.g.vram_write(x & 3, (base + y * 80 + x / 4) as u16, px);
        }
        Ok(())
    }

    /// Number format of `format_long_thousands` as set by the current globals.
    pub fn number_format(&self) -> bmp_game::rng::NumberFormat {
        bmp_game::rng::NumberFormat {
            pad_width: self.number_pad,
            no_separators: self.number_no_separators,
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
            "# comment\nwait 10\nclick 1 2\nrclick 3 4\nmove 7 8\npress 5 6\nrelease\ntype ab\nkey enter\nkey space\nshot x\nquit\n",
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
                ScriptStep::Move { x: 7, y: 8 },
                ScriptStep::Press { x: 5, y: 6 },
                ScriptStep::Release { right: false },
                ScriptStep::Keys(b"ab".to_vec()),
                ScriptStep::Keys(vec![b'\r']),
                ScriptStep::Keys(vec![b' ']),
                ScriptStep::Shot("x".into()),
                ScriptStep::Quit,
            ]
        );
    }

    /// With "Blenden" set, `screen_fade` fades palette entries 0..31 over 30 retraces
    /// (about 0.43 s) and leaves the colours as they were; without it, it only switches
    /// the screen. Needs `BMP_DATA` (the fonts).
    #[test]
    fn screen_fade_fades_the_palette_with_the_option() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let steps = parse_script("wait 100000\n").unwrap();
        let mut host = Host::with_script(&data, steps, &data, false).unwrap();
        // Multiples of 4: `palette_load_rgb4` restores 4-bit channels.
        for i in 0..32u8 {
            host.g
                .palette_set_entry(i, (i % 16) * 4, 0x3c - (i % 16) * 4, 0x20);
        }
        let colours = host.g.dac;
        let t0 = host.now_secs();
        host.screen_fade(1).unwrap();
        assert!(host.now_secs() - t0 < 0.01, "no fade without the option");
        host.palette_fade = true;
        let t0 = host.now_secs();
        host.screen_fade(0).unwrap();
        assert!(host.g.blanked);
        let t1 = host.now_secs();
        assert!(
            (0.42..0.45).contains(&(t1 - t0)),
            "fade out took {}",
            t1 - t0
        );
        assert_eq!(host.g.dac, colours);
        host.screen_fade(1).unwrap();
        assert!(!host.g.blanked);
        assert!((0.42..0.45).contains(&(host.now_secs() - t1)));
        assert_eq!(host.g.dac, colours);
    }

    #[test]
    fn display_layer_switch() {
        // (mod2026, window, lowres)
        assert!(
            display_layer_on(true, true, false),
            "mod data in the SDL host"
        );
        assert!(
            !display_layer_on(false, true, false),
            "original data in the SDL host"
        );
        assert!(
            !display_layer_on(false, false, false),
            "original data headless"
        );
        assert!(!display_layer_on(true, false, false), "mod data headless");
        assert!(
            !display_layer_on(true, true, true),
            "mod data with --lowres"
        );
    }

    /// Needs `BMP_DATA`.
    #[test]
    fn headless_host_keeps_the_display_off() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let host = Host::with_script(&data, Vec::new(), &data, false).unwrap();
        assert!(!host.mod2026);
        assert!(host.g.display().is_none());
        let mod_data =
            std::env::temp_dir().join(format!("bmp-headless-mod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mod_data);
        bmp_game::mod2026::build(&data, &mod_data).unwrap();
        let host = Host::with_script(&mod_data, Vec::new(), &mod_data, false).unwrap();
        let _ = std::fs::remove_dir_all(&mod_data);
        assert!(host.mod2026);
        assert!(host.g.display().is_none());
    }

    /// Selects an SDL video driver that never opens a window on the desktop (#210). The
    /// hint overrides any `SDL_VIDEODRIVER` of the environment. Returns the driver with its
    /// video subsystem, or `None` when neither `offscreen` nor `dummy` initialises on this
    /// machine. Keep the subsystem alive while windows are opened: when the last SDL context
    /// goes, `SDL_Quit` clears the hint.
    fn headless_video_driver() -> Option<(&'static str, sdl2::VideoSubsystem)> {
        ["offscreen", "dummy"].into_iter().find_map(|driver| {
            sdl2::hint::set_with_priority("SDL_VIDEODRIVER", driver, &sdl2::hint::Hint::Override);
            let video = sdl2::init().and_then(|sdl| sdl.video()).ok()?;
            (video.current_video_driver() == driver).then_some((driver, video))
        })
    }

    /// Opens `Host::with_window` on `data` and pumps until one frame is presented; returns
    /// whether the display layer is on, whether it has the 2x fonts and the texture size of
    /// that frame.
    fn present_one_window_frame(
        data: &Path,
        lowres: bool,
        hires_fonts: bool,
        driver: &str,
    ) -> (bool, bool, (usize, usize)) {
        let mut host = Host::with_window(data, 1, false, lowres, hires_fonts).unwrap();
        let Backend::Window { win, .. } = &host.backend else {
            unreachable!("with_window builds the window backend");
        };
        let video = win.video_driver();
        assert_eq!(video, driver, "the window must use the headless driver");
        let layer = host.g.display().is_some();
        let fonts = host.g.hires_fonts_on();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            host.pump().unwrap();
            let Backend::Window { win, .. } = &host.backend else {
                unreachable!("with_window builds the window backend");
            };
            if let Some(&size) = win.presented.first() {
                return (layer, fonts, size);
            }
            assert!(Instant::now() < deadline, "no frame presented within 5 s");
        }
    }

    /// The live SDL window path (#210): `Host::with_window` with the `--lowres` flag and a
    /// presented frame, under an offscreen SDL video driver. With Mod 2026 data the frame goes
    /// through `Window::present_display` (the 640-wide texture), with `--lowres` and with the
    /// original data through the 320x240 `Window::present`. Needs `BMP_DATA`; skips when no
    /// headless SDL video driver is available. All cases run in this one test: SDL must stay
    /// on one thread.
    #[test]
    fn window_host_presents_the_display_layer_with_mod_data_only() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let Some((driver, _video)) = headless_video_driver() else {
            eprintln!(
                "skipping window_host_presents_the_display_layer_with_mod_data_only: \
                 no headless SDL video driver (offscreen, dummy) is available"
            );
            return;
        };
        eprintln!("SDL video driver: {driver}");
        let mod_data = std::env::temp_dir().join(format!("bmp-window-mod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mod_data);
        bmp_game::mod2026::build(&data, &mod_data).unwrap();
        let mod_2x = present_one_window_frame(&mod_data, false, false, driver);
        let mod_hires_fonts = present_one_window_frame(&mod_data, false, true, driver);
        let mod_lowres = present_one_window_frame(&mod_data, true, false, driver);
        let _ = std::fs::remove_dir_all(&mod_data);
        let original = present_one_window_frame(&data, false, true, driver);
        assert_eq!(mod_2x, (true, false, (640, 480)), "Mod 2026 data");
        assert_eq!(
            mod_hires_fonts,
            (true, true, (640, 480)),
            "Mod 2026 data with --hires-fonts"
        );
        assert_eq!(
            mod_lowres,
            (false, false, (320, 240)),
            "Mod 2026 data with --lowres"
        );
        assert_eq!(original, (false, false, (320, 240)), "original data");
    }

    #[test]
    fn script_errors_name_the_line() {
        let err = parse_script("wait 1\nclick 1\n").unwrap_err();
        assert!(err.starts_with("script line 2"), "{err}");
        assert!(parse_script("jump 1 2").is_err());
    }
}
