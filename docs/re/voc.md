# VOC (Creative Voice File) and `SOUND\DIGI.VOC`

The digitised sound effects of the match day (crowd noise, goal cheer, whistle, ...) are
8-bit unsigned mono PCM samples in Creative Voice File format. They are played by the
CT-VOICE driver embedded in BMMAIN.EXE (segment 4dcb, "Copyright (c) Creative Technology
Pte Ltd., 1989-1990") from EMS memory. The FM title music is a separate file, see
[cmf.md](cmf.md).

## 1. File format

All values little-endian.

### 1.1 Header

| Offset | Size | Content |
|---|---|---|
| 0x00 | 20 | `"Creative Voice File"` followed by `0x1a` |
| 0x14 | 2 | offset of the first data block (header size), `0x001a` |
| 0x16 | 2 | format version, minor byte first: `0x010a` = 1.10 |
| 0x18 | 2 | check word = `~version + 0x1234` (`0x1129` for 1.10) |

### 1.2 Data blocks

Every block starts with a type byte. Type 0 is the terminator and has no length. All other
types are followed by a 24-bit length of the block body (excluding the 4 header bytes).

| Type | Name | Body |
|---|---|---|
| 0 | terminator | none (no length field) |
| 1 | sound data | `u8 time_constant`, `u8 pack`, then `length - 2` sample bytes. Rate = `1000000 / (256 - time_constant)` Hz. `pack`: 0 = 8-bit PCM, 1 = 4-bit ADPCM, 2 = 2.6-bit ADPCM, 3 = 2-bit ADPCM |
| 2 | sound continuation | sample bytes in the format of the last type 1 block |
| 3 | silence | `u16 length - 1` (samples), `u8 time_constant` |
| 4 | marker | `u16 marker`; the driver writes it to the caller's status word |
| 5 | ASCII text | zero-terminated string |
| 6 | repeat start | `u16 count`: the blocks up to the next type 7 are played `count + 1` times; `0xffff` = endless |
| 7 | repeat end | none (length 0) |
| 8 | extended (VOC 1.20+) | `u16 time_constant` (`65536 - 256000000 / (channels * rate)`), `u8 pack`, `u8 mode` (0 mono, 1 stereo); applies to the next type 1 block |
| 9 | sound data, new format (VOC 1.20+) | `u32 rate`, `u8 bits`, `u8 channels`, `u16 codec`, 4 reserved bytes, samples |

8-bit PCM samples are unsigned (0x80 = silence).

The embedded CT-VOICE driver (voice-block interpreter around 4dcb:05e6..0748) handles types
0..7: type 1 sends DSP command 0x40 (time constant) and the DMA command from the pack table
at CS:0749 (`0x14` for pack 0 = 8-bit single-cycle DMA); type 2 reuses the last setting;
type 3 programs DSP command 0x80 (silence); type 4 stores the marker in the status word;
type 6 saves the current position and the count (CS:00d2); type 7 rewinds while the count
is non-zero, decrementing it unless it is 0xffff. Types 8 and 9 (newer VOC versions) are
not used by DIGI.VOC; the port only needs types 0, 1, 6 and 7, but its parser accepts all.

## 2. `SOUND\DIGI.VOC`

DIGI.VOC (90326 bytes) is **five complete VOC files concatenated**. Each has its own
26-byte header (version 1.10) and one sound block; all are 8-bit unsigned PCM (pack 0).

| File slot | File offset | Length | Blocks | Rate | Samples | Duration | Voice `n` |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 7018 | 1 | 10000 Hz (tc 156) | 6985 | 0.699 s | 3: whistle (cards) |
| 1 | 7018 | 16080 | 1 | 10000 Hz (tc 156) | 16047 | 1.605 s | 2: goal |
| 2 | 23098 | 29084 | 6 (count 32000), 1, 7 | 10000 Hz (tc 156) | 29041 | 2.904 s per pass | 1: crowd noise loop |
| 3 | 52182 | 22331 | 1 | 20000 Hz (tc 206) | 22298 | 1.115 s | 4: injury |
| 4 | 74513 | 15813 | 1 | 20000 Hz (tc 206) | 15780 | 0.789 s | 5: miss |

Slot 2's repeat block plays the crowd sample 32001 times (about 26 hours), i.e. it loops
for as long as the game lets it. The voice names are inferred from the call sites (§5).

The game does not scan the file for headers: it reads the five slots with the fixed lengths
of the table `DS:0164` (§3).

## 3. Loading

Globals (DGROUP):

| DS | Type | Meaning |
|---|---|---|
| 014c | far ptr | EMS page frame (`ems_get_page_frame`) |
| 0150 | `{u16 handle; u16 pages;}[5]` | EMS handle and page count per slot; pages = 1, 1, 2, 2, 1 |
| 0164 | `u16[5]` | slot lengths: 7018, 16080, 29084, 22331, 15813 |
| 016c | `u16[6]` | voice `n` (1..5) -> slot: `{_, 2, 1, 0, 3, 4}` (entry 0 overlaps `DS:0164[4]`) |
| 0178 | char[] | `"sound\digi.voc"` |
| 018c | u8 | sound enabled (initial 1; cleared by `bmmain 0`) |
| 018d | u8 | digitised sound available |
| 018e | u8 | voice currently started (0 = none) |
| 018f | u8 | crowd loop wanted (set by voice 1, cleared by stopping it) |
| 0190 | u16 | EMS handle whose map was saved, -1 = map not saved |
| 94c2 | u16 | CT-VOICE status word (non-zero while output runs) |

```c
void sound_voice_init(void)                         // 176b:0006, called by main
{
    if (DS:018c && ems_detect() > 6 && (sb_probe_port() & 4) /* DSP */ &&
        sb_detect_irq() && ctvoice_init() == 0) {
        DS:014c = ems_get_page_frame();
        DS:018d = 1;
    }
}

void sound_load_digi_voc(void)                      // 176b:005f, called by main
{
    if (!DS:018d) return;
    u8 far *buf = mem_alloc(0x719c, 0);  if (!buf) fatal_error_exit(4);  // largest slot
    int fd = file_open(DS:0178, 0);      if (!fd)  fatal_error_exit(2);
    for (int i = 0; i < 5; i++) {
        slot[i].handle = ems_alloc_pages(slot[i].pages);
        file_read(fd, buf, len[i]);
        for (j = 0; j < slot[i].pages; j++) ems_map_page(slot[i].handle, j, j);
        far_memcpy(DS:014c, buf, len[i]);
        for (j = 0; j < slot[i].pages; j++) ems_map_or_free(slot[i].handle, j);
    }
    file_close(fd); mem_free(buf, 0);
}

void sound_voice_shutdown(void)                     // 176b:004b, called by fatal_error_exit
{
    if (DS:018d) ctvoice_terminate();
}
```

Without a Sound Blaster DSP, an EMS driver with more than six free pages or a detected IRQ,
`DS:018d` stays 0 and every voice function below is a no-op.

## 4. Playback API

```c
void sound_play_voice(int n)                        // 176b:0182, n = 1..5
{
    if (!DS:018d) return;
    DS:018e = n;
    if (n == 1) DS:018f = 1;                        // the crowd loop is wanted from now on
    int s = DS:016c[n];
    ctvoice_stop();                                 // a running voice is cut off
    if (DS:0190 == -1) { DS:0190 = slot[s].handle; ems_save_map(DS:0190); }
    for (j = 0; j < slot[s].pages; j++) ems_map_page(slot[s].handle, j, j);
    ctvoice_output(DS:014c + *(u16 far *)(DS:014c + 0x14));   // first block after the header
}                                                   // returns at once; output runs by DMA/IRQ

void sound_stop_voice(void)                         // 176b:020f
{
    if (!DS:018d) return;
    if (DS:018e == 1) DS:018f = 0;                  // stopping the crowd ends the loop wish;
    DS:018e = 0;                                    // stopping a one-shot does not
    ctvoice_stop();
    if (DS:0190 != -1) { ems_restore_map(DS:0190); DS:0190 = -1; }
}

void sound_wait_voice_end(void)                     // 176b:024b
{
    if (!DS:018d) return;
    while (DS:94c2 != 0) ;                          // busy wait, no input polling
    DS:018e = 0;
    if (DS:0190 != 0) { ems_restore_map(DS:0190); DS:0190 = -1; }   // NOTE: compares with 0, not -1
}

void sound_restart_loop_voice(void)                 // 176b:027d
{
    if (DS:018d && DS:94c2 == 0 && DS:018f) sound_play_voice(1);
}
```

Semantics for the port:

* Only one voice plays at a time. `play` replaces whatever is playing.
* Voice 1 (crowd) loops by itself through its repeat block. A one-shot (2..5) started
  during the crowd cuts it off; because `DS:018f` stays set, the next
  `sound_restart_loop_voice` after the one-shot ended (status 0) starts the crowd again
  from the beginning. The game calls `sound_restart_loop_voice` in every wait loop of the
  match screens, so the crowd resumes within a frame.
* `sound_stop_voice` stops any voice; it only clears the loop wish when the crowd itself was
  playing (`DS:018e == 1`). Stopping a one-shot while the loop is wanted therefore lets the
  next restart call bring the crowd back.

## 5. Call sites

| Caller | Address | Call | When | Waits |
|---|---|---|---|---|
| `fatal_error_exit` | 176b:12a2 | `sound_voice_shutdown()` | program exit | – |
| `main` | 18bc:0671, 0676 | `sound_voice_init(); sound_load_digi_voc();` | after the title music | – |
| `match_live_period` | 1310:24df | `sound_play_voice(1)` crowd | start of a period shown live (`show != 0`), after the score panels | no |
| `match_live_period` | 1310:2ab4 | `sound_stop_voice()` | end of a period shown live | – |
| `match_ticker_events` | 2a58:1198 | `play(3)` for `type` 2 or 3 (red / yellow-red / yellow card), `play(4)` for `type` 4 (injury); then `sound_wait_voice_end(); sound_restart_loop_voice();` | ticker event of a human club | yes, until the sample ends (0.70 s / 1.12 s) |
| `match_ticker_events` | 2a58:14c0 | `play(2)` goal (`type` 1) or `play(5)` miss (`type` 0); then wait and restart the loop | goal chance when goal scenes are off (`DS:0607 == 0`) and `DS:05d4 == 0`, after the blinking "Chance für ..." text | yes (1.61 s / 0.79 s) |
| `tore_play_frames` | 1c7c:027f..02df | `play(k + 2)` when frame `i == sound_frames[k]` (TORE header bytes 12..15, `DS:52cc..52cf`) | goal scene animation (see [tore.md](tore.md)) | no |
| `tore_play_frames` | 1c7c:005e .. 0385 | `sound_restart_loop_voice()` | before the loop, per object, per frame, in every tick wait | – |
| `tore_play_frames` | 1c7c:0380 | `sound_stop_voice()` if a sound was triggered in this showing and `DS:018e != 1` | after the last frame and the 7-tick pause; the following restart call resumes the crowd | – |

`sound_play_voice` is also called by `sound_restart_loop_voice` (176b:029e). There are no
other callers: digitised sound is heard only on the live match screen (crowd during each
period, whistle on cards, injury sound, goal/miss sound) and in the TORE goal scenes. The
title, office and all menus are silent apart from the FM title music.
