# Input audit: match day and season flow

Scope: the live match view (segment 1310, the ticker in 2a58), the goal scenes (24a4:05ec,
1c7c), the newspaper (3e3a), the half-time and full-time result screens, the season events and
dialogs, the season end, the contract talks at the season end and the game end (2a58, 1cb5).

Method: every caller of the input primitives in `BMMAIN_unpacked.EXE` was listed with
`get_xrefs_to` (`input_poll_mouse` 176b:0319, `input_wait_click` 176b:0d0a, `input_wait_release`
176b:0e27, `ui_hit_test_buttons` 16c7:0002, `ui_scroll_text_box` 176b:07d7, `ui_value_spinner`
4091:0ba5, `dialog_okay` 4091:0e3a, `dialog_two_choice` 4091:146c, `ui_dialog_box` 176b:0e57,
`ui_wait_right_click_button` 4091:1722, `ui_hit_icon_slot` 4091:11cc, `dos_getch` 45ac:326b,
`dos_direct_console_io` 45ac:3272) and filtered to the functions of this area; each site was
compared with the port. The far data (`FAR:xxxx`) is Ghidra segment 5238, DGROUP (`DS:xxxx`)
segment 5cb3.

Status: **ok** = the port does the same; **fixed** = was missing or wrong, ported in the audit
PR; **missing** = still open (issue linked); **deviation** = documented difference.

## Input sites

