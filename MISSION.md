# Mission

**Derived from:** Project brief (2026-10-24): "1:1 port of Bundesliga Manager Professional (DOS) to Linux in Rust, 1:1 look, graphics allow minimal tolerances" + the original executables `BMMAIN.EXE`, `BMINTRO.EXE`, `BMLOADER.EXE`, `TEAMED.EXE` (reverse engineered with Ghidra 12.2 / ReVa / GhidraMCP)
**Last reconciled with it:** 2026-10-24

## What BMP Remake is

BMP Remake is a native Linux port of the DOS game *Bundesliga Manager Professional*
(Software 2000, 1993), written in Rust. Players who own the original start the game
without DOSBox and get exactly the same game: the same screens, the same menus, the
same numbers, the same results.

Assumptions: the game reads the original data files (`PIC/`, `TORE/`, `SOUND/`,
`MANA.DAT`, save games) from a user-supplied directory. Logic and presentation are
reconstructed from the machine code, not imitated. Internal resolution is always
320×200 with a 256-color palette (VGA mode 13h); only the output is scaled.

## Who it is for

- Owners of the original who want to play BMP natively on Linux and keep using their old save games.

BMP Remake is not an "inspired by" remake and not a new football manager.

## Core capabilities (in scope)

**File formats**
- Reading every original data format (`.VGA`, `.CP`, `.PAL`, `TORE/*.T|*.V`, `MANA.DAT`, `*.MAN`, `HIGH.*`, `.CMF`, `.VOC`)
- Reading and writing save games in the original format (byte compatible)

**Presentation**
- Pixel-exact reproduction of every screen in 320×200×8 including palettes, fades, fonts, animations
- Integer scaling, window and fullscreen

**Game logic**
- Bit-exact reconstruction of game flow, match simulation, finances, transfers, AI and random number generator from `BMMAIN.EXE`
- Intro (`BMINTRO.EXE`) and team editor (`TEAMED.EXE`) as part of the package

**Tooling**
- Headless render mode (`--screenshot`) and comparison tools against DOSBox reference frames
- Reverse-engineering documentation in `docs/re/`

## Out of scope -- the factory must never build this

**Content changes**
- Updated squads, real player names, new seasons or leagues
- New game mechanics, "balancing fixes" or fixes of original bugs in the default mode
- HD graphics, redrawn sprites, filters/shaders that change the 320×200 pixel look

**Platform and distribution**
- Shipping or downloading original game data or executables in the repository
- Online multiplayer, accounts, cloud saves, telemetry
- Mobile/web ports (WASM, Android, iOS)

**Technology**
- Embedding an x86/DOS emulator at runtime instead of reconstructed logic
- Translating the in-game text into other languages

## Hard invariants -- not tunable by any issue

1. **Fidelity over elegance.** Where the original and "clean" code would differ, the original wins (rounding, overflow, RNG call order).
2. **Graphics 1:1, minimal tolerance.** Every screen is compared against a DOSBox reference; thresholds are defined in `docs/re/TOLERANCES.md` (phase 1: >= 90 % matching pixels, exact match is the long-term target).
3. **No original data in the repo.** Tests that need original data run only when `BMP_DATA` is set and are skipped otherwise.
4. **English only in the repository.** Source code, identifiers, comments and documentation are English; only verbatim original in-game text is German.
5. **The factory cannot modify governance files.** `MISSION.md`, `FACTORY_RULES.md`
   and the conventions file are the constitution. A PR touching any of them is an
   automatic reject.
6. **The factory cannot modify its own judge.** `harness/` and `.factory/holdout/`
   define what "working" means here. Adding an assertion is
   always welcome; removing or loosening one is a human decision, always.

## Allowed evolutions

- Choice of Rust crates for windowing/audio (e.g. SDL2, minifb, cpal)
- Internal structure of the Rust code, as long as observable behavior stays identical
- Additional debug/analysis tools under `tools/`

## Definition of done

**Gate 1 -- static checks and tests pass.** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

**Gate 2 -- fidelity.** Every logic change cites the Ghidra address(es) of the reconstructed function; every presentation change comes with a reference-frame comparison within tolerance.

**Gate 3 -- the end-to-end path passes as a real user.**

1. Start `bmp --data $BMP_DATA`
2. The title screen appears (reference comparison)
3. Navigate the main menu, load a save game
4. The loaded save shows the same club data as the original

## Open questions -- decisions nobody has made yet

These are undecided, not forbidden. **The factory may propose an answer to any of
them**, build against it, and record what it assumed -- the merge is then held for a
human, so nothing ships on a guess and nothing stops for one.

- **Q1** Which windowing/audio crates (SDL2 vs. minifb + cpal)?
- **Q2** Should an optional "bugfix mode" ever exist (off by default)?

**Except these, which do stop the factory:**

- Anything that overwrites or migrates the user's save games.

## Decisions

- 2026-10-24 **Repository language?** English for code and docs.
- 2026-10-24 **Graphics tolerance?** Phase 1 accepts >= 90 % matching pixels; graphics get refined later (`docs/re/TOLERANCES.md`).
- 2026-10-24 **Public repo?** No, private; original data is never committed.

## What the factory does NOT own -- permanently human

- Does it FEEL like the original — pacing, input behavior, animation timing
- Does it LOOK like the original side by side with DOSBox
- Does it SOUND like the original (OPL music, digitized speech)

The factory owns the file formats, the data model and the simulation: the layer
whose correctness can be asserted.
