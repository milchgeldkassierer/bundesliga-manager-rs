# CMF music and the embedded SBFMDRV driver

`SOUND\BM2TITLE.CMF` is the title tune. `main` (flow.md §2) hands it to Creative's FM driver
SBFMDRV, which BMMAIN.EXE carries embedded at segment `4e67` (0x14b0 bytes, "FMDRV" signature at
4e67:0003, version word 0x0114 at 4e67:000c). This document describes the file format and the
driver's playback behaviour as read from the disassembly, so that the port can reproduce it
register for register on an emulated OPL2.

All addresses below are in segment `4e67` unless noted (`CS:xxxx` = driver data, the driver
runs with DS = CS). "Origin" lines refer to BMMAIN.EXE (unpacked).

## 1. File format

All values little endian.

### 1.1 Header

| Offset | Size | Meaning |
|---|---|---|
| 0x00 | 4 | signature `"CTMF"` |
| 0x04 | u16 | version (0x0101 = 1.1; 1.0 files lack 0x24/0x26 as u16) |
| 0x06 | u16 | file offset of the instrument block |
| 0x08 | u16 | file offset of the music block (event stream) |
| 0x0a | u16 | ticks per quarter note (informational; the driver ignores it) |
| 0x0c | u16 | ticks per second = driver clock rate (see §2.1) |
| 0x0e | u16 | file offset of the title string (0 = none) |
| 0x10 | u16 | file offset of the composer string (0 = none) |
| 0x12 | u16 | file offset of the remarks string (0 = none) |
| 0x14 | 16 × u8 | channel-in-use flags, MIDI channels 0..15 (informational; the driver ignores it) |
| 0x24 | u16 | number of instruments (`main` passes only the low byte, `cmf[0x24]`) |
| 0x26 | u16 | basic tempo (informational; the driver ignores it) |

### 1.2 Instrument record (16 bytes)

Operator register values exactly as they go to the OPL; "mod" = modulator (operator 1),
"car" = carrier (operator 2) of the voice.

| Byte | Register | Content |
|---|---|---|
| 0 | 0x20 mod | AM / VIB / EG-type / KSR / MULT |
| 1 | 0x20 car | AM / VIB / EG-type / KSR / MULT |
| 2 | 0x40 mod | KSL (bits 6-7) / total level (bits 0-5) |
| 3 | 0x40 car | KSL / total level (the driver rescales it, §2.6) |
| 4 | 0x60 mod | attack rate / decay rate |
| 5 | 0x60 car | attack rate / decay rate |
| 6 | 0x80 mod | sustain level / release rate |
| 7 | 0x80 car | sustain level / release rate |
| 8 | 0xE0 mod | wave select |
| 9 | 0xE0 car | wave select |
| 10 | 0xC0 | feedback (bits 1-3) / connection (bit 0) |
| 11..15 | — | unused (zero) |

### 1.3 Music block

A standard MIDI track body without the `MTrk` header: a variable-length delta time (7 bits per
byte, MSB first, bit 7 = more bytes follow) before every event, starting with the first event.
Running status is supported for channel messages. Delta times are in driver ticks (§2.1).

## 2. SBFMDRV playback semantics

### 2.1 Driver functions, timer and tick timing

The game calls the driver through far wrappers in segment `4d6c` (FUNCTION_MAP: `fmdrv_*`);
the dispatcher `fmdrv_dispatch` 4e67:110f selects function BX ≥ 0 from the table CS:0261 and
BX < 0 (as `~BX`) from CS:027f. A re-entrant call returns 0xfff8.