| # | Screen | Input (condition) | Action | Original | Port | Status |
|---|---|---|---|---|---|---|
| 1 | Live match, every minute | Space key, or left click with the pointer at (0, 0) | settings screen over the office background, live screen redrawn, the minute is skipped | 1310:2303 `match_live_period` 1310:2a42 / 2a49 (poll, key) | `live_match::match_pause_poll`, engine `MatchHooks::pause_requested` | ok |
| 2 | Live match, every minute | Right click (`FAR:2ea2 == 3`) once a goal scene was read (`FAR:4bd8` non-empty) | `dialog_okay("Der Erbauer der", "letzten gespielten", "Torszene war:", author)`: the author of the last `TORE\` scene; the OKAY click then reaches the panel hit test at the restored pointer | 1310:252a..256e | `live_match::match_pause_poll`, author kept in `Session::goal_scene_author` by `goal_scene::tore_scene_play` | **fixed** (was `TODO(#32)`; scenario `scene_author`) |
| 3 | Live match, every minute but the last of a period | Left click on a score panel's crest halves of a human pairing | button released, lineup/substitution screen 2a58:1f75, rest of the match re-planned | 1310:3314 `match_handle_panel_click` (release 1310:3417) | `live_match::match_handle_panel_click`, `panel_click_manager` | ok (#141, scenario `live_substitution`) |
| 4 | Lineup screen from a panel click or an injury | release before and after | | 2a58:1f75 `match_substitution_screen` (2a58:1f97, 2a58:2027) | `live_match::match_substitution_screen` | ok |
| 5 | Goal scene | `r` or `w` within 20 ticks after a showing, fewer than 3 showings | slow-motion replay of the same scene | 2a58:0ca3 `match_ticker_events` 2a58:145a | `goal_scene::goal_scene` | ok |
| 6 | Goal scene | (no input; frames timed) | | 1c7c:0004 `tore_play_frames`, 24a4:05ec `tore_scene_play` | `goal_scene::tore_play_frames` / `tore_scene_play` | ok |
| 7 | Goal scene, short read | OKAY | "Ihre Torszenen-Diskette ist offenbar defekt" | 24a4:0574 (`dialog_okay` 24a4:05cb) | `goal_scene::tore_report_read_error` | ok |
| 8 | Penalty shoot-out with a human club | click after the "!    Elfmeterschiessen    !" screen | first live score panel, then the kicks through the ticker | 1310:356d (`input_wait_click` 1310:3848) | `live_match::penalty_shootout_start` via `MatchHooks::penalty_shootout_start` | **fixed** (screens were "not modelled") |
| 9 | Penalty shoot-out result | click after "Endergebnis:" with "h : a   n.E." | | 1310:356d (`input_wait_click` 1310:3b5a) | `live_match::penalty_shootout_end` via `MatchHooks::penalty_shootout_end` | **fixed**; DOSBox scenario still open (#156) |
| 10 | Forfeit | OKAY | "Ihr Spiel wird mit 0:2 gewertet ..." | 2a58:2051 (`dialog_okay` 2a58:2094) | `season_events::forfeit_dialog` | ok (scenario `rare_scenes`) |
| 11 | Season loop | releases before the office turns and matches | | 2a58:3176 `season_main_loop` (2a58:326a, 3af2, 3b60) | `season.rs` office loop / `period_start` | ok |
| 12 | Special dates | left click (Christmas); a birthday waits a minute, an early click earns "Das war keine Minute !" | | 2a58:2a06 `special_date_events` (poll 2a58:2ff0) | `season_events::special_date_screen` | ok |
| 13 | Daily finances | OKAY dialogs (interest, loan due, bankruptcy warnings) | | 212a:0a6d `finance_daily_processing` (`dialog_okay` 212a:0dd6) | `season_events::finance_dialogs` | ok |
| 14 | Season goals | OKAY dialogs (goal secured / lost) | | 212a:314d `season_goal_messages` (3243, 32ee, 34ac, 36c3) | `office.rs` (season_goal_messages loop) | ok |
| 15 | Season end report | OKAY "sponsors released", the two joke boxes, then a click | | 1cb5:0012 `season_end_messages` (0127, 0671, 06a5, `input_wait_click` 06bb) | `season_end::season_end_report` | ok / deviation: in automatic play the port skips the boxes instead of drawing them for one poll |
| 16 | Season end contracts | OKAY "... bleibt Ihnen auch ...", "... kehrt Ihrem Verein den Rücken", fee | | 1cb5:0012 (1149, 1251, 12a0) | `season_end::season_end_event` (Stays/Leaves) | ok |
| 17 | Contract talks at the season end | fee/length number fields, icons, right click info, OKAY boxes | | 3277:2a8f `contract_negotiation` (poll 325e, text boxes 341d / 370d, icons 3271, OKAY 2e4e..39e8) | `negotiation::contract_negotiation_single` | ok |
| 18 | Championship / cup title | release, then click | | 27c2:274d `screen_title_winner` (2933, 2946) | `live_match::screen_title_winner` | ok |
| 19 | Newspaper | click | next paper / back | 3e3a:1b29 (`input_wait_click` 3e3a:219d) | `newspaper::newspaper_show` | ok |
| 20 | Half-time / full-time fixtures, cup results | right-click button of the result screens | | 3a41:120a `screen_fixtures`, 27c2:1ccb `screen_cup_matches` (tables area) | `tables::screen_fixtures`, `cup_screens::screen_cup_matches` | ok (when they are shown: see below) |
| 21 | Game end | high-score table, then click on "ENDE" | | 422b:2366 `highscore_screen` (2857), 2a58:000c `game_end_screen` (008c) | `disk::highscore_screen`, `season_end::game_end_screen` | ok |

## Hooks with no-op defaults

`MatchHooks` (bmp-game) and `SeasonUi` (bmp-game) were checked for display/input hooks whose
default does nothing: `pause_requested`, `match_handle_panel_click`, `period_start`,
`period_minute`, `period_end`, `matchday_results`, `ticker_event_start`, `ticker_event_end`,
`draw_slot`, `draw_close`, `cup_winner` are all implemented by `ScreenSeason`. The penalty
shoot-out had no hook at all; `penalty_shootout_start` / `penalty_shootout_end` were added and are
implemented by `ScreenSeason`.

## Other gaps found

| Behaviour | Original | Port | Status |
|---|---|---|---|
| Cup results after a period are shown regardless of the options once the day counter `DS:07dc` is past 0x13b (the late rounds and finals) | 1310:2303, 1310:2c0e..2c94 | `live_match::period_end_cup_rows` | ok (fixed in parallel by the tables audit, #151) |
| `FAR:4bd8` was described as a "pending_message flag" | | match.md | **fixed**: it is the goal-scene author |

Not in this area (seen while filtering): `matchday_info_panel` 1cb5:2bf9 (`input_wait_click`
1cb5:2e60) belongs to the office options.
