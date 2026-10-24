# Audit: BMMAIN functions without a citation in the port

Scope: every function of `out/funcmap/bmmain.tsv` (540 functions of BMMAIN.EXE) whose address no
`seg:off` in `crates/**/*.rs` names. Addresses were compared as linear addresses (`seg*16+off`), so
citations in the other normalisations of the same byte (`2000:4a93` = `24a4:0053`,
`4000:34bc` = `422b:120c`) count. Before the comparison by linear address 193 functions looked
uncited; three of them (`24a4:0053` `tore_mirror_scene`, `422b:0047`/`013f` save cipher) are cited
in the other form. That leaves **190**. Two more are cited with the pre-relocation segment
(`raw 35ac:...` in `bmp-formats/src/vga.rs`); they are listed as "ported (raw address)".

Method: each function was classified from its Ghidra decompilation/disassembly, its callers
(`get_xrefs_to`; for near calls inside a segment also a byte scan of the unpacked EXE, because
Ghidra misses many `PUSH CS; CALL rel16` calls) and the RE docs. Each class:

* **library**: MSC 6.0 C runtime (segment 4a01) or a thin file/heap/graphics wrapper; replaced by
  Rust std or the port's own `Gfx`.
* **driver**: Sound Blaster detection, the embedded CT-VOICE.DRV and SBFMDRV drivers, EMS; replaced by
  `bmp-sound` (which ports the driver internals it needs and cites them by their own addresses) and SDL.
* **mis-split**: a Ghidra function object that is really the middle or tail of another function
  (the parent is named; the parent is ported).
* **dead code**: no caller in the original.
* **ported**: ported but without a citation; a citation was added where the port has a matching
  function.
* **REAL GAP**: behaviour of the original that the port lacked.

## Results

| Class | Count |
|---|---|
| library (CRT and wrappers) | 119 |
| driver (sound / EMS) | 46 |
| dead code | 10 |
| mis-split | 4 |
| ported (citation added or raw address) | 8 |
| deviation (original bug kept fixed) | 1 |
| REAL GAP | 2 |

Real gaps:

1. **`1cb5:0000`, short save tail** (fixed). `screen_load_game` reads the three tail longs
   (`DS:05f6`, `FAR:2e7a`, `FAR:2e94`) with plain `file_read` calls. When the first one returns
   fewer than 4 bytes (422b:1d28) it calls 1cb5:0000 (`DS:05ac = 1`, copy protection passed) and
   skips the other two; bytes after the tail are never read. The port refused such files (and files
   with trailing bytes) with the disk-error dialog. `SaveGame::parse` now reads the tail like the
   loader (`tail_len`, `tail_short()`), and `read_save` sets the flag. Test:
   `short_tail_loads_and_passes_the_copy_protection` (load_game.rs).
