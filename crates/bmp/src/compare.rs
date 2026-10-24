//! Port frame vs. DOSBox reference frame comparison; the single implementation of
//! the tolerance rules in `docs/re/TOLERANCES.md`.

use std::fmt;
use std::path::Path;

/// A pixel matches when every 8-bit RGB channel differs by at most this much.
pub const CHANNEL_TOLERANCE: u8 = 4;

/// Default acceptance threshold in percent of matching pixels (phase 1).
pub const DEFAULT_THRESHOLD: f64 = 90.0;

const MISMATCH_COLOR: [u8; 3] = [255, 0, 255];

/// An 8-bit RGB image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl RgbImage {
    /// Decodes a PNG of any color type and bit depth to 8-bit RGB like PIL's
    /// `convert("RGB")`: palette and grayscale images are expanded, alpha is
    /// dropped (not composited) and 16-bit channels are reduced to their high
    /// byte, except 16-bit grayscale, which PIL reads as integers and clips to 255.
    pub fn load_png(path: &Path) -> Result<Self, String> {
        let err = |e: &dyn fmt::Display| format!("{}: {e}", path.display());
        let file = std::fs::File::open(path).map_err(|e| err(&e))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| err(&e))?;
        let source = reader.info();
        let clip_gray16 = (source.color_type, source.bit_depth)
            == (png::ColorType::Grayscale, png::BitDepth::Sixteen);
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| err(&e))?;
        let data = &buf[..info.buffer_size()];
        let sample_bytes = match info.bit_depth {
            png::BitDepth::Sixteen => 2,
            _ => 1,
        };
        let to_u8: fn(&[u8]) -> u8 = if clip_gray16 {
            |s| u16::from_be_bytes([s[0], s[1]]).min(255) as u8
        } else {
            |s| s[0]
        };
        let rgb_channels = match info.color_type {
            png::ColorType::Rgb | png::ColorType::Rgba => [0, 1, 2],
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => [0; 3],
            png::ColorType::Indexed => return Err(err(&"palette was not expanded")),
        };
        let pixels = data
            .chunks_exact(info.color_type.samples() * sample_bytes)
            .map(|px| rgb_channels.map(|c| to_u8(&px[c * sample_bytes..])))
            .collect();
        Ok(Self {
            width: info.width,
            height: info.height,
            pixels,
        })
    }

    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        crate::video::write_rgb_png(path, self.width, self.height, self.pixels.as_flattened())
    }
}

/// The two images have different dimensions and cannot be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeMismatch {
    pub port: (u32, u32),
    pub reference: (u32, u32),
}

impl fmt::Display for SizeMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "size mismatch: port {}x{}, reference {}x{}",
            self.port.0, self.port.1, self.reference.0, self.reference.1
        )
    }
}

/// Per-pixel match result of a port frame against a reference frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    matches: Vec<bool>,
}

impl Comparison {
    pub fn total(&self) -> usize {
        self.matches.len()
    }

    pub fn differing(&self) -> usize {
        self.matches.iter().filter(|&&m| !m).count()
    }

    /// Share of matching pixels in percent; NaN for an empty image, which
    /// therefore never passes a threshold.
    pub fn share(&self) -> f64 {
        let matching = self.total() - self.differing();
        100.0 * (matching as f64 / self.total() as f64)
    }

    pub fn passes(&self, threshold: f64) -> bool {
        self.share() >= threshold
    }

    /// The reference darkened to a third with every differing pixel in magenta.
    pub fn diff_image(&self, reference: &RgbImage) -> RgbImage {
        let pixels = reference
            .pixels
            .iter()
            .zip(&self.matches)
            .map(|(px, &ok)| {
                if ok {
                    px.map(|c| c / 3)
                } else {
                    MISMATCH_COLOR
                }
            })
            .collect();
        RgbImage {
            pixels,
            ..*reference
        }
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "match {:.2}% ({} of {} pixels differ)",
            self.share(),
            self.differing(),
            self.total()
        )
    }
}

