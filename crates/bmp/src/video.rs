//! Emulated VGA output: an indexed framebuffer plus a 256-entry 6-bit DAC palette.
//!
//! The original game uses mode 13h (320x200) and an unchained 320x240 mode ("mode X")
//! for full-screen pictures such as the title screen. Both are modelled as a linear
//! indexed buffer; only the visible height differs.

use std::path::Path;

use crate::display::{DISPLAY_W, DisplayFrame};

pub const WIDTH: usize = 320;
pub const MAX_HEIGHT: usize = 242;

/// Video modes used by the original game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Mode 13h, 320x200, 256 colors.
    Mode13h,
    /// Unchained 320x240, 256 colors.
    ModeX,
    /// The team editor's unchained mode: TEAMED.EXE's CRTC table (`16b7:039E`) shows
    /// 242 lines (vertical display end `0xE3`).
    ModeX242,
}

impl Mode {
    pub fn height(self) -> usize {
        match self {
            Mode::Mode13h => 200,
            Mode::ModeX => 240,
            Mode::ModeX242 => 242,
        }
    }
}

/// Indexed framebuffer with a 6-bit-per-channel VGA DAC palette.
pub struct Vga {
    pub mode: Mode,
    pub pixels: Vec<u8>,
    /// DAC entries, 6 bits per channel (0..=63), as programmed through ports 0x3C8/0x3C9.
    pub dac: [[u8; 3]; 256],
}

impl Default for Vga {
    fn default() -> Self {
        Self::new()
    }
}

impl Vga {
    pub fn new() -> Self {
        Self {
            mode: Mode::Mode13h,
            pixels: vec![0; WIDTH * MAX_HEIGHT],
            dac: [[0; 3]; 256],
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.pixels.fill(0);
    }

    pub fn height(&self) -> usize {
        self.mode.height()
    }

    /// Converts a 6-bit DAC value to 8 bits the same way DOSBox does.
    pub fn dac_to_rgb8(v: u8) -> u8 {
        let v = v & 0x3f;
        (v << 2) | (v >> 4)
    }

    /// Renders the visible area to packed RGB24.
    pub fn to_rgb24(&self) -> Vec<u8> {
        rgb24(&self.pixels[..WIDTH * self.height()], &self.dac)
    }

    /// Writes the visible area as an RGB PNG at native resolution.
    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        write_rgb_png(path, WIDTH as u32, self.height() as u32, &self.to_rgb24())
    }
}

/// Expands palette indices to packed RGB24 through `dac`.
pub fn rgb24(pixels: &[u8], dac: &[[u8; 3]; 256]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|&idx| dac[idx as usize].map(Vga::dac_to_rgb8))
        .collect()
}

/// Writes packed 8-bit RGB24 pixels, rows top to bottom, as a PNG.
pub fn write_rgb_png(path: &Path, width: u32, height: u32, rgb24: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(rgb24)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}

/// Window presenting a [`Vga`] (or the Mod 2026 [`DisplayFrame`]) with integer scaling
/// and 4:3 aspect correction.
pub struct Window {
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,
    pub events: sdl2::EventPump,
    mouse: sdl2::mouse::MouseUtil,
    sdl: sdl2::Sdl,
}

impl Window {
    pub fn open(scale: u32) -> Result<Self, String> {
        let sdl = sdl2::init()?;
        let video = sdl.video()?;
        let window = video
            .window("Bundesliga Manager Professional", 320 * scale, 240 * scale)
            .position_centered()
            .resizable()
            .build()
            .map_err(|e| e.to_string())?;
        let canvas = window
            .into_canvas()
            .present_vsync()
            .build()
            .map_err(|e| e.to_string())?;
        let creator = canvas.texture_creator();
        let events = sdl.event_pump()?;
        Ok(Self {
            canvas,
            creator,
            events,
            mouse: sdl.mouse(),
            sdl,
        })
    }

    /// The SDL audio subsystem of the window's SDL context.
    pub fn audio(&self) -> Result<sdl2::AudioSubsystem, String> {
        self.sdl.audio()
    }

    /// Hides the host cursor; the game draws its own pointer into VRAM.
    pub fn hide_cursor(&self) {
        self.mouse.show_cursor(false);
    }

    pub fn present(&mut self, vga: &Vga) -> Result<(), String> {
        self.present_rgb(WIDTH, vga.height(), &vga.to_rgb24())
    }

    /// MOD2026: presents the 2x display frame in place of the logical frame.
    pub fn present_display(&mut self, frame: &DisplayFrame) -> Result<(), String> {
        self.present_rgb(DISPLAY_W, frame.height, &frame.to_rgb24())
    }

    fn present_rgb(&mut self, width: usize, height: usize, rgb: &[u8]) -> Result<(), String> {
        let mut tex = self
            .creator
            .create_texture_streaming(
                sdl2::pixels::PixelFormatEnum::RGB24,
                width as u32,
                height as u32,
            )
            .map_err(|e| e.to_string())?;
        tex.update(None, rgb, width * 3)
            .map_err(|e| e.to_string())?;
        // Every mode is displayed on a 4:3 monitor, so stretch to the full window. The
        // logical size stays 320x240 for any texture: mouse events arrive in it.
        self.canvas
            .set_logical_size(320, 240)
            .map_err(|e| e.to_string())?;
        self.canvas.clear();
        self.canvas.copy(&tex, None, None)?;
        self.canvas.present();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dac_conversion_matches_dosbox() {
        assert_eq!(Vga::dac_to_rgb8(0), 0);
        assert_eq!(Vga::dac_to_rgb8(63), 255);
        assert_eq!(Vga::dac_to_rgb8(32), 130);
    }

    #[test]
    fn rgb_buffer_has_visible_size() {
        let mut vga = Vga::new();
        assert_eq!(vga.to_rgb24().len(), 320 * 200 * 3);
        vga.set_mode(Mode::ModeX);
        assert_eq!(vga.to_rgb24().len(), 320 * 240 * 3);
    }
}
