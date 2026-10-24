//! Emulated VGA output: an indexed framebuffer plus a 256-entry 6-bit DAC palette.
//!
//! The original game uses mode 13h (320x200) and an unchained 320x240 mode ("mode X")
//! for full-screen pictures such as the title screen. Both are modelled as a linear
//! indexed buffer; only the visible height differs.

use std::path::Path;

pub const WIDTH: usize = 320;
pub const MAX_HEIGHT: usize = 240;

/// Video modes used by the original game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Mode 13h, 320x200, 256 colors.
    Mode13h,
    /// Unchained 320x240, 256 colors.
    ModeX,
}

impl Mode {
    pub fn height(self) -> usize {
        match self {
            Mode::Mode13h => 200,
            Mode::ModeX => 240,
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
        let h = self.height();
        let mut out = Vec::with_capacity(WIDTH * h * 3);
        for &idx in &self.pixels[..WIDTH * h] {
            let [r, g, b] = self.dac[idx as usize];
            out.extend_from_slice(&[
                Self::dac_to_rgb8(r),
                Self::dac_to_rgb8(g),
                Self::dac_to_rgb8(b),
            ]);
        }
        out
    }

    /// Writes the visible area as an RGB PNG at native resolution.
    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(
            std::io::BufWriter::new(file),
            WIDTH as u32,
            self.height() as u32,
        );
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&self.to_rgb24())
            .map_err(std::io::Error::other)?;
        writer.finish().map_err(std::io::Error::other)?;
        Ok(())
    }
}

/// Window presenting a [`Vga`] with integer scaling and 4:3 aspect correction.
pub struct Window {
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,
    pub events: sdl2::EventPump,
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
        })
    }

    pub fn present(&mut self, vga: &Vga) -> Result<(), String> {
        let h = vga.height() as u32;
        let mut tex = self
            .creator
            .create_texture_streaming(sdl2::pixels::PixelFormatEnum::RGB24, WIDTH as u32, h)
            .map_err(|e| e.to_string())?;
        tex.update(None, &vga.to_rgb24(), WIDTH * 3)
            .map_err(|e| e.to_string())?;
        // Both modes are displayed on a 4:3 monitor, so stretch to the full window.
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
