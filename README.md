# Bundesliga Manager Professional (Rust Port)

A faithful port of the DOS classic **Bundesliga Manager Professional** (Software 2000, first
released in 1991) to Linux, written in Rust. The port follows version 2.0 of the game (1992),
which offers the 1992/93 season as a second start. It is not a remake "inspired by" the original: every screen, every rule
and every random draw was reconstructed from the original executables with Ghidra, and it is
checked frame by frame against the DOS game running in DOSBox.

<p align="center">
  <img src="docs/screenshots/title.png" width="400" alt="Title screen">
  <img src="docs/screenshots/office.png" width="400" alt="The manager's office">
</p>

> **No original game data is included.** You need your own copy of the DOS game. The port
> reads its pictures, sounds, club database and save games at runtime.

## The game

Bundesliga Manager Professional puts you in charge of a German football club. You pick a club,
type in your name and run the club for one, three or an endless number of seasons, starting
either in the historic 1963/64 season or in 1992/93. Up to four managers can play at the same
table in hot-seat mode.

As a manager you are responsible for everything:

- **The team:** set the line-up and tactics on the tactics board, decide on training and book
  training camps, watch fitness, injuries and suspensions.
- **Transfers and contracts:** buy, sell and loan players on the transfer market, answer offers
  from other clubs, and negotiate contract length and salary with your players.
- **Finances:** ticket prices, sponsors and advertising boards, TV money, loans with interest,
  and the expansion of your stadium, stand by stand.
- **Competitions:** Bundesliga, 2. Bundesliga and Oberliga with promotion, relegation and the
  relegation play-off, the DFB-Pokal and the three European cups, each with its own draw.
- **Match days:** follow your match live with a minute ticker, chances, cards and injuries,
  watch animated goal scenes, read the newspaper report the next morning and see how the other
  results changed the table.

The computer clubs play by the same rules: they shift in strength over the weeks, react to
results and compete with you for players and titles. The game keeps statistics, an eternal
table, the history of each manager and a high-score table.

## Screenshots

All screenshots are rendered by the port.

| | |
|---|---|
| ![Start menu with the club carousel](docs/screenshots/start-menu.png) | ![League table with club information](docs/screenshots/tables.png) |
| *Start menu: choose your club and enter your name* | *League tables with club information* |
| ![Transfer market](docs/screenshots/transfer-market.png) | ![Live match view](docs/screenshots/live-match.png) |
| *Squad and transfer market* | *Live match view with goal panel* |
| ![Goal scene](docs/screenshots/goal-scene.png) | ![DFB-Pokal draw](docs/screenshots/cup-draw.png) |
| *Animated goal scene* | *DFB-Pokal draw* |

## How faithful is it?

| Area | Status |
|---|---|
| Game flow | Logo intro → title with music → start menu → new game or saved game → office → season → season end → game end |
| Office | All six areas: info (statistics, eternal table, history, stadium, loans, advertising), tables, team (line-up, contracts, transfer market, training, training camp, top scorers), cups, disk/options, quit |
| Match day | Live view, goal and miss scenes, half-time and full-time results, tables, newspaper, cup draws, title ceremonies, substitutions |
| Game logic | Complete: finances, computer clubs, match simulation, leagues, cups, squad, transfers, new game, newspaper, season loop |
| Random numbers | Bit-exact: the same seed gives the same season as the DOS game |
| Graphics | 184 of 188 reference frames match the original pixel for pixel, the rest 99.9 % or better |
| Sound | Title music on an OPL2 (YM3812) emulation; the register stream equals the original driver's write for write; digitised crowd, whistle and goal sounds |
| Save games | Read and written byte for byte compatible with the DOS version, in both directions |

Known gaps are tracked in the [issues](../../issues).

The game was also sold in English as *The Manager*, with the English leagues. The engine still
contains the switches for that edition (English ordinal suffixes and number format). An English
version of the port is planned once the German port is complete.

## Building and running

Requirements: a Rust toolchain (edition 2024, e.g. via [rustup](https://rustup.rs)), the SDL2
development libraries (`sudo apt install libsdl2-dev` on Debian/Ubuntu) and a copy of the
original game.

```sh
cargo build --release -p bmp
./target/release/bmp --data /path/to/BMP run
```

`--data` points to the directory that contains `BMMAIN.EXE`, `PIC/`, `SOUND/`, `TORE/` and
`MANA.DAT`. Alternatively set the environment variable `BMP_DATA`.

| Option | Effect |
|---|---|
| `run 0` or `run --nosound` | No sound, like `BMP.BAT 0` in the original |
| `run --skip-intro` | Start directly at the title screen |
| `show-pic <name>` | Show a single picture, e.g. `47.CP` |
| `render-music <out.wav>` | Render the title music to a WAV file |

The game is played with the mouse like the original: left click selects, right click confirms
or leaves a screen. Any key skips the intro.

### Team editor

The team editor `TEAMED.EXE` is ported too:

```sh
./target/release/bmp --data /path/to/BMP teamed
```

It edits `MANA.DAT` in the data directory like the original. You can change club names,
league placement, last season's table (rank, points, goals), player names, the club crests
(paint them or import an IFF/ILBM picture) and the names and strengths of the foreign UEFA
clubs. "PROGRAMM-ENDE" followed by "MACH MAL !" saves, and the next `bmp run` uses the edited
data. The editor writes into the data directory, so keep a backup of `MANA.DAT` and `PIC/`.

## How it was made

1. **Reverse engineering.** The original executables (`BMINTRO.EXE`, `BMMAIN.EXE`, unpacked from
   EXEPACK) were analysed in Ghidra. Every subsystem has a written specification in
   [`docs/re/logic/`](docs/re/logic) with pseudo-code, data tables and the order of the random
   draws; file formats are documented in [`docs/re/`](docs/re).
2. **Faithful port.** The Rust code follows the specifications function by function. Every
   ported function names its origin (`Origin: BMMAIN.EXE seg:off`), including the original's
   bugs and quirks where they change what the player sees.
3. **Verification.** `tools/visual-check.sh` replays scripted mouse and keyboard input on the port
   (headless, on a virtual clock) and on the original in DOSBox, and compares every screenshot
   pixel by pixel (`bmp compare`, tolerance in [`docs/re/TOLERANCES.md`](docs/re/TOLERANCES.md)).
   Random content is compared by recovering the original run's random seed. Logic is covered by
   more than 550 unit tests, many of them against the shipped save games.

## Project layout

| Path | Content |
|---|---|
| `crates/bmp-formats` | Parsers for `.VGA`, `.CP`, `.PAL`, `TORE/*`, `MANA.DAT`, `*.MAN`, `.CMF`, `.VOC`, fonts, EXEPACK |
| `crates/bmp-game` | The game logic reconstructed from `BMMAIN.EXE` |
| `crates/bmp-sound` | OPL2 emulation, the Creative FM driver, music and voice playback |
| `crates/bmp` | The executable: VGA/mouse library, host (SDL2 or scripted headless), all screens |
| `docs/re/` | Reverse-engineering notes, function and memory maps, subsystem specifications |
| `tests/visual/` | Scripted scenarios for the visual comparison with the original |
| `tools/` | `check.sh` (fmt, clippy, tests), `visual-check.sh`, DOSBox capture and Ghidra helpers |

## Legal

This repository contains no code, sounds or data files of the original game; the port loads
them from your own copy at runtime. The screenshots in `docs/screenshots/` show the original
artwork as rendered by the port. Bundesliga Manager Professional is © 1991–1992 Software 2000; all
trademarks belong to their owners. The port's own code is released under the
[MIT licence](LICENSE).