2. **`176b:114c`, `fatal_error_exit`** (issue #153). For codes other than 99 the original draws
   `LIB/GRAFIK/DISK/FONT/MEMORY/BildLade-Error`, waits, writes the crash save `ERROR.MAN` through
   `screen_save_game` and quits. The port panics (`.expect("... fatal_error_exit ...")`) on these
   paths, so no message is shown and no crash save is written. Every one of these paths needs damaged
   or missing game files.

Deviation: **`176b:02a3` `ui_set_pointer_shape`**. Shape 1 (DS:085e) is byte for byte the library
arrow `DEFAULT_POINTER`, so the port needs nothing at startup. Shape 0 (DS:0876) is a single
transparent pixel. Only `tore_report_read_error` 24a4:0574 sets it, after its box, and nothing sets
shape 1 again, so in the original the pointer stays invisible after a damaged goal-scene file. The
port keeps the arrow. This is now documented as `// DEVIATION:` in `goal_scene.rs`.

Doc correction: `text_filter_filename_char` 176b:0d56 is not used by the file selector (stats.md
§5.4). Nothing references it (no xref, no near or far call, no far pointer in the image), and
`ui_scroll_text_box` 176b:07d7 does not filter characters.

The largest functions named in the task: `2ecd:18bf` (3086 bytes) and `3e3a:1b29` (1732) are
mis-splits of `screen_squad_lineup` 2ecd:1560 and `newspaper_generate_and_show` 3e3a:0ea3. The
port has both parents; the tail of 0ea3 (headline selectors on `+0x11`, `DS:57a0`, the
`long_abs_diff` checks) matches `select_templates` in `bmp-game/src/news.rs`. `176b:114c` is the
gap above. `24a4:0053` and `422b:0047` are cited in another normalisation (see Scope). `4d6c:00f0`
and `4d6c:022a` are SB IRQ/port detection (driver). `45ac:3120` is the picture decoder (ported, raw
address).

## Table

Callers = number of distinct calling functions in the funcmap (0 = none found by Ghidra; for
the "dead code" rows a byte scan confirmed this).

| Address | Name | Bytes | Callers | Class | Notes | Status |
|---|---|---|---|---|---|---|
| 1310:2253 | `FUN_1310_2253` | 50 | 1 | ported | `match_live_free_pictures` (free PIC 0x26 / PIC 8 before a goal scene): `LiveView::free_pictures`; citation added | ok |
| 1310:2285 | `FUN_1310_2285` | 126 | 1 | ported | `match_live_load_pictures` (reload PIC 0x26 / PIC 8): `LiveView::load_pictures`; citation added | ok |
| 176b:004b | `sound_voice_shutdown` | 20 | 1 | driver | `sound_voice_shutdown` (CT-VOICE terminate): bmp-sound host teardown | ok |
| 176b:02a3 | `ui_set_pointer_shape` | 65 | 2 | deviation | `ui_set_pointer_shape`: shape 1 (DS:085e) is byte-identical to the library arrow `DEFAULT_POINTER`, so startup needs nothing. Shape 0 (DS:0876, one transparent pixel) is set only after the TORE read-error box (24a4:05e3) and is never undone: the pointer stays invisible for the rest of the session. Kept as an original bug fix, now documented as `// DEVIATION:` in goal_scene.rs | deviation |
| 176b:02e4 | `ui_blit_transparent` | 53 | 7 | library | `ui_blit_transparent` wrapper around `blit_bitmap_transparent`: callers use `Gfx::blit_*` directly | ok |
| 176b:0bdb | `FUN_176b_0bdb` | 112 | 1 | library | `font_load_all`: fonts come from `FontSet`/`Gfx::set_fonts` | ok |
| 176b:0c4b | `FUN_176b_0c4b` | 70 | 1 | library | `font_free_all`: Rust drop | ok |
| 176b:0d56 | `text_filter_filename_char` | 116 | 0 | dead code | `text_filter_filename_char`: no caller, no near/far call and no far pointer to it in the image (byte scan of the unpacked EXE); `ui_scroll_text_box` 176b:07d7 does not filter. stats.md §5.4 claim is wrong | ok |
| 176b:114c | `fatal_error_exit` | 657 | 11 | REAL GAP | `fatal_error_exit`: for codes != 99 draws "<kind>-Error", waits, writes the crash save `ERROR.MAN` and quits; the port panics on every fatal path. Too large for this audit: issue #153. Code 99 (normal quit) = `Quit` | missing (#153) |
| 1cb5:0000 | `FUN_1cb5_0000` | 18 | 1 | REAL GAP | `DS:05ac = 1` when the first tail long of a save cannot be read (422b:1d28). The port refused such a file with the disk-error dialog and also refused trailing bytes; now `SaveGame::parse` reads the tail like the loader and `read_save` sets the flag (test `short_tail_loads_and_passes_the_copy_protection`) | fixed |
| 2ecd:18bf | `FUN_2ecd_18bf` | 3086 | 2 | mis-split | middle of `screen_squad_lineup` 2ecd:1560 (body 1560..24cc, squad.md §0); its "callers" in 3e3a are wrong-CS near calls | ok |
| 3e3a:0919 | `FUN_3e3a_0919` | 37 | 1 | ported | `str_insert_space`, inlined as `Vec::insert` in `TextFlow::justify` (bmp-game news.rs); citation added | ok |
| 3e3a:093e | `FUN_3e3a_093e` | 38 | 1 | mis-split | middle of `str_insert_space` 3e3a:0919 | ok |
| 3e3a:1b29 | `FUN_3e3a_1b29` | 1732 | 1 | mis-split | tail of `newspaper_generate_and_show` 3e3a:0ea3 (body 0ea3..21ee, news.md §0): headline/body template picks; ported in `bmp-game/src/news.rs` `select_templates` | ok |
| 45ac:304a | `far_memcpy_bytes` | 48 | 18 | library | `far_memcpy_bytes`: slice copies | ok |
| 45ac:3107 | `vga_next_plane_helper` | 25 | 1 | ported (raw address) | plane-advance helper of the decoder (cited as `raw 35ac:3147` inner loop) | ok |
| 45ac:3120 | `FUN_45ac_3120` | 181 | 1 | ported (raw address) | `vga_decode_to_bitmap`: cited as `raw 35ac:3120` in bmp-formats/src/vga.rs | ok |
| 45ac:31d6 | `FUN_45ac_31d6` | 148 | 0 | dead code | `vga_encode_rle`: no callers in BMMAIN; ported anyway for tests (`raw 35ac:31d6`) | ok |
| 45ac:326b | `dos_getch` | 7 | 1 | ported | `dos_getch` = `Host::getch`; citation added | ok |
| 45ac:3272 | `dos_direct_console_io` | 11 | 3 | ported | `dos_direct_console_io` = `Host::read_key_nowait` (callers 1310:2a49, 2a58:145a, 362f:0795/0858 all ported); citation added | ok |
| 45ac:337e | `install_int_handler` | 58 | 1 | library | `install_int_handler(0,0)` at startup (18bc:002c): DOS vector hook, no port equivalent needed | ok |
| 45ac:3690 | `dos_delete_file` | 47 | 1 | ported | `dos_delete_file` = `load_game::delete_file` (file_selector 422b:379e); citation added | ok |
| 45ac:36c7 | `ems_detect` | 78 | 1 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:3716 | `ems_get_page_frame` | 57 | 1 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:3750 | `ems_alloc_pages` | 54 | 1 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:3787 | `ems_map_page` | 49 | 2 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:37b8 | `ems_map_or_free` | 60 | 2 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:37f5 | `ems_save_map` | 43 | 1 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 45ac:3820 | `ems_restore_map` | 43 | 2 | driver | EMS (INT 67h) memory for the VOC samples: samples live in memory | ok |
| 4930:000c | `file_close` | 23 | 12 | library | file / heap wrapper (`file_close`): std::fs, Rust allocation | ok |
| 4930:0023 | `file_open` | 101 | 11 | library | file / heap wrapper (`file_open`): std::fs, Rust allocation | ok |
| 4930:0088 | `file_read` | 31 | 11 | library | file / heap wrapper (`file_read`): std::fs, Rust allocation | ok |
| 4930:00a7 | `file_seek` | 47 | 0 | dead code | `file_seek`: no callers | ok |
| 4930:00d6 | `file_write` | 43 | 3 | library | file / heap wrapper (`file_write`): std::fs, Rust allocation | ok |
| 4930:010c | `mem_alloc` | 124 | 24 | library | file / heap wrapper (`mem_alloc`): std::fs, Rust allocation | ok |
| 4930:0188 | `mem_free` | 95 | 22 | library | file / heap wrapper (`mem_free`): std::fs, Rust allocation | ok |
| 4930:0216 | `FUN_4930_0216` | 22 | 1 | library | `font_handle_invalidate` | ok |
| 4930:022c | `FUN_4930_022c` | 53 | 1 | library | `font_open`: `FontSet` lookup | ok |
| 4930:057c | `bitmap_init` | 40 | 1 | library | `bitmap_init`: `Bitmap::new_mem` | ok |
| 4a01:0016 | `crt_start` | 203 | 0 | library | CRT entry point (`crt_start`), replaced by Rust `main` | ok |
| 4a01:00f1 | `crt_amsg_exit` | 36 | 3 | library | MSC 6 CRT (`crt_amsg_exit`): Rust std / core | ok |
| 4a01:0116 | `crt_inherit_file_info` | 197 | 1 | library | MSC 6 CRT (`crt_inherit_file_info`): Rust std / core | ok |
| 4a01:01db | `crt_exit` | 105 | 2 | library | MSC 6 CRT (`crt_exit`): Rust std / core | ok |
| 4a01:025e | `crt_ctermsub` | 45 | 1 | library | MSC 6 CRT (`crt_ctermsub`): Rust std / core | ok |
| 4a01:028b | `crt_initterm` | 19 | 3 | library | MSC 6 CRT (`crt_initterm`): Rust std / core | ok |
| 4a01:029e | `crt_ff_msgbanner` | 34 | 3 | library | MSC 6 CRT (`crt_ff_msgbanner`): Rust std / core | ok |
| 4a01:02c0 | `crt_amsg_exit_jump` | 6 | 1 | library | MSC 6 CRT (`crt_amsg_exit_jump`): Rust std / core | ok |
| 4a01:02c6 | `crt_chkstk` | 36 | 319 | library | MSC 6 CRT (`crt_chkstk`): Rust std / core | ok |
| 4a01:02ea | `crt_nullcheck` | 36 | 1 | library | MSC 6 CRT (`crt_nullcheck`): Rust std / core | ok |
| 4a01:030e | `crt_setargv` | 428 | 1 | library | MSC 6 CRT (`crt_setargv`): Rust std / core | ok |
| 4a01:04bc | `crt_setenvp` | 160 | 1 | library | MSC 6 CRT (`crt_setenvp`): Rust std / core | ok |
| 4a01:055c | `crt_nmsg_text` | 43 | 1 | library | MSC 6 CRT (`crt_nmsg_text`): Rust std / core | ok |
| 4a01:0587 | `crt_nmsg_write` | 54 | 4 | library | MSC 6 CRT (`crt_nmsg_write`): Rust std / core | ok |
| 4a01:05be | `crt_alloc_or_die` | 37 | 1 | library | MSC 6 CRT (`crt_alloc_or_die`): Rust std / core | ok |
| 4a01:05e4 | `crt_dosret0` | 8 | 2 | library | MSC 6 CRT (`crt_dosret0`): Rust std / core | ok |
| 4a01:05ec | `crt_dosretax` | 13 | 2 | library | MSC 6 CRT (`crt_dosretax`): Rust std / core | ok |
| 4a01:05f9 | `crt_dosreturn` | 13 | 6 | library | MSC 6 CRT (`crt_dosreturn`): Rust std / core | ok |
| 4a01:0606 | `crt_dosmaperr_far` | 6 | 1 | library | MSC 6 CRT (`crt_dosmaperr_far`): Rust std / core | ok |
| 4a01:060c | `crt_dosmaperr` | 45 | 3 | library | MSC 6 CRT (`crt_dosmaperr`): Rust std / core | ok |
| 4a01:063a | `crt_close` | 32 | 1 | library | MSC 6 CRT (`crt_close`): Rust std / core | ok |
| 4a01:065a | `crt_lseek` | 122 | 3 | library | MSC 6 CRT (`crt_lseek`): Rust std / core | ok |
| 4a01:06ee | `crt_open` | 417 | 1 | library | MSC 6 CRT (`crt_open`): Rust std / core | ok |
| 4a01:088f | `crt_nop_088f` | 17 | 1 | library | MSC 6 CRT (`crt_nop_088f`): Rust std / core | ok |
| 4a01:08a0 | `crt_read` | 254 | 1 | library | MSC 6 CRT (`crt_read`): Rust std / core | ok |
| 4a01:099e | `crt_write` | 192 | 1 | library | MSC 6 CRT (`crt_write`): Rust std / core | ok |
| 4a01:0a5e | `crt_write_flush_chunk` | 102 | 1 | library | MSC 6 CRT (`crt_write_flush_chunk`): Rust std / core | ok |
| 4a01:0ac4 | `crt_write_raw` | 57 | 1 | library | MSC 6 CRT (`crt_write_raw`): Rust std / core | ok |
| 4a01:0afe | `crt_stackavail` | 20 | 1 | library | MSC 6 CRT (`crt_stackavail`): Rust std / core | ok |
| 4a01:0b12 | `crt_malloc_thunk` | 5 | 2 | library | MSC 6 CRT (`crt_malloc_thunk`): Rust std / core | ok |
| 4a01:0b18 | `crt_ffree` | 19 | 1 | library | MSC 6 CRT (`crt_ffree`): Rust std / core | ok |
| 4a01:0b2b | `crt_fmalloc` | 125 | 2 | library | MSC 6 CRT (`crt_fmalloc`): Rust std / core | ok |
| 4a01:0ba8 | `crt_heap_init_segment` | 52 | 1 | library | MSC 6 CRT (`crt_heap_init_segment`): Rust std / core | ok |
| 4a01:0bdc | `crt_heap_link_segment` | 54 | 1 | library | MSC 6 CRT (`crt_heap_link_segment`): Rust std / core | ok |
| 4a01:0c12 | `crt_heap_search_segment` | 123 | 2 | library | MSC 6 CRT (`crt_heap_search_segment`): Rust std / core | ok |
| 4a01:0d1d | `crt_heap_grow_segment` | 80 | 1 | library | MSC 6 CRT (`crt_heap_grow_segment`): Rust std / core | ok |
| 4a01:0d6e | `crt_heap_find_end` | 32 | 1 | library | MSC 6 CRT (`crt_heap_find_end`): Rust std / core | ok |
| 4a01:0d8e | `crt_heap_new_segment` | 67 | 1 | library | MSC 6 CRT (`crt_heap_new_segment`): Rust std / core | ok |
| 4a01:0dd2 | `crt_fstrcat` | 70 | 54 | library | MSC 6 CRT (`crt_fstrcat`): Rust std / core | ok |
| 4a01:0e18 | `crt_fstrcpy` | 54 | 66 | library | MSC 6 CRT (`crt_fstrcpy`): Rust std / core | ok |
| 4a01:0e4e | `crt_fstrcmp` | 41 | 7 | library | MSC 6 CRT (`crt_fstrcmp`): Rust std / core | ok |
| 4a01:0e78 | `crt_fstrlen` | 23 | 20 | library | MSC 6 CRT (`crt_fstrlen`): Rust std / core | ok |
| 4a01:0e90 | `crt_fstrncpy` | 42 | 1 | library | MSC 6 CRT (`crt_fstrncpy`): Rust std / core | ok |
| 4a01:0eba | `crt_fstrncmp` | 59 | 3 | library | MSC 6 CRT (`crt_fstrncmp`): Rust std / core | ok |
| 4a01:0ef6 | `crt_atoi_thunk` | 3 | 1 | library | MSC 6 CRT (`crt_atoi_thunk`): Rust std / core | ok |
| 4a01:0efa | `crt_atof` | 87 | 1 | library | MSC 6 CRT (`crt_atof`): Rust std / core | ok |
| 4a01:0f52 | `crt_atoi` | 86 | 1 | library | MSC 6 CRT (`crt_atoi`): Rust std / core | ok |
| 4a01:0fa8 | `crt_fptostr` | 162 | 3 | library | MSC 6 CRT (`crt_fptostr`): Rust std / core | ok |
| 4a01:104e | `crt_getenv` | 141 | 1 | library | MSC 6 CRT (`crt_getenv`): Rust std / core | ok |
| 4a01:10dc | `crt_stub_10dc` | 23 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:10f3 | `crt_stub_10f3` | 21 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:1108 | `crt_stub_1108` | 9 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:1111 | `crt_stub_1111` | 22 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:1127 | `crt_stub_1127` | 9 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:1130 | `crt_stub_1130` | 26 | 0 | dead code | CRT stub, no callers | ok |
| 4a01:114a | `crt_huge_ptr_diff` | 43 | 1 | library | MSC 6 CRT (`crt_huge_ptr_diff`): Rust std / core | ok |
| 4a01:1176 | `crt_tell` | 23 | 1 | library | MSC 6 CRT (`crt_tell`): Rust std / core | ok |
| 4a01:118e | `crt_filelength` | 134 | 1 | library | `filelength` of the CMF file in `main` 18bc:0418: `std::fs::read` | ok |
| 4a01:1214 | `crt_fheapmin` | 56 | 1 | library | MSC 6 CRT (`crt_fheapmin`): Rust std / core | ok |
| 4a01:124c | `crt_heapmin_segment` | 109 | 1 | library | MSC 6 CRT (`crt_heapmin_segment`): Rust std / core | ok |
| 4a01:12ba | `crt_heap_unlink_free_segment` | 89 | 1 | library | MSC 6 CRT (`crt_heap_unlink_free_segment`): Rust std / core | ok |
| 4a01:1314 | `crt_intdosx` | 97 | 1 | library | MSC 6 CRT (`crt_intdosx`): Rust std / core | ok |
| 4a01:1376 | `crt_outp` | 14 | 2 | library | `outp`: VGA map-mask writes of the palette-slot store (18bc:0004) and the picture loader; emulated by `Vga` | ok |
| 4a01:1384 | `crt_time` | 85 | 1 | library | `time()`: seeds the RNG in `random_seed_from_time` 176b:0cf0 (cited) | ok |
| 4a01:13da | `crt_tzset_once` | 17 | 1 | library | MSC 6 CRT (`crt_tzset_once`): Rust std / core | ok |
| 4a01:13ec | `crt_tzset` | 207 | 1 | library | MSC 6 CRT (`crt_tzset`): Rust std / core | ok |
| 4a01:14bc | `crt_isindst` | 218 | 1 | library | MSC 6 CRT (`crt_isindst`): Rust std / core | ok |
| 4a01:169a | `crt_fmemmove` | 202 | 1 | library | MSC 6 CRT (`crt_fmemmove`): Rust std / core | ok |
| 4a01:1764 | `crt_fmemset` | 72 | 2 | library | MSC 6 CRT (`crt_fmemset`): Rust std / core | ok |
| 4a01:1809 | `crt_chdir` | 15 | 1 | library | `chdir` in `file_selector`: `SaveSession::change_dir` | ok |
| 4a01:1842 | `crt_getdrive` | 8 | 2 | library | `_getdrive` in `file_selector` | ok |
| 4a01:184a | `crt_chdrive` | 27 | 1 | library | `_chdrive` in `file_selector`: `SaveSession::change_drive` | ok |
| 4a01:1866 | `crt_getcwd` | 24 | 1 | library | `getcwd` in `file_selector`: `SaveSession::cwd_text` | ok |
| 4a01:187e | `crt_getdcwd` | 259 | 1 | library | MSC 6 CRT (`crt_getdcwd`): Rust std / core | ok |
| 4a01:1984 | `crt_dos_handle_call_1984` | 7 | 0 | dead code | CRT `_dos_*` helper, no callers | ok |
| 4a01:198b | `crt_dos_creat` | 38 | 1 | library | MSC 6 CRT (`crt_dos_creat`): Rust std / core | ok |
| 4a01:19b2 | `crt_dos_findnext` | 11 | 1 | library | `_dos_findnext` in `file_selector`: `list_directory` | ok |
| 4a01:19bd | `crt_dos_findfirst` | 51 | 1 | library | `_dos_findfirst` in `file_selector`: `list_directory` | ok |
| 4a01:19f0 | `crt_dos_setdrive` | 23 | 1 | library | `_dos_setdrive` (drive count) in `file_selector`: `LOGICAL_DRIVES` | ok |
| 4a01:1a08 | `crt_aflldiv` | 34 | 2 | library | MSC 6 CRT (`crt_aflldiv`): Rust std / core | ok |
| 4a01:1a2a | `crt_afllmul` | 34 | 1 | library | MSC 6 CRT (`crt_afllmul`): Rust std / core | ok |
| 4a01:1bd0 | `crt_afldiv_assign` | 32 | 7 | library | MSC 6 CRT (`crt_afldiv_assign`): Rust std / core | ok |
| 4a01:1bf0 | `crt_aflmul_assign` | 32 | 5 | library | MSC 6 CRT (`crt_aflmul_assign`): Rust std / core | ok |
| 4a01:1cda | `crt_fmemset_2` | 72 | 1 | library | MSC 6 CRT (`crt_fmemset_2`): Rust std / core | ok |
| 4a01:1d70 | `crt_fltout_core` | 383 | 1 | library | MSC 6 CRT (`crt_fltout_core`): Rust std / core | ok |
| 4a01:2031 | `crt_fltout_pow10` | 62 | 1 | library | MSC 6 CRT (`crt_fltout_pow10`): Rust std / core | ok |
| 4a01:2086 | `crt_ld12cvt` | 526 | 3 | library | MSC 6 CRT (`crt_ld12cvt`): Rust std / core | ok |
| 4a01:254c | `crt_fp_helper_254c` | 145 | 2 | library | MSC 6 CRT (`crt_fp_helper_254c`): Rust std / core | ok |
| 4a01:2621 | `crt_ld12mul` | 394 | 1 | library | MSC 6 CRT (`crt_ld12mul`): Rust std / core | ok |
| 4a01:27a6 | `crt_atoldbl_mantissa` | 174 | 1 | library | MSC 6 CRT (`crt_atoldbl_mantissa`): Rust std / core | ok |
| 4a01:2854 | `crt_shl96` | 11 | 1 | library | MSC 6 CRT (`crt_shl96`): Rust std / core | ok |
| 4a01:2860 | `crt_strgtold_wrap` | 42 | 1 | library | MSC 6 CRT (`crt_strgtold_wrap`): Rust std / core | ok |
| 4a01:28c8 | `crt_strgtold12` | 616 | 2 | library | MSC 6 CRT (`crt_strgtold12`): Rust std / core | ok |
| 4a01:2ba0 | `crt_forcdecpt` | 123 | 0 | library | MSC 6 CRT (`crt_forcdecpt`): Rust std / core | ok |
| 4a01:2c1e | `crt_positive` | 46 | 0 | library | MSC 6 CRT (`crt_positive`): Rust std / core | ok |
| 4a01:2c4c | `crt_fassign` | 107 | 0 | library | MSC 6 CRT (`crt_fassign`): Rust std / core | ok |
| 4a01:2cba | `crt_cftoe` | 390 | 2 | library | MSC 6 CRT (`crt_cftoe`): Rust std / core | ok |
| 4a01:2e42 | `crt_cftoe_g` | 42 | 1 | library | MSC 6 CRT (`crt_cftoe_g`): Rust std / core | ok |
| 4a01:2e6c | `crt_cftof` | 339 | 2 | library | MSC 6 CRT (`crt_cftof`): Rust std / core | ok |
| 4a01:2fc2 | `crt_cftof_g` | 39 | 1 | library | MSC 6 CRT (`crt_cftof_g`): Rust std / core | ok |
| 4a01:2fea | `crt_cftog` | 213 | 1 | library | MSC 6 CRT (`crt_cftog`): Rust std / core | ok |
| 4a01:30c0 | `crt_cfltcvt` | 96 | 0 | library | MSC 6 CRT (`crt_cfltcvt`): Rust std / core | ok |
| 4a01:3122 | `crt_shift_string` | 52 | 2 | library | MSC 6 CRT (`crt_shift_string`): Rust std / core | ok |
| 4a01:318a | `crt_fltin` | 89 | 1 | library | MSC 6 CRT (`crt_fltin`): Rust std / core | ok |
| 4a01:31e4 | `crt_fltout` | 75 | 3 | library | MSC 6 CRT (`crt_fltout`): Rust std / core | ok |
| 4a01:3288 | `crt_cftoe_ld` | 76 | 1 | library | MSC 6 CRT (`crt_cftoe_ld`): Rust std / core | ok |
| 4a01:32d4 | `crt_cftoe_ld_fmt` | 368 | 2 | library | MSC 6 CRT (`crt_cftoe_ld_fmt`): Rust std / core | ok |
| 4a01:344a | `crt_cftof_ld` | 71 | 1 | library | MSC 6 CRT (`crt_cftof_ld`): Rust std / core | ok |
| 4a01:3492 | `crt_cftof_ld_fmt` | 288 | 2 | library | MSC 6 CRT (`crt_cftof_ld_fmt`): Rust std / core | ok |
| 4a01:35b6 | `crt_cftog_ld` | 122 | 1 | library | MSC 6 CRT (`crt_cftog_ld`): Rust std / core | ok |
| 4a01:3630 | `crt_cfltcvt_ld` | 102 | 0 | library | MSC 6 CRT (`crt_cfltcvt_ld`): Rust std / core | ok |
| 4a01:3698 | `crt_fp_tail_3698` | 38 | 1 | library | MSC 6 CRT (`crt_fp_tail_3698`): Rust std / core | ok |
| 4d6c:000e | `sb_dsp_read` | 39 | 2 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:0035 | `sb_dsp_write_timeout` | 40 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:007b | `sb_dsp_write` | 31 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:009a | `dos_set_vector_raw` | 21 | 2 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:00af | `dos_get_vector_raw` | 19 | 2 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:00f0 | `sb_detect_irq` | 199 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:01fc | `sb_detect_port` | 46 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:022a | `sb_probe_port` | 154 | 2 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:02c4 | `opl_wait_status` | 33 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:02e5 | `opl_calibrate_delay` | 125 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:0362 | `opl_write_reg` | 41 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:038b | `pit_set_channel0_divisor` | 13 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:03ab | `ctvoice_init` | 56 | 1 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:03e3 | `ctvoice_speaker` | 16 | 0 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:03f3 | `ctvoice_set_status_addr` | 20 | 0 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:0407 | `ctvoice_stop` | 9 | 2 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:0410 | `ctvoice_terminate` | 9 | 1 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:0419 | `ctvoice_input` | 29 | 0 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:0436 | `ctvoice_output` | 20 | 1 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:045c | `ctvoice_break_loop` | 16 | 0 | driver | CT-VOICE call wrapper: bmp-sound `Voice` | ok |
| 4d6c:046c | `sb_dsp_reset` | 47 | 1 | driver | Sound Blaster detection / DSP / OPL / PIT / IVT access: not needed, the port emulates one fixed card (bmp-sound) | ok |
| 4d6c:049c | `fmdrv_init` | 59 | 1 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:04ee | `fmdrv_play_music` | 19 | 1 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0501 | `fmdrv_set_instruments` | 22 | 1 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0517 | `fmdrv_reset` | 13 | 1 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0524 | `fmdrv_set_system_clock` | 16 | 0 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0534 | `fmdrv_set_driver_clock` | 16 | 1 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0566 | `fmdrv_pause_music` | 13 | 0 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4d6c:0573 | `fmdrv_resume_music` | 13 | 0 | driver | SBFMDRV call wrapper: bmp-sound `FmDriver` | ok |
| 4dcb:0000 | `ctvoice_entry_jmp` | 3 | 8 | driver | embedded CT-VOICE.DRV entry; digitised voices in bmp-sound `voice.rs` | ok |
| 4dcb:05e6 | `ctvoice_internal_05e6` | 70 | 1 | driver | CT-VOICE.DRV internal: bmp-sound `voice.rs` | ok |
| 4dcb:06fe | `ctvoice_internal_06fe` | 32 | 1 | driver | CT-VOICE.DRV internal: bmp-sound `voice.rs` | ok |
| 4dcb:0754 | `ctvoice_dispatch` | 65 | 1 | driver | CT-VOICE.DRV internal: bmp-sound `voice.rs` | ok |
| 4e67:0000 | `fmdrv_entry_jmp` | 3 | 10 | driver | embedded SBFMDRV entry; FM driver re-implemented in bmp-sound `fmdrv.rs` (cites the 4e67 internals it ports) | ok |
| 4e67:093b | `fmdrv_internal_pit_out` | 3 | 1 | driver | SBFMDRV internal: bmp-sound `fmdrv.rs`/`opl.rs` | ok |
| 4e67:093e | `fmdrv_internal_opl_write` | 38 | 1 | driver | SBFMDRV internal: bmp-sound `fmdrv.rs`/`opl.rs` | ok |
| 4e67:0aea | `fmdrv_internal_0aea` | 53 | 1 | driver | SBFMDRV internal: bmp-sound `fmdrv.rs`/`opl.rs` | ok |
| 4e67:0bd3 | `fmdrv_internal_0bd3` | 1 | 1 | mis-split | 1-byte SBFMDRV fragment (false xref from 3e3a:23aa) | ok |
| 4e67:110f | `fmdrv_dispatch` | 94 | 1 | driver | SBFMDRV internal: bmp-sound `fmdrv.rs`/`opl.rs` | ok |
