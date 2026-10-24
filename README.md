# BMP Remake – Bundesliga Manager Professional (Linux/Rust-Port)

Ziel: 1:1-Port des DOS-Spiels *Bundesliga Manager Professional* (Software 2000, 1993) nach Linux in Rust,
basierend auf Reverse Engineering der Original-Executables (`BMMAIN.EXE`, `BMINTRO.EXE`, `BMLOADER.EXE`) mit Ghidra.
Grafik muss pixelgenau dem Original entsprechen (320×200, 256 Farben, Original-Paletten).

**Es werden keine Original-Spieldaten mitgeliefert.** Zum Spielen wird eine eigene Kopie des Originals benötigt
(Pfad per `--data <dir>`).

## Struktur
- `crates/bmp-formats` – Parser für `.VGA`, `.CP`, `.PAL`, `TORE/*.T|V`, `MANA.DAT`, `*.MAN`
- `crates/bmp-game` – Spiellogik (aus BMMAIN.EXE rekonstruiert)
- `crates/bmp` – Binary: Rendering (320×200 Framebuffer), Eingabe, Sound
- `docs/re/` – RE-Notizen (Dateiformate, Funktionskarte, Speicherlayout)
- `tools/` – Ghidra-Skripte und Hilfswerkzeuge

Fortschritt: siehe GitHub Issues & Milestones.
