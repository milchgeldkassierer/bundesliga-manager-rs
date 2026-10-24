//! The goal scenes of the live match view (`TORE\`, segments 24a4 and 1c7c): with the
//! goal-scene option on, a chance or a goal of the live ticker plays a short animation of
//! the attack in the event panel, with its sounds and up to two slow-motion replays (keys
//! `r`/`w`). The scene file is drawn at random; the file format, the mirroring for the away
//! side and the frame drawing live in [`bmp_formats::tore`]. Spec: `docs/re/tore.md` and
//! `docs/re/logic/season.md` §3.9.

use bmp_formats::tore::{self, Anzahl, Scene, ToreError};
use bmp_game::rng::Rng;

use super::live_match::LiveView;
use crate::gfx::BitmapId;
use crate::host::{Host, Run};

/// `TORE\ANZAHL` and the directory of the scene files.
const TORE_DIR: &str = "TORE";
const ANZAHL: &str = "ANZAHL";
/// The ball marker's first cell id: cells from here on are drawn 4 pixels wide.
const BALL_FIRST_ID: u16 = 0x8e;
/// The open attempts after which the selection counter wraps (it is never used otherwise;
/// the port stops retrying there when no scene file can be read at all).
const MAX_ATTEMPTS: u32 = 50 * 1000;

/// The pictures of the scene player while a scene is shown.
pub struct ScenePics {
    /// `FAR:577c` / `FAR:56ea`: `PIC\26.VGA`, the stadium and pitch, the canvas.
    canvas: BitmapId,
    /// `FAR:4a0` / `FAR:20`: rows 0x23..0x70 of the untouched pitch.
    strip: BitmapId,
    /// `FAR:9332` / `FAR:56e6`: `PIC\27.VGA`, the 12x11 player and ball cells.
    sprites: BitmapId,
    /// `FAR:4bca` / `FAR:2ea4`: `PIC\29.VGA`, the two goal frames.
    goals: BitmapId,
}

/// Frees the live view's sprite and panel pictures and loads the scene player's pictures:
/// the pitch (whose rows 0x23..0x70 are saved as the background strip), the player cells and
/// the goal frames.
///
/// Origin: BMMAIN.EXE 24a4:01f9 (match_screen_load_pictures)
pub fn match_screen_load_pictures(host: &mut Host, live: &mut LiveView) -> Option<ScenePics> {
    live.free_pictures(&mut host.g);
    let canvas = host.load_picture(u16::from(tore::PIC_PITCH)).ok()?;
    let strip = host.blank_bitmap(0x140, tore::STRIP_HEIGHT as i16, 5);
    host.g
        .blit_bitmap(canvas, 0, tore::PITCH_TOP, strip, 0, 0, 0x140, 0x4d);
    let sprites = host.load_picture(u16::from(tore::PIC_SPRITES)).ok()?;
    let goals = host.load_picture(u16::from(tore::PIC_GOALS)).ok()?;
    Some(ScenePics {
        canvas,
        strip,
        sprites,
        goals,
    })
}

/// Frees the scene player's pictures and loads the live view's sprites (`PIC\8.VGA`) and
/// score panel (`PIC\38.VGA`) again.
///
/// Origin: BMMAIN.EXE 24a4:0347 (match_screen_restore_pictures)
pub fn match_screen_restore_pictures(
    host: &mut Host,
    live: &mut LiveView,
    pics: Option<ScenePics>,
) {
    if let Some(p) = pics {
        for id in [p.strip, p.sprites, p.canvas, p.goals] {
            host.g.free(id);
        }
    }
    live.load_pictures(host);
}

/// Reads `TORE\ANZAHL` (the game does it once at program start and stops with fatal error 2
/// when it is missing or wrong; the port shows no scene then).
///
/// Origin: BMMAIN.EXE 24a4:0419 (tore_read_anzahl)
fn tore_read_anzahl(host: &Host) -> Option<Anzahl> {
    let bytes = std::fs::read(host.data.join(TORE_DIR).join(ANZAHL)).ok()?;
    tore::parse_anzahl(&bytes).ok()
}

