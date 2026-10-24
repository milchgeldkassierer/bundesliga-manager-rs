#!/usr/bin/env python3
"""Compares a port frame against a DOSBox reference frame (docs/re/TOLERANCES.md).

Usage: tools/compare.py <port.png> <reference.png> [--diff <out.png>] [--threshold 90]
Prints the share of matching pixels (every RGB channel within 4) and exits 1 below the threshold.
"""
import argparse
import sys

import numpy as np
from PIL import Image

CHANNEL_TOLERANCE = 4


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("port")
    ap.add_argument("reference")
    ap.add_argument("--diff")
    ap.add_argument("--threshold", type=float, default=90.0)
    a = ap.parse_args()
    port = np.asarray(Image.open(a.port).convert("RGB"), dtype=np.int16)
    ref = np.asarray(Image.open(a.reference).convert("RGB"), dtype=np.int16)
    if port.shape != ref.shape:
        print(f"size mismatch: port {port.shape[1]}x{port.shape[0]}, reference {ref.shape[1]}x{ref.shape[0]}")
        return 1
    ok = (np.abs(port - ref) <= CHANNEL_TOLERANCE).all(axis=2)
    share = 100.0 * ok.mean()
    print(f"match {share:.2f}% ({(~ok).sum()} of {ok.size} pixels differ)")
    if a.diff:
        vis = (ref // 3).astype(np.uint8)
        vis[~ok] = [255, 0, 255]
        Image.fromarray(vis).save(a.diff)
    return 0 if share >= a.threshold else 1


if __name__ == "__main__":
    sys.exit(main())
