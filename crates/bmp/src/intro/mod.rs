//! BMINTRO.EXE: the animated Software 2000 logo that `BMP.BAT` runs before
//! `bmmain` (see docs/re/logic/intro.md).
//!
//! The original switches to mode 13h, programs its own palette and then loops:
//! wait for the vertical retrace, run one animation iteration, check the keyboard.
//! The animation completes after 343 iterations and the final picture then stays
//! until a key is pressed; any key ends the program, which restores text mode.
//! There is no sound and no timer: the vertical retrace is the only clock.

pub mod anim;

use crate::host::{Host, Run};
use crate::video::{Mode, Vga};

/// Vertical refresh of mode 13h: 25.175 MHz / (800 x 449).
pub const REFRESH_HZ: f64 = 25_175_000.0 / (800.0 * 449.0);

/// Waits for the start of the next vertical retrace.
///
/// The original (0000:0be4) polls port 3DAh until bit 3 is set, so it returns at
/// once while the retrace is still active; on the machines of the time one
/// iteration (with its DOS keyboard call) outlasted the short retrace pulse, and
/// the port runs exactly one iteration per refresh.
///
/// Origin: BMINTRO.EXE 0000:0bbb (VGA path 0000:0be4)
pub(crate) fn wait_vretrace(host: &mut Host) -> Run<()> {
    let frame = |host: &Host| (host.now_secs() * REFRESH_HZ).floor() as u64;
    let start = frame(host);
    while frame(host) == start {
        host.pump()?;
    }
    Ok(())
}

fn present(host: &mut Host, intro: &anim::Intro) {
    let vga = host.vga_mut();
    vga.pixels[..anim::WIDTH * anim::HEIGHT].copy_from_slice(intro.screen());
}

/// Runs the intro until a key is pressed. `Ok(Err(Quit))` when the window is
/// closed or the script ends.
///
/// Origin: BMINTRO.EXE 0000:0000 (entry)
pub fn run(host: &mut Host) -> Result<Run<()>, String> {
    let path = host.data.join("BMINTRO.EXE");
    let exe = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ds = anim::data_segment(&exe).map_err(|e| e.to_string())?;
    let palette = bmp_formats::pal::bmintro_palette(&exe).map_err(|e| e.to_string())?;
    // INT 10h AX=0013h, then all 256 DAC entries (0000:0b39, 0000:0aba).
    host.set_raw_video(true);
    {
        let vga = host.vga_mut();
        vga.set_mode(Mode::Mode13h);
        vga.dac = palette.entries;
    }
    let mut intro = anim::Intro::new(ds);
    let result = (|| {
        loop {
            wait_vretrace(host)?;
            intro.iterate();
            present(host, &intro);
            // INT 21h AH=06h DL=FFh: any key ends the intro.
            if host.key_hit() {
                return Ok(());
            }
        }
    })();
    // INT 10h AX=0003h: text mode, an empty black screen until bmmain sets its mode.
    let vga: &mut Vga = host.vga_mut();
    vga.set_mode(Mode::Mode13h);
    vga.dac = [[0; 3]; 256];
    host.set_raw_video(false);
    Ok(result)
}