| Fn | Handler | Effect |
|---|---|---|
| 0 | 116d | version (0x0114) |
| 1 | 1171 | set the address of the status byte (DX:AX); returns the old one; writes 0 to the new byte |
| 2 | 1184 | set instrument table: pointer DX:AX, count CL (`CS:0016`, `CS:01e0`); rhythm off (§2.8); voice init (§2.9) |
| 3 | 11c6 | set system clock divisor `CS:0032` (init: 0xffff) |
| 4 | 11ca | set driver clock divisor `CS:0034` and program PIT channel 0 with it at once |
| 5 | 11d3 | set transpose `CS:0042` (signed semitones) |
| 6 | 0c39 | play music block DX:AX (§2.2) |
| 7 | 11b6 | stop music (§2.3) |
| 8 | 0c00 | reset driver (§2.9) |
| 9 / 10 | 0c98 / 0cad | pause (all notes off) / resume |
| 11 | 11d9 | install a user SysEx trap (far call on F0 events) |
| 12 | 11e6 | address of the 16-byte channel-enable table `CS:0219` (1 = enabled) |
| 13 | 11ed | current music pointer |
| 14 | 11fb | volume fade (not used by the game) |
| −1 | 1487 | init: calibrate delays, detect the OPL (timer test), defaults (§2.9), hook INT 8 and INT 9 |
| −2 | 149d | stop and unhook |
| −3 | 14a6 | set base port |

The driver clock is a raw PIT channel-0 **divisor**, not a rate. `main` passes
`0x1234DC / cmf[0x0c]` (integer division, truncated: 1193180 / ticks-per-second). Every INT 8 is
one driver tick, so the tick rate is `PIT_HZ / divisor` with the hardware PIT clock
`PIT_HZ = 1193182` Hz (DOSBox uses the same value). The INT 8 handler 1052 chains to the old BIOS
handler at the original ~18.2 Hz by accumulating the divisor (`CS:0038 += CS:0036`, call the old
vector when it reaches the system divisor `CS:0032`); the port does not need this.

Delay counting (INT 8 handler 1052, event pump 0cbf):

```c
// play (fn 6) 0c39:
delay = read_varlen();           // first delta, stored WITHOUT the -1 below
// INT 8, while playing:
if (delay-- == 0)                // 32-bit counter CS:0022; fires when it borrows
    process_events();
// process_events 0cbf:
tick_counter++;                  // CS:0040, voice age clock (§2.5), once per call
for (;;) {
    dispatch_one_event();        // §2.4
    if (!playing) return;
    delay = read_varlen();
    if (delay != 0) { delay--; return; }
}
```

Consequences: an event with delta `d > 0` fires exactly `d` ticks after the previous one; events
with delta 0 fire in the same tick; the very first event fires on INT 8 number `delta0 + 1`
after `play` (the first INT 8 comes one divisor period after the PIT was programmed). The tick
on which the end-of-track event is processed is `1 + Σ all deltas` after `play`.

### 2.2 Play (fn 6) 0c39

Fails (0xfffe) if already playing. Otherwise: store start/current pointer, clear the 16 per-channel
pitch offsets `CS:01f9`, mark all voice owners free (`CS:0148[0..8] = 0xff`), read the first
delta, `tick_counter = 0`, program the PIT with the driver clock, rhythm-mode registers off
(§2.8: 9 voices, BD = 0xC0; the rhythm *flag* `CS:01e7` is left as it was), `playing = 1`,
status byte = 0xff. Instruments are *not* written to the chip here; they are loaded lazily per
voice (§2.5).

### 2.3 Stop 1198 (fn 7, event FC, meta FF 2F)

If playing: `playing = 0`, PIT back to the system divisor, all notes off, status byte = 0.
The song does **not** loop: it stops at FF 2F (or FC).

All notes off 0964 (also pause): for voices 8..0 it writes register **0x83 = 0x13** nine times
(a driver bug — `add al,bl` instead of an operator offset — it sets the carrier SL/RR of voice
0 only), then for every voice whose owner byte ≤ 0x7f (note held) it rewrites A0+v / B0+v
from the stored frequency word without the key-on bit. In rhythm mode it then clears the five
drum bits: BD &= 0xE0.

### 2.4 Event dispatch 0cbf

A byte with bit 7 set is a status byte: channel = low nibble (`CS:003c`), type = high nibble − 8
(`CS:003e`); otherwise running status (the previous status, also after F0/FF). Handler table
CS:0229:

