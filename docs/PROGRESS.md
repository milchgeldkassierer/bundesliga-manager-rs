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

Visual regression: `tools/visual-check.sh` replays `tests/visual/*.port.txt` on the port and compares with DOSBox references kept in `$BMP_HOLDOUT/visual/`.
