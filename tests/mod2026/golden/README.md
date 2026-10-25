# Golden frames of the Mod 2026 playthrough

Port-vs-port golden frames (PRD R7, #194): screenshots of the playthrough scripts
`tests/mod2026/playthrough_endless.port.txt` (run A) and
`tests/mod2026/playthrough_one_year.port.txt` (run B), written by `bmp run --script --shots` on
Mod 2026 data built from `BMP_DATA`. `crates/bmp/tests/mod2026_playthrough.rs` requires every
screenshot of the same name to equal its golden pixel for pixel; run B's office shot `b01-office`
is compared with `a01-office.png`, since the office does not show the league. Mod screens have
no DOSBox reference; these frames take its place.

| File | Screen |
| --- | --- |
| `a01-office.png` | Runs A and B: the office of the new game, 29 July 2026 |
| `a02-table-bundesliga.png` | Run A: the Bundesliga table after match day 1 (18 clubs) |
| `a03-table-2-bundesliga.png` | Run A: the 2. Bundesliga table after match day 1 (18 clubs) |
| `a04-table-3-liga.png` | Run A: the 3. Liga table after match day 1 (20 clubs) |
| `a08-play-off-1.png` | Run A: the return leg of the Bundesliga relegation play-off at full time |
| `a09-play-off-2.png` | Run A: the return leg of the 2. Bundesliga relegation play-off at full time |
| `a11-season-end.png` | Run A: the "Saisonende" report of the 2026/27 season |
| `a12-office-2027.png` | Run A: the first office of season 2027/28, 29 July 2027 |
| `b02-table-bundesliga.png` | Run B: the Bundesliga table after match day 1 |

The frames show the game's artwork; they are screenshots of the port, not original files, and
`tools/public-exclude.txt` keeps them out of a public copy.
Their content comes from the mod's game, so a change to its clubs, squads, league rules or
timing that changes these screens also needs the goldens regenerated. Regenerate and review
them before committing:

```
BMP_DATA=~/Apps/bmp-pristine BMP_PLAYTHROUGH_GOLDEN_WRITE=1 cargo test -p bmp --test mod2026_playthrough
```