pub fn compare(port: &RgbImage, reference: &RgbImage) -> Result<Comparison, SizeMismatch> {
    if (port.width, port.height) != (reference.width, reference.height) {
        return Err(SizeMismatch {
            port: (port.width, port.height),
            reference: (reference.width, reference.height),
        });
    }
    let matches = port
        .pixels
        .iter()
        .zip(&reference.pixels)
        .map(|(p, r)| {
            p.iter()
                .zip(r)
                .all(|(a, b)| a.abs_diff(*b) <= CHANNEL_TOLERANCE)
        })
        .collect();
    Ok(Comparison { matches })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, px: [u8; 3]) -> RgbImage {
        RgbImage {
            width,
            height,
            pixels: vec![px; (width * height) as usize],
        }
    }

    #[test]
    fn identical_images_match_fully() {
        let img = solid(4, 3, [10, 20, 30]);
        let c = compare(&img, &img).unwrap();
        assert_eq!(c.differing(), 0);
        assert!(c.passes(100.0));
        assert_eq!(c.to_string(), "match 100.00% (0 of 12 pixels differ)");
    }

    #[test]
    fn channel_off_by_tolerance_matches() {
        let reference = solid(2, 2, [100, 100, 100]);
        let mut port = reference.clone();
        port.pixels[0] = [104, 100, 100];
        port.pixels[1] = [100, 96, 100];
        assert_eq!(compare(&port, &reference).unwrap().differing(), 0);
    }

    #[test]
    fn channel_off_by_more_than_tolerance_differs() {
        let reference = solid(2, 2, [100, 100, 100]);
        let mut port = reference.clone();
        port.pixels[3] = [100, 100, 95];
        let c = compare(&port, &reference).unwrap();
        assert_eq!(c.differing(), 1);
        assert_eq!(c.to_string(), "match 75.00% (1 of 4 pixels differ)");
        assert!(!c.passes(DEFAULT_THRESHOLD));
        assert!(c.passes(75.0));
    }

    #[test]
    fn size_mismatch_is_reported() {
        let err = compare(&solid(320, 200, [0; 3]), &solid(320, 240, [0; 3])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "size mismatch: port 320x200, reference 320x240"
        );
    }

    #[test]
    fn diff_image_darkens_reference_and_marks_mismatches() {
        let reference = solid(2, 1, [200, 100, 5]);
        let mut port = reference.clone();
        port.pixels[1] = [0, 0, 0];
        let diff = compare(&port, &reference).unwrap().diff_image(&reference);
        assert_eq!(diff.pixels, vec![[66, 33, 1], MISMATCH_COLOR]);
        assert_eq!((diff.width, diff.height), (2, 1));
    }

    /// Writes a 2x1 PNG with the given format; `palette` adds a PLTE chunk.
    fn write_png(
        path: &Path,
        color: png::ColorType,
        depth: png::BitDepth,
        palette: Option<&[u8]>,
        data: &[u8],
    ) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(file, 2, 1);
        enc.set_color(color);
        enc.set_depth(depth);
        if let Some(plte) = palette {
            enc.set_palette(plte.to_vec());
        }
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bmp-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn png_color_types_decode_like_pil() {
        use png::{BitDepth::*, ColorType::*};
        let dir = temp_dir("compare-decode");
        let path = dir.join("img.png");
        let img = RgbImage {
            width: 2,
            height: 1,
            pixels: vec![[1, 2, 3], [250, 128, 0]],
        };
        img.save_png(&path).unwrap();
        assert_eq!(RgbImage::load_png(&path).unwrap(), img);

        let decode = |color, depth, palette: Option<&[u8]>, data: &[u8]| {
            write_png(&path, color, depth, palette, data);
            RgbImage::load_png(&path).unwrap().pixels
        };
        let rgba = [1, 2, 3, 0, 250, 128, 0, 77];
        assert_eq!(decode(Rgba, Eight, None, &rgba), img.pixels);
        let plte = [1, 2, 3, 250, 128, 0];
        assert_eq!(
            decode(Indexed, Eight, Some(&plte), &[1, 0]),
            [[250, 128, 0], [1, 2, 3]]
        );
        assert_eq!(
            decode(Grayscale, Eight, None, &[7, 200]),
            [[7; 3], [200; 3]]
        );
        assert_eq!(
            decode(GrayscaleAlpha, Eight, None, &[7, 0, 200, 9]),
            [[7; 3], [200; 3]]
        );
        // 16-bit channels keep their high byte, except plain 16-bit grayscale,
        // which PIL clips to 255.
        let rgb16 = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 0, 0, 0xff, 0xff, 0];
        assert_eq!(
            decode(Rgb, Sixteen, None, &rgb16),
            [[0x12, 0x56, 0x9a], [0, 0, 0xff]]
        );
        let la16 = [0x12, 0x34, 0xff, 0xff, 0x00, 0x80, 0, 0];
        assert_eq!(
            decode(GrayscaleAlpha, Sixteen, None, &la16),
            [[0x12; 3], [0; 3]]
        );
        let gray16 = [0x12, 0x34, 0x00, 0x80];
        assert_eq!(
            decode(Grayscale, Sixteen, None, &gray16),
            [[255; 3], [0x80; 3]]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
