#!/usr/bin/env python3
"""Compares the port's rendering of a tune with a DOSBox recording of the original.

Usage: tools/audio-compare.py <port.wav> <dosbox.wav>

Both files are 16-bit PCM WAVs (mono or stereo, any rate). The recording may start
with silence or other sounds before the tune; the port's file starts with the tune.
DOSBox's WAV recordings are not time-exact (the mixer drifts and jitters by tens of
milliseconds when the emulation runs behind), so timing is checked on the OPL register
stream instead (tools/opl-compare.py) and this script aligns every 0.5 s window of the
port's rendering separately before comparing the sound: correlation of the loudness
envelopes, cosine similarity of the magnitude spectra and of the chroma (pitch class)
vectors over 50 ms frames, and the level difference. Needs numpy.
"""
import sys
import wave

import numpy as np

RATE = 16000  # common analysis rate
HOP = 0.005  # envelope frame (s)


def load(path):
    with wave.open(path) as w:
        ch, rate, n = w.getnchannels(), w.getframerate(), w.getnframes()
        a = np.frombuffer(w.readframes(n), np.int16).astype(np.float64)
    a = a.reshape(-1, ch).mean(axis=1)
    # Linear resampling to the common rate (both sources are band-limited enough
    # for an envelope/spectrum comparison).
    t = np.arange(int(len(a) * RATE / rate)) * rate / RATE
    return np.interp(t, np.arange(len(a)), a) / 32768.0


def envelope(a):
    hop = int(RATE * HOP)
    n = len(a) // hop
    return np.sqrt((a[: n * hop].reshape(n, hop) ** 2).mean(axis=1) + 1e-12)


def frame_features(a):
    win = 2048
    hop = int(RATE * 0.05)
    frames = np.lib.stride_tricks.sliding_window_view(a, win)[::hop] * np.hanning(win)
    mag = np.abs(np.fft.rfft(frames, axis=1))
    freqs = np.fft.rfftfreq(win, 1 / RATE)
    band = (freqs > 40) & (freqs < 4000)
    spec = mag[:, band]
    midi = 69 + 12 * np.log2(freqs[band] / 440.0)
    pc = np.round(midi).astype(int) % 12
    chroma = np.stack([spec[:, pc == k].sum(axis=1) for k in range(12)], axis=1)
    return spec, chroma


def cosine(x, y):
    num = (x * y).sum(axis=1)
    den = np.linalg.norm(x, axis=1) * np.linalg.norm(y, axis=1) + 1e-12
    return num / den


def main():
    port, ref = load(sys.argv[1]), load(sys.argv[2])
    ep, er = envelope(port), envelope(ref)
    # Coarse alignment: the first frame above -40 dBFS, refined by cross-correlation
    # of the log envelopes over +-0.5 s.
    first = lambda e: int(np.argmax(e > 0.01))
    lp, lr = np.log(ep + 1e-4), np.log(er + 1e-4)
    base = first(er) - first(ep)
    span = int(0.5 / HOP)
    n = min(len(lp), len(lr) - base - span) - span
    best = max(
        range(-span, span + 1),
        key=lambda d: np.corrcoef(lp[span : span + n], lr[base + d + span : base + d + span + n])[0, 1],
    )
    shift = (base + best) * HOP
    # Fine alignment: cross-correlation of the waveforms' absolute values (robust
    # against phase differences of the synthesis) over +-10 ms on 20 s.
    k = int(shift * RATE)
    seg = slice(RATE, 21 * RATE)
    w = int(0.01 * RATE)
    a = np.abs(port[seg])
    fine = max(
        range(-w, w + 1),
        key=lambda d: np.dot(a, np.abs(ref[k + d + RATE : k + d + 21 * RATE])),
    )
    shift = (k + fine) / RATE
    print(f"alignment: the tune starts {shift:.3f} s into the recording")
    ref = ref[int(round(shift * RATE)) :]
    m = min(len(port), len(ref))
    port, ref = port[:m], ref[:m]

    # Local alignment per window, following the recording's drifting clock.
    win, lags, ports, refs, lag = RATE // 2, [], [], [], 0
    w = int(0.06 * RATE)
    step = int(0.0005 * RATE)
    for start in range(4 * w, m - win - 4 * w, win):
        if start + abs(lag) + w + win > m:
            break
        a = np.abs(port[start : start + win])
        lag = max(
            range(lag - w, lag + w + 1, step),
            key=lambda d: np.dot(a, np.abs(ref[start + d : start + d + win])),
        )
        lags.append(1000 * lag / RATE)
        ports.append(port[start : start + win])
        refs.append(ref[start + lag : start + lag + win])
    lags = np.array(lags)
    print(
        f"recording clock: local offset {lags.min():+.1f} .. {lags.max():+.1f} ms over "
        f"{len(lags)} windows of 0.5 s (largest step between windows {np.abs(np.diff(lags)).max():.1f} ms)"
    )
    port, ref = np.concatenate(ports), np.concatenate(refs)

    ep, er = envelope(port), envelope(ref)
    db = lambda e: 20 * np.log10(e + 1e-4)
    print(f"envelope correlation (dB, 5 ms frames, windows aligned): {np.corrcoef(db(ep), db(er))[0, 1]:.3f}")
    gain = 20 * np.log10(np.sqrt((port**2).mean()) / np.sqrt((ref**2).mean()))
    print(f"level: port is {gain:+.1f} dB relative to the original")
    sp, cp = frame_features(port)
    sr, cr = frame_features(ref)
    loud = (sp.sum(axis=1) > 1e-3 * sp.sum(axis=1).max()) & (sr.sum(axis=1) > 1e-3 * sr.sum(axis=1).max())
    print(f"spectral similarity (cosine of 50 ms magnitude spectra, 40 Hz-4 kHz): {cosine(sp, sr)[loud].mean():.3f}")
    print(f"chroma similarity (cosine of 50 ms pitch-class vectors): {cosine(cp, cr)[loud].mean():.3f}")
    # Timbre: band levels (24 log-spaced bands, 40 Hz-8 kHz, dB) per aligned window,
    # and of the whole tune.
    edges = np.geomspace(40, 8000, 25)

    def bands(x):
        mag = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
        f = np.fft.rfftfreq(len(x), 1 / RATE)
        return np.array([10 * np.log10(mag[(f >= lo) & (f < hi)].sum() + 1e-9) for lo, hi in zip(edges, edges[1:])])

    bp = np.array([bands(x) for x in ports])
    br = np.array([bands(x) for x in refs])
    per = [np.corrcoef(a, b)[0, 1] for a, b in zip(bp, br)]
    print(f"band-level correlation per 0.5 s window (timbre + notes): mean {np.nanmean(per):.3f}")
    lp, lr = bands(port), bands(ref)
    print(f"long-term spectrum: correlation {np.corrcoef(lp, lr)[0, 1]:.3f}, "
          f"mean |difference| {np.abs((lp - lp.mean()) - (lr - lr.mean())).mean():.1f} dB")


if __name__ == "__main__":
    main()
