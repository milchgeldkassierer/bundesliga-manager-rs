#!/usr/bin/env python3
"""Compares the port's OPL register writes with a DOSBox raw OPL capture (.dro).

Usage: tools/opl-compare.py <port-opl.txt> <capture.dro>

<port-opl.txt> comes from `bmp render-music <wav> --opl-log <txt>` ("<ms> <reg> <val>"
lines, hex), the capture from `tools/dosbox-capture.sh` ("opl start" / "opl stop <name>").
Writes that do not change a register are dropped on both sides (DOSBox does not record
them). Both streams are aligned at their first key-on (B0..B8 with bit 5, or a BD
drum bit), then compared write by write: the share of identical (register, value)
pairs in order, and the timing deviation of the matching pairs over the tune
(DRO timestamps are milliseconds of emulated time).
"""
import difflib
import struct
import sys


def read_dro(path):
    b = open(path, "rb").read()
    if b[:8] != b"DBRAWOPL":
        sys.exit("not a DRO file")
    major, minor = struct.unpack_from("<HH", b, 8)
    if major != 2:
        sys.exit(f"DRO version {major}.{minor} not supported")
    pairs, _ms, _hw, _fmt, _comp, short, long_, cmlen = struct.unpack_from("<IIBBBBBB", b, 12)
    codemap = b[26 : 26 + cmlen]
    at, t, out = 26 + cmlen, 0, []
    for _ in range(pairs):
        code, val = b[at], b[at + 1]
        at += 2
        if code == short:
            t += val + 1
        elif code == long_:
            t += (val + 1) << 8
        elif code & 0x80 == 0:
            out.append((float(t), codemap[code], val))
    return out


def read_port(path):
    out = []
    for line in open(path):
        ms, reg, val = line.split()
        out.append((float(ms), int(reg, 16), int(val, 16)))
    return out


def changed_only(writes):
    # The chip starts with all registers 0; the capture begins with a dump of them.
    regs, out = dict.fromkeys(range(256), 0), []
    for t, r, v in writes:
        if regs.get(r) != v:
            regs[r] = v
            out.append((t, r, v))
    return out


def first_key_on(writes):
    for i, (_, r, v) in enumerate(writes):
        if (0xB0 <= r <= 0xB8 and v & 0x20) or (r == 0xBD and v & 0x1F):
            return i
    sys.exit("no key-on found")


def main():
    port = changed_only(read_port(sys.argv[1]))
    dro = changed_only(read_dro(sys.argv[2]))
    ip, idr = first_key_on(port), first_key_on(dro)
    p0, d0 = port[ip][0], dro[idr][0]
    port, dro = port[ip:], dro[idr:]
    print(f"writes from the first key-on: port {len(port)}, capture {len(dro)}")
    sm = difflib.SequenceMatcher(None, [w[1:] for w in port], [w[1:] for w in dro], autojunk=False)
    matched = sum(bl.size for bl in sm.get_matching_blocks())
    print(f"identical (register, value) writes in order: {matched} = {100 * matched / max(len(port), len(dro)):.2f}%")
    devs = []
    for bl in sm.get_matching_blocks():
        for k in range(bl.size):
            tp = port[bl.a + k][0] - p0
            td = dro[bl.b + k][0] - d0
            devs.append((tp, td - tp))
    devs.sort()
    absd = sorted(abs(d) for _, d in devs)
    print(
        f"timing of matching writes (capture - port): median |dev| {absd[len(absd) // 2]:.2f} ms, "
        f"95% {absd[int(0.95 * len(absd))]:.2f} ms, max {absd[-1]:.2f} ms"
    )
    print(f"last matching write: port {devs[-1][0]:.1f} ms, deviation {devs[-1][1]:+.1f} ms")
    print(f"span first key-on -> last write: port {port[-1][0] - p0:.1f} ms, capture {dro[-1][0] - d0:.1f} ms")
    shown = 0
    for tag, a1, a2, b1, b2 in sm.get_opcodes():
        if tag != "equal" and shown < 10:
            shown += 1
            print(f"  {tag} port[{a1}:{a2}] {[f'{r:02x}={v:02x}' for _, r, v in port[a1:a2][:6]]}"
                  f" capture[{b1}:{b2}] {[f'{r:02x}={v:02x}' for _, r, v in dro[b1:b2][:6]]}")


if __name__ == "__main__":
    main()