| Status | Handler | Data bytes | Action |
|---|---|---|---|
| 8x | 0d29 | note, vel | note off (§2.7) |
| 9x | 0da0 | note, vel | note on (§2.6); velocity 0 = note off |
| Ax | 0f8d | 2 | ignored |
| Bx | 0efa | ctrl, value | controllers 0x66..0x69 only (below), others ignored |
| Cx | 0f90 | program | program change (§2.6.3) |
| Dx | 0f8e | 1 | ignored |
| Ex | 0f8d | 2 | **ignored** (no MIDI pitch bend; use controllers 0x68/0x69) |
| Fx | 100a | — | table CS:0241 by the low nibble |

System events (table CS:0241): F0 (1017): call the user SysEx trap if installed, then skip a
varlen length and that many bytes; F7 (102d): skip varlen + bytes; F2: skip 2; F3: skip 1;
FC (1038): stop; FF (103c): type byte, if 0x2F stop, then skip varlen + bytes; all others: no data.

Controllers (table CS:0239, `value` = second data byte):

| Ctrl | Handler | Action |
|---|---|---|
| 0x66 | 0f10 | marker: status byte = value (a marker 0 would end `main`'s title loop) |
| 0x67 | 0f16 | rhythm mode: voice init (§2.9 `voices_init`, which also resets all 16 channel programs to 0!), then flag `CS:01e7` = value; value ≠ 0: 6 melodic voices, BD = 0xE0; value = 0: 9 voices, BD = 0xC0; write BD |
| 0x68 | 0f3c | pitch up: `pitch[ch] = (s8)value >> 2` (1/64 semitone units) |
| 0x69 | 0f3a | pitch down: `pitch[ch] = (s8)(-value) >> 2` (arithmetic shift) |

After 0x68/0x69 every voice whose owner byte equals `ch` (note held) is re-tuned: frequency
recomputed from its stored octave/semitone byte (§2.6.2, transpose not re-applied, the age stamp
renewed) and A0/B0 rewritten **with** key-on.

### 2.5 Voice state and allocation `voice_alloc` 0b6e

Per voice v (0..8, rhythm slots 6..10): owner `CS:0148[v]` (0xff = never used / freed;
`ch` = playing for MIDI channel ch; `ch | 0x80` = released by ch), last raw note `CS:015e[v]`,
octave/semitone byte `CS:0153[v]`, frequency word `CS:018a[v]` (A0 low byte, B0 bits without
key-on), age stamp `CS:01a0[v]` (= `tick_counter` when the frequency was set), KSL bits
`CS:0169[v]`, level `CS:0174[v]` (§2.6.1). `n` = number of melodic voices (`CS:003a`, 9 or 6).

Search, each over v = 0..n−1, first match wins:

1. a voice released by the same channel (owner == ch | 0x80);
2. a free voice (owner == 0xff);
3. any released voice (owner > 0x7f);
4. otherwise the oldest voice: largest `(u16)(tick_counter - age[v])`; on a tie the **highest**
   index wins (comparison `>=`, starting from age 0). This voice is keyed off first
   (A0+v / B0+v rewritten without key-on).

### 2.6 Note on 0da0 (velocity ≠ 0)

```c
if (channel_enable[ch] == 0) return;                 // CS:0219, all 1 after reset
if (rhythm_flag && ch >= 11) { rhythm_note_on(); return; }
v = voice_alloc();
old = owner[v]; owner[v] = ch;
if ((old & 0x7f) != ch) load_instrument(program[ch], v);   // §2.6.3
// carrier total level from velocity:
att = (level[v] * (vel | 0x80)) >> 8;                // 8x8 -> high byte
opl(0x43 + mod_op[v], (0x3f - att) | ksl[v]);
set_frequency(v, note);                              // §2.6.2
opl(0xa0 + v, freq[v] & 0xff);
opl(0xb0 + v, (freq[v] >> 8) | 0x20);                // key on
```

`mod_op[v]` (CS:01bd) = 00 01 02 08 09 0A 10 11 12; the carrier is `mod_op + 3`.

#### 2.6.1 Instrument levels

When an instrument is loaded into voice v, `L = 0x3f - (lvl & 0x3f)` is stored in `CS:017f[v]`,
`lvl & 0xc0` in `ksl[v]` and `level[v] = (L * volume + 0x80) >> 8` (`mul`, then `add al,al;
adc ah,0` = round) where `volume = CS:01b6` = 0xff after reset (fn 14 fades change it), so
`level = L` in the game. `lvl` is instrument byte 3 (carrier level), except in rhythm mode for
slots 7..10 (snare, tom, cymbal, hi-hat) where it is byte 2. Example: L = 63, vel 127 → att 62 →
TL 1; vel 101 → att 56 → TL 7.

#### 2.6.2 Frequency `set_frequency` 0e75 / 0e9e

```c
note_raw[v] = note;
n = clamp(note + transpose, 0, 127);                 // CS:0042, signed
os = OCT_SEMI[n];                                    // CS:0285: n<24: n%12 (octave 0);
                                                     // 24..119: ((n/12-1)<<4)|(n%12); >=120: 0x7b
octsemi[v] = os;
block4 = (os & 0x70) >> 2;                           // block << 2 (bits 2-4 of B0)
idx = (os & 0x0f) * 64 + pitch[ch];                  // pitch: §2.4, 1/64 semitone
if (idx < 0) { idx += 0x300; block4 -= 4; if (block4 < 0) { block4 = 0; idx = 0; } }
else if (idx >= 0x300) { idx -= 0x300; block4 += 4;
                         if (block4 > 0x1c) { idx = 0x2ff; block4 = 0x1c; } }
freq[v] = FNUM[idx] | (block4 << 8);
age[v] = tick_counter;
```

`FNUM` (CS:0305, 768 words, 64 steps per semitone over one octave) runs from 0x157 to 0x2ad;
semitone entries (idx = 64·s): 157 16B 181 198 1B0 1CA 1E5 202 220 241 263 287. Every entry
equals `floor(342.9215 * 2^(idx/768) + 0.5)` (verified against all 768 words).
Example: MIDI note 60 → octave 4, semitone 0 → F-number 0x157, block 4 (B0 = 0x31 with key on).

#### 2.6.3 Instrument load `load_instrument` 0a1e and program change 0f90

`load_instrument(p, v)`: ignored if `p >= count`. Melodic voices and the bass-drum slot 6
(two-operator path, `op = mod_op[v]`):

```
0x20+op = b0   0x23+op = b1   0x40+op = b2          (carrier TL not written here)
0x60+op = b4   0x63+op = b5   0x80+op = b6   0x83+op = b7
0xE0+op = b8   0xE3+op = b9   0xC0+v  = b10
```

Rhythm slots 7..10 (one operator `op = DRUM_OP[slot]`, only the modulator bytes are used):
`0x20+op = b0, 0x60+op = b4, 0x80+op = b6, 0xE0+op = b8, 0xC0+DRUM_CH[slot] = b10`
(0x40 is written at note on).

Program change: `p` is reduced modulo the instrument count (repeated subtraction), stored in
`program[ch]` (`CS:01e9`, 16 bytes, all 0 after voice init). In rhythm mode and ch ≥ 11 the
instrument is loaded into slot `ch − 5` at once. Otherwise: every voice released by ch
(owner == ch|0x80) becomes free (0xff), and every voice still playing for ch (owner == ch) gets the
new instrument loaded immediately.

### 2.7 Note off 0d29 (8x, or 9x with velocity 0)

Rhythm mode (`n <= 6`) and ch ≥ 11: clear the channel's BD bit and write BD. Otherwise search
`note_raw[0..n-1]` for the note; the first voice with owner == ch (exactly, i.e. still held) is
released: owner |= 0x80, A0+v / B0+v rewritten from `freq[v]` without key-on. No match: ignored.

### 2.8 Rhythm mode

`rhythm_off` 0bee: `n = 9`, BD = 0xC0 (AM depth 4.8 dB, vibrato depth 14 cents), written; the
rhythm flag `CS:01e7` itself is only changed by controller 0x67.

Rhythm mode: `n = 6`, BD = 0xE0. MIDI channels 11..15 are the five drums (slot = ch − 5):

| MIDI ch | Slot | Drum | Operator(s) (`DRUM_OP` CS:01cb+slot) | OPL ch for A0/B0/C0 (CS:01d5+slot) | BD bit (CS:01cb+ch) |
|---|---|---|---|---|---|
| 11 | 6 | bass drum | 0x10 + 0x13 (both, TL on 0x13) | 6 | 0x10 |
| 12 | 7 | snare | 0x14 | 7 | 0x08 |
| 13 | 8 | tom-tom | 0x12 | 8 | 0x04 |
| 14 | 9 | cymbal | 0x15 | 8 | 0x02 |
| 15 | 10 | hi-hat | 0x11 | 7 | 0x01 |

`rhythm_note_on` 0e22:

```c
s = ch - 5;
bd |= BD_BIT[ch];
att = (level[s] * (vel | 0x80)) >> 8;
opl(0x40 + DRUM_OP[s] + (s == 6 ? 3 : 0), (0x3f - att) | ksl[s]);
set_frequency(s, note);                     // uses pitch[ch], stores in slot s
opl(0xa0 + DRUM_CH[s], freq[s] & 0xff);
opl(0xb0 + DRUM_CH[s], freq[s] >> 8);       // no key-on bit for drums
opl(0xbd, bd);
```

The driver does not clear the drum bit before setting it, so a drum whose bit is still set (no
note off yet) is not re-triggered on the chip. Snare/hi-hat share channel 7's frequency and
tom/cymbal channel 8's; the last write wins. Drum instruments are loaded only by program
changes on channels 11..15 (no lazy load).

### 2.9 Reset, init, voice init

`init` (fn −1) 1487 → `defaults` 140d: system divisor 0xffff, driver divisor 0x48d3 (64 Hz),
status byte address = internal `CS:01e5`, write **0x01 = 0x20** (waveform select enable) and
**0x08 = 0x00**, then `reset`.

`reset` (fn 8) 0c00: volume `CS:01b6/01b7 = 0xff`, stop, `rhythm_off`, all 16 channels enabled,
instrument table = the driver's 16 default instruments at CS:0048 (count 16), `voices_init`,
driver divisor 0x48d3 (stored, PIT not reprogrammed), transpose 0.

`voices_init` 09ad (also fn 2 and controller 0x67): programs of all 16 channels = 0, owners
`CS:0148[0..10] = 0xff`; for each voice v = 0..8: `rhythm_off`, 0x08 = 0x00, and with
`op = mod_op[v]` the fixed patch at CS:01c6: `0x20+op=01 0x23+op=11 0x40+op=4F 0x43+op=00
0x60+op=F1 0x63+op=F2 0x80+op=53 0x83+op=74 0xE0+op=00 0xE3+op=00`. The 11th byte (0x08, from
CS:01d0) goes to register `op + v` instead of 0xC0+v (driver bug: registers 00, 02, 04, 0B, 0D,
0F, 16, 18, 1A get 0x08 — test/timer registers without audible effect). 0xC0..0xC8 are not
touched.

### 2.10 Status byte and `main`'s wait loop

The status byte (moved to `DS:94c4` by `fmdrv_init` 4d6c:049c via fn 1) is 0xff after play, the
value of each controller-0x66 marker, and 0 after stop. `fmdrv_poll_status` 4d6c:0580 returns 0
if it is 0, otherwise returns it and resets it to 0xff (CLI, XCHG). `main` loops while it is
non-zero, i.e. until the song stops (FF 2F) or the player clicks (`fmdrv_stop_music`, which sets
it to 0, then `fmdrv_shutdown`).

INT 9 (10da) compares a non-negative scan code with 0x83 and can never match: no hot key.

## 3. BM2TITLE.CMF

30319 bytes, version 0x0101, instruments at 0x28 (12 records), music at 0xe8, 41 ticks per
quarter, **96 ticks per second**, no title/composer/remarks, channel flags set for 0-5, 11, 12,
15, basic tempo 140.

Instrument use (the patch bytes themselves are not reproduced here; `bmp-formats::cmf`
reads them from the file): 0 → ch 0, 1 → ch 1, 2 → ch 2, 3 and 4 → ch 3 (alternating),
5 → ch 4, 6 → ch 5, 7 → ch 11 (bass drum), 8 → ch 12 (snare), 11 → ch 15 (hi-hat);
9 and 10 are unused drum patches.

Tick 0 events, in order: controller 0x67 = 1 on ch 0 (rhythm mode on — before the program
changes, so they survive), then programs ch0=0, ch1=1, ch2=2, ch3=3, ch4=5, ch5=6, ch11=7,
ch12=8, ch15=11. Channel 3 later switches between programs 4 and 3 (ticks 4278, 5924, 9216,
9874, 12507, 13494, 15469, 17444). No other controllers, no pitch changes, no markers, no SysEx.
Only note-on events (note off = velocity 0): 7852 in total, 3926 with velocity > 0 (velocities
127, 101 and a few 25..114), notes 24..79, at most 6 melodic notes held at once (= the 6 voices
of rhythm mode, so no voice stealing). Common deltas: 0, 10, 11 ticks.

Last event FF 2F at tick 18761 (sum of deltas; first delta 0); one stray 0xFF byte follows it.
With divisor `0x1234DC / 96 = 12428` the tick lasts 12428 / 1193182 s = 10.4158 ms (96.007 Hz),
so the driver processes the end of track on tick 18762, **195.42 s** after `fmdrv_play_music`;
the title loop in `main` ends at the next poll after that.

## 4. Verification of the port

`crates/bmp-sound` reimplements §2 register for register (`fmdrv.rs`) on its own OPL2 emulation
(`opl.rs`). Reference data from the original comes from `tools/dosbox-capture.sh` with
`DOSBOX_AUDIO=1` (Sound Blaster 2.0, OPL2), script lines `opl start` / `opl stop <name>` (raw OPL
register capture, `.dro`) and `audio start` / `audio stop <name>` (WAV). The recordings are not
kept in the repository.

* **Register stream** (`bmp render-music out.wav --opl-log port.txt`, then
  `tools/opl-compare.py port.txt title.dro`): from the first key-on to the end, all 12304
  register-changing writes of the port equal the original's, in the same order (the capture has one
  more, `01=00`, from the driver shutdown after the recording). Timing: after the first event group
  every write lies within the capture's 1 ms resolution of the port's tick grid over the whole
  195.4 s (no drift); the capture places the first event group one tick (10.4 ms) earlier relative to
  the rest. Two captures agree on this; the driver code (play 4e67:0c39, INT 8 4e67:1052) gives no
  reason for it, so it is taken to be an artefact of how DOSBox starts the capture.
* **Sound** (`tools/audio-compare.py port.wav title.wav`): DOSBox's WAV recordings are not
  time-exact (their clock wanders by up to seconds against the music when the emulation runs behind),
  so only time-independent measures are meaningful. Long-term spectrum (24 bands, 40 Hz–8 kHz):
  correlation 0.991, mean deviation 0.6 dB (two recordings of the original against each other:
  0.995 / 0.4 dB). Overall level: the port is 3 dB louder than DOSBox's mixer output (which applies
  the SB 2.0 output filter and a compressor). Frame-wise measures (50 ms spectra / chroma) reach 0.54
  / 0.79 against the original and 0.51 / 0.76 between two recordings of the original, i.e. they are
  limited by the recordings' timing.
