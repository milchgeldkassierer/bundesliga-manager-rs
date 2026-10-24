# Input and completeness audit: summary

Seven audits compared the port with `BMMAIN.EXE` (and `BMINTRO.EXE`) going from the original to
the port. Six covered one area each: every caller of the input functions, every mouse or key
test, and every hook whose default does nothing. The seventh, the completeness critic, looked
for what the six could all have missed. This file collects their results and lists the known
differences that remain.

## Per area

| Area | Table | Input sites | ok | fixed | deviation | open issues |
|---|---|---|---|---|---|---|
| start (intro, title, start menu, options, load/save, file selector, settings, high scores, quit) | [start.md](start.md) | 35 | 32 | 3 (one cause: the 28-character `text_input` cap) | 1 (Backspace on an empty text) | none |
| office (dispatcher, free clicks, mail, statistics, eternal table, history, stadium, loans, sponsor offer, shared dialogs) | [office.md](office.md) | 52 | 50 | 2 (autosave at the start of the turn, no icon selected after a right-click close) | 0 | none |
| tables (league table, fixtures, strength table, club info, RESTPROGRAMM, cup lists and draws) | [tables.md](tables.md) | 31 | 29 | 2 (cup lists of the relegation play-off) | 0 | none |
| team (squad, tactics board, contracts, transfer market, training, camp, scorers, player info) | [team.md](team.md) | 44 | 42 | 2 (team strength recomputed on leaving the transfer market and the training camp) | 0 | none |
| match day (live match, goal scenes, shoot-out, season events, season end, newspaper) | `matchday.md` on branch `audit/matchday` | 21 | 18 | 2 (+1 that #151 fixed at the same time) | 1 (automatic play skips the season-end boxes) | #156 |
| uncited functions (every BMMAIN function no `seg:off` in the port names) | [uncited.md](uncited.md) | 190 functions | 187 | 1 (save with a short tail) | 1 (pointer shape 0 after a TORE read error) | #153 |
| completeness critic (this file) | below | 21 | 17 | 3 (two causes: the "Blenden" fade, the construction message) | 0 | #158 (1 missing) |

The match-day audit is not on `main` yet. Its branch `audit/matchday` (commits `dff1e95` and the
merge `7853735`) passed `tools/check.sh`, and its full visual check showed no regression (279
frames at 100 %; the new `scene_author` frames are a00 99.73 % and a06 99.91 %). It was never
pushed or merged. When it lands it adds `matchday.md`, the goal-scene author box after a right
click in the live match, and the penalty shoot-out screens.

## Completeness critic

Method:

* Read every audit table.
* List the callers of every input primitive again (`input_poll_mouse` 176b:0319,
  `ui_hit_test_buttons` 16c7:0002, `ui_scroll_text_box` 176b:07d7, `ui_value_spinner`
  4091:0ba5, the dialogs, `input_wait_click`, `input_wait_release`,
  `ui_wait_right_click_button`, `ui_hit_icon_slot`, `dos_getch`, `dos_direct_console_io`) and
  look for functions that no audit table names.
* Search the instruction operands and bytes for every settings address: options `DS:05fe..060c`,
  speed `DS:063a` and the cheat flag `DS:05f5`.
* Read the command line handling in `main`.
* Check the sound triggers.
* Collect the `TODO` markers of the port and check the gameplay-related ones against the
  original.
* Spot-check rows that the audits marked ok.

| # | Input site / behaviour | Original | Port | Status |
|---|---|---|---|---|
| 1 | Settings option "Blenden" (`DS:060c`, option 14): `screen_fade` fades palette entries 0..31 over 30 retraces instead of switching the screen off and on | 176b:1416 `screen_fade` (with `vga_set_palette_rgb4` 45ac:33ca) | `Host::screen_fade`, `Host::palette_fade` mirrored by `Host::sync_options` (settings toggle, load, game start) | **fixed** (the option could be switched on but did nothing, "not modelled yet") |
| 2 | A period played without the live screen turns the screen on without the palette fade (`DS:060c` saved, cleared, restored around `screen_fade(1)`) | 1310:24eb..2518 `match_live_period` | `live_match::period_start` | **fixed** (with 1) |
| 3 | A construction that finishes in the winter break (`0x80 < day < 0xcf`) or after day 0x142 is shown at once: office background, manager header, the mail viewer with the message (`message_post(show = 1)` calls `screen_messages`, which waits for the exit button), then the screen goes off and the buttons are removed | 1000:2295..234c `stadium_construction_daily`, 4091:0190 `message_post`, 4091:05c4 `screen_messages` | `season_events::finance_dialogs` (`FinanceEvent::ConstructionFinished { shown: Some(..) }` now carries the message count the viewer reads) | **fixed** (the logic built the message, but no screen showed it: `TODO(#82)`) |
| 4 | Options 0..8 (half-time scores, results, table per league) | 1310:2b93, 1310:2bb4 (`[BX+0x5fe]`, `[BX+0x5ff]`), 1310:1b75 (`[BX+0x600]`) | `live_match` `OPTION_HALF_TIME/FULL_TIME/TABLE` | ok |
| 5 | Season end rewrites options 0..8 when a manager changes league | 1cb5:0f10..0f83 `season_end_messages` | `bmp_game::season` (options rows, test at season.rs 4891) | ok |
| 6 | Option 9 goal scenes, 10 DFB-Pokal, 11 European cups, 12 rescheduled games, 13 newspaper | 2a58:127e..14a2, 1310:1a55/1aa4/2c2f/2c41, 27c2:0044, 1310:2b1e, 3e3a:0eef | `season.rs` `GOAL_SCENES_OPTION`, `live_match`, `cups::SHOW_DRAW_OPTION`, `news::NEWSPAPER_OPTION` | ok |
| 7 | Speed slider `DS:063a`: minute delay `0x26 - speed/2` below 0x47; the ticker reads it; the (1, 1) click sets 0x47 | 1310:29de/29e9, 2a58:19db, 18bc:09f2, 362f:0747/0a69 | `live_match` (period_minute, ticker event), `office.rs`, `disk::screen_settings` | ok |
| 8 | Settings cheat `p` sets `DS:05f5 = 1` | 362f:0936 | `Session::cheat_05f5` | ok: nothing in BMMAIN reads the byte (no operand and no `f5 05` byte pair besides the write and the 99,999,999 constant of cheat `a`) |
| 9 | Command line: `bmmain 0` turns the sound off (`DS:018c = 0`); BMP.BAT passes `%1` | 18bc:0418 `main` | `main.rs` `sound_enabled` (trailing `0` or `--nosound`) | ok |
| 10 | Keyboard reads: `dos_direct_console_io` has exactly four callers (live-match space pause, goal-scene `r`/`w` replay, the two settings cheat reads); `dos_getch` only in `ui_scroll_text_box`. There is no global hot key in the shared helpers | 1310:2a49, 2a58:145a, 362f:0795/0858, 176b:0842 | covered by matchday rows 1 and 5, start row 14, `widgets::text_input` | ok |
| 11 | `screen_manager_contract` (sponsor boards, shirt sponsor, TV, expenses): three mouse polls (main loop, accept/decline press, arrow press), right click leaves, `ui_dialog_box` "noch unter Vertrag". No audit table listed it | 362f:2be8 (polls 362f:2e2a, 35e1, 36a6; box 2ed2) | `contract::screen_manager_contract`; visual frames `info/i13..i18` | ok (compared branch by branch: accept/decline hit box and pressed images, expenses step and wrap, tab selection with the shirt guard `prev_tab != 10`, same-tab early loop, offer field, arrows) |
| 12 | `matchday_info_panel` (header click in the office) | 1cb5:2bf9 (`input_wait_click` 2e60) | `office::matchday_info_panel` | ok (office row 14) |
| 13 | Sound triggers: `sound_play_voice` (4 callers), `sound_stop_voice` (2), `sound_restart_loop_voice` (9) | 1310:2303, 2a58:0ca3, 1c7c:0004 | `live_match` (crowd at period start, ticker sounds, stop at period end), `goal_scene` | ok |
| 14 | Title music stopped by a left click | 18bc:0632 | `game::run` | ok (start row 1) |
| 15 | Team editor `TEAMED.EXE`: in scope per MISSION.md; the port has only the MANA.DAT reader and writer | TEAMED.EXE (214 functions) | none | **missing**, #158 |
| 16 | `BMLOADER.EXE` (title picture and music) | BMLOADER.EXE | none | ok: BMP.BAT never starts it (`bmintro`, `bmmain %1`); BMMAIN shows its own title |
| 17..21 | Spot checks of rows marked ok: office rows 11–15 (`office_handle_free_click`: auto-play stop at (0, 0), calendar-sheet end of turn and `DS:05ad`, auto-play start at (1, 1) with `DS:063a = 0x47`, header panel, desk mail only while no icon is selected) and tables rows 2–4 (league table mode by x < 0x87 / < 0xd7, row = (y − 0x1a) / 7 < teams, exit on hit 2 or the abort flag) | 18bc:08f2, 3a41:214c | `office.rs`, `tables::screen_league_table` | ok, condition for condition |

Tests for the fixes:

* `host::tests::screen_fade_fades_the_palette_with_the_option`: without the option there is no
  delay. With it, the fade takes 30 retraces (about 0.43 s) in each direction, and the colours
  are back afterwards.
* `season_events::tests::a_construction_shown_at_once_waits_in_the_mail_viewer`.
* Visual scenario `palette_fade` (DOSBox references): "Blenden" on, leave the settings, open the
  high scores and go back. All four frames are taken after the fades and match at 100 %. A
  frame in the middle of a fade depends on DOSBox timing, so none is taken.

## Open issues from all audits

* #153: `fatal_error_exit` 176b:114c. For codes other than 99 the original shows the error, waits
  and writes `ERROR.MAN`; the port panics. This only happens with damaged or missing game files.
* #156: a visual scenario for the penalty shoot-out screens. The code is on the unmerged
  `audit/matchday` branch.
* #158: the team editor `TEAMED.EXE` is not ported.

## Remaining known differences to the original

The original fails in these cases, and the port deliberately does something else. Each is
documented in the code as `// DEVIATION:`:

* `text_input`: Backspace on an empty text appends control character 8 in the original. The port
  ignores it (start.md B).
* Pointer shape 0 after a TORE read error: in the original the pointer stays invisible for the
  rest of the session. The port keeps the arrow (uncited.md).

These differences are deliberate, but they are not marked as `// DEVIATION:` in the code:

* Season-end report in automatic play: the port skips the boxes. The original draws them for one
  poll (matchday.md row 15, on the unmerged branch).

These differences are not fixed yet:

* Fatal errors (#153) and the team editor (#158), as listed above.

These are not differences in behaviour:

* The first-round cup draw does not call `input_wait_release` (27c2:0a6b). Nothing visible
  changes, because the draw takes no input (tables.md row 24).
* Several reads of uninitialised or out-of-table memory in the original are approximated. Each
  one is a `TODO(re)` in the code, for example the stale `offer` of the transfer bid, the
  `forced` flag of the negotiation, and reads past the 20 league rows for round counter 0.
  These are original bugs with undefined results, not missing features.

Stale markers seen during the critic audit:

* `matchsim.rs` `TODO(#32)` (the right-click dialog): fixed on `audit/matchday`.
* `finance.rs` "ticket income ... presumably booked in the match area": the gate receipts are
  ported in `season.rs` (2a58:20b2).
* `mod.rs` `TODO(#7)` (`tore_scene_open`): the goal scenes play (scenario `goal_scenes`).
