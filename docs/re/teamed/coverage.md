# TEAMED.EXE – port coverage

Status: complete. Every function of `/unpacked/TEAMED_unpacked.EXE` (214 Ghidra functions plus
`input_wait_no_event` `1000:00fb`, which Ghidra had left without a function) is either ported
with an `Origin: TEAMED.EXE` line, served by the library code shared with `BMMAIN.EXE`, part of
the C runtime, a no-op in the port, or dead code. Every input site of the editor is handled.

Summary: 32 ported, 82 library (shared with BMMAIN, see `overview.md` "Library reuse"),
80 C runtime, 2 no-ops, 19 without a caller in TEAMED.

## Input sites

All input goes through `input_poll_mouse` (`1000:08c8`) and, for the keyboard, `dos_getch`
(`16b7:322a`), whose only caller is `text_input`. No other code reads the keyboard.

| call site | function | what it reads | port |
|---|---|---|---|
| `12a5:0464` | `teamed_main_loop` | main loop: hover, bar, crest select, buttons, crest editor, player list, debug switch | `teamed::main_loop` |
| `12a5:0941` | `teamed_main_loop` | league swap prompt "MIT WEM TAUSCHEN ?" | `club::swap_prompt` |
| `12a5:198f` | `teamed_main_loop` | crest editor paint loop (button held) | `crest::click` |
| `12a5:1cc9` | `teamed_main_loop` | crest import crop frame | `crest_import::import` |
| `1000:0111` | `input_wait_no_event` | crop: wait until no event is pending | `Host::input_wait_release` |
| `12a5:2731` | `uefa_screen` | UEFA list rows and bar | `uefa::run` |
| `1000:2383` | `dialog_two_choice` | save question | `dialog::two_choice` |
| `1579:0cad` | `crest_import_browser` | browser: cancel, drives, paging, entries, input lines | `crest_import::browser` |
| `1000:089f` | `input_wait_click` | error screen click | `teamed::fatal_error_exit` |
| `12a5:06b0` | `text_input` | panel fields (name, rank, points, goals) | `club::edit_panel` |
| `12a5:2183` | `text_input` | player names | `club::edit_player` |
| `12a5:2979`, `2a27`, `2b1a`, `2bfa` | `text_input` | UEFA name and A/M/S | `uefa::edit_club` |
| `1579:0f93`, `0fe2` | `text_input` | browser directory and file-name lines | `crest_import::browser` |

## End-to-end verification

`tests/visual/teamed_e2e.*` edits every screen in one session and saves: club 14 (1.FC
NUERNBERG) gets a new name, rank, points, goals, two player names and painted crest pixels,
club 2 swaps its league with club 20, the UEFA club 64 gets a new name and strengths, then
"MACH MAL !". The `MANA.DAT` and `PIC\95.VGA` the port writes are byte-identical to the files
TEAMED.EXE writes in DOSBox. Starting the game on that data (`bmp run` with
`tests/visual/new_game.port.txt`, seeds 45817/45841) shows "FC FRANKEN" with the Uerdingen
crest swapped into the carousel and TESTSPIELER / ZWEITER MANN in the squad; the original
BMMAIN.EXE on the original's edited files (DOSBox, seed aimed at 45817) shows the same four
frames (carousel, options, office, squad: all 100 %).

`tests/visual/teamed_fatal.*` covers `fatal_error_exit` ("DISK" for a missing crest picture).

## Functions

"refs" is Ghidra's reference count to the entry point. Library functions without a port
column are reached through `crate::gfx::Gfx`, `crate::host::Host` and `crate::gfx::ui_*`
like their BMMAIN counterparts (segments `16b7` = `45ac`, `1a46` = `4930`, `1000` UI helpers =
`16c7`/`176b`/`4091`).

