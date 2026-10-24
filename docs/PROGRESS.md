# Progress log

Screenshots live in the local, git-ignored `progress/` folder (they show original artwork).
File names: `YYYY-MM-DD_NN_<source>_<screen>.png`, where `<source>` is `original_dosbox` (reference) or `port` (our build).

| Date | Shot | What it shows |
|---|---|---|
| 2026-10-24 | `01_original_dosbox_software2000_logo` | Reference: publisher logo, mode 13h 320x200 |
| 2026-10-24 | `02_original_dosbox_title` | Reference: title screen, 320x240 (mode X), PIC/22.CP |
| 2026-10-24 | `03_original_dosbox_club_select` | Reference: club/manager selection after clicking the title, 320x240 |
| 2026-10-24 | `04_original_dosbox_manager_name_entry` | Reference: club carousel rotates; manager name typed into slot 1 |
| 2026-10-24 | `05_port_vs_original_title` | Left original (DOSBox), middle port (`bmp show-pic 47.CP --palette 2`), right diff: **99.93 % match** (only the mouse cursor differs) |
| 2026-10-24 | `06_port_vs_original_start_menu` | Left port (`bmp run`, headless script), right original: start menu with carousel, manager "ADI" and the chosen club. **100 % match** (seed 35372 recovered from the original's carousel) |
| 2026-10-24 | `07_port_vs_original_new_game_options` | Left port, right original: new-game level/mode dialog. **100 % match** |
| 2026-10-24 | `08_port_vs_original_office` | Left port, right original: office after loading `07072019.MAN` via the start menu's file selector (banner "15. Spieltag", date panel, position curve, six main icons). Scenario `office`: load screen, picked file, office, "Tabellen" submenu and closed submenu all **100 % match** |
| 2026-10-24 | `09_port_vs_original_squad` | Left port, right original: squad/lineup screen (office "Mannschaft" -> first sub icon) for `07072019.MAN`. Scenario `squad`: list, two players brought into the eleven, one taken out, tactics board opened, badges 6 and 8 swapped, board closed, back to the office: all 7 frames **100 % match** |
| 2026-10-24 | `10_port_vs_original_tables` | Left port, right original: Bundesliga table (home mode) with the club info of Borussia Dortmund (office "Tabellen" -> first sub icon). Scenario `tables`: league tables of all three tiers with a mode switch, club info, fixtures with paging forward/back, club info from a pair, Oberliga fixtures, strength tables with a part switch and the combined club info, back to the office: 8 of 17 frames **100 % match**, the rest 97.3-99.8 %: the club strengths differ because the weekly strength drift of the season loop (`league_strength_drift` 1f9d:0697, #24/#31) does not run before the port's first office turn |
| 2026-10-24 | `11_port_vs_original_new_game_office` | Left port, right original: first office turn of a new game (manager "ADI", 1.FC Nürnberg, level 2, endless game, Saturday 29 July 1992). Scenario `new_game`: start menu, options, office **100 % match**; squad screen 98.0 %: the skills differ because the European cup draws before the start menu (#27) are not ported yet (skipping their ~150 random numbers by hand reproduces the original's rows). The cup draw screens after the options are skipped (#27) |

Visual regression: `tools/visual-check.sh` replays `tests/visual/*.port.txt` on the port and compares with DOSBox references kept in `$BMP_HOLDOUT/visual/`.
