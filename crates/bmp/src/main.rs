mod video;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmp_formats::{cp, pal, vga};
use video::{Mode, Vga, Window};

const USAGE: &str = "usage: bmp [--data <dir>] [--version]
       bmp [--data <dir>] show-pic <name> [--palette <0..2>] [--screenshot <out.png>]
  <name> is a file below PIC/, e.g. 47.CP or 120.VGA";

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

fn run() -> Result<(), String> {
    let args = parse_args()?;
    if args.rest.iter().any(|a| a == "--version") {
        println!("bmp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    match args.rest.first().map(String::as_str) {
        Some("show-pic") => show_pic(&args),
        Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command '{other}'\n{USAGE}")),
        None => Err(USAGE.to_string()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
