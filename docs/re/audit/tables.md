# Input audit: tables and cup screens

Scope: segment 3a41 (league table, fixtures, strength table, club info), `1000:28c4`
(RESTPROGRAMM) and segment 27c2 (cup lists, draw screens, title winner), plus the places in
the match day (1310) that open the cup lists.

Method: every xref of `input_poll_mouse` (176b:0319), every compare of `FAR:2ea2`, every read
of the mouse position `FAR:6dc0` (`[0]` = y, `[1]` = x), `ui_hit_test_buttons` (16c7:0002),
`input_wait_click` (176b:0d0a), `input_wait_release` (176b:0e27), the dialogs (4091:146c,
4091:0e3a, 176b:0e57), the text/number inputs (176b:07d7), the spinner (4091:0ba5) and the
keyboard reads (45ac:326b, 45ac:3272) was filtered to these functions. No function of the
area reads the keyboard, a text field, a spinner or `dialog_okay`; no right-click shortcut or
special coordinate exists besides the ones listed. The exit of every list is the hit-test
result 2 (left click on the OK icon), followed by `ui_wait_right_click_button(0x10c, 0xc1)`.

| # | Screen | Input site | Original | Port | Status |
|---|--------|------------|----------|------|--------|
| 1 | League table | hit test (panel, OK icon) | 3a41:2bc8 | `tables::screen_league_table` | ok |
| 2 | League table | left click, y > 0xa9: mode by x (< 0x87 home, < 0xd7 total, else away), redraw on change | 3a41:2bd6..2c1c | same | ok |
| 3 | League table | left click, 0x1a < y < 0xa9: row (y - 0x1a) / 7 < teams → club info (side by mode, arg = (office arg == 0)) | 3a41:2c3d..2caa | same | ok |
| 4 | League table | exit: hit 2 or abort flag DS:05d4 | 3a41:2cb0..2ccc | same (`auto_play`) | ok |
| 5 | Fixtures | hit test (panel, OK, and in office mode the arrows) | 3a41:1b30 | `tables::screen_fixtures` | ok |
| 6 | Fixtures | office mode, hit 4 next / hit 3 previous round (wrap), wait on the arrow | 3a41:1b50..1ba8 | same | ok |
| 7 | Fixtures | left click, y > 0xe: pair (y - 0xe) / 0x11 * 2 + (x > 0x8c) → club info (side, from list, minute 90 flag), pairings saved and restored | 3a41:1bac..1ca8 | same | ok |
| 8 | Fixtures | exit: hit 2 or abort flag | 3a41:1cab..1cc7 | same | ok |
| 9 | Strength table | hit test | 3a41:3ed5 | `tables::screen_strength_table` | ok |
| 10 | Strength table | left click, y > 0xb1: next part (all, defence, midfield, attack) | 3a41:3ef7..3f12 | same | ok |
| 11 | Strength table | left click, 0x25 < y < 0xb2: row (y - 0x25) / 7 → club info (side 2) | 3a41:3f16..3f7a | same | ok |
| 12 | Strength table | exit: hit 2 only (no abort flag check) | 3a41:3f80..3f90 | same | ok |
| 13 | Club info | poll + icon hit, hit > count → 0 | 3a41:0e27..0e41 | `tables::screen_opponent_info` | ok |
| 14 | Club info | left click on the last icon: toggle the crest bit 0x80, re-register the icon | 3a41:0e4b..0ea5 | same | ok |
| 15 | Club info | left click on icon 2 when three icons: RESTPROGRAMM; result 1 closes, else redraw | 3a41:0eac..0ee2 | same | ok |
| 16 | Club info | left click on icon 1: close and restore the background | 3a41:0ee8..0f01 | same | ok |
| 17 | RESTPROGRAMM | poll + icon hit, hit > 3 → 0 | 1000:3076 | `tables::screen_team_fixtures` | ok |
| 18 | RESTPROGRAMM | left click on icon 3: other half | 1000:3095 | same | ok |
| 19 | RESTPROGRAMM | any other left click: clear, rebuild the round pairings, return the icon (0..2) | 1000:30ab | same | ok |
| 20 | Cup list | hit test | 27c2:265e | `cup_screens::screen_cup_matches` | ok |
| 21 | Cup list | left click, y > top + 5: side x > 0x82, first row with ry - 6 < y <= ry → club info (side) | 27c2:266c..26dd | same | ok |
| 22 | Cup list | exit: hit 2 or abort flag | 27c2:26e5..2701 | same | ok |
| 23 | Draw question | `dialog_two_choice` "ABER KLAR" / "KEIN GEDANKE" (skipped with the abort flag; DFB final draw without asking) | 27c2:0193 | `cup_screens::DrawBoard::ask`, `bmp_game::cups::cup_draw_screen_open` | ok |
| 24 | First-round draw | `input_wait_release` before the draw | 27c2:0a6b | not called | ok (no observable effect: the draw needs no input, the next poll latches only new presses) |
| 25 | First-round draw | poll per DFB slot, left click → place the remaining labels at once | 27c2:0cfc / 0d07 | `DrawBoard::animate_slot` (poll before each slot) | ok |
| 26 | First-round draw | poll per European slot, left click → fast | 27c2:0e78 / 0e83 | same | ok |
| 27 | Next-round draw | poll per slot, left click → fast | 27c2:1513 / 151e | same | ok |
| 28 | Draw close | "TASTE DRÜCKEN" + `input_wait_click` when the board was shown | 27c2:099b | `DrawBoard::close` | ok |
| 29 | Title winner | `input_wait_release` + `input_wait_click` | 27c2:2933 / 2946 | `live_match::screen_title_winner` | ok |
| 30 | Match day, end of a cup period | cup list shown when its option is set **or from day 316 on** (`DS:07dc > 0x13b`, the relegation play-off) | 1310:2c0e..2c94 | `live_match::period_end_cup_rows` | fixed (the day condition was a TODO) |
| 31 | Match day, after a shoot-out | European rows shown when `DS:0609` **or the relegation leg flag `FAR:57dc`** is set | 1310:1a51..1aec | `live_match::shootout_cup_rows` | fixed (FAR:57dc was only mentioned in a comment) |

Hooks with no-op defaults in the area: `CupDrawUi::{animate_slot, close, title_winner}` and
`SeasonUi::{draw_slot, draw_close, cup_winner}` are implemented by `DrawScreens` (new game)
and `ScreenSeason` (season loop); the closure implementation of `CupDrawUi` is used only by
tests. Every caller of the area's screens in the original (office 18bc:1ac6/1adb/1aea/1b62,
match day 1310:1a6e/1ac9/1b89, live periods 1310:2b3d/2be7/2c5b/2c86, draw close 27c2:09b8,
season loop 2a58:3945, cup round 27c2:1940/1aaf) has a port counterpart.

Rows 30 and 31 are covered by unit tests (`period_end_shows_the_relegation_list_without_the_option`,
`shootout_lists_follow_the_options_or_the_relegation_leg`); no visual scenario reaches a
relegation play-off yet.
