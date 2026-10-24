# Graphics tolerances

Each rendered 320x200 frame is compared against a DOSBox reference capture of the same screen.

- At most 0.5 % of pixels may differ.
- Per differing pixel, each 8-bit RGB channel may differ by at most 4 (VGA DAC 6-bit to 8-bit rounding).
- Geometry (positions, sizes, glyph shapes) must match exactly; tolerance covers color rounding only.
