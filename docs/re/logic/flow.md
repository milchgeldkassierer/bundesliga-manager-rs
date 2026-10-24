# Program flow, new game, start menu and office (area "flow")

Reconstructed from `/unpacked/BMMAIN_unpacked.EXE` (Ghidra project `BMP`). This spec is written so that
an implementer without Ghidra can port the code bit-exactly. Everything was read from the
**disassembly** (the decompiler output is wrong for most calls in this area, because the callees
have no prototypes, and `office_menu_dispatcher` does not decompile at all: it contains MS
floating-point-emulator opcodes, see §7.2).

Scratch material (listings, scripts) is in `out/flow/` (gitignored). `out/flow/cdis.py` is a capstone
based disassembler that resolves the FP-emulator `INT 34h..3Dh` opcodes and the `ES` segment
selectors; use it if something here needs re-checking:
`out/flow/cd.sh <seg> <start> <end> [jumptable-ranges]`.

## 0. Conventions

* Addresses are Ghidra `seg:off` in the unpacked program. `DS:xxxx` = DGROUP `5cb3`, `FAR:xxxx` = far data
  segment `5238`. Each global access was resolved through the selector table (`MOV ES,[DS:99xx]`).
* Types: `u8/s8/u16/s16/u32/s32`. MSC 6 large model, `int` = 16 bit. All function arguments are pushed as
  16-bit words (a `u8` argument is pushed as a word whose high byte is garbage, so callees only look at the
  low byte). Return values in `AX` (`DX:AX` for long / far pointer).
* `random_range(lo, hi)` (176b:0cc7) = `rand() % (hi - lo + 1) + lo` with the MSC LCG, see FUNCTION_MAP §3.4.
  **Every** call is listed in this document in execution order. Arguments are `int`.
* `_aFlmul/_aFldiv` are signed 32-bit multiply/divide (truncating toward zero).
* The mouse state struct pointed to by `FAR:6dc0` (`mouse_state*`) holds **`word[0] = y`, `word[1] = x`**
  (the start menu compares `word[1]` with values up to 0x139 = 313, so `word[1]` is x). Below `mx`/`my`
  always mean x/y. (MEMORY_MAP §3.1 states the opposite for `6dc0`; this is the corrected reading.)
* Mouse events (`FAR:2ea2`, set by `input_poll_mouse` 176b:0319 from `mouse_pop_button_event` 45ac:192d):
  `0` none, `1` left button pressed, `2` left released, `3` right pressed, `4` right released
  (four latched flags `CS:0023..0026` in the INT 33h handler; the assignment 2/3/4 is inferred from use:
  carousel scrolling stops on `2`, "finish/confirm" and "quit" use `3`).
* Records used everywhere (base addresses in FAR):
  * **Manager record** `M[m]` = `FAR:2242 + 0x30a*m`, `m` = 0..3 (save block 13). Field offsets are given
    relative to that base, e.g. `M[m].team` = `+0x1e` (`FAR:2260+0x30a*m`). See §11.2 for all fields used here.
  * **Club record** `CLUB[t]` = `FAR:3066 + 0x22*t`, `t` = 0..199 (save block 15): `+0x00` name (20 bytes),
    `+0x21` crest code (`& 0x7f`, crest picture = `PIC\<code+0x50>.VGA`).
  * **Pool person record** `P[p]` = `FAR:57dd + 0x25*p`, `p` = 1..150 (save block 17, 151 records, #0
    unused). These are the **players** of the game world (4 categories, see DS:07a6). Fields §11.3.
  * **Squad slot record** `S[m][k]` = `FAR:774a + 0x34*(m*25 + k)` (save block 18), `k` = 0..24.
  * **Team statistics record** `TS[t]` = `FAR:0ecc + 0x36*t`, `t` = 0..63 (save block 16).
* Library/UI calls are described by their arguments only; their behaviour is the UI/gfx agents' business.
  Argument order of the most frequent ones (cdecl, first argument = last push):
  * `vga_load_picture(u8 id, u16 w, u16 h, u8 unused, u8 load)` (176b:0356, still `FUN_176b_0356`):
    returns a far pointer to a **rastport-like object whose first far pointer is the bitmap**.
    `w == 30000` loads `PIC\<id>.CP` straight to the screen; `load == 0` allocates a blank `w x h` sprite.
  * `blit_bitmap(src_bitmap, sx, sy, dst_rastport, dx, dy, w, h, u8 minterm)` (4930:0991). `minterm` is always 0xc0.
  * `ui_blit_transparent(src_bitmap, sx, sy, dst_rastport, dx, dy, w, h)` (176b:02e4) — colour 0 transparent.
  * `ui_fill_rect(rp, x0, y0, x1, y1, u8 color)` (16c7:083c).
  * `ui_draw_text_at(rp, str, x, y, u8 color)` (16c7:07d5).
  * `ui_draw_text_centered_shadow(rp, str, x0, x1, y, u8 color, u8 shadow)` (16c7:087c); shadow 0xff = none.
  * `ui_draw_line(rp, x0, u8 y0, x1, u8 y1, u8 color)` (16c7:09b0) — note **byte** y coordinates.
  * `ui_draw_button(rp, x, y, w, u16 icon_and_flags, rect_out far*, u8 a, u8 b)` (16c7:00a7): with `w == 0`
    it draws a 50x41 icon button (`rect_out` receives `{x, y, 0x32, 0x29}`), icon = low byte, flags = high byte.
  * `ui_hit_test_buttons(rect_array far*, int n, u8 only_on_click)` (16c7:0002) → 1-based index or 0.
  * `ui_redraw_buttons(u8 owner, int)` (16c7:0563); always called as `(0x7f, 0)`.
  * `screen_fade(screen, u8 in)` (176b:1416); `screen` = `FAR:2e70` everywhere below, omitted.
  * `rp` = `FAR:2f28` (main screen rastport) unless noted; `screen_bitmap` = `FAR:a6ca`.