/// One showing of a goal scene. The first showing (`replay` 0) draws the file: a plain
/// scene `2..=max_scene`, with chance 1/16 (goal) or 1/26 (miss) or when `force_e` is set
/// a penalty ("E") scene, else with chance 1/401 a joke ("J") scene; a file that cannot be
/// opened or read is drawn again. A replay shows the file of `name` again (a new file is
/// drawn when it cannot be read). The scene is
/// mirrored for the away side (`scoring_side` 0) and played at `screen_y`. Returns the
/// penalty-scene flag.
///
/// Origin: BMMAIN.EXE 24a4:05ec (tore_scene_play)
#[allow(clippy::too_many_arguments)]
pub fn tore_scene_play(
    host: &mut Host,
    pics: &ScenePics,
    rng: &mut Rng,
    counts: &Anzahl,
    goal: bool,
    scoring_side: u8,
    screen_y: u8,
    mut force_e: bool,
    replay: u8,
    name: &mut String,
) -> Run<bool> {
    host.g.mouse_pointer_hide(0);
    // DS:07b2: format_long_thousands writes no separators from here on.
    host.number_no_separators = true;
    let mut j_flag = false;
    let mut attempts = 0u32;
    // A replay opens the file of `name` first; every failed open or read draws a new file,
    // in a replay too (24a4:0748 and 24a4:07b3 jump back to the draw at 24a4:0624).
    let mut draw = replay == 0;
    let scene = loop {
        if std::mem::replace(&mut draw, true) {
            let choice = tore::select_scene(counts, goal, force_e, &mut j_flag, &mut || rng.rand());
            force_e = choice.force_e;
            *name = choice.file_name();
        }
        attempts += 1;
        let Ok(bytes) = std::fs::read(host.data.join(TORE_DIR).join(name.as_str())) else {
            if attempts >= MAX_ATTEMPTS {
                break None;
            }
            continue;
        };
        match tore::parse_scene(&bytes) {
            Ok(s) => break Some(s),
            // A failed strcmp closes the file and shows nothing; the 12 bytes are read
            // without a length check.
            Err(
                ToreError::BadSignature
                | ToreError::Truncated {
                    what: "signature", ..
                },
            ) => {
                break None;
            }
            // TODO(#16): tore_report_read_error 24a4:0574 shows a disk error box before the
            // retry; no shipped file is short.
            Err(_) if attempts < MAX_ATTEMPTS => continue,
            Err(_) => break None,
        }
    };
    if let Some(mut scene) = scene {
        tore::mirror_scene(&mut scene, 1u8.wrapping_sub(scoring_side) != 0);
        tore_play_frames(host, pics, &scene, i16::from(screen_y), replay)?;
    }
    host.g.mouse_pointer_show();
    Ok(force_e)
}

/// Waits for the timer deadline, restarting the crowd noise meanwhile.
fn wait_deadline(host: &mut Host) -> Run<()> {
    while !host.g.timer_deadline_reached() {
        host.voice_restart_loop();
        host.pump()?;
    }
    Ok(())
}

/// Plays the frames of a scene into the 182x96 window at (0x45, `screen_y`), page flipped:
/// 2 BIOS ticks a frame, 5 in a replay; the first replay (`replay` 1) starts 18 frames
/// before the end. The sounds start at their frames; frame 0 is held 10 more ticks and
/// the last one 7.
///
/// Origin: BMMAIN.EXE 1c7c:0004 (tore_play_frames)
fn tore_play_frames(
    host: &mut Host,
    pics: &ScenePics,
    scene: &Scene,
    screen_y: i16,
    replay: u8,
) -> Run<()> {
    let last = i16::from(scene.last_frame as i8);
    let mut start: i16 = 0;
    if replay == 1 {
        start = last - tore::REPLAY_TAIL_FRAMES;
    }
    if start < 0 {
        start = 0;
    }
    let ticks = if replay != 0 {
        tore::TICKS_REPLAY
    } else {
        tore::TICKS_NORMAL
    };
    host.voice_restart_loop();
    let mut sound = 0u8;
    let mut i = start as u8;
    while last >= i16::from(i) {
        for (k, &f) in scene.sound_frames.iter().enumerate() {
            if i16::from(f as i8) == i16::from(i) {
                host.voice_play(k as u8 + 2);
                sound = k as u8 + 2;
            }
        }
        host.g.timer_set_deadline(ticks);
        host.g.blit_bitmap(
            pics.strip,
            0,
            0,
            pics.canvas,
            0,
            tore::PITCH_TOP,
            0x140,
            0x4d,
        );
        let frame = scene.frames.get(usize::from(i));
        for o in frame.iter().flat_map(|f| f.objects.iter()) {
            host.voice_restart_loop();
            let id = o.sprite;
            if id > 999 {
                if id == tore::LEFT_GOAL_ID {
                    host.g.blit_bitmap_transparent(
                        pics.goals,
                        0,
                        0,
                        pics.canvas,
                        0,
                        0x36,
                        0x18,
                        0x14,
                    );
                } else {
                    host.g.blit_bitmap_transparent(
                        pics.goals,
                        0x18,
                        0,
                        pics.canvas,
                        0x128,
                        0x36,
                        0x17,
                        0x14,
                    );
                }
            } else {
                let (ofs, w) = if id >= BALL_FIRST_ID { (4, 4) } else { (0, 12) };
                host.g.blit_bitmap_transparent(
                    pics.sprites,
                    ((id % 24) * 12 + ofs) as i16,
                    ((id / 24) * 11) as i16,
                    pics.canvas,
                    o.x as i16,
                    i16::from(o.y) + tore::PITCH_TOP,
                    w as i16,
                    11,
                );
            }
        }
        host.voice_restart_loop();
        if let Some(f) = frame {
            let screen = host.screen();
            host.g.blit_bitmap(
                pics.canvas,
                i16::from(f.camera_x),
                i16::from(f.camera_y),
                screen,
                tore::VIEW_SCREEN_X,
                screen_y,
                tore::VIEW_WIDTH as i16,
                tore::VIEW_HEIGHT as i16,
            );
        }
        if i == 0 {
            host.g.vga_copy_page(0, crate::gfx::PAGE_SIZE);
        }
        host.g.vga_show_page(0);
        wait_deadline(host)?;
        if i == 0 {
            host.g.timer_set_deadline(tore::FIRST_FRAME_HOLD_TICKS);
            wait_deadline(host)?;
        }
        i = i.wrapping_add(1);
    }
    if (scene.last_frame.wrapping_sub(start as u8)) & 1 != 0 {
        host.g.vga_copy_page(crate::gfx::PAGE_SIZE, 0);
    }
    host.g.vga_show_page(1);
    host.g.timer_set_deadline(7);
    wait_deadline(host)?;
    if sound != 0 && host.voice_current() != 1 {
        host.voice_stop();
    }
    host.voice_restart_loop();
    Ok(())
}

