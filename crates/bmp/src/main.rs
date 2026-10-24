mod audio;
mod compare;
mod game;
mod gfx;
mod host;
mod intro;
mod teamed;
mod video;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmp_formats::{cp, pal, vga};
use video::{Mode, Vga, Window};

const USAGE: &str = "usage: bmp [--data <dir>] [--version]
       bmp [--data <dir>] show-pic <name> [--palette <0..2>] [--screenshot <out.png>]
       bmp [--data <dir>] run [--script <file> --shots <dir>] [--time <seconds>] [--time2 <seconds>]
                              [--nosound] [--skip-intro] [0]
       bmp [--data <dir>] teamed [--script <file> --shots <dir>]
       bmp [--data <dir>] render-music <out.wav> [--tail <ms>] [--opl-log <out.txt>]
       bmp [--data <dir>] play-sound music|1..5
       bmp compare <port.png> <reference.png> [--diff <out.png>] [--threshold <percent>]
  <name> is a file below PIC/, e.g. 47.CP or 120.VGA
  run starts the game like BMP.BAT: the Software 2000 logo intro (BMINTRO, ended by
  any key; --skip-intro leaves it out), then the title; with --script it runs headless
  on a virtual clock
  (see host::parse_script); --time fixes the time of the first RNG seed and --time2
  that of the second one, read when a new game starts (default: --time when given,
  else the clock at that moment); --nosound or a trailing 0 (BMP.BAT's argument)
  disable sound like `bmmain 0` (no title music: the title is shown only briefly)
  teamed starts the team editor (TEAMED.EXE), which edits MANA.DAT in the data
  directory; with --script it runs headless like run
  render-music writes the title tune (SOUND/BM2TITLE.CMF) as rendered by the OPL2
  emulation to a 16-bit mono WAV file at 49716 Hz, plus <ms> (default 1000) of release;
  --opl-log lists the OPL register writes as \"<ms> <register> <value>\" lines (hex)
  play-sound plays the title tune or digitised voice 1..5 (1 crowd, 2 goal, 3 whistle,
  4 injury, 5 miss) on the default audio device
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

/// Starts the game in a window, or headless driven by an input script.
fn run_game(args: &Args) -> Result<(), String> {
    let mut rest = args.rest[1..].to_vec();
    let script = take_opt(&mut rest, "--script")?;
    let shots = take_opt(&mut rest, "--shots")?.unwrap_or_else(|| ".".into());
    let parse = |flag: &str, v: Option<String>| -> Result<Option<i32>, String> {
        v.map(|t| t.parse::<i32>().map_err(|e| format!("{flag}: {e}")))
            .transpose()
    };
    let time = parse("--time", take_opt(&mut rest, "--time")?)?;
    let time2 = parse("--time2", take_opt(&mut rest, "--time2")?)?.or(time);
    let time = time.unwrap_or_else(game::time_now);
    let skip_intro = take_flag(&mut rest, "--skip-intro");
    let sound = sound_enabled(&mut rest);
    if let Some(extra) = rest.first() {
        return Err(format!("unexpected argument '{extra}'\n{USAGE}"));
    }
    let dir = data_dir(args)?;
    let mut host = match script {
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            std::fs::create_dir_all(&shots).map_err(|e| format!("{shots}: {e}"))?;
            host::Host::with_script(&dir, host::parse_script(&text)?, Path::new(&shots), sound)?
        }
        None => host::Host::with_window(&dir, 3, sound)?,
    };
    // BMP.BAT: `bmintro`, then `bmmain %1`.
    if !skip_intro && intro::run(&mut host)?.is_err() {
        return Ok(());
    }
    match game::run(&mut host, time, time2)? {
        Ok(game::Exit::GameOver) => {}
        // `fatal_error_exit` ends the original with `exit(0)` after its error screen.
        Ok(game::Exit::Fatal) => {}
        Err(host::Quit) => {}
    }
    Ok(())
}

/// Starts the team editor in a window, or headless driven by an input script.
fn run_teamed(args: &Args) -> Result<(), String> {
    let mut rest = args.rest[1..].to_vec();
    let script = take_opt(&mut rest, "--script")?;
    let shots = take_opt(&mut rest, "--shots")?.unwrap_or_else(|| ".".into());
    if let Some(extra) = rest.first() {
        return Err(format!("unexpected argument '{extra}'\n{USAGE}"));
    }
    let dir = data_dir(args)?;
    // TEAMED has no sound.
    let mut host = match script {
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            std::fs::create_dir_all(&shots).map_err(|e| format!("{shots}: {e}"))?;
            host::Host::with_script(&dir, host::parse_script(&text)?, Path::new(&shots), false)?
        }
        None => host::Host::with_window(&dir, 3, false)?,
    };
    match teamed::run(&mut host)? {
        Ok(teamed::Exit::Ended) | Err(host::Quit) => Ok(()),
    }
}

