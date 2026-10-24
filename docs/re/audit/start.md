# Input audit: start (intro, title, start menu, new-game options, load/save, file selector, settings, high scores, quit)

Method: every caller of the input functions (`input_poll_mouse` 176b:0319, `ui_hit_test_buttons`
16c7:0002, `ui_scroll_text_box` 176b:07d7, `ui_value_spinner` 4091:0ba5, `dialog_two_choice`
4091:146c, `dialog_okay` 4091:0e3a, `ui_dialog_box` 176b:0e57, `input_wait_click` 176b:0d0a,
`ui_wait_right_click_button` 4091:1722, `dos_direct_console_io` 45ac:3272, `dos_getch` 45ac:326b)
inside segments 18bc, 362f (settings part) and 422b, plus BMINTRO.EXE, was listed with Ghidra and
traced to the port. Mouse words: `FAR:6dc0[0]` = y, `[1]` = x; `FAR:2ea2` 1 left down, 2 left up,
3 right down.

Out of area (other audits): `office_menu_dispatcher` 18bc:0b5f and `office_handle_free_click`
18bc:08f2 (office), `screen_statistics`/`screen_eternal_table`/`screen_manager_history`/
`screen_manager_contract` in 362f (statistics/contract). Only the "Disk/Optionen" sub icons that
lead into this area are listed below.