/// The goal scene of a live ticker chance (`goal`: the ball goes in) of side `side` in the
/// event panel at `ybase`: the scene pictures replace the live view's, the scene plays, and
/// while `r` or `w` is pressed after it, it is replayed (at most three showings). Returns
/// the penalty-scene flag of the last showing (`prev` starts as the event's penalty flag).
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:1290..1300)
#[allow(clippy::too_many_arguments)]
pub fn goal_scene(
    host: &mut Host,
    live: &mut LiveView,
    rng: &mut Rng,
    auto: bool,
    goal: bool,
    side: u8,
    ybase: i16,
    penalty: u8,
) -> Run<u8> {
    let pics = match_screen_load_pictures(host, live);
    let mut prev = penalty != 0;
    let mut replay: u8 = 0;
    if let (Some(p), Some(counts)) = (&pics, tore_read_anzahl(host)) {
        if !auto {
            host.delay_ticks(30)?;
        }
        let mut name = String::new();
        loop {
            prev = tore_scene_play(
                host,
                p,
                rng,
                &counts,
                goal,
                1u8.wrapping_sub(side),
                ybase.wrapping_add(5) as u8,
                prev,
                replay,
                &mut name,
            )?;
            replay += 1;
            if !auto {
                host.delay_ticks(20)?;
            }
            let key = host.read_key_nowait();
            if !((replay as i8) < 3 && (key == b'r' || key == b'w')) {
                break;
            }
        }
    }
    match_screen_restore_pictures(host, live, pics);
    Ok(u8::from(prev))
}

/// The stretcher of an injury: four times the four cells of the two carriers at
/// (0xdc, `ybase` + 0x59), 13 ticks each.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:1e3f..1ebd)
pub fn stretcher_animation(host: &mut Host, sprites: Option<BitmapId>, ybase: i16) -> Run<()> {
    for _ in 0..4 {
        for f in 0..4i16 {
            let alt = i16::from(f == 1 || f == 3);
            if let Some(src) = sprites {
                let screen = host.screen();
                host.g.blit_bitmap(
                    src,
                    0xa3 + 10 * alt,
                    0x73 + 10 * i16::from(f > 1),
                    screen,
                    0xdc,
                    ybase + 0x59,
                    0x0a,
                    0x0a,
                );
            }
            host.delay_ticks(13)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The draws of a first showing follow `docs/re/tore.md`: number, E chance, then the J
    /// chance (no E) and nothing more for a plain scene.
    #[test]
    fn a_plain_scene_draws_three_numbers() {
        let counts = Anzahl {
            max_scene: 43,
            e_count: 5,
            j_count: 2,
        };
        for seed in 0..200u16 {
            let mut rng = Rng::new(seed);
            let mut want = rng;
            let mut j = false;
            let c = tore::select_scene(&counts, true, false, &mut j, &mut || rng.rand());
            let n = want.random_range(2, 43);
            let e = want.random_range(0, 15) == 0;
            if e {
                assert_eq!(c.number, want.random_range(2, 5));
                assert_eq!(c.file_name(), format!("{}.TE", c.number));
            } else {
                let jj = want.random_range(0, 400) == 0;
                if jj {
                    assert_eq!(c.number, want.random_range(2, 2));
                } else {
                    assert_eq!(c.number, n);
                    assert_eq!(c.file_name(), format!("{n}.T"));
                }
            }
            assert_eq!(rng.state(), want.state());
        }
    }
}