/// Takes `flag` out of `rest`; whether it was present.
fn take_flag(rest: &mut Vec<String>, flag: &str) -> bool {
    let found = rest.iter().position(|a| a == flag);
    if let Some(i) = found {
        rest.remove(i);
    }
    found.is_some()
}

/// Takes the sound switches out of the arguments after the command: `--nosound`, and
/// the program argument that `BMP.BAT` passes through to `bmmain`; `main`
/// (18bc:0418) clears the sound flag `DS:018c` when it is `0`.
fn sound_enabled(rest: &mut Vec<String>) -> bool {
    let mut sound = true;
    if take_flag(rest, "--nosound") {
        sound = false;
    }
    if let Some(i) = rest.iter().position(|a| !a.starts_with("--")) {
        sound &= rest.remove(i) != "0";
    }
    sound
}

fn load_title_music(dir: &Path) -> Result<bmp_formats::cmf::Cmf, String> {
    let path = dir.join("SOUND").join("BM2TITLE.CMF");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    bmp_formats::cmf::Cmf::parse(&bytes).map_err(|e| e.to_string())
}

/// Renders the title tune offline to a WAV file at the OPL2's native rate.
fn render_music(args: &Args) -> Result<(), String> {
    let mut rest = args.rest[1..].to_vec();
    let tail_ms = take_opt(&mut rest, "--tail")?
        .map(|t| t.parse::<u64>().map_err(|e| format!("--tail: {e}")))
        .transpose()?
        .unwrap_or(1000);
    let log = take_opt(&mut rest, "--opl-log")?;
    let [out] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let cmf = load_title_music(&data_dir(args)?)?;
    let rate = bmp_sound::opl::NATIVE_RATE;
    let (samples, writes) = bmp_sound::music::Music::render_all(
        &cmf,
        (tail_ms as f64 * rate / 1000.0) as usize,
        log.is_some(),
    );
    if let Some(path) = log {
        let text: String = writes
            .iter()
            .map(|(n, r, v)| format!("{:.3} {r:02x} {v:02x}\n", *n as f64 * 1000.0 / rate))
            .collect();
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    audio::write_wav(Path::new(out), rate.round() as u32, &samples)
        .map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: {:.3} s ({} samples at {rate:.1} Hz, {tail_ms} ms release included)",
        samples.len() as f64 / rate,
        samples.len()
    );
    Ok(())
}

/// Plays the title tune or one digitised voice on the default audio device.
fn play_sound(args: &Args) -> Result<(), String> {
    let what = args.rest.get(1).ok_or(USAGE)?;
    let dir = data_dir(args)?;
    let sdl = sdl2::init()?;
    let card = audio::SoundCard::open(&sdl.audio()?)?;
    if what == "music" {
        card.mixer().music = Some(bmp_sound::music::Music::start(&load_title_music(&dir)?));
    } else {
        let n: u8 = what.parse().map_err(|_| USAGE.to_string())?;
        if !(1..=5).contains(&n) {
            return Err(USAGE.to_string());
        }
        let path = dir.join("SOUND").join("DIGI.VOC");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut voices = bmp_sound::voice::Voices::from_digi(&bytes).map_err(|e| e.to_string())?;
        voices.play(n);
        card.mixer().voices = Some(voices);
    }
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut m = card.mixer();
        let music = m.music.as_mut().is_some_and(|m| m.poll_status() != 0);
        let voice = m.voices.as_ref().is_some_and(|v| v.busy());
        // The crowd loops for hours; stop it after one pass.
        if !music && (!voice || (what == "1" && start.elapsed().as_secs_f64() > 2.904)) {
            return Ok(());
        }
    }
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
        Some("run") => run_game(&args).map(|()| ExitCode::SUCCESS),
        Some("teamed") => run_teamed(&args).map(|()| ExitCode::SUCCESS),
        Some("render-music") => render_music(&args).map(|()| ExitCode::SUCCESS),
        Some("play-sound") => play_sound(&args).map(|()| ExitCode::SUCCESS),
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

    #[test]
    fn sound_switches() {
        let v = |a: &[&str]| {
            let mut rest: Vec<String> = a.iter().map(|s| s.to_string()).collect();
            (sound_enabled(&mut rest), rest)
        };
        let none: Vec<String> = Vec::new();
        assert_eq!(v(&[]), (true, none.clone()));
        assert_eq!(v(&["0"]), (false, none.clone()));
        assert_eq!(v(&["--nosound"]), (false, none.clone()));
        assert_eq!(v(&["1"]), (true, none));
    }

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
