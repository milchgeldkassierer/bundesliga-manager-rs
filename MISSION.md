# Mission

**Derived from:** Projektauftrag (2026-10-24): „1:1-Portierung von Bundesliga Manager Professional (DOS) nach Linux in Rust, Optik 1:1, Grafiken mit minimalen Toleranzen“ + Original-Executables `BMMAIN.EXE`, `BMINTRO.EXE`, `BMLOADER.EXE`, `TEAMED.EXE` (Reverse Engineering mit Ghidra 12.2 / ReVa / GhidraMCP)
**Last reconciled with it:** 2026-10-24

## What BMP Remake is

BMP Remake ist ein nativer Linux-Port des DOS-Spiels *Bundesliga Manager Professional*
(Software 2000, 1993), geschrieben in Rust. Spieler, die eine eigene Kopie des Originals
besitzen, starten das Spiel ohne DOSBox und erleben exakt dasselbe Spiel: dieselben
Bildschirme, dieselben Menüs, dieselben Zahlen, dieselben Ergebnisse.

Annahmen: Das Spiel liest die Original-Datendateien (`PIC/`, `TORE/`, `SOUND/`, `MANA.DAT`,
Spielstände) aus einem vom Nutzer angegebenen Verzeichnis. Logik und Darstellung werden aus
dem Maschinencode rekonstruiert, nicht nachempfunden. Interne Auflösung ist immer 320×200
mit 256-Farben-Palette (VGA Mode 13h); nur die Ausgabe wird skaliert.

## Who it is for

- Besitzer des Originals, die BMP unter Linux nativ spielen und ihre alten Spielstände weiter nutzen wollen.

BMP Remake ist kein „inspiriert von“-Remake und kein neuer Fußballmanager.

## Core capabilities (in scope)

**Dateiformate**
- Lesen aller Original-Datenformate (`.VGA`, `.CP`, `.PAL`, `TORE/*.T|*.V`, `MANA.DAT`, `*.MAN`, `HIGH.*`, `.CMF`, `.VOC`)
- Lesen und Schreiben von Spielständen im Originalformat (byte-kompatibel)

**Darstellung**
- Pixelgenaue Wiedergabe aller Bildschirme in 320×200×8 inkl. Paletten, Fades, Fonts, Animationen
- Integer-Skalierung und Fenster/Vollbild

**Spiellogik**
- Bitgenaue Rekonstruktion von Spielablauf, Simulation, Finanzen, Transfers, KI und Zufallsgenerator aus `BMMAIN.EXE`
- Intro (`BMINTRO.EXE`) und Teameditor (`TEAMED.EXE`) als Teil des Pakets

**Werkzeuge**
- Headless-Render-Modus (`--screenshot`) und Vergleichswerkzeuge gegen Referenzbilder aus DOSBox
- RE-Dokumentation in `docs/re/`

## Out of scope -- the factory must never build this

**Inhaltliche Änderungen**
- Aktualisierte Kader, echte Spielernamen, neue Saisons oder Ligen
- Neue Spielmechaniken, „Balancing-Fixes“ oder Korrekturen von Original-Bugs im Standardmodus
- HD-Grafiken, neu gezeichnete Sprites, Filter/Shader, die die 320×200-Pixeloptik ändern

**Plattform & Verbreitung**
- Mitlieferung oder Download von Original-Spieldaten oder -Executables im Repository
- Online-Multiplayer, Accounts, Cloud-Spielstände, Telemetrie
- Mobile-/Web-Portierung (WASM, Android, iOS)

**Technik**
- Einbetten eines x86-/DOS-Emulators zur Laufzeit statt rekonstruierter Logik
- Übersetzung der Oberflächentexte in andere Sprachen

## Hard invariants -- not tunable by any issue

1. **Originaltreue vor Eleganz.** Wo Original und „sauberer“ Code abweichen würden, gewinnt das Original (inkl. Rundungen, Überläufen, RNG-Reihenfolge).
2. **Grafik 1:1, minimale Toleranz.** Jeder Bildschirm wird gegen eine DOSBox-Referenz verglichen; Abweichung ≤ 0,5 % der Pixel und je Pixel ≤ 4 Stufen pro 8-bit-Kanal (nur DAC-Rundung). Größere Abweichungen sind Fehler.
3. **Keine Originaldaten im Repo.** Tests, die Originaldaten benötigen, laufen nur mit gesetztem `BMP_DATA` und werden sonst übersprungen.
4. **The factory cannot modify governance files.** `MISSION.md`, `FACTORY_RULES.md`
   and the conventions file are the constitution. A PR touching any of them is an
   automatic reject.
5. **The factory cannot modify its own judge.** `harness/` and `.factory/holdout/`
   define what "working" means here. Adding an assertion is
   always welcome; removing or loosening one is a human decision, always.

## Allowed evolutions

- Wahl der Rust-Bibliotheken für Fenster/Audio (z. B. SDL2, minifb, cpal)
- Interne Struktur des Rust-Codes, solange beobachtbares Verhalten identisch bleibt
- Zusätzliche Debug-/Analysewerkzeuge unter `tools/`

## Definition of done

**Gate 1 -- static checks and tests pass.** `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`

**Gate 2 -- Originaltreue.** Jede Änderung an Logik verweist auf die Ghidra-Adresse(n) der rekonstruierten Funktion; jede Änderung an Darstellung hat einen Referenzbild-Vergleich innerhalb der Toleranz.

**Gate 3 -- the end-to-end path passes as a real user.**

1. `bmp --data $BMP_DATA` starten
2. Titelbild erscheint (Referenzvergleich)
3. Hauptmenü bedienen, Spielstand laden
4. Der geladene Spielstand zeigt dieselben Vereinsdaten wie im Original

## Open questions -- decisions nobody has made yet

These are undecided, not forbidden. **The factory may propose an answer to any of
them**, build against it, and record what it assumed -- the merge is then held for a
human, so nothing ships on a guess and nothing stops for one.

- **Q1** Welche Fenster-/Audio-Bibliothek (SDL2 vs. minifb+cpal)?
- **Q2** Soll ein optionaler „Bugfix-Modus“ jemals existieren (standardmäßig aus)?

**Except these, which do stop the factory:**

- Alles, was Spielstände des Nutzers überschreibt oder migriert.

## Decisions

- 2026-10-24 **Toleranz der Grafik?** Minimale Toleranzen erlaubt (siehe Invariante 2).
- 2026-10-24 **Repo öffentlich?** Nein, privat; Originaldaten nie committen.

## What the factory does NOT own -- permanently human

- Fühlt es sich an wie das Original — Tempo, Eingabeverhalten, Timing von Animationen
- Sieht es aus wie das Original im direkten Nebeneinander mit DOSBox
- Klingt es wie das Original (OPL-Musik, digitalisierte Sprache)

The factory owns the file formats, the data model and the simulation: the layer
whose correctness can be asserted.
