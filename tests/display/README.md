# Golden frames of the 2x display (Mod 2026)

Port-vs-port golden frames of the 2x display buffer (`crates/bmp/src/display.rs`), checked
exactly by `display::tests::newspaper_display_matches_the_golden_frame`,
`display::tests::mod_newspaper_display_matches_the_golden_frame` and
`display::tests::mod_office_and_table_display_match_the_golden_frames`.

A full display frame would show the original artwork, which is not committed. Each golden
therefore stores only the display pixels that differ from the nearest-neighbour doubling of the
logical frame: an 8-bit grayscale-alpha PNG of the display frame's size, where alpha 255 marks a
differing pixel and its gray value is the pixel's palette index, and every other pixel is
`(0, 0)`. The test requires every display pixel to be the golden's where the golden marks one,
and the doubled logical pixel everywhere else.

| File | Scenario | Screenshot |
| --- | --- | --- |
| `matchday-m08-newspaper.png` | `tests/visual/matchday.port.txt` on the original data with the 2x fonts | `m08-newspaper` |
| `matchday-m08-newspaper-mod.png` | `tests/mod2026/matchday_2026.port.txt` on Mod 2026 data with the 2x fonts | `m08-newspaper` |
| `matchday-m01-office-mod.png` | `tests/mod2026/matchday_2026.port.txt` on Mod 2026 data with the 2x fonts and crests | `m01-office` |
| `matchday-m07-table-mod.png` | `tests/mod2026/matchday_2026.port.txt` on Mod 2026 data with the 2x fonts and crests | `m07-table` |

Every scenario switches the display layer with the 2x fonts and crests on as the SDL host does
(`Host::enable_display_layer`); only the mod's crests have a 2x version. The original-data golden runs on a copy of `BMP_DATA`; its game
is the frozen v1.0.0, so it only changes with the 2x display itself. The mod goldens run on Mod
2026 data built from `BMP_DATA` (`bmp_game::mod2026::build`) and show what a player of the mod
sees; their text comes from the mod's game, so a change to the mod's league structure, clubs or
squads that changes these screens also needs these goldens regenerated. An original save is
refused on Mod 2026 data (#218), so the mod setups write a save the port played on that data (a
new game) as `07072019.MAN`, and the mod goldens run the match day of that game.

Regenerate after an intended change of the display detail (for example new 2x glyphs or crests)
and review the new golden before committing it (the filter matches every golden test):

```
BMP_DATA=~/Apps/bmp-pristine BMP_DISPLAY_GOLDEN_WRITE=1 cargo test -p bmp --bin bmp the_golden_frame
```

`BMP_DISPLAY_SHOTS_OUT=<directory outside the repo>` additionally writes the full display frames
of the scenario for a look at them.