| address | Ghidra name | refs | class | port / note |
|---|---|---|---|---|
| `1000:0000` | grid_clear | 4 | ported | `teamed::clear_grid` |
| `1000:0068` | bytes_reverse4 | 1 | ported | `bmp_formats::iff` (big-endian reads in `read_picture`) |
| `1000:00b4` | bytes_reverse2 | 4 | ported | `bmp_formats::iff` (big-endian reads in `read_picture`) |
| `1000:00fb` | input_wait_no_event | 1 | ported | `Host::input_wait_release` in `crest_import::import` (only caller 12a5:1caa) |
| `1000:012b` | mem_swap | 11 | ported | `club::swap_clubs` (`std::mem::swap` of the fields) |
| `1000:019c` | crest_write_vga | 1 | ported | `crest::crest_write_vga`, `crest_file_bytes` |
| `1000:0268` | crest_import_read_picture | 1 | ported | `bmp_formats::iff::read_picture` |
| `1000:049e` | crest_import_load | 1 | ported | `crest_import::load_picture` |
| `1000:04f1` | crest_import_convert | 1 | ported | `crest_import::load_picture` (DEVIATION: the uninitialised descriptor pointer) |
| `1000:0663` | ui_draw_line | 5 | library | BMMAIN `16c7:09b0` ui_draw_line |
| `1000:06ac` | ui_draw_frame | 15 | library | BMMAIN `16c7:0753` ui_draw_frame |
| `1000:072e` | format_long_thousands | 19 | library | BMMAIN `176b:0681` (0.91) -> `teamed::format_number` |
| `1000:0859` | ui_blit_transparent | 0 | dead | library (BMMAIN `176b:02e4`), no caller in TEAMED |
| `1000:088e` | input_wait_click | 1 | ported | loop in `teamed::fatal_error_exit` (only caller) |
| `1000:08c8` | input_poll_mouse | 9 | library | BMMAIN `176b:0319` (0.97) -> `Host::input_poll_mouse` |
| `1000:0909` | ui_fill_rect | 48 | library | BMMAIN `16c7:083c` ui_fill_rect |
| `1000:0949` | ui_draw_text_centered_shadow | 16 | library | BMMAIN `16c7:087c` ui_draw_text_centered_shadow |
| `1000:0a7d` | text_input | 8 | ported | `dialog::text_input` |
| `1000:0ae9` | text_input_part2 | 1 | ported | `dialog::text_input` (interior of 0a7d, mis-split by Ghidra) |
| `1000:0c0e` | text_input_part3 | 1 | ported | `dialog::text_input` (interior of 0a7d, mis-split by Ghidra) |
| `1000:0d8d` | ems_init | 1 | no-op | EMS check: no effect in the port (`teamed::init`) |
| `1000:0e01` | FUN_176b_0c4b_copy | 1 | library | BMMAIN `176b:0c4b` (teardown of fatal_error_exit) -> process exit |
| `1000:0e47` | ui_set_font | 17 | library | BMMAIN `176b:0c91` ui_set_font |
| `1000:0e7d` | ui_draw_text_at | 43 | library | BMMAIN `16c7:07d5` ui_draw_text_at |
| `1000:0ee4` | fatal_error_exit | 13 | ported | `teamed::fatal_error_exit` (scenario `teamed_fatal`) |
| `1000:10e6` | long_abs_diff | 0 | dead | library (BMMAIN `4091:10d5`), no caller in TEAMED |
| `1000:111a` | main | 1 | ported | `teamed::run` |
| `1000:1136` | picture_load | 11 | library | BMMAIN `176b:0356` (0.89, no CP path) -> `Host::load_picture` |
| `1000:13dd` | bitmap_free | 9 | library | BMMAIN `176b:0dca` bitmap_free |
| `1000:1439` | teamed_init_and_load_mana | 1 | ported | `teamed::init`, `teamed::load_mana`, `bmp_formats::mana::ManaDat::parse` |
| `1000:1a31` | dialog_message | 4 | ported | `dialog::message` |
| `1000:1bd2` | dialog_message_uefa | 3 | ported | `uefa::message` |
| `1000:1d73` | crest_editor_draw | 3 | ported | `crest::draw_editor` |
| `1000:207d` | parse_pair | 2 | ported | `club::parse_pair` |
| `1000:215b` | dialog_two_choice | 1 | ported | `dialog::two_choice` |
| `1000:2446` | text_to_upper_de | 3 | library | BMMAIN `4091:105d` text_to_upper_de |
| `1000:24be` | panel_draw_hovered | 2 | ported | `teamed::draw_hover_panel`, `draw_panel` |
| `1000:26f1` | panel_draw_selected | 1 | ported | `teamed::draw_panel` on the copies (same code as 24be) |
| `1000:2924` | grid_draw_page | 6 | ported | `teamed::draw_page` |
| `1000:29ec` | random_range | 7 | library | BMMAIN `176b:0cc7` random_range |
| `1000:2a2f` | clamp_int | 6 | library | BMMAIN `4091:1009` clamp_int |
| `12a5:000a` | crest_import_hit_slot | 1 | library | BMMAIN `4091:11cc` (0.79) -> `TextButtons::hit` (browser icon slots) |
| `12a5:0087` | crest_import_register_slot | 1 | library | BMMAIN `4091:123d` (0.92) -> `TextButtons::register_on` |
| `12a5:02bd` | teamed_main_loop | 1 | ported | `teamed::main_loop` with `club`, `crest`, `crest_import`, `uefa::run`, `end_program` |
| `12a5:233d` | uefa_screen | 1 | ported | `uefa::run`, `draw_list`, `edit_club` |
| `12a5:2ce0` | uefa_strength_ok | 3 | ported | `uefa::strength_ok` |
| `12a5:2d39` | view_switch_stub | 4 | no-op | empty view switch; its call sites are commented in `main_loop`, `init`, `dialog::two_choice` |
| `1579:0002` | screen_set_visible | 26 | ported | `teamed::screen_set_visible` |
| `1579:0030` | teamed_save_mana | 1 | ported | `save::teamed_save_mana`, `bmp_game::strength::recompute_strengths`, `ManaDat::to_bytes` |
| `1579:05b7` | crest_import_browser | 2 | ported | `crest_import::browser` |
| `1579:1215` | crest_import_dialog | 0 | dead | no caller (`crest.md`); not ported |
| `16b7:16c0` | font_map_char | 2 | library | BMMAIN `45ac:16a0` font_map_char |
| `16b7:16f0` | rect_hits_mouse_pointer | 10 | library | BMMAIN `45ac:16d0` rect_hits_mouse_pointer |
| `16b7:1766` | mouse_pointer_restore_bg | 13 | library | BMMAIN `45ac:1746` mouse_pointer_restore_bg |
| `16b7:17c0` | mouse_pointer_draw | 12 | library | BMMAIN `45ac:17a0` mouse_pointer_draw |
| `16b7:194d` | mouse_pop_button_event | 1 | library | BMMAIN `45ac:192d` mouse_pop_button_event |
| `16b7:1a39` | mouse_set_pointer_shape | 0 | dead | library (BMMAIN `45ac:1a19`), no caller in TEAMED |
| `16b7:1aa1` | mouse_pointer_hide | 0 | dead | library (BMMAIN `45ac:1a81`), no caller in TEAMED |
| `16b7:1ae7` | mouse_pointer_show | 1 | library | BMMAIN `45ac:1ac7` mouse_pointer_show |
| `16b7:1b28` | mouse_install | 1 | library | BMMAIN `45ac:1b08` mouse_install |
| `16b7:1c6b` | mouse_uninstall | 1 | library | BMMAIN `45ac:1c4b` mouse_uninstall |
| `16b7:1ca0` | vga_set_mode_x_320x240 | 1 | library | BMMAIN `45ac:1c80` with another CRTC table -> `Gfx::vga_set_mode_x_teamed` |
| `16b7:1d28` | vga_set_text_mode | 1 | library | BMMAIN `45ac:1d08` vga_set_text_mode |
| `16b7:1d3c` | blit_rect | 10 | library | BMMAIN `45ac:1d1c` blit_rect |
| `16b7:20ee` | blit_rect_transparent | 4 | library | BMMAIN `45ac:20c8` blit_rect_transparent |
| `16b7:24b2` | put_pixel | 5 | library | BMMAIN `45ac:2486` put_pixel |
| `16b7:25b3` | xor_pixel | 5 | library | BMMAIN `45ac:2586` xor_pixel |
| `16b7:26c2` | get_pixel | 1 | library | BMMAIN `45ac:2694` get_pixel |
| `16b7:27ba` | draw_line | 1 | library | BMMAIN `45ac:278b` draw_line |
| `16b7:28c7` | draw_line_xor | 1 | library | BMMAIN `45ac:2898` draw_line_xor |
| `16b7:29d3` | fill_rect | 1 | library | BMMAIN `45ac:29a4` fill_rect |
| `16b7:2b6e` | fill_rect_xor | 1 | library | BMMAIN `45ac:2b3e` fill_rect_xor |
| `16b7:2d21` | clear_bitmap | 0 | dead | library (BMMAIN `45ac:2cf0`), no caller in TEAMED |
| `16b7:2db4` | draw_text | 1 | library | BMMAIN `45ac:2d83` draw_text |
| `16b7:2ee5` | text_width | 1 | library | BMMAIN `45ac:2eb4` text_width |
| `16b7:2f47` | font_find_by_name | 1 | library | BMMAIN `45ac:2f16` font_find_by_name |
| `16b7:2fc4` | video_screen_off | 1 | library | BMMAIN `45ac:2f93` video_screen_off |
| `16b7:2fcc` | video_screen_on | 1 | library | BMMAIN `45ac:2f93` video_screen_off |
| `16b7:2fd4` | vga_set_dac_color | 2 | library | BMMAIN `45ac:2fa3` vga_set_dac_color |
| `16b7:3009` | far_memcpy_bytes | 16 | library | BMMAIN `45ac:304a` far_memcpy_bytes |
| `16b7:3039` | timer_set_deadline | 1 | library | BMMAIN `45ac:307a` timer_set_deadline |
| `16b7:307b` | timer_deadline_reached | 1 | library | BMMAIN `45ac:30bc` timer_deadline_reached |
| `16b7:30c6` | vga_next_plane_helper | 3 | library | BMMAIN `45ac:3107` vga_next_plane_helper |
| `16b7:30df` | picture_decode_rle | 2 | library | .VGA RLE decoder of `picture_load` -> `bmp_formats::vga::parse` |
| `16b7:3195` | rle_encode | 1 | ported | RLE encoder of `crest_write_vga` -> `crest::crest_file_bytes` |
| `16b7:322a` | dos_getch | 1 | library | BMMAIN `45ac:326b` dos_getch |
| `16b7:3231` | vga_show_page | 8 | library | BMMAIN `45ac:327d` (0.85) plus mode 3 -> `Gfx::vga_show_page_teamed` |
| `16b7:3316` | vga_copy_page | 0 | dead | library (BMMAIN `45ac:332b`), no caller in TEAMED |
| `16b7:3369` | install_int_handler | 1 | library | BMMAIN `45ac:337e` install_int_handler |
| `16b7:34da` | vga_wait_vretrace | 1 | library | BMMAIN `45ac:34ef` vga_wait_vretrace |
| `16b7:34e7` | vga_restore_rect_from_page | 0 | dead | library (BMMAIN `45ac:34fc`), no caller in TEAMED |
| `16b7:3670` | dos_delete_file | 0 | dead | library (BMMAIN `45ac:3690`), no caller in TEAMED |
| `16b7:36f6` | ems_get_page_frame | 0 | dead | library (BMMAIN `45ac:3716`), no caller in TEAMED |
| `16b7:3730` | ems_alloc_pages | 0 | dead | library (BMMAIN `45ac:3750`), no caller in TEAMED |
| `16b7:3767` | ems_map_page | 0 | dead | library (BMMAIN `45ac:3787`), no caller in TEAMED |
| `16b7:3798` | ems_map_or_free | 0 | dead | library (BMMAIN `45ac:37b8`), no caller in TEAMED |
| `16b7:37d5` | ems_save_map | 0 | dead | library (BMMAIN `45ac:37f5`), no caller in TEAMED |
| `16b7:3800` | ems_restore_map | 0 | dead | library (BMMAIN `45ac:37f5`), no caller in TEAMED |
| `16b7:382b` | ilbm_decode_body | 1 | ported | `bmp_formats::iff::decode_body` |
| `1a46:000c` | file_close | 8 | library | BMMAIN `4930:000c` file_close |
| `1a46:0023` | file_open | 6 | library | BMMAIN `4930:0023` file_open |
| `1a46:0088` | file_read | 18 | library | BMMAIN `4930:0088` file_read |
| `1a46:00a7` | file_seek | 1 | library | BMMAIN `4930:00a7` file_seek |
| `1a46:00d6` | file_write | 8 | library | BMMAIN `4930:00d6` file_write |
| `1a46:010c` | bitmap_alloc | 22 | library | BMMAIN `4930` allocator -> Rust allocation (`Gfx::alloc`) |
| `1a46:014b` | mem_free_far | 16 | library | BMMAIN `4930` far free -> `Gfx::free` / drop |
| `1a46:016b` | delay_ticks | 3 | library | BMMAIN `4930:01e7` delay_ticks |
| `1a46:019a` | font_handle_invalidate | 1 | library | BMMAIN `4930:0216` font_handle_invalidate |
| `1a46:01b0` | font_open | 1 | library | BMMAIN `4930:022c` font_open |
| `1a46:01e5` | rastport_set_font | 1 | library | BMMAIN `4930:0261` rastport_set_font |
| `1a46:0215` | rastport_draw_text | 4 | library | BMMAIN `4930` wrapper plus the bitmap origin -> `Gfx::with_origin` |
| `1a46:028b` | rastport_text_width | 7 | library | BMMAIN `4930:02f6` rastport_text_width |
| `1a46:02b1` | rastport_move_to | 6 | library | BMMAIN `4930:031c` rastport_move_to |
| `1a46:02d0` | rastport_line_to | 5 | library | BMMAIN `4930` wrapper plus the bitmap origin -> `Gfx::rastport_line_to` |
| `1a46:038d` | rastport_put_pixel | 11 | library | BMMAIN `4930` wrapper plus the bitmap origin -> `Gfx::rastport_put_pixel` |
| `1a46:040d` | rastport_get_pixel | 3 | library | BMMAIN `4930` wrapper plus the bitmap origin -> `Gfx::rastport_get_pixel` |
| `1a46:044e` | rastport_fill_rect | 9 | library | BMMAIN `4930` wrapper plus the bitmap origin -> `Gfx::rastport_fill_rect` |
| `1a46:04ee` | rastport_set_color | 18 | library | BMMAIN `4930:04fd` rastport_set_color |
| `1a46:0506` | rastport_set_bg_color | 1 | library | BMMAIN `4930:04fd` rastport_set_color |
| `1a46:051e` | rastport_set_draw_mode | 13 | library | BMMAIN `4930:04fd` rastport_set_color |
| `1a46:0536` | rastport_clear | 3 | library | BMMAIN `4930` rastport_clear -> `Gfx::rastport_clear` |
| `1a46:0596` | bitmap_init | 2 | library | bitmap descriptor init -> `gfx::Bitmap` |
| `1a46:05c6` | rastport_init | 2 | library | BMMAIN `4930:05a4` rastport_init |
| `1a46:05ff` | palette_set_entry | 1 | library | BMMAIN `4930:05dd` palette_set_entry |
| `1a46:0646` | palette_load_rgb4 | 2 | library | BMMAIN `4930:0624` (0.87, no palette-slot branch) -> `Gfx::palette_load_rgb4` |
| `1a46:06cc` | palette_get_rgb4 | 0 | dead | library (BMMAIN `4930:06d5`), no caller in TEAMED |
| `1a46:0709` | screen_open | 1 | library | BMMAIN `4930:0712` (320x329, origin) -> `teamed::init` |
| `1a46:07ad` | mouse_open | 1 | library | BMMAIN `4930:077e` mouse_open |
| `1a46:0848` | screen_close | 1 | library | BMMAIN `4930:0819` screen_close |
| `1a46:0855` | mouse_close | 1 | library | BMMAIN `4930:0826` mouse_close |
| `1a46:0878` | blit_bitmap_transparent | 0 | dead | library, no caller in TEAMED |
| `1a46:09a8` | blit_bitmap | 29 | library | BMMAIN `4930` blit plus the bitmap origin -> `Gfx::blit_bitmap` |
| `1a46:0ae4` | blit_bitmap_transparent_via_temp | 1 | dead | only caller is the uncalled `ui_blit_transparent` 1000:0859 |
| `1a46:0c14` | draw_circle_octants | 2 | library | BMMAIN `4930:0bad` draw_circle_octants |
| `1a46:0cf7` | draw_circle | 0 | dead | library, no caller in TEAMED |
| `1b1f:0012` | entry | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:00ed` | FUN_1b1f_00ed | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0112` | crt_inherit_file_info | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:01d7` | crt_exit | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:025a` | crt_ctermsub | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0287` | crt_initterm | 8 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:029a` | crt_ff_msgbanner | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:02bc` | crt_amsg_exit_jump | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:02c2` | crt_chkstk | 86 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:02e6` | crt_nullcheck | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:030a` | crt_setargv | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:04b8` | crt_setenvp | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0558` | crt_nmsg_text | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0583` | crt_nmsg_write | 5 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:05ba` | crt_alloc_or_die | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:05e0` | crt_dosret0 | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:05e8` | crt_dosretax | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:05f5` | crt_dosreturn | 7 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0602` | crt_dosmaperr_far | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0608` | crt_dosmaperr | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0636` | crt_close | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0656` | crt_lseek | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:06ea` | crt_open | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:088b` | crt_nop_088f | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:089c` | crt_read | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:099a` | crt_write | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0a5a` | crt_write_flush_chunk | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0ac0` | crt_write_raw | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0afa` | crt_stackavail | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0b0e` | crt_malloc_thunk | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0b14` | crt_ffree | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0b27` | crt_fmalloc | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0ba4` | crt_heap_init_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0bd8` | crt_heap_link_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0c0e` | crt_heap_search_segment | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0d19` | crt_heap_grow_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0d6a` | crt_heap_find_end | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0d8a` | crt_heap_new_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0dce` | crt_fstrcat | 19 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0e14` | crt_fstrcpy | 25 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0e4a` | crt_fstrcmp | 7 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0e74` | crt_fstrlen | 12 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0e8c` | crt_fstrncpy | 5 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0eb6` | crt_fstrncmp | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0ef2` | crt_atoi_thunk | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0ef6` | crt_atoi | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0f4c` | crt_getenv | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0fda` | crt_tell | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:0ff2` | crt_fheapmin | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:102a` | crt_heapmin_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1098` | crt_heap_unlink_free_segment | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:10f2` | crt_intdosx | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1154` | crt_outp | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1162` | crt_time | 0 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:11b8` | crt_tzset_once | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:11ca` | crt_tzset | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:129a` | crt_isindst | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1374` | crt_loctotime | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1478` | crt_fmemset | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:14c0` | crt_srand | 0 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:14d2` | crt_rand | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1501` | crt_chdir | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:153a` | crt_getdrive | 4 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1542` | crt_chdrive | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:155e` | crt_getcwd | 3 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1576` | crt_getdcwd | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:167c` | crt_dos_handle_call_1984 | 0 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1683` | crt_dos_creat | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:16aa` | crt_dos_findnext | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:16b5` | crt_dos_findfirst | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:16e8` | crt_dos_setdrive | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1700` | crt_afldiv | 17 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:179a` | crt_aflmul | 9 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:17cc` | crt_aflrem | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:186c` | crt_aflshl | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1878` | crt_aflshr | 2 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:1884` | crt_afldiv_assign | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:18a4` | crt_aflmul_assign | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:18c4` | crt_aflrem_assign | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
| `1b1f:18e4` | crt_afuldiv | 1 | CRT | MSC C runtime (BMMAIN `4a01`) -> Rust std; `crt_rand`/`crt_afldiv` are modelled in `bmp_game::rng` / `bmp_game::strength` |