* German strings are quoted verbatim as stored (`[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü, `~`=ß).

## 1. Function list of the area

| Address | Ghidra name | Role | Section |
|---|---|---|---|
| 18bc:0418 | `main` | program entry after CRT | §2 |
| 18bc:0004 | `startup_init_screen_mouse_palette` (renamed) | screen/mouse/fonts/PIC\1.PAL | §3.1 |
| 18bc:01fb | `startup_clear_state` (renamed) | clears FAR:9336, slot numbers | §3.2 |
| 18bc:034d | `office_load_bitmaps` (renamed) | loads PIC 4, 0x17, 3 | §3.3 |
| 18bc:02ef | `office_free_bitmaps` (renamed) | frees them | §3.3 |
| 18bc:3b92 | `office_show_cached_background` (renamed) | PIC\22.CP cached in VRAM | §3.4 |
| 18bc:2184 | `start_menu_new_or_load` (renamed) | start menu / add manager / new-game manager init | §4 |
| 18bc:1cdd | `draw_club_crest` (renamed) | crest PIC | §4.9 |
| 18bc:1df3 | `team_in_european_cup_lists` (renamed) | | §4.9 |
| 18bc:1e75 | `wait_5_ticks` (renamed) | | §4.9 |
| 18bc:1e96 | `start_menu_swap_portraits` (renamed) | | §4.9 |
| 18bc:1fc4 | `team_stats_swap` (renamed) | | §4.9 |
| 18bc:2061 | `start_menu_draw_level_box` (renamed) | | §4.9 |
| 18bc:0a63 | `manager_compute_wage_budget` (renamed) | RNG | §4.8 |
| 362f:0004 | `FUN_362f_0004` (UI, not renamed: owned by settings area) | yes/no toggle pair | §4.9 |
| 422b:035c | `newgame_init_player_pool` (renamed) | RNG, MANA.DAT | §5.1 |
| 422b:01d0 | `mana_team_player_name_ptr` (renamed) | | §5.2 |
| 422b:022e | `mana_pick_unused_player_name` (renamed) | RNG | §5.2 |
| 24a4:19fb | `assign_pool_players_to_teams` (renamed) | RNG via callees | §5.3 |
| 1310:09c5 | `league_shuffle_teams` (named by another agent) | RNG | §5.4 |
| 1310:0b24 | `team_swap_slots` | | §5.5 |
| 1310:0ae8 | `swap_team_ref_byte` (other agent) | | §5.5 |
| 1310:0f44 | `team_swap_in_table_a4ca` (other agent) | | §5.5 |
| 27c2:0ef2 | `euro_cup_qualifiers_build` (other agent) | | §5.6 |
| 27c2:09e0 | `cup_draw_first_round` (other agent; = "competition init") | RNG | §5.7 |
| 27c2:1187 | `cup_amateur_home_swap` (other agent) | | §5.7 |
| 27c2:1b6d | `cup_clear_match_goals` (other agent) | | §5.8 |
| 212a:000a | `interest_rates_update` (other agent) | RNG | §5.9 |
| 18bc:0b5f | `office_menu_dispatcher` | office turn | §6 |
| 18bc:08f2 | `office_handle_free_click` (renamed) | | §6.6 |
| 18bc:1c1d | `quit_game_confirm` (renamed) | | §6.7 |
| 1310:000a | `has_rescheduled_match_today` (other agent) | | §6.8 |
| 16c7:09f9 | `copy_protection_penalty` | | §8 |

Plate comments `[flow spec]` were added to every renamed function. The jump tables at 18bc:1a15 and
18bc:1b42 were re-typed as word data (my listing tool had accidentally disassembled them as code; restored).

External callees used but specified elsewhere (named by their owners): `mana_load_club_table` 362f:36ec,
`tore_scene_open` 24a4:0419 (`FUN_24a4_0419`), `season_main_loop` 2a58:3176, `league_strength_drift`
1f9d:0697, `transfer_market_refresh` 3277:1e38, `squad_insert_player` 2ecd:37d8, `auto_lineup` 2ecd:3360,
`squad_recalc_team_strength` 1f9d:0002, `player_value_calc` 3277:25de, `FUN_24a4_2cb4`
(`advertising_offers_generate`, stats.md §3.6), `squad_count_by_flag` 4091:1109, `league_tier_of_team` 27c2:1bb5,
`team_is_euro_qualified` 27c2:11e7, `cup_draw_screen_open` 27c2:0006, `cup_draw_animate_slot` 27c2:0378,
`cup_draw_screen_close` 27c2:089a, `rank_tier_teams_by_strength` 1cb5:2a3b, `pick_random_free_team_in_tier`
1cb5:2987, `FUN_24a4_1798`, `manager_index_of_team` 1310:32b1, all screens of §6.5.

## 2. `main` 18bc:0418

```c
void main(int argc, char far * far *argv)          // 18bc:0418, cdecl far
{
    s16 cmf_fd; u16 cmf_len; u8 far *cmf = NULL;    // [bp-0x10],[bp-0x1c],[bp-0x20]
    u8 music = 0;                                   // [bp-0x16]

    if (argc >= 2 && _fstrcmp(argv[1], DS:021e /* "0" */) == 0)
        DS:018c = 0;                                // sound disabled (BMP.BAT passes %1)
    s16 unused_26 = 0;                              // [bp-0x26], set at 18bc:0424, never written again
    startup_init_screen_mouse_palette((u8)unused_26);   // §3.1; pushes AL=0 (18bc:0461-0466), callee ignores it
    screen_fade(0);
    FAR:57bc = vga_load_picture(0, 1, 1, 0, 0);     // blank 1x1 "work" object
    FAR:56fa = *(bitmap far**)FAR:57bc;             // its bitmap
    FAR:56fa->w = 0x140; FAR:56fa->h = 0xF0;        // turned into a 320x240 descriptor (used by
                                                    // blit_bitmap_or_restore_page2 4930:0849 = page-2 restore)
    if (DS:018c && sb_detect_port() && fmdrv_init()) {
        music = 1;
        cmf_fd = file_open(DS:0220 /* "sound\bm2title.cmf" */, 0);
        if (!cmf_fd) fatal_error_exit(2);
        cmf_len = filelength(cmf_fd);               // low word only
        cmf = mem_alloc(cmf_len, 0);  if (!cmf) fatal_error_exit(4);
        file_read(cmf_fd, cmf, cmf_len); file_close(cmf_fd);
        u8 far *instr = cmf + *(u16*)(cmf+6);
        u8 far *music_data = cmf + *(u16*)(cmf+8);
        fmdrv_reset();
        fmdrv_set_driver_clock((u16)(0x001234DCL / (s32)(s16)*(u16*)(cmf+0x0c)));   // _aFldiv
        if (FP_OFF(instr) != 0) fmdrv_set_instruments(instr, cmf[0x24]);
        fmdrv_play_music(music_data);
    }
    vga_load_picture(0x2f, 30000, 0, 0, 1);         // PIC\47.CP title picture straight to screen
    palette_load_rgb4(FAR:4b50, DS:0234 /* = 0000:0002 -> palette slot 2 */, 0x100);
    mouse_pointer_show();
    screen_fade(1);
    if (music)
        while (fmdrv_poll_status() != 0) {
            input_poll_mouse(FAR:6dc0);
            if (FAR:2ea2 == 1) { fmdrv_stop_music(); fmdrv_shutdown(); }
        }
    if (cmf) mem_free(cmf, 0);
    sound_voice_init();      // 176b:0006: if DS:018c && ems_detect() > 6 && (sb_probe_port() & 4)
                             //   && sb_detect_irq() && ctvoice_init() == 0: DS:014c = EMS page frame, DS:018d = 1
    sound_load_digi_voc();   // 176b:005f: if DS:018d: DIGI.VOC = 5 concatenated VOC files, slot i of
                             //   length DS:0164[i] (7018,16080,29084,22331,15813) copied to its own EMS
                             //   handle (DS:0150[i], 1/1/2/2/1 pages); see docs/re/voc.md
    mana_load_club_table();                         // 362f:36ec (MANA.DAT); 800 RNG draws on the
                                                    // power-on generator state (before seed #1)
    for (int i = 0; i < 2; i++) {                   // button frame pictures
        FAR:578c[i] = vga_load_picture(0x18 + i, 0, 0, 0, 1);   // PIC\24.VGA, PIC\25.VGA
        FAR:548e[i] = *FAR:578c[i];                              // bitmap
    }
    DS:05b0 = vga_load_picture(0, 0xa4, 0x26, 5, 0);    // blank 164x38: crest cache of the 4 managers
    FAR:5354 = *DS:05b0;                                //   (slot m at x = m*0x29)
    random_seed_from_time();                        // srand(time(NULL))  -- RNG seed #1
    DS:07ab = 0;                                    // no managers yet
    startup_clear_state();                          // §3.2
    league_shuffle_teams();                         // §5.4   RNG
    euro_cup_qualifiers_build();                    // §5.6
    cup_clear_match_goals();                        // §5.8
    for (int c = 1; c < 4; c++) cup_draw_first_round((u8)c, 0);   // §5.7  RNG (European cups)
    FAR:4d38 = vga_load_picture(0, 0, 0, 0, 1);     // PIC\0.VGA = manager portrait sheet
    FAR:2f24 = *FAR:4d38;
    FAR:6db8 = vga_load_picture(0, 0x20, 0x17, 0, 0);   // blank 32x23
    FAR:57c0 = *FAR:6db8;
    FAR:57d4 = vga_load_picture(3, 0, 0, 0, 1);     // PIC\3.VGA icon sheet
    FAR:2f3c = *FAR:57d4;
    blit_bitmap(FAR:2f3c, 0xa5, 0x78, FAR:6db8, 0, 0, 0x20, 0x17, 0xc0);  // icon used by button 0x23
    delay_ticks(150L);
    u8 loaded = start_menu_new_or_load(1);          // [bp-2] (18bc:084b); §4; 0 = new game, 1 = game loaded
    if (loaded == 0) {
        bitmap_free(FAR:57d4);                      // PIC 3
        league_strength_drift(10);                  // 1f9d:0697 (other area)
        random_seed_from_time();                    // RNG seed #2 (re-seeded!)
        DS:225a[0] = 1;                             // league round counter of tier 0
        assign_pool_players_to_teams();             // §5.3   RNG
        cup_draw_first_round(0, 0);                 // §5.7   DFB-Pokal draw, RNG
        for (int c = 1; c < 4; c++) cup_draw_first_round((u8)c, 1);   // no RNG: shows the European cup draw
                                                    // made at startup (screen open, slot animation if FAR:57c8,
                                                    // screen close) and sets FAR:0008[c]=1; see §5.7
        DS:07dc = (s32)(s8)DS:224c;                 // day counter = season start offset
        transfer_market_refresh();                  // 3277:1e38 (other area)
        office_load_bitmaps(2);
    } else
        office_load_bitmaps(1);
    tore_scene_open();                              // 24a4:0419 (TORE\ANZAHL)
    season_main_loop(loaded);                       // 2a58:3176 (other area), byte arg pushed at 18bc:08da-08de.
                                                    // In season_main_loop: if arg != 0 (loaded game) it saves
                                                    // FAR:304a to a local at entry (2a58:3183) and later writes it
                                                    // back and clears the arg (2a58:3ad4/3ae4, 3b3d/3b4d).
                                                    // Does not return normally.
    fatal_error_exit(99);                           // 99 = normal exit
}
```

RNG stream summary for a new game: seed #1 (`time()`), then `league_shuffle_teams` (330 calls),
`cup_draw_first_round(1..3,0)`, `newgame_init_player_pool`, the per-manager initialisation in the start
menu, `interest_rates_update`; then seed #2 (`time()` again), `league_strength_drift(10)` was called
**before** the re-seed (it runs on stream #1), then `assign_pool_players_to_teams`,
`cup_draw_first_round(0,0)`, `transfer_market_refresh`. `cup_draw_first_round(1..3,1)` uses no RNG; it
only displays the European cup draws made under seed #1, so the player sees the DFB-Pokal draw followed by
the three European draws (each only animated if the user enabled the display, `FAR:57c8`). Because both seeds are `time()` (seconds), a
bit-exact replay needs both seed values.

### 2.1 Sound

The title tune is `SOUND\BM2TITLE.CMF`, played by the embedded SBFMDRV (format and driver
semantics: [cmf.md](../cmf.md)). It runs from `fmdrv_play_music` until the driver processes the
end-of-track event 18762 ticks of 12428 PIT clocks later (**195.42 s**); `main`'s wait loop then
sees status byte 0. A left click instead stops the music at once (`fmdrv_stop_music`, which sets the
status byte to 0, and `fmdrv_shutdown`). The title does not wait at all without music (`bmmain 0`,
no Sound Blaster or no FM driver): it only stays up during the loading and the `delay_ticks(150)`.

Digitised sound (format, sample table and API semantics: [voc.md](../voc.md)) is only heard on the
live match screen and in the goal scenes. Voices: 1 crowd loop, 2 goal, 3 whistle, 4 injury,
5 miss. All calls are no-ops unless `DS:018d`.

```c
match_live_period:                       // 1310
    if (show) { ...; sound_play_voice(1); }                  // 1310:24df  period start: crowd loop
    ...minute loop...
    if (show) sound_stop_voice();                            // 1310:2ab4  period end
match_ticker_events(type, ...):          // 2a58
    v = (type == 2 || type == 3) ? 3 : (type == 4) ? 4 : 0;  // cards -> whistle, injury
    if (v) { sound_play_voice(v); sound_wait_voice_end(); sound_restart_loop_voice(); }  // 2a58:1198
    ...
    if (type <= 1 && DS:05d4 == 0 && DS:0607 == 0) {         // no goal scenes
        sound_play_voice(type == 1 ? 2 : 5);                 // 2a58:14c0 goal / miss
        sound_wait_voice_end(); sound_restart_loop_voice();
    }
tore_play_frames:                        // 1c7c:0004, from tore_scene_play 24a4:08bb
    sound_restart_loop_voice();
    for (i = start; i <= last; i++) {
        for (k = 0; k < 4; k++) if ((s8)sound_frames[k] == i) { sound_play_voice(k + 2); played = 1; }
        ... draw (sound_restart_loop_voice() per object), wait ticks (restart in the wait loop) ...
    }
    ... 7-tick pause with restarts ...
    if (played && DS:018e != 1) sound_stop_voice();          // 1c7c:0380
    sound_restart_loop_voice();
fatal_error_exit:  sound_voice_shutdown();                   // 176b:12a2 -> ctvoice_terminate
```

Port: `Host::music_start` / `music_poll_status` / `music_stop` and `Host::voice_*`
(`crates/bmp/src/host.rs`) on the emulated card of `crates/bmp-sound`. `bmp run --nosound` or
`bmp run 0` clears the sound flag like `bmmain 0`. Headless runs keep the flag set and run the card
silently on the virtual clock, so the title still ends with the tune or a click.

## 3. Startup helpers

### 3.1 `startup_init_screen_mouse_palette` 18bc:0004 — `void (u8 unused)`

The only caller (main) pushes one byte, always 0 (`[bp-0x26]`); the function never reads `[bp+6]`,
so the argument is unused (Ghidra shows `void (void)`).

```c
char path[10] = DS:0206 "pic\1.pal";              // copied to the stack (5 words)
install_int_handler(0, 0);                         // 45ac:337e
FAR:2e70 = screen_open(&FAR:3050);  if (!FAR:2e70) fatal_error_exit(0);
FAR:4c7a = FAR:2e70;
FAR:6dc0 = mouse_open(&FAR:4c7a);   if (!FAR:6dc0) fatal_error_exit(0);
FAR:2f28 = *(far*)(FAR:6dc0 + 8);                  // screen rastport
FAR:4b50 = FAR:2e70 + 8;                           // palette pointer (far ptr arithmetic on offset)
FAR:a6ca = *FAR:2f28;                              // screen bitmap
if (font_load_all() == 0) fatal_error_exit(3);     // 176b:0bdb
u8 far *buf = mem_alloc(0x300, 0); if (!buf) fatal_error_exit(4);
int fd = file_open(path, 0); if (!fd) fatal_error_exit(2);
for (s8 i = 0; i < 3; i++) {                        // 3 palettes of 256 RGB triplets
    file_read(fd, buf, 0x300);
    buf[0x2fd] = buf[0x2fe] = buf[0x2ff] = 0;      // colour 255 forced to black
    outp(0x3c4, 2); outp(0x3c5, 1 << i);           // VGA map mask = plane i
    far_memcpy_bytes(buf, MK_FP(0xA000, 0xAE10), 0x300);  // stored in off-screen VRAM, plane i
}
file_close(fd); mem_free(buf, 0);
ui_set_pointer_shape(FAR:6dc0, 1);
```
The three palettes are "palette slots" 0..2; `palette_load_rgb4(dst, (far*)n, 0x100)` with a pointer value
`n < 4` restores slot `n` (see FUNCTION_MAP, `vga_restore_palette_slot`). `DS:0234 = 0000:0002` (slot 2,
title/menu palette), `DS:0794 = 0000:0000` (slot 0, office palette).

### 3.2 `startup_clear_state` 18bc:01fb — `void (void)`

```c
_fmemset(FAR:9336, 0, 0x1394);
for (u8 m = 0; m < DS:07ab; m++) for (u8 j = 2; j < 5; j++) M[m].b[0x132 + j] = 0x1e;   // FAR:2374
for (u8 m = 0; m < 4; m++) {
    M[m].slot = m + 1;                                       // +0x1d (FAR:225f)
    for (u8 t = 0; t < 0x40; t++) for (u8 j = 0; j < 5; j++) for (u8 k = 0; k < 2; k++)
        FAR:9336[(((m << 6) + t) * 5 + j) * 2 + k] = 0xff;
}
```
(Called with `DS:07ab = 0`, so the first loop does nothing at startup.)

### 3.3 `office_load_bitmaps` 18bc:034d — `int (int mode)` / `office_free_bitmaps` 18bc:02ef — `int (int mode)`

```c
int office_load_bitmaps(int mode) {
    FAR:4b56 = vga_load_picture(4, 0, 0, 0, 1);    FAR:0024 = *FAR:4b56;   // PIC\4.VGA office header
    FAR:551c = vga_load_picture(0x17, 0, 0, 0, 1); FAR:2e9a = *FAR:551c;   // PIC\23.VGA office desk
    if (mode == 1) return 1;
    FAR:57d4 = vga_load_picture(3, 0, 0, 0, 1);    FAR:2f3c = *FAR:57d4;   // PIC\3.VGA icons
    return (mode == 2) ? 1 : <garbage AX>;
}
int office_free_bitmaps(int mode) {
    bitmap_free(FAR:551c); bitmap_free(FAR:4b56);
    if (mode == 1) return 1;
    bitmap_free(FAR:57d4);
    return (mode == 2) ? 1 : <AX of bitmap_free>;
}
```
Callers use the pairs `(3)/(3)` around big screens (they free/reload the icon sheet, see §6.5) and `(2)/(2)`
around the start menu.

### 3.4 `office_show_cached_background` 18bc:3b92 — `void (void)`

```c
screen_fade(0);
mouse_pointer_hide(0);
if (DS:0210 == 0) {
    vga_load_picture(0x16, 30000, 0, 0, 1);   // PIC\22.CP (blue marble) straight to the screen
    vga_copy_page(0, 0x9600);                 // VRAM page 0 -> VRAM offset 0x9600 (cache)
    DS:0210 = 1;
} else
    vga_copy_page(0x9600, 0);                 // restore from the cache
mouse_pointer_show();
```
No fade-in: callers fade in after drawing.

## 4. `start_menu_new_or_load` 18bc:2184 — `int (u8 allow_load)`

Called by `main` with `allow_load = 1` (new game or load) and by the office (menu 5, icon 5 "new
manager", §6.5) with `0` (add managers to a running game). Returns 0 when it finished normally (new
game / managers added), 1 when a save game was loaded.

Locals (all `int` unless noted): `saved_cur = (s8)FAR:304a` ([bp-0x18]); `n0 = DS:07ab` at entry ([bp-0x36],
number of managers that already exist); `pos = 0` ([bp-4], carousel start index 0..63); `center = 3`
([bp-0xa], club index under the centre crest); `finish = 0` ([bp-0x5e]); `pending_swap = 0` ([bp-0x2c]);
`u8 flags = 1` ([bp-0x58], game-mode bits).

### 4.1 Setup and pictures

```c
ui_set_font(rp, 2);
hdr  = vga_load_picture(4, 0, 0, 0, 1);         // PIC\4.VGA   ([bp-0x24], bitmap [bp-0x12])
strip= vga_load_picture(0, 0x135, 0x26, 5, 0);  // blank 309x38 carousel buffer ([bp-0x16], bitmap [bp-8])
frame= vga_load_picture(0x22, 0, 0, 0, 1);      // PIC\34.VGA carousel frame + arrows ([bp-0x32], bmp [bp-0x1c])
slotp= vga_load_picture(0x23, 0, 0, 0, 1);      // PIC\35.VGA manager slot frame ([bp-0x20], bmp [bp-0xe])
screen_fade(0);
palette_load_rgb4(FAR:4b50, DS:0794 /* slot 0 */, 0x100);
```

### 4.2 Full redraw (label 18bc:22ca; also re-entered after a cancelled load)

```c
office_show_cached_background();                               // PIC\22.CP
blit_bitmap(frame_bmp, 0, 0, rp, 5, 0x28, 0x135, 0x41, 0xc0);  // carousel frame at (5,40)
blit_bitmap(hdr_bmp,   0, 0, rp, 0, -3,  0x140, 0x2c, 0xc0);   // header bar
copy_protection_penalty(/* 4 dummy args: 0,0,hdr,0x2c */);    // §8
for (i = 0; i < 4; i++) {
    blit_bitmap(slotp_bmp, 0, 0, rp, i*0x50 + 0x0c, 0x69, 0x3a, 0x89, 0xc0);     // slot frame
    blit_bitmap(FAR:2f24, i*0x29 + 1, 0, rp, i*0x50 + 0x15, 0xaa, 0x27, 0x28, 0xc0); // portrait i
    if (i + 1 > n0) M[i].team = 100;                           // 100 = "no club"
    else {
        blit_bitmap(FAR:5354, i*0x29, 0, rp, i*0x50 + 0x16, 0x72, 0x26, 0x26, 0xc0); // its crest
        ui_draw_text_at(rp, M[i].name, i*0x50 + 0x14, 0xe6, 0x13);
    }
}
blit_bitmap(screen_bitmap, 0x11, 0x30, strip, 0, 0, 0x11f, 0x26, 0xc0);   // background under the strip
for (i = 0; i < 7; i++) draw_club_crest(CLUB[i].crest & 0x7f, strip, i*0x29 + 1, 0);
blit_bitmap(strip_bmp, 0, 0, rp, 0x11, 0x30, 0x11f, 0x26, 0xc0);
ui_set_font(rp, 1);
rastport_set_draw_mode(rp, 0);
ui_draw_text_centered_shadow(rp, CLUB[3].name /* FAR:30cc */, 0x55, 0xeb, 0x62, 0, 0xff);
screen_fade(1);
```
Note the redraw uses `CLUB[i]` for i = 0..6 and `CLUB[3]`, not the current `pos/center` (after a cancelled
load the strip shows clubs 0..6 again while `pos/center` keep their values — faithful quirk).

Screen layout: header (y -3..40), carousel frame at (5,40) 309x65 with the 7-crest strip at (17,48) and
the centre crest at x = 0x8d (141); club name centred in x 0x55..0xeb at y 0x62; four manager slots at
x = 0x0c + 0x50*i: crest box (0x16+0x50i, 0x72) 38x38, portrait (0x15+0x50i, 0xaa) 39x40, name at
(0x14+0x50i, 0xe6).

### 4.3 Main input loop (label 18bc:2591)

```c
for (;;) {
    if (finish) goto FINISH;                                   // §4.6
    input_poll_mouse(FAR:6dc0);
    if (FAR:2ea2 == 3 && DS:07ab != 0) {                        // right click = "done"
        int open = 0;
        for (i = 0; i < DS:07ab; i++) if (M[i].team == 100) open = 1;
        if (!open) finish = 1;
    }
    if (FAR:2ea2 != 1) continue;
    my = mouse_state[0]; mx = mouse_state[1];
    if (my < 0x24 && allow_load) {                             // click on the header bar: load game
        office_show_cached_background();
        if (screen_load_game(1) /* 422b:120c */ != 0) {
            bitmap_free(slotp); bitmap_free(frame); bitmap_free(hdr); bitmap_free(strip);
            return 1;
        }
        goto FULL_REDRAW;                                      // §4.2
    }
    if (my > 0x2e && my < 0x69) {                              // carousel row (y 47..104)
        ui_set_font(rp, 1);
        if      (mx > 5    && mx < 0xa0)  scroll(-1);          // §4.4
        else if (mx > 0x9f && mx < 0x13a) scroll(+1);
    }
    slot_clicks(mx, my);                                       // §4.5
}
```

### 4.4 Carousel scrolling

Left half (`dir = -1`) / right half (`dir = +1`):
```c
ui_blit_transparent(frame_bmp, dir<0 ? 0x0d : 0x13, 0x41, rp, dir<0 ? 0x0a : 0x131, 0x32, 6, 0x22); // pressed arrow
delay_ticks(8L);
do {
    if (dir < 0) { if (--pos < 0) pos = 0x3f; }
    else         { if (++pos > 0x3f) pos = 0; }
    center = pos + 3; if (center > 0x3f) center -= 0x40;
    if (dir < 0) {
        for (k = 6; k > 0; k--)                                // shift strip right by one crest
            blit_bitmap(strip_bmp, k*0x29 - 0x28, 0, strip, k*0x29 + 1, 0, 0x28, 0x26, 0xc0);
        draw_club_crest(CLUB[pos].crest & 0x7f, strip, 1, 0);
    } else {
        for (k = 0; k < 6; k++)                                // shift strip left
            blit_bitmap(strip_bmp, k*0x29 + 0x2a, 0, strip, k*0x29 + 1, 0, 0x28, 0x26, 0xc0);
        n = pos + 6; if (n > 0x3f) n -= 0x40;
        draw_club_crest(CLUB[n].crest & 0x7f, strip, 0xf7, 0);
    }
    blit_bitmap(strip_bmp, 0, 0, rp, 0x11, 0x30, 0x11f, 0x26, 0xc0);
    ui_fill_rect(rp, 0x55, 0x59, 0xeb, 0x63, 5);
    ui_draw_text_centered_shadow(rp, CLUB[center].name, 0x55, 0xeb, 0x62, 0, 0xff);
    input_poll_mouse(FAR:6dc0);
    wait_5_ticks();
} while (FAR:2ea2 != 2);                                       // until left button released
ui_blit_transparent(frame_bmp, dir<0 ? 0 : 6, 0x41, rp, dir<0 ? 0x0a : 0x131, 0x32, 6, 0x22);  // released arrow
```
Only club indices 0..63 (the German clubs of tiers 0..2 and the rest of the first 64) can be chosen.

### 4.5 Manager slot clicks (label 18bc:2a90)

For `m = n0 .. 3` (existing managers cannot be edited), with `xl = m*0x50 + 0x16`:

```c
// (a) crest box: assign the centre club
if (DS:07ab >= m + 1 && xl < mx && my > 0x72 && mx < xl + 0x29 && my < 0x99) {
    int used = 0;
    for (i = 0; i < DS:07ab; i++) if (M[i].team == center) used = 1;
    if (!used) {
        M[m].team = (u8)center;
        blit_bitmap(screen_bitmap, 0x8d, 0x30, rp, xl, 0x72, 0x26, 0x26, 0xc0);   // copy centre crest
    }
}
// (b) portrait: click two slots to swap their portraits
if (xl < mx && my > 0x97 && my < 0xd2 && mx < xl + 0x29) {
    if (pending_swap) {
        pending_swap--;                                        // = other slot index
        start_menu_swap_portraits((u8)pending_swap, (u8)m);    // §4.9
        swap(M[pending_swap].slot /* +0x1d */, M[m].slot);
        pending_swap = 0;
    } else pending_swap = m + 1;
}
// (c) name field (only if the slot is active or already has a name)
if ((DS:07ab >= m || M[m].name[0] != 0) && xl < mx && my > 0xd2 && mx < xl + 0x29) {
    ui_fill_rect(rp, xl - 3, 0xe1, xl + 0x27, 0xe7, 0);
    ui_set_font(rp, 2);
    ui_scroll_text_box(FAR:6dc0, M[m].name, xl - 2, 0xe6, 0x28, 0x13, 0, 1);   // 176b:07d7 text input
    if (M[m].name[0] == 0) {                                   // name erased: drop this and all later slots
        for (k = m; k < 4; k++) {
            M[m].team = 100;                                   // NB: index m, not k (original bug)
            ui_fill_rect(rp, k*0x50 + 0x16, 0x72, k*0x50 + 0x3d, 0x97, 0);
            ui_fill_rect(rp, k*0x50 + 0x13, 0xe1, k*0x50 + 0x3b, 0xe7, 0);
        }
        DS:07ab = (u8)m;
    } else if (DS:07ab < m + 1)
        DS:07ab = (u8)(m + 1);                                 // a new manager slot becomes active
}
```
The text input (`ui_scroll_text_box`, UI agent) reads keys with `dos_getch`; the name buffer is the 29-byte
`M[m].name`.

### 4.6 Finish (label 18bc:25cb)

```c
bitmap_free(slotp); bitmap_free(frame); bitmap_free(hdr); bitmap_free(strip);
for (i = n0; i < DS:07ab; i++)                                 // cache the chosen crests
    blit_bitmap(screen_bitmap, i*0x50 + 0x16, 0x72, DS:05b0, i*0x29, 0, 0x28, 0x26, 0xc0);
ui_set_font(rp, 1);
if (n0 == 0) OPTIONS_DIALOG();                                 // §4.7 (new game)
else         ADD_MANAGERS();                                   // §4.7b (running game)
MANAGER_INIT_LOOP();                                           // §4.8
FAR:304a = (u8)saved_cur;
if (n0 == 0) interest_rates_update();                          // §5.9  RNG
return 0;
```

### 4.7 New-game options dialog (n0 == 0)

```c
dlg = vga_load_picture(0x21, 0, 0, 0, 1);                     // PIC\33.VGA yes/no toggle sprites
int four = (DS:2252/2254 == DS:224e/2250) ? 4 : 3;            // number of game-mode rows (longs compared)
ui_draw_button(rp, 0x4e, 0x45, 0xa4, 0x8e - 0x14*(four == 3), &rect, 1, 8);  // dialog frame
for (i = 0; i < 3; i++) {
    ui_draw_text_centered_shadow(rp, DS:4a2a[i], 0x4f, 0xb9, i*0x12 + 0x57, 1, 0); // "Spiel-Level", "", ""
    FUN_362f_0004(*dlg, 0xb5, i*0x12 + 0x4b, 0);               // draws two 24x17 sprites, see §4.9
    start_menu_draw_level_box(2*i,     i == 1);                // level 2 is highlighted initially
    start_menu_draw_level_box(2*i + 1, 0);
}
for (i = 0; i < four; i++) {
    ui_draw_text_centered_shadow(rp, DS:4a36[i], 0x4f, 0xb9, i*0x12 + 0x92, 1, 0);
    // "Endlosspiel", "1-Jahres-Spiel", "3-Jahres-Spiel", "Histor. Start"
    FUN_362f_0004(*dlg, 0xb5, i*0x12 + 0x86, (flags >> i) & 1);
}
do {
    input_poll_mouse(FAR:6dc0); my = ms[0]; mx = ms[1];
    if (FAR:2ea2 == 1)
        for (i = 0; i < 4; i++) {                               // i and mx are clobbered below (faithful)
            if (!(mx > 0xb4 && mx < 0xe8)) continue;
            if (my > 0x85 && my < 0xcd - 0x12*(four == 3)) {    // game-mode rows
                i = (my - 0x86) / 0x12;                         // signed int division
                if (i == 3) flags &= 7; else flags &= 8;
                flags |= (u8)((mx < 0xcd) << i);                // left half = on, right half = off
                for (mx = 0; mx < four; mx++)                   // mx reused as loop counter!
                    FUN_362f_0004(*dlg, 0xb5, mx*0x12 + 0x86, (flags >> mx) & 1);
            }
            if (my > 0x4a && my < 0x80) {                       // level boxes (2 columns x 3 rows)
                start_menu_draw_level_box((s8)DS:4a28, 0);
                DS:4a28 = (u8)(((my - 0x4b) / 0x12) * 2 + (mx > 0xcd));
                start_menu_draw_level_box((s8)DS:4a28, 1);
            }
        }
} while (!(FAR:2ea2 == 3 && (flags & 7) != 0));               // right click with a mode chosen
FAR:513c = 0x56ce;                                             // end year: "endless"
if (flags & 2) FAR:513c = 0x7ac;                               // 1964: 1-year game
if (flags & 4) FAR:513c = 0x7ae;                               // 1966: 3-year game
FAR:56f4 = (((s8)flags >> 3) + 1) & 1;                         // 1 = modern start (bit 3 "Histor. Start" clear)
if (FAR:56f4) { FAR:513c += 0x1d; DS:07e0 = 0x7c8; }           // +29 years, season year 1992
DS:4a28 = 5 - DS:4a28;                                         // stored difficulty = 5 - shown level
screen_fade(0); bitmap_free(dlg); ui_redraw_buttons(0x7f, 0);
newgame_init_player_pool();                                    // §5.1  RNG
FAR:304a = (u8)n0;                                             // = 0
```
Resulting `FAR:513c` ("end year", save block 43): historical start 0x56ce/0x7ac/0x7ae, modern start
0x56eb/0x7c9/0x7cb. `DS:07e0` (block 24) is the current season year (initial data value 0x7ab = 1963).
Note the flag value `(flags>>3)` uses an arithmetic shift of the byte; with flags <= 0x0f it is 0 or 1.

#### 4.7b Adding managers to a running game (n0 != 0)

```c
screen_fade(0);
for (FAR:304a = n0; (s8)FAR:304a < DS:07ab; FAR:304a++) {
    m = FAR:304a; FAR:5358 = m;
    for (i = 0; i < 0x26; i++) M[m].b[0x10b + i] = 10;         // FAR:234d: table-position history
    for (i = 0; i < 5; i++)    M[m].b[0x132 + i] = 0x1e;       // FAR:2374
    for (i = 0; i < 0x32; i++) M[m].b[0x3e + 4*i] = 0xff;      // FAR:2280: result log
    for (k = 0; k < 4; k++)                                     // categories: GK, DEF, MID, FWD
        for (j = 0; j < DS:9438[k]; j++) {                      // 2, 5, 8, 5 players
            tries = 0;
            do { p = random_range(DS:07a6[k], DS:07a7[k]); tries++; }   // RNG
            while (P[p].owner != 5 && tries < 1000);
            P[p].owner = (u8)m;                                 // +0x21
            squad_insert_player((u8)p, 1, 0, 15000L, 0x18);     // 2ecd:37d8 (other area)
        }
}
FAR:304a = (u8)n0;
```
(`[bp-4]` ends this path holding `DS:9438[3] = 5`; it is reused in §4.8, faithful quirk.)

The forward range `DS:07a6[3]..DS:07a7[3]` = `0x6a..0x97` includes record 151, one past the pool (its
owner byte is `FAR:6dd1`); the original takes it when that byte is 5 or the 1000 tries end on it. The
port never takes it (see §13). The office reaches this path through sub icon 5/5 (§6.5); the port's
screen side is `start_menu_add_managers` (slots `0..n0` show the existing managers' crests from the
crest cache `FAR:5354` and their names; the header bar does not load with `allow_load = 0`), its logic
side `newgame::add_managers`. Verified in DOSBox (scenario `add_manager`): the new manager's club slot,
squad and finances match the original, and the season loop gives the new manager an office turn on the
same day because its manager loops re-read `DS:07ab` (season.md §3).

### 4.8 Per-manager initialisation (label 18bc:3950)

For `FAR:304a = n0 .. DS:07ab-1` (signed byte compare), `m = FAR:304a`, `FAR:5358 = m`:

```c
int is1y = (FAR:513c == 0x7ac || FAR:513c == 0x7c9);
int is3y = (FAR:513c == 0x7ae || FAR:513c == 0x7cb);
if (is1y) {                                   // DS:066c + 0x24*m = 9 longs per manager (save block 41)
    DS:066c[m][8] = 30000L; DS:066c[m][0] = 180000L;
    for (i = 1; i < 7; i++) DS:066c[m][i] = 45000L;
    bp4 = 7;                                  // [bp-4] after this loop (used below!)
}
if (FAR:2242 /* M[0].name[0] */ == 0) M[m].team = (u8)(m * 11);   // checks manager 0's name (faithful)
int tf = 0;                                   // start tier ([bp-2])
if ((FAR:513c > 0x7ae && !FAR:56f4) || (FAR:513c > 0x7cb && FAR:56f4)) tf = 2;   // unsigned compares
                                              //  -> endless games start in tier 2
M[m].l[0x1f0] = (DS:4a28 == 4) ? 1900000L : 1500000L;          // FAR:2432 (s32)
M[m].l[0x1ec] = 99999L;                                       // FAR:242e
int base = (s8)DS:4a28 / 2 + 0x1b;                             // [bp-0x60]; signed idiv
if (tf == 0) base += 0x29;
if (is1y)    base += 0x0c;
// --- move the chosen club into a random slot of tier tf ---
if (tf != 0 && (league_tier_of_team(M[m].team) >> 1) == tf && M[m].team <= DS:2277) {
    /* keep the slot */
} else {
    do {
REROLL:
        r = random_range(DS:226e[tf], DS:226e[tf] + DS:225e[tf] - 1);   // RNG (s8 table values)
        dup = 0; for (i = 0; i < (s8)FAR:304a; i++) if (M[i].team == r) dup = 1;
    } while (dup || team_in_european_cup_lists((u8)r, 7));
    t = M[m].team;
    team_swap_slots((u8)t, (u8)r, 0);       // §5.5: club identity moves to slot r
    team_stats_swap(t, r);                   // swaps TS[t]/TS[r] back (net: statistics stay per slot)
}
if (is1y && n0 == 0) {                       // 1-year game: club must play in the UEFA cup list (c=3)
    M[m].b[0x135] = 1;                       // FAR:2377
    do {
        r = random_range(DS:226e[tf], DS:226e[tf] + DS:225e[tf] - 1);   // RNG
        dup = 0; for (i = 0; i < (s8)FAR:304a; i++) if (M[i].team == r) dup = 1;
    } while (dup || !team_in_european_cup_lists((u8)r, 4));
    t = M[m].team; team_swap_slots((u8)t, (u8)r, 0); team_stats_swap(t, r);
}
// --- squad attributes (4 x squad_count passes; index of the count is i, of the record is m) ---
for (i = 0; i < 4; i++)
    for (k = 0; k < squad_count_by_flag((u8)i, 0); k++)
        for (j = 0; j < 3; j++) {
            v = random_range(base, base + 5);                    // RNG
            S[m][k].b[0x10 + j] = (u8)v;                          // FAR:775a+j
            w = random_range(0x28, 0x3c);                        // RNG (always drawn)
            if (j == 2) S[m][k].b[0x12] = (u8)w;
            if (j == 0) P[bp4].b[0x1c] = (u8)v;                   // FAR:57f9 of pool record [bp-4] (!)
            else if (j == 1) P[bp4].b[0x1d] = (u8)v;
            else             P[bp4].b[0x1e] = (u8)w;
        }
for (k = 0; k < squad_count_by_flag((u8)m, 0); k++)
    S[m][k].l[0x28] = player_value_calc((u8)k, 1);                 // 3277:25de, FAR:7772
for (k = 0; k < 5; k++) {
    M[m].b[0x141 + k] = DS:0286[k];                              // 05 04 06 05 06
    if (k < 4) M[m].b[0x146 + k] = DS:028c[k];                   // 01 03 03 03
}
M[m].b[0x138] = (u8)tf;                                          // FAR:237a: tier
M[m].b[0x10a] = (u8)((is1y + 8) * 2 - tf * 3);                   // FAR:234c
M[m].l[0x166] = (s32)(5 - tf) * 4000L;                           // FAR:23a8
M[m].l[0x15e] = (s32)(2 - tf) * 3000L;                           // FAR:23a0
if (is1y) { M[m].l[0x15e] += 5000; M[m].l[0x166] += 10000; }
M[m].l[0x16e] = (s32)(2 - tf) * 3500L;                           // FAR:23b0
M[m].l[0x186] = is1y + 4;                                        // FAR:23c8
M[m].l[0x18e] = is1y + 3;                                        // FAR:23d0
if (FAR:513c < 2000) {                                           // unsigned; historical start only
    M[m].l[0x17e] = 3 - 2*is3y;                                  // FAR:23c0
    M[m].l[0x176] = 3 - is3y;                                    // FAR:23b8
}
M[m].b[0x131] = 0x10;                                            // FAR:2373
M[m].w[0x1dc] = (u16)(tf * 0xffec) + 0x32;                       // FAR:241e = 50 - 20*tf (u16 MUL)
manager_compute_wage_budget((u8)m);                              // 18bc:0a63  RNG
FUN_24a4_2cb4();                                                 // advertising_offers_generate (stats.md §3.6, RNG)
if (n0 != 0) { auto_lineup(); squad_recalc_team_strength((u8)m, 0); }
```
`bp4` is the value of the local `[bp-4]` at this point: `7` after the 1-year loop of the same iteration;
otherwise whatever it held before — in a new game the carousel position `pos` (first manager) and `7`
or `pos` for later ones; in the add-managers path `5`. This writes into the pool record `P[bp4]`
(+0x1c/+0x1d/+0x1e) and is an original bug that must be reproduced.

`squad_count_by_flag(i, 0)` (4091:1109) counts the players of squad `i`; the outer loop uses `i` for the
count but always writes squad `m`.

#### `manager_compute_wage_budget` 18bc:0a63 — `void (u8 m)`

```c
u16 d = 2 - M[m].b[0x138];                         // tier as u8 -> word, 16-bit wrap
u16 r = random_range(15, 20);                      // RNG
u16 a = (u16)(d * (r + 40));                       // MUL, low word
u16 b = (u16)(((u16)(d * (u16)(0x6e - M[m].w[0x1dc]))) & 0xfffe) * 500;   // low words
M[m].w[0x1de] = a + b;                             // FAR:2420
int r2 = random_range((d + 10) * 10, (d + 4) * 25);                          // RNG (16-bit MUL)
M[m].w[0x1de] = (u16)(((s32)(u32)M[m].w[0x1de] * (s32)r2) / 100L);          // _aFlmul, _aFldiv
s32 x = (s32)(s16)((2 - M[m].b[0x138]) * 30) + (u32)M[m].w[0x1dc];        // IMUL, then ADD/ADC
DS:066c[m][7] /* DS:0688+0x24*m */ = x * 1000L;
```

### 4.9 Start-menu helpers

* **`draw_club_crest` 18bc:1cdd** — `void (u8 code, rastport far *rp, int x, int y)`:
  ```c
  s8 i = 0; while (DS:05dc[i] - code != 0x50 && DS:05dc[(s8)++i] != 0) ;   // see exact loop below
  if (DS:05da != 0 && DS:05dc[i] != 0) {           // "blank crest" list active and code listed
      spr = vga_load_picture(0, 0x26, 0x26, 5, 0); ui_fill_rect(spr, 0, 0, 0x25, 0x25, 0x1d);
  } else spr = vga_load_picture(code + 0x50, 0, 0, 0, 1);   // PIC\<80+code>.VGA
  ui_fill_rect(rp, x + 0x26, y, x + 0x27, y + 0x25, 0);
  blit_bitmap(*spr, 0, 0, rp, x, y, 0x26, 0x26, 0xc0);
  bitmap_free(spr);
  ```
  Exact search loop: `i = 0; loop: if ((u16)DS:05dc[i] - code == 0x50) break; i++; if (DS:05dc[i] == 0) break; goto loop;`
  `DS:05dc` = `67 68 6b 73 79 7a 7d 7e 7f 80 82 83 85 86 87 88 89 8a 8b 8d 8e 8f 90 00`, `DS:05da = 0`.
* **`team_in_european_cup_lists` 18bc:1df3** — `int (u8 team, u8 mask)`:
  `n = DS:07d6[FAR:0009]; for c = 1..3: if (mask & (1 << (c-1))) for i = 0; i < 2*n; i++: if FAR:76ca[c*32 + i] == team return 1; return 0;`
  (`n` = number of pairings of the current round; `DS:07d6` = `20 10 08 04 02 01`.)
* **`wait_5_ticks` 18bc:1e75** — `timer_set_deadline(5); while (!timer_deadline_reached());`
* **`start_menu_swap_portraits` 18bc:1e96** — `void (u8 a, u8 b)`:
  ```c
  blit_bitmap(FAR:2f24, a*0x29 + 1, 0, rp, b*0x50 + 0x15, 0xaa, 0x27, 0x28, 0xc0);
  blit_bitmap(FAR:2f24, b*0x29 + 1, 0, rp, a*0x50 + 0x15, 0xaa, 0x27, 0x28, 0xc0);
  blit_bitmap(screen_bitmap, b*0x50 + 0x15, 0xaa, FAR:4d38, b*0x29 + 1, 0, 0x27, 0x28, 0xc0);
  blit_bitmap(screen_bitmap, a*0x50 + 0x15, 0xaa, FAR:4d38, a*0x29 + 1, 0, 0x27, 0x28, 0xc0);
  ```
  (the portrait sheet `PIC\0.VGA` is updated so that portrait `k` belongs to slot `k`).
* **`team_stats_swap` 18bc:1fc4** — `void (int a, int b)`: allocate 0x36 bytes (fatal 4 on failure),
  swap `TS[a]` and `TS[b]` (54 bytes) via `far_memcpy_bytes(src, dst, n)`, free.
* **`start_menu_draw_level_box` 18bc:2061** — `void (int k, int selected)`:
  ```c
  int y = (k & 0xfe) * 9, x = (k % 2) * 0x1a;      // 2 columns, 3 rows
  u8 c1 = selected ? 0 : 1, c2 = selected ? 1 : 3;
  ui_fill_rect(rp, x + 0xb7, y + 0x4d, x + 0xca, y + 0x59, c1);
  ui_fill_rect(rp, x + 0xb8, y + 0x4e, x + 0xca, y + 0x59, c2);
  ui_fill_rect(rp, x + 0xb8, y + 0x4e, x + 0xc9, y + 0x58, 6);
  ui_draw_text_centered_shadow(rp, format_long_thousands((s32)k), x + 0xb8, x + 0xce, y + 0x56,
                               (u8)(selected * 0xf + 4), 0);
  ```
* **`FUN_362f_0004` 362f:0004** (UI, used by the settings screen too) — `void (bitmap far *b, int x, int y, int on)`:
  `blit_bitmap(b, 0x64 + 0x30*on, 0, rp, x, y, 0x18, 0x11, 0xc0); blit_bitmap(b, 0xac - 0x30*on, 0, rp, x + 0x1a, y, 0x18, 0x11, 0xc0);`

## 5. New-game world initialisation

### 5.1 `newgame_init_player_pool` 422b:035c — `void (void)`

```c
u8 far *buf = mem_alloc(40000, 0); if (!buf) fatal_error_exit(4);
char name[]; _fstrcpy(name, *(char far**)DS:93e8 /* "mana.dat" */);
fd = file_open(name, 0);
file_read(fd, buf, 0x3d8);           // skipped header part (overwritten)
file_read(fd, buf, 0x87c0);          // 64 team blocks of 0x21f bytes (see mana.md)
file_close(fd);
for (u16 s = 0; s < 4; s++) for (u16 k = 0; k < 25; k++) S[s][k].b[0x0f] = 0;   // FAR:7759
for (u16 p = 1; p < 0x97; p++) {     // RNG order per record:
    P[p].b[0x1a] = random_range(0x12, 0x21);                  // age 18..33
    u16 t = random_range(0x1e, 0x59);
    if (t < 0x2d && random_range(0, 1) != 0) t = random_range(0x1e, 0x59);
    P[p].b[0x1d] = (u8)(10 - ((u8)random_range(0, 0x14) - (u8)t));    // = t - r + 10 (u8)
    P[p].b[0x1e] = random_range(0x2d, 0x37);
    P[p].b[0x1c] = (u8)(10 - ((u8)random_range(0, 0x14) - (u8)t));
    P[p].b[0x1b] = (u8)(((u16)P[p].b[0x1c] + P[p].b[0x1d] + P[p].b[0x1e]) / 3);   // signed idiv
    P[p].b[0x20] = random_range(0, 6);
    P[p].owner /* +0x21 */ = 5;                                // 5 = free
}
for (FAR:304a = 0; (s8)FAR:304a < DS:07ab; FAR:304a++) {      // give each human manager a squad
    FAR:5358 = FAR:304a; s8 n = 0 /*[bp-0x48]*/;
    for (s8 k = 0; k < 4; k++)
        for (s8 j = 0; j != DS:9438[k]; j++) {                 // 2,5,8,5
            u16 p = DS:07a7[k] - DS:943c[k] + 1;              // take records from the top of the range
            DS:943c[k]--;                                      // (13,2c,2a,2d initially; not restored!)
            char far *nm = mana_team_player_name_ptr(buf, M[FAR:304a].team, (s32)n);
            _fstrcpy(P[p].name, nm);
            if (P[p].name[0] == 0)
                _fstrcpy(P[p].name, mana_pick_unused_player_name(buf, 0, k, 0xff));   // RNG
            n++;
            P[p].owner = FAR:304a;
            P[p].b[0x1a] = random_range(0x13, 0x1f);               // RNG
            squad_insert_player((u8)p, 2, 0, 15000L, 0x18);        // 2ecd:37d8 (other area)
            P[p].team /* +0x24 */ = M[FAR:304a].team;
        }
}
u8 k = 0;
for (u16 p = 1; p < 0x97; p++) {                              // remaining names
    if (!(DS:07a7[k] > p)) k++;
    if (P[p].name[0] == 0) {
        _fstrcpy(P[p].name, mana_pick_unused_player_name(buf, 1, k, (u8)p));   // RNG
        if (FAR:513c > 0x7d0) P[p].team = 0xff;               // endless game (end year > 2000): no team
    }
}
for (p = 1;    p <= 0x14; p++) P[p].b[0x1f] = 0;
for (p = 0x15; p <= 0x3f; p++) P[p].b[0x1f] = random_range(0, 0x10) + 0x19;   // RNG
for (p = 0x40; p <= 0x69; p++) P[p].b[0x1f] = random_range(0, 0x10) + 0x32;
for (p = 0x6a; p <= 0x96; p++) P[p].b[0x1f] = random_range(0, 0x10) + 0x4b;
mem_free(buf, 0);
```
Category ranges `DS:07a6..07aa` = `01 14 40 6a 97` (k: [1,20], [20,64], [64,106], [106,151]); note the
second loop's category switch uses `07a7[k] <= p`, so record 20 counts as category 1 there.
`DS:943c` is decremented permanently; a second new game in the same process would continue from there.

### 5.2 MANA.DAT name helpers

* **`mana_team_player_name_ptr` 422b:01d0** — `char far *(u8 far *buf, u8 team, s32 idx)`:
  `p = buf; for (u8 i = 0; i < 0x40; i++) { if (_fstrcmp(CLUB[team].name, p) == 0) break; p += 0x21f; }
   return p + 0x17 + (u16)(0x1a * (u16)idx);` (if no block matches, p = buf + 64*0x21f).
* **`mana_pick_unused_player_name` 422b:022e** — `char far *(u8 far *buf, u8 unused, s8 cat, u8 rec)`:
  ```c
  for (;;) {
      u8 slot = random_range(DS:9432[cat], DS:9433[cat] - 1);        // RNG; ranges 0-1, 2-7, 8-14, 15-19
      u8 t;
      do { t = random_range(0, DS:225e - 1);                          // RNG; a tier-0 club ...
           human = 0; for (s8 i = 0; i < DS:07ab; i++) if (M[i].team == t) human = 1;
      } while (human);                                                // ... not managed by a human
      nm = mana_team_player_name_ptr(buf, t, (s32)slot);
      _fstrcpy(tmp, nm);
      if (tmp[0] == 0) continue;
      for (i = 1; i < 0x97; i++) if (_fstrcmp(tmp, P[i].name) == 0) break;
      if (i != 0x97) continue;                                        // already used
      if (rec != 0xff) P[rec].team = t;
      return nm;
  }
  ```

### 5.3 `assign_pool_players_to_teams` 24a4:19fb — `void (void)`

```c
int n[3]; FUN_24a4_1798(n);                       // other area (uses RNG): counts per tier
u8 lst[3][20];
for (u8 k = 0; k < 3; k++) rank_tier_teams_by_strength(k, lst[k]);   // 1cb5:2a3b
for (u8 p = 1; p < 0x97; p++)
    if (manager_index_of_team(P[p].team, 0xff) == 0 &&
        (P[p].team == 0xff || FAR:a7a0 /* s32 */ != 0))
        for (u8 k = 0; k < 3; k++)
            if (n[k] != 0) { P[p].team = pick_random_free_team_in_tier(k, lst[k]); n[k]--; }   // 1cb5:2987 RNG
```
(Several `k` may overwrite the same record; faithful.) The year test is `!= 0` (disassembly 24a4:1a5d..1a76:
team `0xff` jumps into the tier loop, otherwise a zero `FAR:a7a0|a7a2` skips the player). The callee
never reads `lst[k]` (1cb5:2987..2a3a only reads its first argument), so the ranking has no effect.

### 5.4 `league_shuffle_teams` 1310:09c5 — `void (void)`

```c
for (u8 t = 0; t < 3; t++) {
    u8 lo = DS:226e[t], hi = (u8)(lo + DS:225e[t] - 1);         // tiers: 0..17, 18..37, 38..57
    for (u8 n = 0; n < 0x37; n++) {                              // 55 swaps per tier
        u8 a = random_range(lo, hi);                             // RNG
        u8 b = random_range(lo, hi);                             // RNG
        team_swap_slots(a, b, 1);                                // flag = 1: DS:07ac..07b0 swapped too
        for (u8 i = 0; (s8)DS:225e[t] > i; i++)
            if (FAR:535a[t*20 + i] == a)                         // swap entries a and b in the
                for (u8 j = 0; (s8)DS:225e[t] > j; j++)          // league order list FAR:535a[3][20]
                    if (FAR:535a[t*20 + j] == b) {
                        swap(FAR:535a[t*20 + i], FAR:535a[t*20 + j]);
                        i = j = 0x14;                            // ends both loops
                    }
    }
}
```
Note on the call: the compiler pushes the constant third argument `1` **before** evaluating the second
`random_range` (cdecl right-to-left), so in the listing it looks like a third argument of
`random_range`; it is the `flag` of `team_swap_slots`. If `a == b` nothing changes (the swaps cancel).

### 5.5 `team_swap_slots` 1310:0b24 — `void (u8 a, u8 b, u8 flag)` (+ helpers)

```c
if (a > 0x3f || b > 0x3f) return;                    // only the 64 German slots
for (u8 p = 1; p < 0x97; p++)                        // pool records: team field
    if (P[p].team == a) P[p].team = b; else if (P[p].team == b) P[p].team = a;
for (u8 m = 0; m < DS:07ab; m++)
    if (M[m].team == a) M[m].team = b; else if (M[m].team == b) M[m].team = a;
if (flag) for (x in DS:07ac, 07ad, 07ae, 07af, 07b0) swap_team_ref_byte(&x, a, b);
swap 0x22 bytes CLUB[a] <-> CLUB[b]          (temp mem_alloc(0x22), fatal 4)
swap 0x36 bytes TS[a]   <-> TS[b]            (temp mem_alloc(0x36), fatal 4)
swap 0x15 bytes FAR:9d36+0x15*a <-> +0x15*b  (stack temp)
swap 8 bytes    FAR:a2ca+8*a    <-> +8*b
swap 8 bytes    FAR:a4ca+8*a    <-> +8*b
team_swap_in_table_a4ca(a, b);
for (u8 m = 0; m < DS:07ab; m++)
    swap 10 bytes FAR:9336 + 10*(m*64 + a) <-> FAR:9336 + 10*(m*64 + b)
```
* `swap_team_ref_byte` 1310:0ae8 — `void (u8 far *p, u8 a, u8 b)`: `if (*p - 1 == a) *p = b + 1; else if (*p - 1 == b) *p = a + 1;` (16-bit compare of `(u16)*p - a - 1 == 0`).
* `team_swap_in_table_a4ca` 1310:0f44 — `void (u8 a, u8 b)`: for `i < 0x40`, `j < 8`: `v = FAR:a4ca[i*8+j]; if (v == a) v = b; else if (v == b) v = a;`.

### 5.6 `euro_cup_qualifiers_build` 27c2:0ef2 — `void (void)`

Builds `FAR:57aa[3][6]` (save block 26; row 0 at 57aa, row 1 at 57b0, row 2 at 57b6; 0x80 = empty) and
the "already qualified" list `FAR:56fe[]` with length `FAR:2e74` (`team_is_euro_qualified` 27c2:11e7
searches it). `DS:07ac..07b0` hold **1-based** team ids (0 = none) of last season's results.

```c
s8 extra = DS:226d;                         // number of additional UEFA places (4)
u8 r0 = 0, r1 = 0, r2 = 0, next = 1;        // [bp-2],[bp-0x10],[bp-6],[bp-8]
FAR:2e74 = 0;
for (i < 3) for (j < 6) FAR:57aa[i*6+j] = 0x80;
#define ADD(v) (FAR:56fe[FAR:2e74++] = (v))
FAR:57aa[r0++] = FAR:535a[0]; ADD(FAR:535a[0]);                  // champion (league order list tier 0)
if (DS:07ad) { v = !team_is_euro_qualified(DS:07ad-1) ? DS:07ad-1 : FAR:535a[1];
               FAR:57aa[r0] = v; ADD(v); }                        // r0 not incremented
if (!team_is_euro_qualified(DS:07ac-1))      { v = DS:07ac-1; FAR:57b0[r1++] = v; ADD(v); }
else if (!team_is_euro_qualified(DS:07b0-1)) { v = DS:07b0-1; FAR:57b0[r1++] = v; ADD(v); }
if (DS:07ae) {
    if (!team_is_euro_qualified(DS:07ae-1))      { v = DS:07ae-1; FAR:57b0[r1++] = v; ADD(v); }
    else if (!team_is_euro_qualified(DS:07b0-1)) { v = DS:07b0-1; FAR:57b0[r1]   = v; ADD(v); }  // no ++
}
if (DS:07af) {
    if (!team_is_euro_qualified(DS:07af-1)) { v = DS:07af-1; FAR:57b6[r2++] = v; ADD(v); }
    else extra++;
}
while (extra != 0) {                         // fill UEFA places from the league order
    v = FAR:535a[next];
    if (!team_is_euro_qualified(v)) { FAR:57b6[r2++] = v; ADD(v); extra--; }
    next++;
}
```
(All bytes `u8`; `- 1` on a 0 value gives 0xff.)

### 5.7 `cup_draw_first_round` 27c2:09e0 — `void (u8 c, u8 already)` ("competitions init")

`c` = 0 DFB-Pokal, 1..3 European cups. Draw table `FAR:76ca[4][32]` (save block 28), `FAR:0008[c]` =
current round of competition `c` (block 37), `FAR:57c8` = "draw is being shown" flag, `M[m].b[0x132+c]`
(`FAR:2374+c`) = manager's team takes part (1) / not (0x1e).

```c
FAR:57c8 = 0; FAR:0008[c] = 0; u8 skip = 0;
// 27c2:0a0b je 0a15 (c==0 -> open); 27c2:0a10 already==0 -> je 0a1e (skip open)
if (c == 0 || already != 0) cup_draw_screen_open(c);         // 27c2:0006, UI only (no RNG); may set 57c8
// => startup calls (1..3, 0): no screen, FAR:57c8 stays 0, draw only (RNG).
//    new game (0, 0): screen + draw;  new game (1..3, 1): screen, TAIL animation, close; no RNG.
if (c != 0 && already != 0) goto TAIL;
u8 lo  = (c != 0) << 6;                                       // 0 / 64
u8 hi  = (u8)((0xc7 - DS:2277) * (c != 0) + DS:2277);         // 57 / 199
u8 r_from = (c != 0), r_to = 4 - 3*(c == 0);                  // rows checked for duplicates: [0,1) / [1,4)
input_wait_release();
if (c == 1) for (i < 32) for (k < 3) FAR:21e2[k*32 + i] = 0;
for (m < DS:07ab) M[m].b[0x132 + c] = 0x1e;
u8 hs[4];                                                     // human slots (c == 0)
if (c == 0)
    for (m = 0; m < DS:07ab; m++) {
        hs[m] = 0;
        do { hs[m] = random_range(0, 31);                     // RNG
             dup = 0; for (i = 0; i < m; i++) if (hs[i] == hs[m]) dup = 1; } while (dup);
        M[m].b[0x132] = 1;
        FAR:76ca[hs[m]] = M[m].team;
    }
mouse_pointer_hide(0);
u8 s = 0;                                                     // loop test at 27c2:0ce8
while (s < 32) {
    input_poll_mouse(FAR:6dc0); if (FAR:2ea2 == 1) skip = 1;  // 27c2:0cee: click = fast draw (once per slot)
RETRY:                                                        // 27c2:0d13
    dup = 0;
    for (m = 0; m < DS:07ab; m++) if (c == 0 && hs[m] == s) m = 0x14;   // ends with m == 0x15
    if (m != 0x15) {
        FAR:76ca[c*32 + s] = random_range(lo, hi);            // RNG
        for (r = r_from; r < r_to; r++) {
            u8 end = (r == c) ? s : 32;
            for (i = 0; i < end; i++) if (FAR:76ca[r*32 + i] == FAR:76ca[c*32 + s]) dup = 1;
        }
        for (m = 0; m < DS:07ab; m++) if (FAR:76ca[c*32 + s] == M[m].team) dup = 1;
    }
    if (dup) goto RETRY;                                      // 27c2:0ca6 jnz 0d13: redraw same slot,
                                                              // mouse is NOT polled again
    if (c == 0 && FAR:57c8) cup_draw_animate_slot(s, FAR:76ca[c*32+s], skip, c);  // 27c2:0378
    s++;                                                      // then poll mouse for the next slot
}
mouse_pointer_show();
if (c == 0) {
    for (s = 0; s < 32; s++) cup_amateur_home_swap(FAR:76ca[s], FAR:76c9[s] /* slot s-1 */, s);
    goto TAIL;
}
FAR:1d17[c] = 0;
for (k = 0; FAR:57a4[c*6 + k] != 0x80; k++) {                // qualifier row c-1 (§5.6)
    do r = random_range(1, 31); while (FAR:76ca[c*32 + r] < 0x40);   // RNG: replace a foreign club
    FAR:76ca[c*32 + r] = FAR:57a4[c*6 + k];
    for (m < DS:07ab) if (FAR:76ca[c*32 + r] == M[m].team) M[m].b[0x132 + c] = 1;
}
TAIL:
if (c != 0 && FAR:57c8) {
    mouse_pointer_hide(0);
    for (s = 0; s < 32; s++) { input_poll_mouse(FAR:6dc0); if (FAR:2ea2 == 1) skip = 1;
                               cup_draw_animate_slot(s, FAR:76ca[c*32+s], skip, c); }
    mouse_pointer_show();
}
FAR:0008[c] = 1;
if (FAR:57c8) cup_draw_screen_close(c);                       // 27c2:089a "TASTE DRUECKEN"
```
* **`cup_amateur_home_swap` 27c2:1187** — `void (u8 t, u8 prev, u8 s)`:
  `if (league_tier_of_team(t) == 4 && league_tier_of_team(prev) < 4 && FAR:0008[0] < 4 && (s & 1)) swap(FAR:76c9[s], FAR:76ca[s]);`
  (For `s = 0` the bit test fails, so `FAR:76c9` itself is never touched.)

### 5.8 `cup_clear_match_goals` 27c2:1b6d — `void (void)`
`for (u8 i = 0; i < 0x20; i++) for (u8 k = 0; k < 4; k++) FAR:4c7e[k*32 + i] = 0;` (save block 33).

### 5.9 `interest_rates_update` 212a:000a — `void (void)`
```c
int e = (s8)DS:4a28 > 2;                        // easier levels
DS:0630 += random_range(0, 2 + e) + e - 1;       // RNG, s16
clamp_int(&DS:0630, 2, 7);                       // 4091:1009
int v = DS:0630, d = 5 - DS:0630;
for (u8 i = 0; i < 6; i++) {
    DS:060e[i] = (u8)v;                          // save block 0 (observed 07 05 03 02 02 02)
    v += d; if (d > 1) d--;
    clamp_int(&v, 2, 7);
}
```
Note `if (d > 1) d--` tests the old `d` (the `ADD` used it before the decrement).

## 6. `office_menu_dispatcher` 18bc:0b5f — `int (u8 arg)`

Called once per manager and day by `season_main_loop`. `arg == 0xff` forces the "SAISONENDE" banner
(otherwise `arg` is unused except as the "open submenu" seed, see §6.6). Returns when the manager ends
the turn (value of `office_handle_free_click`, 1). Local frame 0x18c bytes.

### 6.1 Entry (once)

```c
u8 reopen = 0 /*[bp-0x5a]*/, warned = 0 /*[bp-0x56]*/;
if (DS:5256 /* autosave_pending, set by month end 212a:0a6d */) { screen_save_game(DS:026b /* "autosave.man" */); DS:5256 = 0; }   // 422b:07fe
FAR:5358 = FAR:304a;
s32 day_saved = DS:07dc;                         // [bp-0xc]
screen_fade(0);
DS:07b2 = 1;
for (int t = 0; t < 3; t++)                      // make sure the league tables are current
    if ((s8)DS:225a[t] < (s8)DS:2262[t]) {
        FUN_3a41_10e3(t, DS:225a[t]);            // league_team_range (select round)
        FUN_3a41_2d33((u8)t, 1, 1);              // compute table
    }
```

### 6.2 Office redraw (label 18bc:0c1c, re-entered after every screen)

```c
u8 team = M[cur].team;                            // cur = FAR:304a
s8 games = TS[team].b[0x1e] + TS[team].b[0x1f];   // [bp-0x156]
float step = (games > 1) ? (float)(116.0 / (double)(s16)(games - 1)) : 116.0f;   // FP emulator code
office_show_cached_background();
float acc = 0.0f;  u8 sel = 0xff /*[bp-0x14]*/;  s8 k = 0 /*[bp-6]*/;
int px = 0x3f;                                    // [bp-0x54]
int py = (games > 1) ? M[cur].b[0x10b] + 0x40 : 0x4a;   // [bp-0x58], first point of the curve
rastport_set_draw_mode(rp, 0);
blit_bitmap(FAR:0024, 0, 0, rp, 0, -3, 0x140, 0x2c, 0xc0);        // PIC 4 header
blit_bitmap(FAR:2e9a, 0, 0, rp, 0, 0x29, 0x13f, 0x45, 0xc0);      // PIC 0x17 desk/top panel
blit_bitmap(FAR:2f24, cur*0x29 + 1, 0, rp, 0x10, 0x3b, 0x27, 0x28, 0xc0);   // manager portrait
blit_bitmap(FAR:5354, cur*0x29, 0, rp, 0xbb, 0x3c, 0x26, 0x26, 0xc0);       // club crest
ui_set_font(rp, 1);
DS:079c = 1;
```
**Competition-of-the-day banner** (`buf[]` = `[bp-0x52]`), with `tier = M[cur].b[0x138]` and
`wk = DS:2280[FAR:016e]` (per-week competition bitmask, save block 50):
```c
FAR:513e = 0;
FAR:56ee = 15 - 2*(DS:224e/2250 == 4L);           // max lineup slot (bench size)
if (arg == 0xff ||
    ((s32)((s8)DS:224c + 0x14) == FAR:2eba && FAR:2e8e == 5L))      // day == start+20 and month == 5
    strcpy(buf, DS:25b8[] /* "SAISONENDE" */);
else if ((wk >> tier) & 1) {                       // league match day of this tier
    strcpy(buf, format_long_thousands((s32)(s8)DS:225a[tier]));
    append_ordinal_suffix(buf, DS:225a[tier]);     // 1310:1fe7 (".")
    strcat(buf, DS:25bc[] /* " Spieltag" */);
} else if (FAR:0008[0] == M[cur].b[0x132] && wk == 8) {
    strcpy(buf, "DFB-Pokal" /* DS:25c0 */); FAR:513e = 1;
} else if ((M[cur].team == FAR:5369 || M[cur].team == FAR:5370) && wk == 0x10)
    strcpy(buf, "Relegation" /* DS:25c4 */);
else {
    int eu = 0; for (i = 1; i < 4; i++) if (FAR:0008[i] == M[cur].b[0x132 + i]) eu = 1;
    if (eu && (wk & 0x70) && wk != 0x10) { strcpy(buf, "Europapokal" /* DS:25c8 */);
                                          FAR:56ee = 15; FAR:513e = 1; }
    else if (has_rescheduled_match_today(FAR:016e) && (wk & 0x80))
        strcpy(buf, "Nachholspiel" /* DS:25cc */);
    else strcpy(buf, "Spielfrei" /* DS:25d0 */);
}
```
**Bench-size normalisation** of the own squad (`n = squad_count_by_flag(cur, 0)`, slot byte `S[cur][k].b[0x0a]`, `L = (s8)FAR:56ee`):
```c
int has_L = 0, x = 0;
for (k < n) if (S[cur][k].b[0x0a] == L) { has_L = 1; x = 0; }
for (k < n) if ((int)S[cur][k].b[0x0a] - L == 1 && !(x | has_L)) S[cur][k].b[0x0a] = (u8)FAR:56ee;
for (k < n) if ((int)S[cur][k].b[0x0a] > L) S[cur][k].b[0x0a] = 0;
auto_lineup();                                     // 2ecd:3360
ui_draw_text_centered_shadow(rp, buf,        0x4a, 0xa5, 0x3a, 0, 4);
ui_draw_text_centered_shadow(rp, M[cur].name, 0x3e, 0xb2, 0x62, 0, 4);
office_draw_date_panel();                          // 1310:0fb6
for (i = 0; i < 3; i++) {                          // six main icon buttons
    ui_draw_button(rp, 5,     i*0x2b + 0x6e, 0, i,     &main_rect[i],   0, 8);
    ui_draw_button(rp, 0x10a, i*0x2b + 0x6e, 0, i + 3, &main_rect[i+3], 0, 8);
}
DS:07dc = day_saved;
```
**League-position curve** (x 0x3f..0xb1, y = 0x40 + position, colour 0x0b), all float math in
single precision stored to `[bp-0x14e]`/`[bp-0x154]` (the emulator computes in 80-bit and rounds on
each `FSTP dword`):
```c
for (int i = 0; i < 0x73; i++) {
    if (acc > step) {                              // FCOM, strictly greater
        acc = acc - step;
        ui_draw_line(rp, px, (u8)py, i + 0x3e, (u8)(M[cur].b[0x10c + k] + 0x41), 0x0b);
        px = i + 0x3e;
        py = M[cur].b[0x10c + k] + 0x41;           // FAR:234e+k
        k++;
    }
    acc = acc + 1.0;                               // DS:9a58 (double 1.0)
}
if (DS:07dc > 1L)
    ui_draw_line(rp, px, (u8)py, 0x3e + 0x73, (u8)(M[cur].b[0x10c + k] + 0x41), 0x0b);
else
    ui_draw_line(rp, 0x3f, 0x4a, 0xb1, 0x4a, 7);  // empty chart: grey line
if (FAR:56e2[cur]) { FAR:56e2[cur] = 0; screen_messages(0, 0); }   // 4091:05c4: unread mail
if (!reopen) screen_fade(1);
if (!warned) { season_goal_messages(); warned = 1; }               // 212a:314d
```
Constants: `DS:9a22` = 116.0 (double), `DS:9a2c` = 116.0f, `DS:9a30` = 0.0f, `DS:9a58` = 1.0 (double).

### 6.3 Main-icon loop (label 18bc:141e)

```c
for (;;) {
    int si;
    if (reopen) si = reopen;                               // a submenu has to be reopened
    else {
        si = ui_hit_test_buttons(main_rect, 6, 1);
        if (si == 0) {
            if (FAR:2ea2 == 3) { if (ui_hit_test_buttons(main_rect, 6, 0) == 6) quit_game_confirm(); }
            else ret = office_handle_free_click(sel);       // §6.6, ret = [bp-0x182]
            // NB: after quit_game_confirm the stale previous ret is tested (faithful)
            if (ret == 1) return;                           // end of turn
            if (ret == 2) goto REDRAW;                      // §6.2
            continue;
        }
    }
    if (si >= 6 || si == (s8)sel) continue;                 // icon 6 = quit (right click only)
    int quick = (reopen && sel != 0xff);                    // [bp-0x150]
    OPEN_SUBMENU(si, quick);                                // §6.4
}
```
Main icons (icon numbers passed to `ui_draw_button`): left column x=5: 0 "Info", 1 "Tabellen",
2 "Mannschaft"; right column x=0x10a: 3 "Pokal/Europa", 4 "Disk/Optionen", 5 "Spiel beenden"
(names inferred from the screens they open; y = 0x6e + 0x2b*row).

### 6.4 Submenu (3x3 icon grid)

`DS:0279[si]` = base icon, `DS:027f[si]` = number of sub-icons, `si` = 1..6:
base `00 06 0f 15 19 00`, count `06 09 06 04 05 00`.

```c
if (sel >= 1)     // unpress the previous main icon
    ui_draw_button(rp, 5 + 0x105*((sel-1) > 2), 0x6e + 0x2b*((sel-1) % 3), 0, (sel-1) + 0x2000, &tmp, 0, 8);
ui_draw_button(rp, 5 + 0x105*((si-1) > 2), 0x6e + 0x2b*((si-1) % 3), 0, (si-1) + 0xA000 /* (si-1)-0x6000 */, &tmp, 0, 8);
s8 c = 0;
for (int col = 0; col < 3; col++)                 // di
    for (int row = 0; row < 3; row++) {           // [bp-0x188]
        int icon = (c == 0x63) ? 0x1023 : c + (s8)DS:0279[si];
        if (icon == 0x1c) icon = 0x5e;
        if (icon == 0x1d) icon = 0x90;
        int x = col*0x36 + 0x52, y = row*0x2a + 0x70;
        ui_draw_button(rp, x, y, 0, (quick << 14) + icon + 6, &sub_rect[col*3 + row], 0, 8);
        if ((si == 3 && c == 4 && M[cur].b[0x139] != 0) ||                       // FAR:237b, 18bc:1624
            (si == 1 && c == 5 && (FAR:513c == 0x7ac || FAR:513c == 0x7c9)) ||       // 18bc:163b
            (si == 5 && c == 4 && DS:07ab == 4))
            ui_draw_button(rp, x, y, 0, 0x4029, &sub_rect[col*3 + row], 0, 8);    // "disabled" overlay
        if (c != 0x63 && (s8)DS:027f[si] - 1 > c) c++; else c = 0x63;
    }
sel = si;
far_memcpy_bytes(main_rect, &sub_rect[9], 0x30);  // rect list = 9 sub + 6 main = 15 rects
if (reopen) screen_fade(1);
```
Grid click loop:
```c
for (;;) {
    si = ui_hit_test_buttons(sub_rect, 15, 1);
    int valid = si != 0 && si - sel != 9 && !((s8)DS:027f[sel] < si && si < 10) && si != 15
             && !(sel == 3 && si == 5 && M[cur].b[0x139] != 0)                  // 18bc:17a8
             && !(sel == 1 && si == 6 && (FAR:513c == 0x7ac || FAR:513c == 0x7c9))   // 18bc:17bf
             && !(sel == 5 && si == 5 && DS:07ab == 4);
    if (!valid) {
        if (FAR:2ea2 == 3) {
            if (ui_hit_test_buttons(sub_rect, 15, 0) != 15) {         // right click: close the submenu
                blit_bitmap_or_restore_page2(FAR:56fa, 0x39, 0x70, screen_bitmap, 0x39, 0x70, 0xce, 0x89, 0xc0, 7, 0, 0);
                if (sel >= 1) ui_draw_button(rp, 5 + 0x105*((sel-1) > 2), 0x6e + 0x2b*((sel-1)%3), 0, sel-1, &tmp, 0, 8);
                sel = 0xff; reopen = 0; goto MAIN_LOOP;               // §6.3
            }
            quit_game_confirm();                                      // right click on icon 15 (= main 6)
        }
        ret = office_handle_free_click(sel);
        if (ret == 1) return; if (ret == 2) goto REDRAW; continue;
    }
    reopen = 0;
    if (si > 9) {                                                     // another main icon
        reopen = si - 9;
        ui_draw_button(rp, 5 + 0x105*((sel-1) > 2), 0x6e + 0x2b*((sel-1)%3), 0, sel-1, &tmp, 0, 8);
        goto MAIN_LOOP;
    }
    int k = si - 1;
    ui_wait_right_click_button(k/3*0x36 + 0x52, k%3*0x2a + 0x70);    // 4091:1722 highlight
    input_wait_release();
    office_show_cached_background();
    ui_redraw_buttons(0x7f, 0);
    DISPATCH(sel, si);                                                // §6.5
    screen_fade(0);
    palette_load_rgb4(FAR:4b50, DS:0794 /* slot 0 */, 0x100);
    ui_redraw_buttons(0x7f, 0);
    if (!(sel == 5 && si <= 2)) reopen = sel;                         // reopen the submenu afterwards
    calendar_compute_date(0);                                         // 1310:1190
    FAR:304a = FAR:5358;
    goto REDRAW;
}
```

### 6.5 Submenu actions (two jump tables + direct cases)

| main (sel) | sub (si) | call |
|---|---|---|
| 1 Info (table 18bc:1a15) | 1 | `FUN_362f_0af7()` *screen_statistics* |
| | 2 | `FUN_362f_18e7()` *screen_eternal_table* |
| | 3 | `FUN_362f_390e()` *screen_manager_history* |
| | 4 | `screen_stadium()` 1000:0602 |
| | 5 | `screen_credits()` 212a:167f |
| | 6 | `office_free_bitmaps(3); FUN_362f_2be8()` *screen_manager_contract*`; office_load_bitmaps(3)` (only in 1-year games, see disable rule) |
| 2 Tabellen (direct) | 1..3 | `FUN_3a41_214c(si-1, 1)` *screen_league_table* (tier si-1) |
| | 4..6 | `FUN_3a41_120a(si-4, 0, 0, 1)` *screen_fixtures* (bytes) |
| | 7..9 | `FUN_3a41_3891(si-7)` *screen_strength_table* |
| 3 Mannschaft (table 18bc:1b42) | 1 | `screen_squad_lineup(0, 0xff)` 2ecd:1560 |
| | 2 | `screen_squad_lineup(0x63, 0xff)` |
| | 3 | `screen_transfer_market()` 3277:04a5 |
| | 4 | `screen_training()` 212a:274c |
| | 5 | `office_free_bitmaps(3); screen_training_camp(); office_load_bitmaps(3)` (disabled while `M[cur].b[0x139] != 0`, the camp cooldown) |
| | 6 | `FUN_24a4_1ad5(0)` *screen_top_scorers* |
| 4 Pokal (direct) | 1..4 | `screen_cup_matches(si-1, 0)` 27c2:1ccb |
| 5 Disk (direct) | 1 | `FUN_422b_120c(0)` *screen_load_game*; then `day_saved = DS:07dc; warned = 0` |
| | 2 | if `DS:05ad == 0`: `FUN_422b_07fe(DS:0278 /* "" */)` *screen_save_game* |
| | 3 | if `DS:05da == 0`: `FUN_422b_2366(1)` *highscore_screen* |
| | 4 | `FUN_362f_01b8()` *screen_settings* (Einstellungen; owned by the settings area) |
| | 5 | `save = FAR:304a; office_free_bitmaps(2); start_menu_new_or_load(0); office_load_bitmaps(2); FAR:304a = FAR:5358 = save;` (disabled with 4 managers) |

Sub-icon numbers passed to `ui_draw_button` are `base + c + 6` (after the 0x1c→0x5e, 0x1d→0x90 remap):
menu 1: 6..11, menu 2: 12..20, menu 3: 21..26, menu 4: 27..30, menu 5: 31, 32, 33, 0x64, 0x96; empty
cells: 0x1029.

### 6.6 `office_handle_free_click` 18bc:08f2 — `int (u8 sel)`

Handles a click that hit no icon. `my = ms[0]`, `mx = ms[1]`. `DS:05d4` = auto-play flag ("Spiele
automatisch"), `DS:05ad` = flag that disables saving and ending the turn by click (0 at program start;
`screen_save_game` sets it after every save from the menu, see stats.md §5.2; the settings cheat `d`
clears it). Verified in DOSBox: after "Spielstand Speichern" the save sub-icon only redraws the office and
a click on the calendar sheet no longer ends the turn.
```c
if (FAR:2ea2 != 1 && DS:05d4 == 0) return 0;
if (my == 0 && mx == 0 && DS:05d4) {                     // pointer in the top-left corner: stop auto play
    ui_dialog_box("Spiele nicht" /*DS:0242*/, "alleine !" /*DS:0238*/); DS:05d4 = 0; return 0;
}
if (DS:05ad == 0 &&
    (DS:05d4 || (mx > 0xf2 && my > 0x31 && mx < 0x132 && my < 0x66))) {   // "next day" area / auto play
    screen_fade(0); ui_redraw_buttons(0x7f, 0); return 1;                 // END OF TURN
}
if (my == 1 && mx == 1 && DS:05d4 == 0) {                // click at (1,1): start auto play
    ui_dialog_box("Spiele automatisch" /*DS:0258*/, "weiter !" /*DS:024f*/);
    DS:05d4 = 1; DS:063a = 0x47; return 0;
}
if (my < 0x2c) {                                         // header bar
    screen_fade(0); matchday_info_panel(); ui_redraw_buttons(0x7f, 0); return 2;   // 1cb5:2bf9
}
if (sel == 0xff && mx > 0x39 && my > 0x71 && mx < 0x108 && my < 0xfd)
    screen_messages(0, 0);                               // desk area: mail
return 0;
```
(With `DS:05d4` set, every call ends the turn immediately: automatic play.)

### 6.7 `quit_game_confirm` 18bc:1c1d — `void (void)`
```c
ui_draw_button(rp, 0x10a, 0xc4, 0, 0x40c8, &r, 0, 8);      // pressed quit icon (icon 200 = PIC 0x25)
if (dialog_two_choice(FAR:304a, "M|CHTEN SIE", "DAS SPIEL WIRKLICH", "BEENDEN ?",
                      "LEIDER JA", "KEIN GEDANKE"))          // 4091:146c, strings DS:4d94..4dce
    fatal_error_exit(99);
ui_draw_button(rp, 0x10a, 0xc4, 0, 0x4005, &r, 0, 8);      // normal quit icon
```

### 6.8 `has_rescheduled_match_today` 1310:000a — `u8 (int week)`
```c
u8 tier = M[cur].b[0x138], found = 0;
for (u8 i = 0; i < 20; i++) {
    u8 *e = &FAR:5714[5*i];                     // rescheduled-match table (save block 14)
    if (e[1] != week) continue;
    if (FAR:304a == 0x1e) return 1;
    if (e[3] != tier) continue;
    FUN_3a41_10e3(tier, e[2]);                  // select fixture round
    u8 *f = &FAR:4b5e[(tier*10 + e[4]) * 2];    // pairing (home, away)
    if (f[0] == M[cur].team || f[1] == M[cur].team) found = 1;
}
FUN_3a41_10e3(tier, DS:225a[tier]);             // restore current round
return found;
```

## 7. Notes for the implementer

### 7.1 Office screen layout summary

| element | position | source |
|---|---|---|
| background | full screen | `PIC\22.CP` via VRAM cache (§3.4) |
| header | (0,-3) 320x44 | `PIC\4.VGA` |
| top panel | (0,41) 319x69 | `PIC\23.VGA` (0x17) |
| portrait | (16,59) 39x40 | `PIC\0.VGA` cell `cur` |
| crest | (187,60) 38x38 | crest cache `FAR:5354` cell `cur` |
| banner text | centred x 74..165, y 58, font 1, colour 0 shadow 4 | §6.2 |
| manager name | centred x 62..178, y 98 | |
| main icons | x 5 / 266, y 110, 153, 196 (50x41) | `ui_draw_button` |
| sub icons | x 82 + 54*col, y 112 + 42*row | |
| curve | x 63..177, y 0x40 + table position | §6.2 |
| "end of turn" hot area | x 243..305, y 50..101 | §6.6 |
| mail hot area | x 58..263, y 114..252, only without open submenu | §6.6 |

### 7.2 Floating point in the office

The office is the only place in this area that uses floating point (MS 8087 emulator). The code is
`FILD word; FDIVR qword [DS:9a22]; FSTP dword` for the step, `FLD dword; FCOM dword; FSTSW; SAHF; JA`
for the comparison and `FLD; FSUBR; FSTP` / `FLD; FADD qword [DS:9a58]; FSTP` for the accumulator.
Implement with `f32` storage and `f64` (or exact) intermediate; `acc` takes the values k*1.0 minus
multiples of `step`, so `f32` rounding matters only for non-integral `step`.

## 8. Copy protection

`copy_protection_penalty` 16c7:09f9 — `void (...)` (no frame; callers push 4–5 dummy arguments to make
it look like a drawing call; it ignores them):
```c
if (DS:05ac == 0 && random_range(0, 100) == 0)       // RNG call only when the flag is 0
    for (;;) screen_fade(random_range(0, 1));        // endless flicker = hang
```
Trigger flag **`DS:05ac`**: initial data value **1** (= passed). It is written only by
* `screen_save_game` 422b:07fe: `DS:05fa[0] = DS:05ac` before writing save block 54;
* `screen_load_game` 422b:120c: `DS:05ac = DS:05fa[0]` after reading block 54;
* `1cb5:0000`: `DS:05ac = 1` (called by the loader when the tail of the file is short).

So in this version the trap only fires after loading a save whose block 54 byte 0 is 0 (e.g. written by
a cracked/older build); with the flag 0, each of the 21 callers (all big screens, including the start
menu redraw) consumes one RNG value and hangs with probability 1/101. No code wheel / manual query was
found in BMMAIN. A faithful port must keep the conditional `random_range(0,100)` call because it shifts
the RNG stream when the flag is 0.

## 9. Data tables (DGROUP)

| DS | bytes | meaning |
|---|---|---|
| 0206 | `"pic\1.pal"` | palette file |
| 0210 | `00` | office background cached flag |
| 021e | `"0"` | argv value that disables sound |
| 0220 | `"sound\bm2title.cmf"` | title music |
| 0234 | far ptr `0000:0002` | title/menu palette = slot 2 |
| 0238/0242/024f/0258 | `"alleine !"`, `"Spiele nicht"`, `"weiter !"`, `"Spiele automatisch"` | auto-play dialogs |
| 026b | `"autosave.man"` | autosave name |
| 0278 | `""` | default save name |
| 027a..027f | `00 06 0f 15 19 00` | office submenu base icon per main icon 1..6 (indexed `DS:0279+si`) |
| 0280..0285 | `06 09 06 04 05 00` | office submenu icon count (indexed `DS:027f+si`) |
| 0286..028a | `05 04 06 05 06` | copied to `M[m].b[0x141..0x145]` |
| 028c..028f | `01 03 03 03` | copied to `M[m].b[0x146..0x149]` |
| 05ac | `01` | copy protection passed |
| 05ad | `00` | "save/turn-end by click disabled" flag (set by `screen_save_game` 422b:11fd, cleared by the settings cheat `d`) |
| 05d4 | `00` | auto-play flag |
| 05da | `00` | crest blanking active |
| 05dc | `67 68 6b 73 79 7a 7d 7e 7f 80 82 83 85 86 87 88 89 8a 8b 8d 8e 8f 90 00` | crest codes+0x50 that are blanked when 05da != 0 |
| 060e[6] | `07 06 05 04 03 02` | interest-rate ramp (save block 0) |
| 0630 | `0005` (s16) | interest base rate |
| 066c[4][9] | s32 | per-manager values (block 41); [0]=180000/[1..6]=45000/[8]=30000 in 1-year games, [7] from §4.8 |
| 0794 | far ptr `0000:0000` | office palette = slot 0 |
| 07a6..07aa | `01 14 40 6a 97` | pool category boundaries (GK, DEF, MID, FWD) |
| 07ab | `00` | number of human managers (block 7) |
| 07ac..07b0 | `07 00 00 00 00` | last season's qualifiers, 1-based team ids (blocks 8–12) |
| 07d6[6] | `20 10 08 04 02 01` | pairings per cup round |
| 07dc | s32 | day counter (block 23) |
| 07e0 | `07ab` (u16) | season year, 1963; 1992 for a modern start (block 24) |
| 224c | `00` (s8) | season start-day offset |
| 224e/2250 | s32 `1` | game option long A (4 = special: bench 13) |
| 2252/2254 | s32 `1` | game option long B (A != B hides "Histor. Start") |
| 225a[3] | `01 01 01` | current round per tier (block 34) |
| 225e[3] | `12 14 14` | teams per tier (18, 20, 20) |
| 2262[3] | `22 26 26` | rounds per tier (34, 38, 38) |
| 226d | `04` | extra UEFA places |
| 226e[3] | `00 12 26` | first team index per tier |
| 2277 | `39` | last German team index (57) |
| 2280[95] | see dump in `out/flow` | per-week competition mask (block 50): bit t league tier t, 8 DFB-Pokal, 0x10 relegation, 0x70 European cups, 0x80 rescheduled matches |
| 25b8..25d0 | far ptrs | `"SAISONENDE"`, `" Spieltag"`, `"DFB-Pokal"`, `"Relegation"`, `"Europapokal"`, `"Nachholspiel"`, `"Spielfrei"` |
| 4a28 | `02` | difficulty, stored as `5 - level` (block 42) |
| 4a2a[3] | far ptrs | `"Spiel-Level"`, `""`, `""` |
| 4a36[4] | far ptrs | `"Endlosspiel"`, `"1-Jahres-Spiel"`, `"3-Jahres-Spiel"`, `"Histor. Start"` |
| 4d94..4dce | far ptrs | quit dialog `"M|CHTEN SIE"`, `"DAS SPIEL WIRKLICH"`, `"BEENDEN ?"`, `"LEIDER JA"`, `"KEIN GEDANKE"` |
| 5256 | `00` | `autosave_pending`, autosave request flag |
| 9432..9436 | `00 02 08 0f 14` | MANA.DAT player-slot ranges per category (022e) |
| 9438[4] | `02 05 08 05` | players per category for a new manager |
| 943c[4] | `13 2c 2a 2d` | "remaining" counters used by `newgame_init_player_pool` |
| 93e8 | far ptr | `"mana.dat"` |
| 9a22 / 9a2c / 9a30 / 9a58 | f64 116.0 / f32 116.0 / f32 0.0 / f64 1.0 | office curve constants |

## 10. Callers / callees summary

| Function | Callers | Callees (non-library) |
|---|---|---|
| main | crt_start | startup_init_screen_mouse_palette, startup_clear_state, league_shuffle_teams, euro_cup_qualifiers_build, cup_clear_match_goals, cup_draw_first_round, start_menu_new_or_load, league_strength_drift, assign_pool_players_to_teams, transfer_market_refresh, office_load_bitmaps, tore_scene_open, season_main_loop, mana_load_club_table, sound_* |
| start_menu_new_or_load | main, office (5/5) | office_show_cached_background, screen_load_game, copy_protection_penalty, draw_club_crest, start_menu_swap_portraits, start_menu_draw_level_box, FUN_362f_0004, newgame_init_player_pool, squad_insert_player, team_swap_slots, team_stats_swap, team_in_european_cup_lists, league_tier_of_team, squad_count_by_flag, player_value_calc, manager_compute_wage_budget, FUN_24a4_2cb4, auto_lineup, squad_recalc_team_strength, interest_rates_update |
| office_menu_dispatcher | season_main_loop | screen_save_game, FUN_3a41_10e3, FUN_3a41_2d33, office_show_cached_background, append_ordinal_suffix, has_rescheduled_match_today, squad_count_by_flag, auto_lineup, office_draw_date_panel, screen_messages, season_goal_messages, office_handle_free_click, quit_game_confirm, ui_wait_right_click_button, all §6.5 screens, office_free/load_bitmaps, calendar_compute_date |
| manager_compute_wage_budget | start menu, season_main_loop | — |
| team_swap_slots | league_shuffle_teams, start menu, season_main_loop | swap_team_ref_byte, team_swap_in_table_a4ca |
| cup_draw_first_round | main, season_main_loop | cup_draw_screen_open, cup_draw_animate_slot, cup_amateur_home_swap, cup_draw_screen_close |
| euro_cup_qualifiers_build | main, season_main_loop | team_is_euro_qualified |
| league_shuffle_teams | main, season_main_loop | team_swap_slots |
| office_load_bitmaps | main, office, matchday_play 1310:15db, season_main_loop | — |
| office_free_bitmaps | office, matchday_play, season_main_loop | — |
| office_show_cached_background | 10 screens (see FUNCTION_MAP) | — |
| interest_rates_update | start menu, finance_weekly_processing 212a:0a6d | clamp_int |

## 11. Globals used

### 11.1 Scalars and tables

| Address | Type | Suggested name | Save block |
|---|---|---|---|
| DS:018c | u8 | sound_enabled | — |
| DS:0210 | u8 | office_bg_cached | — |
| DS:05ac | u8 | copy_protection_ok | via block 54 (DS:05fa[0]) |
| DS:05ad | u8 | save_and_endturn_disabled | — |
| DS:05b0 | far ptr | crest_cache_rastport | — |
| DS:05d4 | u8 | auto_play | — |
| DS:05da | u8 | crest_blanking | — |
| DS:05fa[4] | u8 | protection_copy | 54 |
| DS:060e[6] | u8 | interest_ramp | 0 |
| DS:0630 | s16 | interest_base | — (not saved) |
| DS:063a | s16 | (set to 0x47 when auto play starts) | 47 |
| DS:066c[4][9] | s32 | manager_money_table | 41 |
| DS:079c | u8 | picture_loading_flag | — |
| DS:07ab | u8 | num_managers | 7 |
| DS:07ac..07b0 | u8 | last_season_qualifiers (1-based) | 8–12 |
| DS:07b2 | u8 | message_window_open | — |
| DS:07dc | s32 | day_counter | 23 |
| DS:07e0 | u16 | season_year | 24 |
| DS:224c | s8 | season_start_day | — |
| DS:224e / 2252 | s32 | game_option_a / game_option_b | — |
| DS:225a[3] | u8 | tier_round | 34 |
| DS:2280[95] | u8 | week_competition_mask | 50 |
| DS:4a28 | s8 | difficulty (5 - level) | 42 |
| DS:5256 | u8 | autosave_pending (producer: month end 212a:0a6d sets it when month % 4 == 0 (months 0, 4, 8); consumer: `office_menu_dispatcher` 18bc:0b5f entry (call at 18bc:0b84) saves `"autosave.man"` via `screen_save_game` 422b:07fe and clears it) | — |
| DS:943c[4] | u8 | pool_take_counter | — |
| FAR:0008[4] | u8 | competition_round | 37 |
| FAR:016e | u8 | week_index | 49 |
| FAR:0024 / 4b56 | far ptr | office_header_bitmap / rastport (PIC 4) | — |
| FAR:1d17[4] | u8 | (cleared per European cup) | 31 (FAR:1d18 + ...) |
| FAR:21e2[3][32] | u8 | european_cup_goals (cleared for c==1) | 29 |
| FAR:2242 + 0x30a*m | struct | manager_record[4] | 13 |
| FAR:2e70 | far ptr | screen | — |
| FAR:2e74 | u8 | euro_qualified_count | — |
| FAR:2e8e | s32 | month | 21 |
| FAR:2e9a / 551c | far ptr | office_panel_bitmap / rastport (PIC 0x17) | — |
| FAR:2ea2 | u16 | mouse_event | — |
| FAR:2eba | s32 | day_of_month | 20 |
| FAR:2f24 / 4d38 | far ptr | portrait_sheet bitmap / rastport (PIC 0) | — |
| FAR:2f28 | far ptr | screen_rastport | — |
| FAR:2f3c / 57d4 | far ptr | icon_sheet bitmap / rastport (PIC 3) | — |
| FAR:304a | s8 | current_manager | 3 |
| FAR:3066 + 0x22*t | struct | club_record[200] | 15 |
| FAR:4b50 | far ptr | palette | — |
| FAR:4b5e | u8[] | fixture pairings | 19 |
| FAR:4c7a | far ptr | screen copy for mouse_open | — |
| FAR:4c7e[4][32] | u8 | cup_match_goals | 33 |
| FAR:513c | u16 | game_end_year | 43 |
| FAR:513e | u8 | cup_day_flag | — |
| FAR:5354 | far ptr | crest_cache_bitmap | — |
| FAR:5358 | s8 | saved_current_manager | 4 |
| FAR:535a[3][20] | u8 | league_order | 32 |
| FAR:5369 / 5370 | u8 | relegation teams (inside league_order) | 32 |
| FAR:548e[2] / 578c[2] | far ptr | button frame bitmaps / rastports (PIC 24, 25) | — |
| FAR:56e2[4] | u8 | unread_mail_flag | 53 |
| FAR:56ee | s8 | max_lineup_slot | — |
| FAR:56f4 | u8 | modern_start | — |
| FAR:56fa / 57bc | far ptr | work 320x240 descriptor / object | — |
| FAR:56fe[] | u8 | euro_qualified_list | 25 |
| FAR:5714[20][5] | u8 | rescheduled_matches | 14 |
| FAR:57aa[3][6] | u8 | euro_qualifiers | 26 |
| FAR:57c0 / 6db8 | far ptr | icon23 bitmap / rastport | — |
| FAR:57c8 | u16 | cup_draw_visible | — |
| FAR:57dd + 0x25*p | struct | pool_person[151] | 17 |
| FAR:6dc0 | far ptr | mouse_state | — |
| FAR:76ca[4][32] | u8 | cup_draw | 28 |
| FAR:774a + 0x34*(m*25+k) | struct | squad_slot | 18 |
| FAR:0ecc + 0x36*t | struct | team_stats | 16 |
| FAR:9336 | u8[0x1394] | per-manager/team table (0xff filled) | 35 |
| FAR:9d36 + 0x15*t | u8[21] | per-team table swapped by team_swap_slots | — |
| FAR:a2ca / a4ca + 8*t | u8[8] | per-team tables swapped by team_swap_slots | — |
| FAR:a6ca | far ptr | screen_bitmap | — |
| FAR:a7a0 | s32 | (block 22; 0 enables free-player assignment) | 22 |

### 11.2 Manager record fields touched here (`M[m]` = FAR:2242 + 0x30a*m)

| Offset | FAR (m=0) | Type | Meaning / initial value |
|---|---|---|---|
| +0x000 | 2242 | char[29] | name |
| +0x01d | 225f | u8 | slot / portrait number (1..4) |
| +0x01e | 2260 | u8 | team (100 = none) |
| +0x03e + 4i | 2280 | u8 | result log, 0xff (i < 50) |
| +0x10a | 234c | u8 | `(is1y+8)*2 - 3*tier` |
| +0x10b..+0x130 | 234d | u8[38] | table-position history (10) |
| +0x131 | 2373 | u8 | 0x10 |
| +0x132+c | 2374 | u8 | competition participation (1 / 0x1e) |
| +0x135 | 2377 | u8 | 1 in 1-year games |
| +0x138 | 237a | u8 | tier |
| +0x139 | 237b | u8 | enables training camp icon |
| +0x141..+0x145 | 2383 | u8[5] | from DS:0286 |
| +0x146..+0x149 | 2388 | u8[4] | from DS:028c |
| +0x15e | 23a0 | s32 | (2-tier)*3000 (+5000) |
| +0x166 | 23a8 | s32 | (5-tier)*4000 (+10000) |
| +0x16e | 23b0 | s32 | (2-tier)*3500 |
| +0x176 | 23b8 | s32 | 3 - is3y (historical start) |
| +0x17e | 23c0 | s32 | 3 - 2*is3y (historical start) |
| +0x186 | 23c8 | s32 | is1y + 4 |
| +0x18e | 23d0 | s32 | is1y + 3 |
| +0x1dc | 241e | u16 | 50 - 20*tier |
| +0x1de | 2420 | u16 | wage budget (§4.8) |
| +0x1ec | 242e | s32 | 99999 |
| +0x1f0 | 2432 | s32 | 1 500 000 (1 900 000 at DS:4a28 == 4) — start balance |

### 11.3 Pool person record (`P[p]` = FAR:57dd + 0x25*p)

| Offset | FAR (p=0) | Meaning |
|---|---|---|
| +0x00 | 57dd | name (26 bytes) |
| +0x1a | 57f7 | age (18..33, human squads 19..31) |
| +0x1b | 57f8 | average of +0x1c..+0x1e |
| +0x1c / +0x1d / +0x1e | 57f9..57fb | skill values |
| +0x1f | 57fc | category bonus (0 / 25+r / 50+r / 75+r) |
| +0x20 | 57fd | random 0..6 |
| +0x21 | 57fe | owner: 5 = free, else manager index |
| +0x24 | 5801 | team id (0xff = none) |

## 12. Open questions

1. ~~`DS:05ad` has no writer~~ (answered: `screen_save_game` 422b:11fd sets it after a menu save when
   the game options `DS:224e` and `DS:2252` are equal, which they are in the shipped data; the writes
   go through a selector, so the direct-reference scan missed them).
2. Mouse event codes 2/3/4 are inferred from usage (release / right press / right release); the INT 33h
   handler at 45ac was not traced.
3. `team_swap_slots(a, a, ...)` with equal ids: all swaps are no-ops except that every pool/manager
   field equal to `a` is rewritten with `a` — harmless, but a port must not "optimise" the RNG call away.
4. The meaning of most pool/manager money fields (`+0x15e..+0x18e`, `DS:066c`) belongs to the finance
   area; only their initial values are specified here.
5. `FUN_24a4_1798`, `rank_tier_teams_by_strength`, `pick_random_free_team_in_tier`,
   `league_strength_drift`, `transfer_market_refresh`, `squad_insert_player`, `FUN_24a4_2cb4` consume RNG
   during new-game setup; their exact call counts are in the specs of their owners, not here.
6. `cup_draw_screen_open` sets `FAR:57c8` from a user dialog (it is only called for `c == 0` or
   `already != 0`, i.e. never for the three startup European draws); whether the draw is shown never affects the
   RNG, but the mouse polls inside the draw loops do not either (they only set `skip`).
7. ~~Sub-icon semantics of menu 5 icons 0x64/0x96~~ (answered: 4 = settings, 5 = new manager; the
   DOSBox scenario `add_manager` opens the start menu with sub icon 5/5, whose picture reads "NEU").

## 13. Deviations

Obvious original bugs the port fixes (MISSION.md invariant 1); each is marked `// DEVIATION:` in the code.

| Where | Original | Port | Test |
|---|---|---|---|
| §4.7b, `add_manager_squads` (18bc:2184 add path) | the forward draw `random_range(0x6a, 0x97)` can return record 151, past the 151-record pool: its owner byte `FAR:6dd1` is read, and when it is 5 or the 1000 tries end on it the manager is written there and player 151 is inserted into the squad (memory corruption) | record 151 is never taken; the loop draws again (the random stream differs only where the original corrupts memory) | `newgame::tests::an_added_manager_never_gets_the_record_past_the_pool` |
