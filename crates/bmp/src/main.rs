mod compare;
mod video;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmp_formats::{cp, pal, vga};
use video::{Mode, Vga, Window};

const USAGE: &str = "usage: bmp [--data <dir>] [--version]
       bmp [--data <dir>] show-pic <name> [--palette <0..2>] [--screenshot <out.png>]
       bmp compare <port.png> <reference.png> [--diff <out.png>] [--threshold <percent>]
  <name> is a file below PIC/, e.g. 47.CP or 120.VGA
  compare exits 0 when at least <percent> (default 90) of the pixels match, 1 otherwise";

struct Args {
    data: Option<PathBuf>,
    rest: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut data = None;
    let mut rest = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = Some(PathBuf::from(it.next().ok_or("--data needs a value")?)),
            _ => rest.push(a),
        }
    }
    Ok(Args { data, rest })
}

fn data_dir(args: &Args) -> Result<PathBuf, String> {
    args.data
        .clone()
        .or_else(bmp_formats::data_dir)
        .ok_or_else(|| "no game data directory: pass --data <dir> or set BMP_DATA".to_string())
}

/// Takes the value following `flag` out of `rest`, if present.
fn take_opt(rest: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    match rest.iter().position(|a| a == flag) {
        Some(i) if i + 1 < rest.len() => {
            let v = rest.remove(i + 1);
            rest.remove(i);
            Ok(Some(v))
        }
        Some(_) => Err(format!("{flag} needs a value")),
        None => Ok(None),
    }
}

/// Loads `PIC/<name>` into the framebuffer: `.CP` files are full 320x240 mode X
/// screens, `.VGA` sprites are drawn at the top-left corner of a mode 13h screen.
fn load_picture(vga_out: &mut Vga, dir: &Path, name: &str, palette: usize) -> Result<(), String> {
    let path = dir.join("PIC").join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pals = pal::PalFile::load(dir).map_err(|e| e.to_string())?;
    vga_out.dac = pals.vram_palette(palette).entries;
    if name.to_ascii_uppercase().ends_with(".CP") {
        let img = cp::decode(&bytes).map_err(|e| e.to_string())?;
        vga_out.set_mode(Mode::ModeX);
        vga_out.pixels[..img.pixels.len()].copy_from_slice(&img.pixels);
    } else {
        let img = vga::parse(&bytes).map_err(|e| e.to_string())?;
        vga_out.set_mode(Mode::Mode13h);
        let w = usize::from(img.width).min(video::WIDTH);
        let h = usize::from(img.height).min(vga_out.height());
        for y in 0..h {
            let src = &img.pixels[y * usize::from(img.width)..][..w];
            vga_out.pixels[y * video::WIDTH..][..w].copy_from_slice(src);
        }
    }
    Ok(())
}

fn show_pic(args: &Args) -> Result<(), String> {
    let mut rest = args.rest[1..].to_vec();
    let palette = take_opt(&mut rest, "--palette")?
        .map(|p| p.parse::<usize>().map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or(0);
    if palette > 2 {
        return Err("--palette must be 0, 1 or 2".into());
    }
    let shot = take_opt(&mut rest, "--screenshot")?;
    let name = rest.first().ok_or(USAGE)?;
    let mut vga_out = Vga::new();
    load_picture(&mut vga_out, &data_dir(args)?, name, palette)?;
    if let Some(out) = shot {
        return vga_out.save_png(Path::new(&out)).map_err(|e| e.to_string());
    }
    let mut win = Window::open(3)?;
    loop {
        for ev in win.events.poll_iter() {
            use sdl2::event::Event;
            use sdl2::keyboard::Keycode;
            match ev {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(()),
                _ => {}
            }
        }
        win.present(&vga_out)?;
    }
}

/// Prints the match share of a port frame against a reference frame and
/// fails the comparison (exit 1) below the threshold or on a size mismatch.
fn compare_frames(args: &Args) -> Result<ExitCode, String> {
    let mut rest = args.rest[1..].to_vec();
    let diff = take_opt(&mut rest, "--diff")?;
    let threshold = take_opt(&mut rest, "--threshold")?
        .map(|t| t.parse::<f64>().map_err(|e| format!("--threshold: {e}")))
        .transpose()?
        .unwrap_or(compare::DEFAULT_THRESHOLD);
    let [port, reference] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let port = compare::RgbImage::load_png(Path::new(port))?;
    let reference = compare::RgbImage::load_png(Path::new(reference))?;
    let result = match compare::compare(&port, &reference) {
        Ok(result) => result,
        Err(mismatch) => {
            println!("{mismatch}");
            return Ok(ExitCode::FAILURE);
        }
    };
    println!("{result}");
    if let Some(out) = diff {
        result
            .diff_image(&reference)
            .save_png(Path::new(&out))
            .map_err(|e| format!("{out}: {e}"))?;
    }
    Ok(if result.passes(threshold) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn run() -> Result<ExitCode, String> {
    let args = parse_args()?;
    if args.rest.iter().any(|a| a == "--version") {
        println!("bmp {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    match args.rest.first().map(String::as_str) {
        Some("show-pic") => show_pic(&args).map(|()| ExitCode::SUCCESS),
        Some("compare") => compare_frames(&args),
        Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!("unknown command '{other}'\n{USAGE}")),
        None => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compare::RgbImage;

    fn compare_args(rest: &[&str]) -> Args {
        Args {
            data: None,
            rest: std::iter::once("compare")
                .chain(rest.iter().copied())
                .map(String::from)
                .collect(),
        }
    }

    #[test]
    fn compare_frames_maps_results_to_exit_codes() {
        let dir = std::env::temp_dir().join(format!("bmp-compare-exit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = |name: &str, width: u32, pixels: Vec<[u8; 3]>| {
            let path = dir.join(name);
            let height = pixels.len() as u32 / width;
            RgbImage {
                width,
                height,
                pixels,
            }
            .save_png(&path)
            .unwrap();
            path.to_str().unwrap().to_string()
        };
        let reference = save("ref.png", 2, vec![[100; 3]; 4]);
        let one_off = save("one-off.png", 2, vec![[100; 3], [100; 3], [100; 3], [0; 3]]);
        let wide = save("wide.png", 4, vec![[100; 3]; 4]);
        let diff = dir.join("diff.png").to_str().unwrap().to_string();
        let missing = dir.join("missing.png").to_str().unwrap().to_string();
        let run = |rest: &[&str]| compare_frames(&compare_args(rest));

        assert_eq!(run(&[&reference, &reference]), Ok(ExitCode::SUCCESS));
        assert_eq!(run(&[&one_off, &reference]), Ok(ExitCode::FAILURE));
        assert_eq!(
            run(&[&one_off, &reference, "--threshold", "75"]),
            Ok(ExitCode::SUCCESS)
        );
        assert_eq!(run(&[&wide, &reference]), Ok(ExitCode::FAILURE));
        assert!(run(&[&missing, &reference]).is_err());
        assert!(run(&[&reference]).is_err());
        assert!(run(&[&reference, &reference, "--threshold", "x"]).is_err());

        assert_eq!(
            run(&[&one_off, &reference, "--diff", &diff]),
            Ok(ExitCode::FAILURE)
        );
        let diff = RgbImage::load_png(Path::new(&diff)).unwrap();
        assert_eq!(diff.pixels, [[33; 3], [33; 3], [33; 3], [255, 0, 255]]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