| # | Screen | Input site (condition → action) | Original | Port | Status |
|---|--------|---------------------------------|----------|------|--------|
| 1 | Title (BM2TITLE.CMF playing) | left down → stop music, leave title | 18bc:0632 `main` | `game::run` (mod.rs) | ok |
| 2 | Start menu | right down, ≥1 manager, every active slot has a club → finish | 18bc:25e7 `start_menu_new_or_load` | `start_menu::run_menu` | ok |
| 3 | Start menu | left down y ≤ 0x23 and `allow_load` → `screen_load_game(1)`; 0 = redraw | 18bc:25e7.. | `run_menu` | ok |
| 4 | Start menu | left down y 0x2f..0x68, x 6..0x9f → scroll carousel left while held | 18bc:25e7.. | `Screen::scroll(-1)` | ok |
| 5 | Start menu | left down y 0x2f..0x68, x 0xa0..0x139 → scroll right while held | 18bc:25e7.. | `Screen::scroll(1)` | ok |
| 6 | Start menu scroll loop | poll until left up (2) | 18bc:286a, 18bc:2a41 | `Screen::scroll` | ok |
| 7 | Start menu | crest box of slot m ≤ count → assign centre club if unused | 18bc:2a90.. | `Screen::slot_clicks` (a) | ok |
| 8 | Start menu | portrait of any slot, two clicks → swap portraits/slots | 18bc:2a90.. | `slot_clicks` (b) | ok |
| 9 | Start menu | name field y > 0xd2 → text input; empty name truncates the managers | 18bc:2c60 | `slot_clicks` (c), `text_input` | fixed (see A) / deviation (see B) |
| 10 | New-game options | left down x 0xb5..0xe7, y 0x86..0xcc → mode toggle (row 3 = historical bit) | 18bc:2fce | `options_dialog` | ok |
| 11 | New-game options | left down x 0xb5..0xe7, y 0x4b..0x7f → level box | 18bc:2fce | `options_dialog` | ok |
| 12 | New-game options | right down with a game length set → accept | 18bc:2fce | `options_dialog` | ok |
| 13 | Quit icon | `dialog_two_choice` "LEIDER JA"/"KEIN GEDANKE" | 18bc:1c97 `quit_game_confirm` | `disk::quit_game_confirm` | ok |
| 14 | Settings | key while the pointer is on a colour-0x0b pixel: `0`..`5` level, `s`, `a`, `d`, `g`, `p` cheats, then drain the keyboard under a white frame | 362f:0795, 362f:0858 | `settings_cheat`, `screen_settings` | ok |
| 15 | Settings | left down on one of the 15 toggles (unsigned y compare) → on/off | 362f:0778.. | `screen_settings` | ok |
| 16 | Settings | left down x 0xbf..0x121, y 0x98..0xaa → speed slider `DS:063a` | 362f:0778.. | `screen_settings` | ok |
| 17 | Settings | left down on exit icon (hit test, slot 2) → pressed icon, return | 362f:099f, 362f:0a85 | `screen_settings` | ok |
| 18 | File selector | pointer over the list → XOR highlight of row/column | 422b:324a | `file_selector` | ok |
| 19 | File selector | right down, y/row < 0xa5, > 36 entries → next page (wraps) | 422b:324a.. | `file_selector` | ok |
| 20 | File selector | left/right down on the directory field → text input, `chdir` | 422b:3508 | `file_selector`, `text_input` | fixed (see A) |
| 21 | File selector | left down on the file-name field → text input, accept (spaces → `_`, `.MAN` added) | 422b:3557 | `file_selector`, `text_input` | fixed (see A) |
| 22 | File selector | right down on the file-name field → accept the current name | 422b:3557.. | `file_selector` | ok |
| 23 | File selector | left down on the cancel icon → empty name | 422b:33c3, 422b:3607 | `file_selector` | ok |
| 24 | File selector | left down on a list entry: directory → `chdir`; file → select, second click on the selected file → accept | 422b:324a.. | `file_selector` | ok |
| 25 | File selector | "L\x99SCHEN" button with a name → `dialog_two_choice`, delete | 422b:3778 | `file_selector` | ok |
| 26 | File selector | drive buttons (slot ≤ drive count) → `_chdrive` | 422b:324a.. (`ui_hit_icon_slot`) | `file_selector`, `SaveSession::change_drive` | ok |
| 27 | Save | file exists → overwrite question | 422b:0956 `screen_save_game` | `disk::screen_save_game` | ok |
| 28 | Save/Load | disk error → `dialog_okay` | 422b:003e `disk_error_dialog` | `disk::disk_error_dialog` | ok |
| 29 | Load | no signature → `dialog_okay` (OKAY click) | 422b:1399 `screen_load_game` | `screen_load_game` | ok |
| 30 | Load | wrong version → timed `ui_dialog_box` (no input) | 422b:1420 | `screen_load_game` | ok |
| 31 | Load | save id loaded before → timed `ui_dialog_box` (no input) | 422b:1506 | `screen_load_game` | ok |
| 32 | Load | V1.x save → level spinner (left upper/lower half, right ends) | 422b:2199 | `v1_level_question`, `ui_value_spinner` | ok |
| 33 | High scores | wait for release, then a left click | 422b:2857 `highscore_screen` | `disk::highscore_screen` → `input_wait_click` | ok |
| 34 | BMINTRO | any key (INT 21h AH=06) → end | BMINTRO 0000:002c | `intro::run` (`key_hit`) | ok |
| 35 | Office "Disk/Optionen" | sub icons 1 load, 2 save (not after a menu save), 3 high scores, 4 settings, 5 add managers; right click on main icon 6 → quit question | 18bc:0b5f (office) | `office.rs` main loop | ok |

## Findings

A. **Fixed**: `text_input` (176b:07d7, text mode) accepted at most 28 characters, a limit the
original does not have: it only checks the pixel width, and the callers' buffers (manager name,
the selector's 98-byte directory buffer, the 100-byte name buffer of the save/load screens) hold
more than the width lets through. With font 2 (glyphs 3..8 pixels) the 0xaf-pixel directory field
takes up to 58 characters and the 0x8c-pixel name field up to 46, so typed paths longer than 28
characters were cut short in the port. The cap is gone; unit test
`start_menu::tests::text_input_is_limited_by_pixels_only`.

B. **Deviation (documented)**: Backspace on an empty text falls through to the "add a character"
path (176b:0911 → 176b:0a0e) and appends the control character 8. The port ignores it
(`// DEVIATION:` on `text_input`).

No hooks with no-op defaults in this area: `CupInit` (newgame.rs) has no default methods, and
`main`'s `CupDraws(|_| false)` before the start menu matches `cup_draw_first_round(c, 0)` (no
screen) in the original. Every function of the area that reads input is called in the port.
